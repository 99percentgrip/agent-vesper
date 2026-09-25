// ProviderSession's shared error contract deliberately returns this DTO by value.
#![allow(clippy::result_large_err)]
use crate::{credentials::Credentials, error, wire};
use futures_util::{StreamExt, stream};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::Instant};
use vesper_domain::{
    ContentPart, ContentText, ErrorCategory, FinishOutcome, StreamInterruptionCause,
};
use vesper_provider::*;
use vesper_security::SecretValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum XaiRegion {
    Global,
    Us,
}
impl XaiRegion {
    pub(crate) fn supports(self, model: &str) -> bool {
        self == Self::Global || matches!(model, "grok-4.7" | "grok-4.6")
    }
}

#[derive(Clone)]
pub struct XaiSession {
    credentials: Credentials,
    pub(crate) availability: Arc<std::sync::RwLock<Option<crate::AvailableModels>>>,
    pub(crate) client: reqwest::Client,
    effort: String,
    pub(crate) region: XaiRegion,
    #[cfg(feature = "integration-test-harness")]
    pub(crate) test_route: Option<String>,
}
impl XaiSession {
    pub(crate) fn new(
        credentials: Credentials,
        effort: String,
        region: XaiRegion,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("agent-vesper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                error(
                    "xAI transport is unavailable",
                    ErrorCategory::Transport,
                    false,
                )
            })?;
        Ok(Self {
            credentials,
            availability: Default::default(),
            client,
            effort,
            region,
            #[cfg(feature = "integration-test-harness")]
            test_route: None,
        })
    }
    pub(crate) fn with_availability(
        mut self,
        availability: Arc<std::sync::RwLock<Option<crate::AvailableModels>>>,
    ) -> Self {
        self.availability = availability;
        self
    }
    #[cfg(feature = "integration-test-harness")]
    pub(crate) fn with_test_route(mut self, route: Option<String>) -> Self {
        self.test_route = route;
        self
    }
    pub(crate) async fn resolve_auth(&self) -> Result<SecretValue, CredentialError> {
        #[cfg(feature = "integration-test-harness")]
        if self.test_route.is_some() {
            return Ok(SecretValue::new("fixture-xai-key"));
        }
        self.credentials.load()
    }
    pub(crate) fn validate_availability(
        &self,
        model: &str,
        fixture_route: bool,
    ) -> Result<(), ProviderError> {
        if fixture_route {
            return Ok(());
        }
        let available = self.availability.read().map_err(|_| wire::invalid())?;
        if available
            .as_ref()
            .is_some_and(|models| models.contains(model))
        {
            Ok(())
        } else {
            Err(error(
                "Selected xAI model is not in the current verified account model list; reopen Settings and choose an available model",
                ErrorCategory::InvalidRequest,
                false,
            ))
        }
    }
    async fn dispatch(
        &self,
        request: &ProviderRequest,
        key: &SecretValue,
        cancel: &dyn CancellationSignal,
    ) -> Result<reqwest::Response, ProviderError> {
        let fixture_route = {
            #[cfg(feature = "integration-test-harness")]
            {
                self.test_route.is_some()
            }
            #[cfg(not(feature = "integration-test-harness"))]
            {
                false
            }
        };
        self.validate_availability(request.model.model_id.as_str(), fixture_route)?;
        let body = wire::request(request, &self.effort)?;
        let endpoint = match self.region {
            XaiRegion::Global => "https://api.x.ai/v1/responses",
            XaiRegion::Us => "https://us.api.x.ai/v1/responses",
        };
        #[cfg(feature = "integration-test-harness")]
        let endpoint = self.test_route.as_deref().unwrap_or(endpoint);
        let future = self
            .client
            .post(endpoint)
            .bearer_auth(key.expose().as_str())
            .header("Accept", "text/event-stream")
            .json(&body)
            .send();
        tokio::pin!(future);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if cancel.is_cancelled() {
                return Err(error(
                    "xAI request cancelled",
                    ErrorCategory::Cancellation,
                    false,
                ));
            }
            tokio::select! { biased;
                _ = tokio::time::sleep_until(deadline) => return Err(error("xAI response headers timed out", ErrorCategory::Transport, false)),
                _ = tokio::time::sleep(Duration::from_millis(25)) => {},
                response = &mut future => return response.map_err(|_| error("xAI connection failed", ErrorCategory::Transport, false)),
            }
        }
    }
}
impl ProviderSession for XaiSession {
    fn query_usage<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderUsage, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(error(
                    "xAI usage query cancelled",
                    ErrorCategory::Cancellation,
                    false,
                ));
            }
            Ok(ProviderUsage { authentication: Some("xAI API key (usage-based API billing)".into()), notice: Some("Account balance is unavailable through the verified inference protocol; per-response token usage remains available.".into()), ..Default::default() })
        })
    }
    fn auxiliary(&self) -> Option<&dyn AuxiliaryRequestPort> {
        Some(self)
    }
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        Box::pin(async move {
            // Validate before credential resolution or network access.
            wire::request(&request, &self.effort)?;
            let key = self.resolve_auth().await.map_err(|_| {
                error(
                    "xAI API-key authentication required; open Settings → Providers → xAI / Grok",
                    ErrorCategory::Authentication,
                    false,
                )
            })?;
            let response = self.dispatch(&request, &key, cancel.as_ref()).await?;
            if !response.status().is_success() {
                return Err(crate::http_error::rejection(response, cancel.as_ref()).await);
            }
            let (tx, rx) = mpsc::channel(32);
            let decoder = wire::Decoder::new(&request);
            tokio::spawn(drive(response, decoder, tx, cancel, None));
            Ok(Box::pin(stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            })) as ProviderEventStream)
        })
    }
}
impl AuxiliaryRequestPort for XaiSession {
    fn execute_auxiliary<'a>(
        &'a self,
        _intent: AuxiliaryRequestIntent,
        mut request: ProviderRequest,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ContentPart, ProviderError>> {
        Box::pin(async move {
            request.tools.clear();
            request.tool_choice = vesper_domain::ToolChoiceIntent::None;
            let mut stream = self.start(request, cancel).await?;
            let mut text = String::new();
            let mut completed = false;
            while let Some(event) = stream.next().await {
                match event? {
                    ProviderStreamEvent::ContentDelta {
                        part: ContentPart::Text(delta),
                        ..
                    } => {
                        if text.len() + delta.as_str().len() > wire::MAX_EVENT {
                            return Err(wire::invalid());
                        }
                        text.push_str(delta.as_str());
                    }
                    ProviderStreamEvent::Completed {
                        finish: FinishOutcome::Stop | FinishOutcome::OutputLimit,
                        ..
                    } => completed = true,
                    ProviderStreamEvent::Completed { .. } => {
                        return Err(error(
                            "xAI auxiliary response interrupted",
                            ErrorCategory::Transport,
                            !text.is_empty(),
                        ));
                    }
                    _ => {}
                }
            }
            if !completed || text.is_empty() {
                return Err(wire::invalid());
            }
            Ok(ContentPart::Text(
                ContentText::new(text).map_err(|_| wire::invalid())?,
            ))
        })
    }
}
async fn drive(
    mut response: reqwest::Response,
    mut decoder: wire::Decoder,
    tx: mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
    cancel: Arc<dyn CancellationSignal>,
    byte_limit: Option<u64>,
) {
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    let mut inactive = Instant::now() + Duration::from_secs(90);
    let mut buffer = Vec::new();
    let mut data = String::new();
    let finish = loop {
        if cancel.is_cancelled() {
            break FinishOutcome::Cancelled;
        }
        let chunk = tokio::select! {biased;
            _=tx.closed()=>return,
            _=tokio::time::sleep_until(deadline)=>break FinishOutcome::StreamInterrupted{cause:StreamInterruptionCause::GenerationDeadline,tool_call_started:decoder.tool_started},
            _=tokio::time::sleep_until(inactive)=>break FinishOutcome::StreamInterrupted{cause:StreamInterruptionCause::ReadInactivity,tool_call_started:decoder.tool_started},
            _=tokio::time::sleep(Duration::from_millis(25))=>continue,
            chunk=response.chunk()=>chunk,
        };
        let chunk = match chunk {
            Ok(Some(chunk)) => chunk,
            Ok(None) => {
                break FinishOutcome::StreamInterrupted {
                    cause: StreamInterruptionCause::RemoteEof,
                    tool_call_started: decoder.tool_started,
                };
            }
            Err(_) => {
                break FinishOutcome::StreamInterrupted {
                    cause: StreamInterruptionCause::Transport,
                    tool_call_started: decoder.tool_started,
                };
            }
        };
        inactive = Instant::now() + Duration::from_secs(90);
        for byte in chunk {
            if byte != b'\n' {
                buffer.push(byte);
                if buffer.len() > wire::MAX_EVENT {
                    let _ = send(
                        &tx,
                        Ok(decoder.finish(FinishOutcome::ProtocolError)),
                        deadline,
                        cancel.as_ref(),
                    )
                    .await;
                    return;
                }
                continue;
            }
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            let line = match std::str::from_utf8(&buffer) {
                Ok(line) => line,
                Err(_) => {
                    let _ = send(
                        &tx,
                        Ok(decoder.finish(FinishOutcome::ProtocolError)),
                        deadline,
                        cancel.as_ref(),
                    )
                    .await;
                    return;
                }
            };
            if line.is_empty() && !data.is_empty() {
                let value = serde_json::from_str(data.trim_end());
                data.clear();
                let events = value
                    .map_err(|_| wire::invalid())
                    .and_then(|v| decoder.event(v));
                match events {
                    Ok(events) => {
                        for event in events {
                            if send(&tx, Ok(event), deadline, cancel.as_ref())
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(mut error) => {
                        error.info.visible_output_emitted = decoder.visible;
                        let event = if decoder.visible || decoder.tool_started {
                            Ok(decoder.finish(FinishOutcome::StreamInterrupted {
                                cause: StreamInterruptionCause::Transport,
                                tool_call_started: decoder.tool_started,
                            }))
                        } else {
                            Err(error)
                        };
                        let _ = send(&tx, event, deadline, cancel.as_ref()).await;
                        return;
                    }
                }
                if decoder.terminal {
                    return;
                }
                if byte_limit.is_some_and(|limit| decoder.output_bytes >= limit) {
                    let _ = send(
                        &tx,
                        Ok(decoder.finish(FinishOutcome::OutputLimit)),
                        deadline,
                        cancel.as_ref(),
                    )
                    .await;
                    return;
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                let value = value.strip_prefix(' ').unwrap_or(value);
                if data.len() + value.len() + 1 > wire::MAX_EVENT {
                    let _ = send(
                        &tx,
                        Ok(decoder.finish(FinishOutcome::ProtocolError)),
                        deadline,
                        cancel.as_ref(),
                    )
                    .await;
                    return;
                }
                data.push_str(value);
                data.push('\n');
            }
            buffer.clear();
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), tx.send(Ok(decoder.finish(finish)))).await;
}
async fn send(
    tx: &mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
    event: Result<ProviderStreamEvent, ProviderError>,
    deadline: Instant,
    cancel: &dyn CancellationSignal,
) -> Result<(), ()> {
    let sending = tx.send(event);
    tokio::pin!(sending);
    loop {
        if cancel.is_cancelled() {
            return Err(());
        }
        tokio::select! {biased;_=tokio::time::sleep_until(deadline)=>return Err(()),_=tokio::time::sleep(Duration::from_millis(25))=>{},result=&mut sending=>return result.map_err(|_|())}
    }
}
