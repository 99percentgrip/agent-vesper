// ProviderSession's shared error contract deliberately returns this DTO by value.
#![allow(clippy::result_large_err)]
use crate::{
    credentials::{AuthenticationMode, Credentials, DispatchAuth},
    error, wire,
};
use futures_util::{SinkExt, StreamExt, stream};
use std::{sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{Mutex, OwnedMutexGuard, mpsc},
    time::Instant,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use vesper_domain::{
    ContentPart, ContentText, ErrorCategory, FinishOutcome, StreamInterruptionCause,
};
use vesper_provider::*;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum XaiTransport {
    Http,
    WebSocket,
}

type XaiWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

// The subscription proxy version-gates clients against the first-party Grok
// Build protocol revision pinned by VRO-18 reconnaissance. This is a protocol
// compatibility version, while the User-Agent and client identifier continue
// to identify Agent Vesper truthfully.
const GROK_SESSION_PROTOCOL_VERSION: &str = "1.0.41";

#[derive(Clone)]
pub struct XaiSession {
    credentials: Credentials,
    pub(crate) availability: Arc<std::sync::RwLock<Option<crate::AvailableModels>>>,
    pub(crate) client: reqwest::Client,
    effort: String,
    pub(crate) region: XaiRegion,
    pub(crate) transport: XaiTransport,
    websocket: Arc<Mutex<Option<XaiWebSocket>>>,
    #[cfg(feature = "integration-test-harness")]
    pub(crate) test_route: Option<String>,
    #[cfg(feature = "integration-test-harness")]
    pub(crate) test_auth_mode: AuthenticationMode,
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
            transport: XaiTransport::Http,
            websocket: Arc::new(Mutex::new(None)),
            #[cfg(feature = "integration-test-harness")]
            test_route: None,
            #[cfg(feature = "integration-test-harness")]
            test_auth_mode: AuthenticationMode::ApiKey,
        })
    }
    pub(crate) fn with_transport(mut self, transport: XaiTransport) -> Self {
        self.transport = transport;
        self
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
    #[cfg(feature = "integration-test-harness")]
    pub(crate) fn with_test_auth_mode(mut self, mode: AuthenticationMode) -> Self {
        self.test_auth_mode = mode;
        self
    }
    pub(crate) async fn resolve_auth(
        &self,
        refresh: bool,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<DispatchAuth, CredentialError> {
        #[cfg(feature = "integration-test-harness")]
        if self.test_route.is_some() {
            return Ok(DispatchAuth {
                mode: self.test_auth_mode,
                bearer: vesper_security::SecretValue::new("fixture-xai-key"),
            });
        }
        self.credentials.dispatch(cancel, refresh).await
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
        auth: &DispatchAuth,
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
        if !request.hosted_tools.is_empty()
            && (auth.mode != AuthenticationMode::ApiKey || self.region != XaiRegion::Global)
        {
            return Err(error(
                "xAI provider-hosted tools are verified only for Global API-key mode",
                ErrorCategory::UnsupportedCapability,
                false,
            ));
        }
        if auth.mode == AuthenticationMode::GrokSession && self.region != XaiRegion::Global {
            return Err(error(
                "Grok account sessions use the global Grok subscription endpoint; choose Global or explicitly switch to API-key billing",
                ErrorCategory::InvalidRequest,
                false,
            ));
        }
        let endpoint = match (auth.mode, self.region) {
            (AuthenticationMode::GrokSession, _) => "https://cli-chat-proxy.grok.com/v1/responses",
            (AuthenticationMode::ApiKey, XaiRegion::Global) => "https://api.x.ai/v1/responses",
            (AuthenticationMode::ApiKey, XaiRegion::Us) => "https://us.api.x.ai/v1/responses",
        };
        #[cfg(feature = "integration-test-harness")]
        let endpoint = self.test_route.as_deref().unwrap_or(endpoint);
        let mut builder = self
            .client
            .post(endpoint)
            .bearer_auth(auth.bearer.expose().as_str())
            .header("Accept", "text/event-stream")
            .json(&body);
        if let Some(conversation_id) = request.cache_routing_key.as_ref() {
            builder = builder.header("x-grok-conv-id", conversation_id.as_str());
        }
        if auth.mode == AuthenticationMode::GrokSession {
            let request_id = request.request_id.as_str();
            let conversation_id = request
                .cache_routing_key
                .as_ref()
                .map(vesper_domain::BoundedString::as_str)
                .or_else(|| {
                    request
                        .provider_extensions
                        .as_ref()
                        .and_then(|extension| extension.values.get("xai:prompt-cache-key"))
                        .and_then(serde_json::Value::as_str)
                })
                .unwrap_or(request_id);
            builder = builder
                .header("X-XAI-Token-Auth", "xai-grok-cli")
                .header("x-authenticateresponse", "authenticate-response")
                .header("x-grok-client-version", GROK_SESSION_PROTOCOL_VERSION)
                .header("x-grok-client-identifier", "agent-vesper")
                .header("x-grok-client-mode", "interactive")
                .header("x-grok-conv-id", conversation_id)
                .header("x-grok-req-id", request_id)
                .header("x-grok-session-id", conversation_id)
                .header("x-grok-agent-id", "agent-vesper")
                .header("x-grok-model-override", request.model.model_id.as_str());
        }
        let future = builder.send();
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

    async fn start_websocket(
        &self,
        request: &ProviderRequest,
        auth: &DispatchAuth,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<Option<ProviderEventStream>, ProviderError> {
        if auth.mode != AuthenticationMode::ApiKey || self.region != XaiRegion::Global {
            return Err(error(
                "xAI WebSocket mode is verified only for Global API-key authentication",
                ErrorCategory::UnsupportedCapability,
                false,
            ));
        }
        let mut body = wire::request_websocket(request, &self.effort)?;
        body["type"] = serde_json::json!("response.create");
        let mut guard = Arc::clone(&self.websocket).lock_owned().await;
        if guard.is_none() {
            let endpoint = {
                #[cfg(feature = "integration-test-harness")]
                {
                    if let Some(route) = &self.test_route {
                        let mut url = url::Url::parse(route).map_err(|_| wire::invalid())?;
                        url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
                            .map_err(|_| wire::invalid())?;
                        url.to_string()
                    } else {
                        "wss://api.x.ai/v1/responses".to_owned()
                    }
                }
                #[cfg(not(feature = "integration-test-harness"))]
                {
                    "wss://api.x.ai/v1/responses".to_owned()
                }
            };
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            let mut upgrade = endpoint
                .into_client_request()
                .map_err(|_| wire::invalid())?;
            let authorization = format!("Bearer {}", auth.bearer.expose().as_str());
            upgrade.headers_mut().insert(
                tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
                authorization.parse().map_err(|_| wire::invalid())?,
            );
            let connecting = tokio_tungstenite::connect_async(upgrade);
            tokio::pin!(connecting);
            let deadline = Instant::now() + Duration::from_secs(10);
            let socket = loop {
                if cancel.is_cancelled() {
                    return Err(error(
                        "xAI WebSocket connection cancelled",
                        ErrorCategory::Cancellation,
                        false,
                    ));
                }
                let result = tokio::select! { biased;
                    _ = tokio::time::sleep_until(deadline) => return Ok(None),
                    _ = tokio::time::sleep(Duration::from_millis(25)) => continue,
                    result = &mut connecting => result,
                };
                match result {
                    Ok((socket, _)) => break socket,
                    Err(_) => return Ok(None),
                }
            };
            *guard = Some(socket);
        }
        let payload = serde_json::to_string(&body).map_err(|_| wire::invalid())?;
        if let Err(_error) = guard
            .as_mut()
            .expect("socket initialized")
            .send(Message::Text(payload.into()))
            .await
        {
            *guard = None;
            return Err(error(
                "xAI WebSocket request could not be sent",
                ErrorCategory::Transport,
                false,
            ));
        }
        let (tx, rx) = mpsc::channel(32);
        let decoder = wire::Decoder::new(request);
        tokio::spawn(drive_websocket(guard, decoder, tx, cancel));
        Ok(Some(Box::pin(stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })) as ProviderEventStream))
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
            let authentication = match self.credentials.authentication_method() {
                Ok(Some(method)) if method == "xai-grok-session" => {
                    "Grok account / SuperGrok (account allowance)"
                }
                _ => "xAI API key (usage-based API billing)",
            };
            Ok(ProviderUsage { authentication: Some(authentication.into()), notice: Some("Account allowance/balance is unavailable through the verified protocol; per-response token usage remains available.".into()), ..Default::default() })
        })
    }
    fn auxiliary(&self) -> Option<&dyn AuxiliaryRequestPort> {
        Some(self)
    }
    fn native_compaction(&self) -> Option<&dyn NativeCompactionPort> {
        Some(self)
    }
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        Box::pin(async move {
            // Validate before credential resolution or network access.
            if self.transport == XaiTransport::WebSocket {
                wire::request_websocket(&request, &self.effort)?;
            } else {
                wire::request(&request, &self.effort)?;
            }
            let mut auth = self
                .resolve_auth(false, cancel.clone())
                .await
                .map_err(|_| {
                    error(
                        "xAI authentication required; open Settings → Providers → xAI / Grok",
                        ErrorCategory::Authentication,
                        false,
                    )
                })?;
            if self.transport == XaiTransport::WebSocket
                && let Some(stream) = self
                    .start_websocket(&request, &auth, cancel.clone())
                    .await?
            {
                return Ok(stream);
            }
            let mut response = self.dispatch(&request, &auth, cancel.as_ref()).await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED
                && auth.mode == AuthenticationMode::GrokSession
            {
                auth = self.resolve_auth(true, cancel.clone()).await.map_err(|_| {
                    error(
                        "Grok session refresh failed; sign in again",
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
            tokio::spawn(drive(response, decoder, tx, cancel, None));
            Ok(Box::pin(stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            })) as ProviderEventStream)
        })
    }
}

impl NativeCompactionPort for XaiSession {
    fn compact_native<'a>(
        &'a self,
        request: NativeCompactionRequest,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<NativeCompactionResult, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(error(
                    "xAI compaction cancelled",
                    ErrorCategory::Cancellation,
                    false,
                ));
            }
            let body = wire::compaction_request(&request)?;
            let auth = self
                .resolve_auth(false, cancel.clone())
                .await
                .map_err(|_| {
                    error(
                        "xAI authentication required for native compaction",
                        ErrorCategory::Authentication,
                        false,
                    )
                })?;
            if auth.mode != AuthenticationMode::ApiKey || self.region != XaiRegion::Global {
                return Err(error(
                    "xAI native compaction is verified only for Global API-key mode",
                    ErrorCategory::UnsupportedCapability,
                    false,
                ));
            }
            let endpoint = {
                #[cfg(feature = "integration-test-harness")]
                if let Some(route) = &self.test_route {
                    let mut url = url::Url::parse(route).map_err(|_| wire::invalid())?;
                    url.set_path("/responses/compact");
                    url.set_query(None);
                    url.to_string()
                } else {
                    "https://api.x.ai/v1/responses/compact".to_owned()
                }
                #[cfg(not(feature = "integration-test-harness"))]
                "https://api.x.ai/v1/responses/compact".to_owned()
            };
            let response = self
                .client
                .post(endpoint)
                .bearer_auth(auth.bearer.expose().as_str())
                .header("Accept", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|_| {
                    error(
                        "xAI compaction connection failed",
                        ErrorCategory::Transport,
                        false,
                    )
                })?;
            if !response.status().is_success() {
                return Err(crate::http_error::rejection(response, cancel.as_ref()).await);
            }
            if response
                .content_length()
                .is_some_and(|length| length > (2 * wire::MAX_EVENT) as u64)
            {
                return Err(wire::invalid());
            }
            let mut chunks = response.bytes_stream();
            let deadline = Instant::now() + Duration::from_secs(60);
            let mut bytes = Vec::new();
            loop {
                if cancel.is_cancelled() {
                    return Err(error(
                        "xAI compaction cancelled",
                        ErrorCategory::Cancellation,
                        false,
                    ));
                }
                let chunk = tokio::select! { biased;
                    _ = tokio::time::sleep_until(deadline) => return Err(error("xAI compaction timed out", ErrorCategory::Transport, false)),
                    _ = tokio::time::sleep(Duration::from_millis(25)) => continue,
                    chunk = chunks.next() => chunk,
                };
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|_| wire::invalid())?;
                if bytes.len().saturating_add(chunk.len()) > 2 * wire::MAX_EVENT {
                    return Err(wire::invalid());
                }
                bytes.extend_from_slice(&chunk);
            }
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| wire::invalid())?;
            if value.get("object").and_then(serde_json::Value::as_str)
                != Some("response.compaction")
            {
                return Err(wire::invalid());
            }
            let output = value
                .get("output")
                .and_then(serde_json::Value::as_array)
                .filter(|output| output.len() == 1)
                .and_then(|output| output.first())
                .filter(|item| {
                    item.get("type").and_then(serde_json::Value::as_str) == Some("compaction")
                        && item
                            .get("encrypted_content")
                            .and_then(serde_json::Value::as_str)
                            .is_some()
                })
                .ok_or_else(wire::invalid)?;
            let usage = value.get("usage").ok_or_else(wire::invalid)?;
            let measure = |key: &str| {
                usage
                    .get(key)
                    .and_then(serde_json::Value::as_u64)
                    .map(vesper_domain::UsageMeasurement::exact)
                    .unwrap_or_else(vesper_domain::UsageMeasurement::unavailable)
            };
            let mut normalized =
                vesper_domain::NormalizedUsage::unavailable(vesper_domain::UsageMode::Cumulative);
            normalized.input = measure("input_tokens");
            normalized.output = measure("output_tokens");
            normalized.total = measure("total_tokens");
            normalized.cached_input = usage
                .pointer("/input_tokens_details/cached_tokens")
                .and_then(serde_json::Value::as_u64)
                .map(vesper_domain::UsageMeasurement::exact)
                .unwrap_or_else(vesper_domain::UsageMeasurement::unavailable);
            normalized.reasoning = usage
                .pointer("/output_tokens_details/reasoning_tokens")
                .and_then(serde_json::Value::as_u64)
                .map(vesper_domain::UsageMeasurement::exact)
                .unwrap_or_else(vesper_domain::UsageMeasurement::unavailable);
            Ok(NativeCompactionResult {
                item: vesper_domain::OpaqueContent {
                    provider_id: crate::provider_id(),
                    kind: "compaction".into(),
                    data: vesper_domain::OpaqueProviderData::new(output.clone())
                        .map_err(|_| wire::invalid())?,
                },
                usage: normalized,
                dropped_message_count: usage
                    .get("dropped_message_count")
                    .and_then(serde_json::Value::as_u64),
            })
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
            request.hosted_tools.clear();
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

async fn drive_websocket(
    mut guard: OwnedMutexGuard<Option<XaiWebSocket>>,
    mut decoder: wire::Decoder,
    tx: mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
    cancel: Arc<dyn CancellationSignal>,
) {
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    let mut inactive = Instant::now() + Duration::from_secs(90);
    loop {
        if cancel.is_cancelled() {
            *guard = None;
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                tx.send(Ok(decoder.finish(FinishOutcome::Cancelled))),
            )
            .await;
            return;
        }
        let message = tokio::select! { biased;
            _ = tx.closed() => return,
            _ = tokio::time::sleep_until(deadline) => {
                *guard = None;
                let _ = tx.send(Ok(decoder.finish(FinishOutcome::StreamInterrupted { cause: StreamInterruptionCause::GenerationDeadline, tool_call_started: decoder.tool_started }))).await;
                return;
            },
            _ = tokio::time::sleep_until(inactive) => {
                *guard = None;
                let _ = tx.send(Ok(decoder.finish(FinishOutcome::StreamInterrupted { cause: StreamInterruptionCause::ReadInactivity, tool_call_started: decoder.tool_started }))).await;
                return;
            },
            _ = tokio::time::sleep(Duration::from_millis(25)) => continue,
            message = guard.as_mut().expect("socket held").next() => message,
        };
        inactive = Instant::now() + Duration::from_secs(90);
        let value = match message {
            Some(Ok(Message::Text(text))) if text.len() <= wire::MAX_EVENT => {
                serde_json::from_str(text.as_str()).map_err(|_| wire::invalid())
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
            Some(Ok(Message::Close(_))) | None => Err(error(
                "xAI WebSocket closed before a terminal event",
                ErrorCategory::Transport,
                decoder.visible,
            )),
            Some(Ok(Message::Binary(_))) | Some(Err(_)) | Some(Ok(Message::Text(_))) => {
                Err(wire::invalid())
            }
        };
        let events = value.and_then(|value| decoder.event(value));
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
                if decoder.terminal {
                    return;
                }
            }
            Err(mut error) => {
                *guard = None;
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
    }
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
