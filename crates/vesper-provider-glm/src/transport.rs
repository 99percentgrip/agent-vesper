use std::{collections::BTreeMap, sync::Arc, time::Duration};

use reqwest::{Client, Response};
use tokio::sync::mpsc;
use vesper_domain::{ProviderRequestId, ToolId};
use vesper_provider::{CancellationSignal, ProviderError, ProviderStreamEvent};
use vesper_security::SecretValue;

use crate::{
    GlmAdapterError, GlmConfig,
    response::AttemptState,
    sse::{MAX_ERROR_BODY_BYTES, SseFrame, SseParser},
};

/// Production HTTP client wrapper. Reqwest types never cross neutral ports.
#[derive(Clone)]
pub(crate) struct GlmHttpClient {
    client: Client,
}

impl GlmHttpClient {
    pub(crate) fn build(config: &GlmConfig) -> Result<Self, GlmAdapterError> {
        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .read_timeout(config.read_timeout)
            .user_agent(&config.user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .build()
            .map_err(|_| GlmAdapterError::Configuration("HTTP client could not be constructed"))?;
        Ok(Self { client })
    }

    pub(crate) async fn execute_stream(
        &self,
        config: &GlmConfig,
        credential: &SecretValue,
        body: &serde_json::Value,
        request_id: &ProviderRequestId,
        tool_ids: &BTreeMap<String, ToolId>,
        delivery: (
            Arc<dyn CancellationSignal>,
            &mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
        ),
    ) -> Result<AttemptState, AttemptFailure> {
        let (cancellation, sender) = delivery;
        if cancellation.is_cancelled() {
            return Err(AttemptFailure::Cancelled { visible: false });
        }
        let url = config
            .endpoint
            .chat_completions_url()
            .map_err(AttemptFailure::Adapter)?;
        let mut request = self.client.post(url).json(body);
        if config.endpoint.attach_inference_auth() {
            request = request.bearer_auth(credential.expose().as_str());
        }
        let response = tokio::select! {
            _ = wait_cancelled(Arc::clone(&cancellation)) => {
                return Err(AttemptFailure::Cancelled { visible: false });
            }
            response = request.send() => response.map_err(|error| {
                if error.is_timeout() {
                    AttemptFailure::Timeout { visible: false }
                } else {
                    AttemptFailure::Transport { visible: false }
                }
            })?,
        };
        if !response.status().is_success() {
            return Err(non_success(response, cancellation).await);
        }
        self.consume_response(response, request_id, tool_ids, cancellation, sender)
            .await
    }

    pub(crate) async fn post_json(
        &self,
        config: &GlmConfig,
        credential: &SecretValue,
        body: &serde_json::Value,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> Result<Response, AttemptFailure> {
        let url = config
            .endpoint
            .chat_completions_url()
            .map_err(AttemptFailure::Adapter)?;
        let mut request = self.client.post(url).json(body);
        if config.endpoint.attach_inference_auth() {
            request = request.bearer_auth(credential.expose().as_str());
        }
        tokio::select! {
            _ = wait_cancelled(cancellation) => {
                Err(AttemptFailure::Cancelled { visible: false })
            }
            response = request.send() => {
                response.map_err(|error| {
                    if error.is_timeout() {
                        AttemptFailure::Timeout { visible: false }
                    } else {
                        AttemptFailure::Transport { visible: false }
                    }
                })
            }
        }
    }

    pub(crate) async fn get_quota(
        &self,
        config: &GlmConfig,
        credential: &SecretValue,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> Result<Response, AttemptFailure> {
        let url = config
            .endpoint
            .quota_url()
            .map_err(AttemptFailure::Adapter)?;
        let request = self
            .client
            .get(url)
            // Frozen quota monitor requires the raw API key, not Bearer.
            .header(reqwest::header::AUTHORIZATION, credential.expose().as_str())
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en")
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        tokio::select! {
            _ = wait_cancelled(cancellation) => {
                Err(AttemptFailure::Cancelled { visible: false })
            }
            response = request.send() => {
                response.map_err(|error| {
                    if error.is_timeout() {
                        AttemptFailure::Timeout { visible: false }
                    } else {
                        AttemptFailure::Transport { visible: false }
                    }
                })
            }
        }
    }

    async fn consume_response(
        &self,
        mut response: Response,
        request_id: &ProviderRequestId,
        tool_ids: &BTreeMap<String, ToolId>,
        cancellation: Arc<dyn CancellationSignal>,
        sender: &mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
    ) -> Result<AttemptState, AttemptFailure> {
        let mut parser = SseParser::default();
        let mut state = AttemptState::default();
        loop {
            let chunk = tokio::select! {
                _ = wait_cancelled(Arc::clone(&cancellation)) => {
                    return Err(AttemptFailure::Cancelled { visible: state.visible });
                }
                chunk = response.chunk() => chunk.map_err(|error| {
                    if error.is_timeout() {
                        AttemptFailure::Timeout { visible: state.visible }
                    } else {
                        AttemptFailure::Transport { visible: state.visible }
                    }
                })?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let frames = parser
                .push(&chunk)
                .map_err(|_| AttemptFailure::Adapter(GlmAdapterError::Limit("SSE frame")))?;
            if process_frames(
                frames,
                &mut state,
                request_id,
                tool_ids,
                sender,
                &cancellation,
            )
            .await?
            {
                break;
            }
        }
        let frames = parser
            .finish()
            .map_err(|_| AttemptFailure::Adapter(GlmAdapterError::Limit("SSE frame")))?;
        let _ = process_frames(
            frames,
            &mut state,
            request_id,
            tool_ids,
            sender,
            &cancellation,
        )
        .await?;
        if state.terminal_seen() {
            for event in state
                .complete_tool_events(request_id, tool_ids)
                .map_err(AttemptFailure::Adapter)?
            {
                send_event(sender, event, &cancellation, state.visible).await?;
            }
            Ok(state)
        } else {
            Err(AttemptFailure::Incomplete(Box::new(state)))
        }
    }
}

async fn process_frames(
    frames: Vec<SseFrame>,
    state: &mut AttemptState,
    request_id: &ProviderRequestId,
    tool_ids: &BTreeMap<String, ToolId>,
    sender: &mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
    cancellation: &Arc<dyn CancellationSignal>,
) -> Result<bool, AttemptFailure> {
    for frame in frames {
        if cancellation.is_cancelled() {
            return Err(AttemptFailure::Cancelled {
                visible: state.visible,
            });
        }
        match frame {
            SseFrame::Done => {
                state.mark_done();
                return Ok(true);
            }
            SseFrame::Data(data) => {
                for event in state
                    .accept_data(&data, request_id, tool_ids)
                    .map_err(AttemptFailure::Adapter)?
                {
                    send_event(sender, event, cancellation, state.visible).await?;
                }
            }
        }
    }
    Ok(false)
}

async fn send_event(
    sender: &mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
    event: ProviderStreamEvent,
    cancellation: &Arc<dyn CancellationSignal>,
    visible: bool,
) -> Result<(), AttemptFailure> {
    tokio::select! {
        _ = wait_cancelled(Arc::clone(cancellation)) => {
            Err(AttemptFailure::Cancelled { visible })
        }
        result = sender.send(Ok(event)) => {
            result.map_err(|_| AttemptFailure::ConsumerDropped)
        }
    }
}

async fn non_success(
    mut response: Response,
    cancellation: Arc<dyn CancellationSignal>,
) -> AttemptFailure {
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let mut prefix = Vec::new();
    while prefix.len() < MAX_ERROR_BODY_BYTES {
        let next = tokio::select! {
            _ = wait_cancelled(Arc::clone(&cancellation)) => {
                return AttemptFailure::Cancelled { visible: false };
            }
            value = response.chunk() => value,
        };
        match next {
            Ok(Some(chunk)) => {
                let remaining = MAX_ERROR_BODY_BYTES - prefix.len();
                prefix.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            Ok(None) | Err(_) => break,
        }
    }
    // Decode only to exercise bounded replacement behavior. The raw provider
    // body is never retained in errors, events, tracing, or fixtures.
    let _safe_discarded_prefix = String::from_utf8_lossy(&prefix);
    AttemptFailure::Http {
        status,
        retry_after,
    }
}

pub(crate) async fn bounded_json(
    mut response: Response,
    maximum: usize,
    cancellation: Arc<dyn CancellationSignal>,
) -> Result<serde_json::Value, AttemptFailure> {
    let mut bytes = Vec::new();
    loop {
        let next = tokio::select! {
            _ = wait_cancelled(Arc::clone(&cancellation)) => {
                return Err(AttemptFailure::Cancelled { visible: false });
            }
            value = response.chunk() => value,
        };
        match next {
            Ok(Some(chunk)) => {
                if bytes.len().saturating_add(chunk.len()) > maximum {
                    return Err(AttemptFailure::Adapter(GlmAdapterError::Limit(
                        "JSON response body",
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(error) => {
                return Err(if error.is_timeout() {
                    AttemptFailure::Timeout { visible: false }
                } else {
                    AttemptFailure::Transport { visible: false }
                });
            }
        }
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| AttemptFailure::Adapter(GlmAdapterError::MalformedProtocol))
}

pub(crate) async fn classify_non_success(
    response: Response,
    cancellation: Arc<dyn CancellationSignal>,
) -> AttemptFailure {
    non_success(response, cancellation).await
}

pub(crate) async fn wait_cancelled(cancellation: Arc<dyn CancellationSignal>) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// One attempt failure carrying only classified/safe state.
#[derive(Debug)]
pub(crate) enum AttemptFailure {
    Cancelled {
        visible: bool,
    },
    Timeout {
        visible: bool,
    },
    Transport {
        visible: bool,
    },
    Http {
        status: u16,
        retry_after: Option<String>,
    },
    Incomplete(Box<AttemptState>),
    Adapter(GlmAdapterError),
    ConsumerDropped,
}
