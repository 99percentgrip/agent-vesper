//! LM Studio runtime provider adapter (composition boundary, VRO-3.x).
//!
//! Wires the LM Studio local/LAN model server as a real runtime provider so it
//! appears in the TUI's provider selection, `/model` lists the server's loaded
//! model, and chat dispatches through the standard AgentLoop. Non-streaming
//! (the full response arrives, then yields as events); streaming is a
//! follow-up. The binary owns the `reqwest` client; no foundational crate
//! touches HTTP.

use std::sync::Arc;

use futures_util;
use futures_util::StreamExt;
use std::task::Context;
use vesper_agent::providers::lmstudio::{
    ChatMessage, LmStudioConfig, build_chat_request, build_models_request, parse_models_response,
};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ErrorCategory, ErrorInfo, ExtensionMap, FinishOutcome,
    MessageRole, ModelId, ProviderId, QualifiedModelId, RedactedDiagnostics, Retryability,
    SafeMessage,
};
use vesper_provider::{
    AuthenticationMethodDescriptor, CancellationSignal, CredentialError, ModelCatalog,
    ModelCatalogProvenance, ModelCatalogSnapshot, ModelDescriptor, ProviderCapabilities,
    ProviderConfiguration, ProviderCredentialPort, ProviderDescriptor, ProviderError,
    ProviderEventStream, ProviderFactory, ProviderFuture, ProviderRequest, ProviderSession,
    ProviderStreamEvent, ProviderSuperpowers, SuperpowerDescriptor, SuperpowerKind,
    SuperpowerScope, SuperpowerValue,
};

const ID: &str = "lmstudio";

fn pid() -> ProviderId {
    ProviderId::new(ID).expect("static provider id")
}

fn err(msg: impl Into<String>) -> ProviderError {
    let msg = msg.into();
    ProviderError {
        provider_id: pid(),
        provider_code: None,
        http_status: None,
        continuation_possible: false,
        info: ErrorInfo {
            category: ErrorCategory::Transport,
            retryability: Retryability::Never,
            retry_after_ms: None,
            visible_output_emitted: false,
            safe_message: SafeMessage::new(msg)
                .unwrap_or_else(|_| SafeMessage::new("LM Studio adapter error").expect("bounded")),
            diagnostics: RedactedDiagnostics::default(),
            provider_code: None,
            causes: Vec::new(),
        },
        metadata: ExtensionMap::default(),
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// LM Studio provider factory. Owns the `reqwest` client + the network config.
#[derive(Clone)]
pub struct LmStudioFactory {
    id: ProviderId,
    config: LmStudioConfig,
    model: String,
    client: reqwest::Client,
}

impl LmStudioFactory {
    /// Creates a factory from persisted settings + the auto-discovered (or
    /// pinned) model id.
    #[must_use]
    pub fn new(config: LmStudioConfig, model: impl Into<String>) -> Self {
        Self {
            id: pid(),
            config,
            model: model.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    /// The stable provider id string ("lmstudio").
    #[must_use]
    pub fn provider_id_str() -> &'static str {
        ID
    }

    /// Minimal default configuration (the session ignores it — it uses the
    /// internal LmStudioConfig from the settings).
    #[must_use]
    pub fn default_configuration() -> vesper_provider::ProviderConfiguration {
        use vesper_domain::{ExtensionNamespace, SchemaVersion, VersionedExtensionEnvelope};
        vesper_provider::ProviderConfiguration {
            provider_id: pid(),
            values: VersionedExtensionEnvelope {
                namespace: ExtensionNamespace::new("provider.lmstudio").expect("bounded"),
                version: SchemaVersion::new(1).expect("static schema"),
                values: ExtensionMap::default(),
            },
        }
    }

    fn req_headers(&self, headers: &[(String, String)]) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in headers {
            if let (Ok(n), Ok(v)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                map.append(n, v);
            }
        }
        map
    }
}

impl ProviderFactory for LmStudioFactory {
    type Session = LmStudioSession;

    fn provider_id(&self) -> &ProviderId {
        &self.id
    }

    fn create_session<'a>(
        &'a self,
        _config: &'a ProviderConfiguration,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        Box::pin(async move {
            Ok(LmStudioSession {
                config: self.config.clone(),
                model: self.model.clone(),
                client: self.client.clone(),
            })
        })
    }

    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: self.id.clone(),
            display_name: BoundedString::new("LM Studio").expect("bounded"),
            authentication_methods: vec![AuthenticationMethodDescriptor {
                method_id: BoundedString::new("lmstudio-api-key").expect("bounded"),
                display_name: BoundedString::new("LM Studio API key (optional)").expect("bounded"),
                secret_reference_fields: vec![
                    BoundedString::new("LMSTUDIO_API_KEY").expect("bounded"),
                ],
                external_runtime_owned: false,
                key_url: None,
            }],
            configuration: None,
            metadata: ExtensionMap::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Session (non-streaming chat)
// ---------------------------------------------------------------------------

pub struct LmStudioSession {
    config: LmStudioConfig,
    model: String,
    client: reqwest::Client,
}

impl ProviderSession for LmStudioSession {
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        let client = self.client.clone();
        let config = self.config.clone();
        let model = self.model.clone();
        Box::pin(async move {
            let messages = provider_request_to_chat_messages(&request);
            let chat_req = build_chat_request(&config, &model, &messages);

            // Override the body to enable SSE streaming.
            let mut body_json: serde_json::Value =
                serde_json::from_str(&chat_req.body.unwrap_or_default()).unwrap_or_default();
            body_json["stream"] = serde_json::json!(true);

            let resp = client
                .post(&chat_req.url)
                .headers(reqwest_header_map(&chat_req.headers))
                .body(body_json.to_string())
                .send()
                .await
                .map_err(|e| err(format!("HTTP send: {e}")))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| err(format!("HTTP body: {e}")))?;
                return Err(err(format!("HTTP {status}: {body}")));
            }

            // Spawn a task that reads the SSE byte stream and sends events
            // through an unbounded channel. The receiver is wrapped as a
            // Stream via poll_fn (no extra deps needed).
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let stream_id = BoundedString::<128>::new("content").expect("bounded");
                let _ = tx.send(Ok(ProviderStreamEvent::ResponseStarted {
                    response_id: None,
                    metadata: ExtensionMap::default(),
                }));
                let mut byte_stream = resp.bytes_stream();
                let mut buffer = String::new();
                while let Some(chunk_result) = byte_stream.next().await {
                    match chunk_result {
                        Ok(bytes) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                            while let Some(pos) = buffer.find("\n\n") {
                                let chunk = buffer[..pos].to_string();
                                buffer = buffer[pos + 2..].to_string();
                                if let Some(event) = parse_sse_chunk(&chunk, &stream_id) {
                                    let is_done =
                                        matches!(event, ProviderStreamEvent::Completed { .. });
                                    let _ = tx.send(Ok(event));
                                    if is_done {
                                        return;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(err(format!("stream read: {e}"))));
                            return;
                        }
                    }
                }
                let _ = tx.send(Ok(ProviderStreamEvent::Completed {
                    finish: FinishOutcome::Stop,
                    metadata: ExtensionMap::default(),
                }));
            });

            Ok(Box::pin(futures_util::stream::poll_fn(
                move |cx: &mut Context<'_>| rx.poll_recv(cx),
            )) as ProviderEventStream)
        })
    }
}

/// Parses one SSE chunk into a `ProviderStreamEvent`.
///
/// LM Studio streams OpenAI-compatible Server-Sent Events. For
/// reasoning-capable local models (Qwen3, DeepSeek-R1, etc.), the thinking
/// telemetry rides on `delta.reasoning_content` — the same field name GLM
/// uses. Some servers emit `delta.reasoning` instead, so we accept both. We
/// emit a `ReasoningDelta` BEFORE any `ContentDelta` from the same chunk so
/// the TUI Reasoning panel renders thinking first and the Conversation panel
/// renders the answer second, mirroring the GLM ordering.
fn parse_sse_chunk(chunk: &str, stream_id: &BoundedString<128>) -> Option<ProviderStreamEvent> {
    for line in chunk.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            let data = data.trim();
            if data == "[DONE]" {
                return Some(ProviderStreamEvent::Completed {
                    finish: FinishOutcome::Stop,
                    metadata: ExtensionMap::default(),
                });
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            let Some(delta) = json
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
            else {
                continue;
            };
            // Reasoning / thinking telemetry. Reasoning models emit the chain
            // of thought on `delta.reasoning_content` (OpenAI-compat /
            // Qwen3 / DeepSeek-R1 convention); some servers use
            // `delta.reasoning`. We check both, prefer the canonical name.
            if let Some(reasoning) = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let text = ContentText::new(reasoning.to_string())
                    .unwrap_or_else(|_| ContentText::new("(error)").expect("bounded"));
                return Some(ProviderStreamEvent::ReasoningDelta {
                    stream_id: BoundedString::new("reasoning").expect("bounded stream id"),
                    text,
                    kind: vesper_domain::ReasoningKind::ProviderVisible,
                    retention: vesper_domain::ReasoningRetention::SessionOnly,
                });
            }
            if let Some(content) = delta
                .get("content")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let text = ContentText::new(content.to_string())
                    .unwrap_or_else(|_| ContentText::new("(error)").expect("bounded"));
                return Some(ProviderStreamEvent::ContentDelta {
                    stream_id: stream_id.clone(),
                    part: ContentPart::Text(text),
                });
            }
        }
    }
    None
}

fn reqwest_header_map(headers: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            map.append(n, v);
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Model catalog (live /models discovery)
// ---------------------------------------------------------------------------

impl ModelCatalog for LmStudioFactory {
    fn models<'a>(
        &'a self,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        Box::pin(async move {
            let req = build_models_request(&self.config);
            let resp = self
                .client
                .get(&req.url)
                .headers(self.req_headers(&req.headers))
                .send()
                .await
                .map_err(|e| err(format!("/models HTTP: {e}")))?;
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| err(format!("/models body: {e}")))?;
            let discovered =
                parse_models_response(&body).map_err(|e| err(format!("/models parse: {e}")))?;
            let descriptors = discovered
                .iter()
                .map(|m| ModelDescriptor {
                    model: QualifiedModelId {
                        provider_id: self.id.clone(),
                        model_id: ModelId::new(&m.id).expect("model id is bounded"),
                    },
                    display_name: BoundedString::new(&m.id).expect("bounded"),
                    capabilities: ProviderCapabilities::default(),
                    metadata: ExtensionMap::default(),
                })
                .collect::<Vec<_>>();
            Ok(ModelCatalogSnapshot {
                models: descriptors,
                provenance: ModelCatalogProvenance::Discovered,
                expires_at_unix_ms: None,
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Superpowers (advertise the model superpower)
// ---------------------------------------------------------------------------

impl ProviderSuperpowers for LmStudioFactory {
    fn superpowers(&self) -> Vec<SuperpowerDescriptor> {
        vec![
            SuperpowerDescriptor {
                id: BoundedString::new("lmstudio:model").expect("bounded"),
                provider_id: self.id.clone(),
                display_name: BoundedString::new("Model").expect("bounded"),
                kind: SuperpowerKind::Choice,
                scope: SuperpowerScope::Session,
                default_value: SuperpowerValue::Choice {
                    value: BoundedString::new(&self.model).expect("bounded"),
                },
                allowed_values: vec![SuperpowerValue::Choice {
                    value: BoundedString::new(&self.model).expect("bounded"),
                }],
                command_alias: Some(BoundedString::new("model").expect("bounded")),
                help: Some(
                    BoundedString::new("The model loaded on the LM Studio server.")
                        .expect("bounded"),
                ),
            },
            SuperpowerDescriptor {
                id: BoundedString::new("lmstudio:reasoning").expect("bounded"),
                provider_id: self.id.clone(),
                display_name: BoundedString::new("Thinking").expect("bounded"),
                kind: SuperpowerKind::Choice,
                scope: SuperpowerScope::Session,
                default_value: SuperpowerValue::Choice {
                    value: BoundedString::new("disabled").expect("bounded"),
                },
                allowed_values: ["disabled", "enabled", "high"]
                    .into_iter()
                    .map(|v| SuperpowerValue::Choice {
                        value: BoundedString::new(v).expect("bounded"),
                    })
                    .collect(),
                command_alias: Some(BoundedString::new("thinking").expect("bounded")),
                help: Some(
                    BoundedString::new("Toggle reasoning/thinking mode for the loaded model.")
                        .expect("bounded"),
                ),
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Credential port (env-var API key)
// ---------------------------------------------------------------------------

impl ProviderCredentialPort for LmStudioFactory {
    fn credential_present(&self) -> Result<bool, CredentialError> {
        // LM Studio's API key is OPTIONAL — the server may run without auth.
        // Always report the credential as present so the TUI never blocks the
        // user with an authentication screen. The optional key (from
        // LMSTUDIO_API_KEY env var or the config) is injected per-request by
        // the transport if it is set.
        Ok(true)
    }
    fn store_credential(&self, _secret: &str) -> Result<(), CredentialError> {
        // No-op: LM Studio keys are env/config-only, not OS credential store.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ProviderRequest → OpenAI-compatible ChatMessage conversion
// ---------------------------------------------------------------------------

fn provider_request_to_chat_messages(request: &ProviderRequest) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    for sys in &request.system_instructions {
        let text = extract_text(&sys.content);
        if !text.is_empty() {
            out.push(ChatMessage {
                role: "system".into(),
                content: text,
            });
        }
    }
    for msg in &request.messages {
        let role = match msg.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            _ => "user",
        };
        let text = extract_text(&msg.content);
        if !text.is_empty() {
            out.push(ChatMessage {
                role: role.into(),
                content: text,
            });
        }
    }
    out
}

fn extract_text(parts: &[ContentPart]) -> String {
    parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text(t) => Some(t.as_str().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    #![forbid(unsafe_code)]
    use super::*;

    fn stream_id() -> BoundedString<128> {
        BoundedString::new("content").expect("bounded stream id")
    }

    #[test]
    fn parse_sse_chunk_emits_reasoning_delta_from_reasoning_content() {
        // LM Studio / Qwen3 / DeepSeek-R1 stream the chain of thought on
        // `delta.reasoning_content` — the same field name GLM uses.
        let chunk =
            r#"data: {"choices":[{"delta":{"reasoning_content":"thinking about the user"}}]}"#;
        let event = parse_sse_chunk(chunk, &stream_id()).expect("must emit");
        match event {
            ProviderStreamEvent::ReasoningDelta { text, kind, .. } => {
                assert_eq!(text.as_str(), "thinking about the user");
                assert_eq!(kind, vesper_domain::ReasoningKind::ProviderVisible);
            }
            other => panic!("expected ReasoningDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_sse_chunk_emits_reasoning_delta_from_legacy_reasoning_field() {
        // Some servers use the bare `delta.reasoning` field name.
        let chunk = r#"data: {"choices":[{"delta":{"reasoning":"alt field"}}]}"#;
        let event = parse_sse_chunk(chunk, &stream_id()).expect("must emit");
        assert!(matches!(event, ProviderStreamEvent::ReasoningDelta { .. }));
    }

    #[test]
    fn parse_sse_chunk_emits_content_delta_for_plain_answer() {
        let chunk = r#"data: {"choices":[{"delta":{"content":"answer"}}]}"#;
        let event = parse_sse_chunk(chunk, &stream_id()).expect("must emit");
        match event {
            ProviderStreamEvent::ContentDelta { .. } => {}
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_sse_chunk_emits_completed_on_done_marker() {
        let chunk = "data: [DONE]";
        let event = parse_sse_chunk(chunk, &stream_id()).expect("must emit");
        assert!(matches!(event, ProviderStreamEvent::Completed { .. }));
    }

    #[test]
    fn parse_sse_chunk_skips_empty_deltas() {
        let chunk = r#"data: {"choices":[{"delta":{}}]}"#;
        assert!(parse_sse_chunk(chunk, &stream_id()).is_none());
    }

    #[test]
    fn parse_sse_chunk_skips_malformed_json() {
        let chunk = "data: not-json-at-all";
        assert!(parse_sse_chunk(chunk, &stream_id()).is_none());
    }
}
