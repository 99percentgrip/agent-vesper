// ProviderSession's shared error contract deliberately returns this DTO by value.
#![allow(clippy::result_large_err)]
use crate::{
    auth::AuthenticationMode,
    credentials::{Credentials, DispatchAuth},
    error, wire,
};
use futures_util::{StreamExt, stream};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::Instant};
use vesper_domain::{
    ContentPart, ContentText, ErrorCategory, FinishOutcome, StreamInterruptionCause,
};
use vesper_provider::*;

#[derive(Clone)]
pub struct OpenAiSession {
    credentials: Credentials,
    client: reqwest::Client,
    effort: String,
    #[cfg(feature = "integration-test-harness")]
    test_route: Option<(String, AuthenticationMode)>,
}
impl OpenAiSession {
    pub(crate) fn new(credentials: Credentials, effort: String) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("agent-vesper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                error(
                    "OpenAI transport is unavailable",
                    ErrorCategory::Transport,
                    false,
                )
            })?;
        Ok(Self {
            credentials,
            client,
            effort,
            #[cfg(feature = "integration-test-harness")]
            test_route: None,
        })
    }
    #[cfg(feature = "integration-test-harness")]
    pub(crate) fn with_test_route(mut self, route: Option<(String, AuthenticationMode)>) -> Self {
        self.test_route = route;
        self
    }
    pub(crate) async fn resolve_auth(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        refresh: bool,
    ) -> Result<DispatchAuth, CredentialError> {
        #[cfg(feature = "integration-test-harness")]
        if let Some((_, mode)) = &self.test_route {
            if refresh {
                return Err(CredentialError::Failed);
            }
            return Ok(DispatchAuth {
                mode: *mode,
                bearer: vesper_security::SecretValue::new("fixture-openai-key"),
                account: if *mode == AuthenticationMode::ChatGpt {
                    Some(vesper_security::SecretValue::new("fixture-account"))
                } else {
                    None
                },
            });
        }
        self.credentials.dispatch(cancel, refresh).await
    }
    async fn dispatch(
        &self,
        request: &ProviderRequest,
        auth: &DispatchAuth,
        cancel: &dyn CancellationSignal,
    ) -> Result<reqwest::Response, ProviderError> {
        let body = wire::request(request, auth.mode, &self.effort)?;
        let endpoint = match auth.mode {
            AuthenticationMode::ApiKey => "https://api.openai.com/v1/responses",
            AuthenticationMode::ChatGpt => "https://chatgpt.com/backend-api/codex/responses",
        };
        #[cfg(feature = "integration-test-harness")]
        let endpoint = self
            .test_route
            .as_ref()
            .map(|(url, _)| url.as_str())
            .unwrap_or(endpoint);
        let mut builder = self
            .client
            .post(endpoint)
            .bearer_auth(auth.bearer.expose().as_str())
            .header("Accept", "text/event-stream")
            .json(&body);
        if let Some(account) = &auth.account {
            builder = builder
                .header("ChatGPT-Account-Id", account.expose().as_str())
                .header("originator", "agent-vesper");
        }
        let future = builder.send();
        tokio::pin!(future);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if cancel.is_cancelled() {
                return Err(error(
                    "OpenAI request cancelled",
                    ErrorCategory::Cancellation,
                    false,
                ));
            }
            tokio::select! {biased;
                _=tokio::time::sleep_until(deadline)=>return Err(error("OpenAI response headers timed out",ErrorCategory::Transport,false)),
                _=tokio::time::sleep(Duration::from_millis(25))=>{},
                response=&mut future=>return response.map_err(|_|error("OpenAI connection failed",ErrorCategory::Transport,false)),
            }
        }
    }
}
impl ProviderSession for OpenAiSession {
    fn query_usage<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderUsage, ProviderError>> {
        Box::pin(async move {
            let operation = async {
                let mut auth = self
                    .resolve_auth(cancel.clone(), false)
                    .await
                    .map_err(|_| {
                        error(
                            "OpenAI usage authentication failed; sign in through Settings",
                            ErrorCategory::Authentication,
                            false,
                        )
                    })?;
                if auth.mode == AuthenticationMode::ApiKey {
                    return Ok(ProviderUsage { authentication: Some("API key (usage-based billing)".into()), notice: Some("Subscription limits do not apply. API project limits: https://platform.openai.com/settings/organization/limits".into()), ..Default::default() });
                }
                let endpoint = "https://chatgpt.com/backend-api/wham/usage";
                #[cfg(feature = "integration-test-harness")]
                let test_endpoint = self.test_route.as_ref().map(|(url, _)| {
                    let mut url = url::Url::parse(url).expect("validated fixture URL");
                    url.set_path("/usage");
                    url.to_string()
                });
                #[cfg(feature = "integration-test-harness")]
                let endpoint = test_endpoint.as_deref().unwrap_or(endpoint);
                for attempt in 0..2 {
                    let mut builder = self
                        .client
                        .get(endpoint)
                        .bearer_auth(auth.bearer.expose().as_str())
                        .header("originator", "agent-vesper");
                    if let Some(account) = &auth.account {
                        builder = builder.header("ChatGPT-Account-Id", account.expose().as_str());
                    }
                    let mut response = builder.send().await.map_err(|_| {
                        error(
                            "OpenAI usage connection failed",
                            ErrorCategory::Transport,
                            false,
                        )
                    })?;
                    if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                        auth = self.resolve_auth(cancel.clone(), true).await.map_err(|_| {
                            error(
                                "OpenAI usage session expired; sign in again",
                                ErrorCategory::Authentication,
                                false,
                            )
                        })?;
                        continue;
                    }
                    if !response.status().is_success() {
                        return Err(error(
                            "OpenAI usage service rejected the request; check account access",
                            ErrorCategory::Transport,
                            false,
                        ));
                    }
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response.chunk().await.map_err(|_| {
                        error(
                            "OpenAI usage response interrupted",
                            ErrorCategory::Transport,
                            false,
                        )
                    })? {
                        if bytes.len() + chunk.len() > 65_536 {
                            return Err(crate::wire::invalid());
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    let payload =
                        serde_json::from_slice(&bytes).map_err(|_| crate::wire::invalid())?;
                    return crate::usage::parse_usage(&payload);
                }
                Err(crate::wire::invalid())
            };
            tokio::select! {
                result = tokio::time::timeout(Duration::from_secs(30), operation) => result.map_err(|_| error("OpenAI usage query timed out", ErrorCategory::Transport, false))?,
                _ = async { while !cancel.is_cancelled() { tokio::time::sleep(Duration::from_millis(25)).await; } } => Err(error("OpenAI usage query cancelled", ErrorCategory::Cancellation, false)),
            }
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
            // Validate before resolving credentials or making any network call.
            wire::request(&request, AuthenticationMode::ApiKey, &self.effort)?;
            let mut auth = self
                .resolve_auth(cancel.clone(), false)
                .await
                .map_err(|_| {
                    error(
                        "OpenAI authentication required; open Settings → Providers → OpenAI",
                        ErrorCategory::Authentication,
                        false,
                    )
                })?;
            let mut response = self.dispatch(&request, &auth, cancel.as_ref()).await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED
                && auth.mode == AuthenticationMode::ChatGpt
            {
                auth = self.resolve_auth(cancel.clone(), true).await.map_err(|_| {
                    error(
                        "OpenAI subscription sign-in expired; sign in again",
                        ErrorCategory::Authentication,
                        false,
                    )
                })?;
                response = self.dispatch(&request, &auth, cancel.as_ref()).await?;
            }
            if !response.status().is_success() {
                return Err(crate::http_error::rejection(response, cancel.as_ref()).await);
            }
            let (tx, rx) = mpsc::channel(32);
            let decoder = wire::Decoder::new(&request);
            // Subscription transport has no max_output_tokens control. Enforce
            // a local visible-output byte bound, then stop the stream. This
            // cannot bound server-side hidden reasoning or billed tokens.
            let byte_limit = if auth.mode == AuthenticationMode::ChatGpt {
                request.maximum_output_tokens
            } else {
                None
            };
            tokio::spawn(drive(response, decoder, tx, cancel, byte_limit));
            Ok(Box::pin(stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            })) as ProviderEventStream)
        })
    }
}
impl AuxiliaryRequestPort for OpenAiSession {
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
                            "OpenAI auxiliary response interrupted",
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
                        Ok(decoder.finish(FinishOutcome::StreamInterrupted {
                            cause: StreamInterruptionCause::Transport,
                            tool_call_started: decoder.tool_started,
                        })),
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
                        Ok(decoder.finish(FinishOutcome::StreamInterrupted {
                            cause: StreamInterruptionCause::Transport,
                            tool_call_started: decoder.tool_started,
                        })),
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
