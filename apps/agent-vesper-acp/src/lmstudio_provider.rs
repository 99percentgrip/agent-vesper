//! LM Studio runtime provider adapter for the ACP composition boundary.
//!
//! Mirrors the TUI's `lmstudio_provider.rs`: wires the LM Studio local/LAN
//! model server as a real runtime provider so it appears in the ACP provider
//! picker and chat dispatches through the standard AgentLoop. The binary owns
//! the `reqwest` client; no foundational crate touches HTTP.

use std::sync::Arc;
use std::task::Context;

use futures_util::StreamExt;
use vesper_agent::providers::lmstudio::{ChatMessage, LmStudioConfig, build_chat_request};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ErrorCategory, ErrorInfo, ExtensionMap, FinishOutcome,
    MessageRole, ModelId, ProviderId, QualifiedModelId, RedactedDiagnostics, Retryability,
    SafeMessage,
};
use vesper_provider::{
    AuthenticationMethodDescriptor, CancellationSignal, CredentialError, MediaCapability,
    ModelCatalog, ModelCatalogProvenance, ModelCatalogSnapshot, ModelDescriptor, ModelLimits,
    ProviderCapabilities, ProviderConfiguration, ProviderCredentialPort, ProviderDescriptor,
    ProviderError, ProviderEventStream, ProviderFactory, ProviderFuture, ProviderRequest,
    ProviderSession, ProviderStreamEvent, ProviderSuperpowers, ReasoningCapability,
    SuperpowerDescriptor, SuperpowerKind, SuperpowerScope, SuperpowerValue, SupportLevel,
    ToolCapability,
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

/// LM Studio provider factory. Owns the `reqwest` client + the network config.
#[derive(Clone)]
pub(crate) struct LmStudioFactory {
    id: ProviderId,
    config: LmStudioConfig,
    model: String,
    client: reqwest::Client,
    /// Shared native-catalog cache (PRD provider-capability-gating P5):
    /// filled by `refresh_catalog`/`ModelCatalog::models` from the verified
    /// `GET /api/v1/models` schema; the composition boundary reads it so
    /// advertised footer controls derive from live model data. `None` ⇒
    /// fail-closed surface. Clones share the same cache handle.
    catalog: std::sync::Arc<std::sync::RwLock<Option<ModelCatalogSnapshot>>>,
}

impl LmStudioFactory {
    /// Creates a factory from persisted settings + the auto-discovered (or
    /// pinned) model id.
    #[must_use]
    pub(crate) fn new(config: LmStudioConfig, model: impl Into<String>) -> Self {
        Self {
            id: pid(),
            config,
            model: model.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            catalog: std::sync::Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// The stable provider id string ("lmstudio").
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn provider_id_str() -> &'static str {
        ID
    }

    /// The pinned/acting local model id from the persisted settings.
    #[must_use]
    pub(crate) fn pinned_model(&self) -> &str {
        &self.model
    }

    /// The cached native-catalog snapshot, when one has been fetched.
    #[must_use]
    pub(crate) fn cached_snapshot(&self) -> Option<ModelCatalogSnapshot> {
        self.catalog.read().expect("catalog lock poisoned").clone()
    }

    /// Fetches LM Studio's native model catalog (`GET /api/v1/models`) and
    /// refreshes the shared cache. Verified response schema: LM Studio
    /// developer docs `1_developer/2_rest/list.md` (capabilities: vision /
    /// trained_for_tool_use / reasoning.allowed_options; max_context_length)
    /// — evidence recorded in PRD provider-capability-gating P5. Best-effort
    /// at startup: errors leave the previous cache (fail-closed when none).
    pub(crate) async fn refresh_catalog(&self) -> Result<ModelCatalogSnapshot, String> {
        let url = native_models_url(&self.config.api_base_url);
        let mut request = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default()
            .get(&url);
        if let Some(key) = self.config.api_key.as_ref() {
            request = request.bearer_auth(key.secret());
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("/api/v1/models HTTP: {error}"))?;
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("/api/v1/models body: {error}"))?;
        if !status.is_success() {
            return Err(format!("/api/v1/models HTTP {status}"));
        }
        let snapshot =
            snapshot_from_native(&body, &self.id).map_err(|error| format!("parse: {error}"))?;
        *self.catalog.write().expect("catalog lock poisoned") = Some(snapshot.clone());
        Ok(snapshot)
    }

    /// Minimal default configuration (the session ignores it — it uses the
    /// internal LmStudioConfig from the settings).
    #[must_use]
    pub(crate) fn default_configuration() -> ProviderConfiguration {
        use vesper_domain::{ExtensionNamespace, SchemaVersion, VersionedExtensionEnvelope};
        ProviderConfiguration {
            provider_id: pid(),
            values: VersionedExtensionEnvelope {
                namespace: ExtensionNamespace::new("provider.lmstudio").expect("bounded"),
                version: SchemaVersion::new(1).expect("static schema"),
                values: ExtensionMap::default(),
            },
        }
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

pub(crate) struct LmStudioSession {
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
/// uses. Some servers emit `delta.reasoning` instead, so we accept both.
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

impl ModelCatalog for LmStudioFactory {
    fn models<'a>(
        &'a self,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        Box::pin(async move { self.refresh_catalog().await.map_err(err) })
    }
}

// ---------------------------------------------------------------------------
// Native /api/v1/models catalog mapping (PRD provider-capability-gating P5).
// Mirrors the TUI adapter copy: verified schema, fail-closed capabilities.
// ---------------------------------------------------------------------------

/// Derives the native models URL from the OpenAI-compat base (typically
/// `http://host:1234/v1`): strips a trailing `/v1` or `/api/v0` segment and
/// appends the verified native path.
fn native_models_url(api_base_url: &str) -> String {
    let trimmed = api_base_url.trim_end_matches('/');
    let root = trimmed
        .strip_suffix("/v1")
        .or_else(|| trimmed.strip_suffix("/api/v0"))
        .unwrap_or(trimmed);
    format!("{root}/api/v1/models")
}

/// Builds a model's typed capabilities from one verified native entry.
/// Every unreported field stays `Unknown` (fail-closed); reported-absent
/// capabilities become `Unsupported` with the adapter's own reason.
fn capabilities_from_native(entry: &serde_json::Value) -> ProviderCapabilities {
    let mut capabilities = ProviderCapabilities::default();
    if let Some(context) = entry.get("max_context_length").and_then(|v| v.as_u64()) {
        capabilities.limits = SupportLevel::Native {
            details: ModelLimits {
                context_tokens: Some(context),
                output_tokens: None,
                exact: true,
            },
        };
    }
    let Some(reported) = entry.get("capabilities") else {
        return capabilities;
    };
    match reported.get("vision").and_then(|v| v.as_bool()) {
        Some(true) => {
            capabilities.vision = SupportLevel::Native {
                details: MediaCapability {
                    media_types: vec!["image/png".into(), "image/jpeg".into(), "image/webp".into()],
                    maximum_items: None,
                    references: false,
                    inline_data: true,
                },
            };
        }
        Some(false) => {
            capabilities.vision = SupportLevel::Unsupported {
                reason: BoundedString::new("model does not support image inputs")
                    .expect("bounded reason"),
            };
        }
        None => {}
    }
    match reported
        .get("trained_for_tool_use")
        .and_then(|v| v.as_bool())
    {
        Some(true) => {
            capabilities.tools = SupportLevel::Native {
                details: ToolCapability {
                    schema_dialect: "lmstudio.openai-chat-completions.tools-v1".into(),
                    choice_modes: Vec::new(),
                    parallel: false,
                    streamed_arguments: false,
                },
            };
        }
        Some(false) => {
            capabilities.tools = SupportLevel::Unsupported {
                reason: BoundedString::new("model was not trained for tool use")
                    .expect("bounded reason"),
            };
        }
        None => {}
    }
    if let Some(options) = reported
        .get("reasoning")
        .and_then(|r| r.get("allowed_options"))
        .and_then(|v| v.as_array())
    {
        let effort_levels: Vec<String> = options
            .iter()
            .filter_map(|option| option.as_str().map(str::to_string))
            .collect();
        if !effort_levels.is_empty() {
            capabilities.reasoning = SupportLevel::Native {
                details: ReasoningCapability {
                    effort_levels,
                    visible_modes: vec!["provider-visible".into()],
                },
            };
        }
    }
    capabilities
}

/// Parses a verified native `GET /api/v1/models` body into a catalog
/// snapshot. Embedding models are skipped (they are not chat models).
fn snapshot_from_native(
    body: &serde_json::Value,
    provider: &ProviderId,
) -> Result<ModelCatalogSnapshot, String> {
    let entries = body
        .get("models")
        .and_then(|m| m.as_array())
        .ok_or_else(|| "response missing `models` array".to_string())?;
    let mut models = Vec::new();
    for entry in entries {
        if entry.get("type").and_then(|t| t.as_str()) != Some("llm") {
            continue;
        }
        let key = entry
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| "model entry missing `key`".to_string())?;
        let display = entry
            .get("display_name")
            .and_then(|d| d.as_str())
            .unwrap_or(key);
        models.push(ModelDescriptor {
            model: QualifiedModelId {
                provider_id: provider.clone(),
                model_id: ModelId::new(key).map_err(|e| format!("model id `{key}`: {e}"))?,
            },
            display_name: BoundedString::new(display)
                .map_err(|_| format!("display name too long for `{key}`"))?,
            capabilities: capabilities_from_native(entry),
            metadata: ExtensionMap::default(),
        });
    }
    Ok(ModelCatalogSnapshot {
        models,
        provenance: ModelCatalogProvenance::Discovered,
        expires_at_unix_ms: None,
    })
}

impl ProviderSuperpowers for LmStudioFactory {
    fn superpowers(&self) -> Vec<SuperpowerDescriptor> {
        // PRD P5: advertised controls derive from the cached native catalog.
        // No cache ⇒ only the pinned-model selector; a thinking dial is
        // advertised ONLY when the pinned model reports reasoning options
        // (verified `reasoning.allowed_options`). The former unconditional
        // disabled/enabled/high dial never reached the wire and is removed —
        // an unbacked control is worse than an absent one.
        let snapshot = self.cached_snapshot();
        let mut values: Vec<SuperpowerValue> = Vec::new();
        if let Some(snapshot) = snapshot.as_ref() {
            for descriptor in &snapshot.models {
                values.push(SuperpowerValue::Choice {
                    value: BoundedString::new(descriptor.model.model_id.as_str())
                        .expect("catalog model ids are bounded"),
                });
            }
        }
        if !values.iter().any(|value| matches!(value, SuperpowerValue::Choice { value } if value.as_str() == self.model))
        {
            values.insert(
                0,
                SuperpowerValue::Choice {
                    value: BoundedString::new(&self.model).expect("bounded"),
                },
            );
        }
        let mut descriptors = vec![SuperpowerDescriptor {
            id: BoundedString::new("lmstudio:model").expect("bounded"),
            provider_id: self.id.clone(),
            display_name: BoundedString::new("Model").expect("bounded"),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new(&self.model).expect("bounded"),
            },
            allowed_values: values,
            command_alias: Some(BoundedString::new("model").expect("bounded")),
            help: Some(
                BoundedString::new("A model available on the LM Studio server.").expect("bounded"),
            ),
        }];
        // Thinking dial: only when the pinned (active) model reports its own
        // allowed reasoning options — labels travel verbatim (off/on/low/
        // medium/high per the verified schema).
        if let Some(snapshot) = snapshot.as_ref()
            && let Some(active) = snapshot
                .models
                .iter()
                .find(|d| d.model.model_id.as_str() == self.model)
            && let SupportLevel::Native { details } = &active.capabilities.reasoning
            && !details.effort_levels.is_empty()
        {
            descriptors.push(SuperpowerDescriptor {
                id: BoundedString::new("lmstudio:reasoning").expect("bounded"),
                provider_id: self.id.clone(),
                display_name: BoundedString::new("Thinking").expect("bounded"),
                kind: SuperpowerKind::Choice,
                scope: SuperpowerScope::Session,
                default_value: SuperpowerValue::Choice {
                    value: BoundedString::new(details.effort_levels[0].as_str()).expect("bounded"),
                },
                allowed_values: details
                    .effort_levels
                    .iter()
                    .map(|label| SuperpowerValue::Choice {
                        value: BoundedString::new(label.as_str()).expect("bounded"),
                    })
                    .collect(),
                command_alias: Some(BoundedString::new("thinking").expect("bounded")),
                help: Some(
                    BoundedString::new("Reasoning options reported by this model.")
                        .expect("bounded"),
                ),
            });
        }
        descriptors
    }
}

impl ProviderCredentialPort for LmStudioFactory {
    fn credential_present(&self) -> Result<bool, CredentialError> {
        // LM Studio's API key is OPTIONAL — the server may run without auth.
        // Always report the credential as present so hosts never block the
        // user with an authentication screen.
        Ok(true)
    }
    fn store_credential(&self, _secret: &str) -> Result<(), CredentialError> {
        // No-op: LM Studio keys are env/config-only, not OS credential store.
        Ok(())
    }
}

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

/// Builds the ACP composition's LM Studio factory from environment settings.
///
/// Mirrors the TUI's `register_default_providers`: reads
/// `$AGENT_VESPER_LMSTUDIO_ROOT/settings.json` when present (falling back to
/// `.agent-vesper/lmstudio/settings.json`), defaults to
/// `http://localhost:1234/v1`, and applies the optional `LMSTUDIO_API_KEY`.
pub(crate) fn factory_from_settings() -> LmStudioFactory {
    let settings = lmstudio_settings();
    let url = if settings.0.trim().is_empty() {
        "http://localhost:1234/v1".to_string()
    } else {
        settings.0
    };
    let model = if settings.1.trim().is_empty() {
        "local-model".to_string()
    } else {
        settings.1
    };
    let mut config = LmStudioConfig::new(&url).expect("LM Studio URL default is valid");
    if let Ok(key) = std::env::var("LMSTUDIO_API_KEY")
        && !key.is_empty()
    {
        config = config.with_api_key(key);
    }
    LmStudioFactory::new(config, model)
}

/// Reads `(api_base_url, model)` from the shared LM Studio settings file.
fn lmstudio_settings() -> (String, String) {
    let root = std::env::var("AGENT_VESPER_LMSTUDIO_ROOT").map_or_else(
        |_| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".agent-vesper")
                .join("lmstudio")
        },
        std::path::PathBuf::from,
    );
    let path = root.join("settings.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (String::new(), String::new());
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (String::new(), String::new());
    };
    let url = json
        .get("api_base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let model = json
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    (url, model)
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
    fn parse_sse_chunk_emits_content_delta_for_plain_answer() {
        let chunk = r#"data: {"choices":[{"delta":{"content":"answer"}}]}"#;
        let event = parse_sse_chunk(chunk, &stream_id()).expect("must emit");
        assert!(matches!(event, ProviderStreamEvent::ContentDelta { .. }));
    }

    #[test]
    fn parse_sse_chunk_emits_completed_on_done_marker() {
        let chunk = "data: [DONE]";
        let event = parse_sse_chunk(chunk, &stream_id()).expect("must emit");
        assert!(matches!(event, ProviderStreamEvent::Completed { .. }));
    }

    #[test]
    fn credential_port_always_reports_present() {
        let factory = LmStudioFactory::new(
            LmStudioConfig::new("http://localhost:1234/v1").expect("valid"),
            "local-model",
        );
        assert!(factory.credential_present().unwrap_or(false));
    }

    // ------------------------------------------------------------------
    // PRD provider-capability-gating P5: verified native /api/v1/models
    // schema (lmstudio-ai/docs 1_developer/2_rest/list.md).
    // ------------------------------------------------------------------

    fn native_body() -> serde_json::Value {
        serde_json::json!({
            "models": [
                {
                    "type": "llm",
                    "publisher": "google",
                    "key": "google/gemma-4-26b-a4b",
                    "display_name": "Gemma 4 26B A4B",
                    "max_context_length": 262144,
                    "capabilities": {
                        "vision": true,
                        "trained_for_tool_use": true,
                        "reasoning": {
                            "allowed_options": ["off", "on"],
                            "default": "on"
                        }
                    }
                },
                {
                    "type": "llm",
                    "publisher": "deepseek",
                    "key": "deepseek-r1",
                    "display_name": "DeepSeek R1",
                    "max_context_length": 131072,
                    "capabilities": {
                        "vision": false,
                        "trained_for_tool_use": true,
                        "reasoning": {"allowed_options": ["on"], "default": "on"}
                    }
                },
                {
                    "type": "embedding",
                    "publisher": "gaianet",
                    "key": "text-embedding-nomic-embed-text-v1.5-embedding",
                    "display_name": "Nomic Embed Text v1.5"
                }
            ]
        })
    }

    #[test]
    fn native_models_url_strips_compat_suffixes() {
        assert_eq!(
            native_models_url("http://localhost:1234/v1"),
            "http://localhost:1234/api/v1/models"
        );
        assert_eq!(
            native_models_url("http://192.168.1.10:1234/v1/"),
            "http://192.168.1.10:1234/api/v1/models"
        );
        assert_eq!(
            native_models_url("http://localhost:1234"),
            "http://localhost:1234/api/v1/models"
        );
    }

    #[test]
    fn snapshot_from_native_maps_capabilities_and_skips_embeddings() {
        let snapshot = snapshot_from_native(&native_body(), &pid()).expect("parsed");
        assert_eq!(snapshot.models.len(), 2, "embedding models are skipped");
        assert_eq!(snapshot.provenance, ModelCatalogProvenance::Discovered);

        let gemma = &snapshot.models[0];
        assert_eq!(gemma.model.model_id.as_str(), "google/gemma-4-26b-a4b");
        assert!(matches!(
            &gemma.capabilities.vision,
            SupportLevel::Native { .. }
        ));
        assert!(matches!(
            &gemma.capabilities.tools,
            SupportLevel::Native { .. }
        ));
        match &gemma.capabilities.reasoning {
            SupportLevel::Native { details } => {
                assert_eq!(details.effort_levels, vec!["off", "on"])
            }
            other => panic!("expected native reasoning, got {other:?}"),
        }
        match &gemma.capabilities.limits {
            SupportLevel::Native { details } => {
                assert_eq!(details.context_tokens, Some(262144))
            }
            other => panic!("expected native limits, got {other:?}"),
        }

        let deepseek = &snapshot.models[1];
        match &deepseek.capabilities.vision {
            SupportLevel::Unsupported { reason } => {
                assert_eq!(reason.as_str(), "model does not support image inputs")
            }
            other => panic!("expected unsupported vision, got {other:?}"),
        }
    }

    #[test]
    fn superpowers_follow_the_cached_catalog_and_reasoning_evidence() {
        let config = LmStudioConfig::new("http://localhost:1234/v1").unwrap();
        let factory = LmStudioFactory::new(config, "deepseek-r1");
        // No cache: only the pinned-model selector; NO thinking dial (the
        // former unconditional disabled/enabled/high dial was never sent on
        // the wire — an unbacked control is removed, PRD P5).
        let cold = factory.superpowers();
        assert_eq!(cold.len(), 1);
        assert!(cold[0].id.as_str() == "lmstudio:model");

        // With the cache: model lists every LLM; the thinking dial appears
        // only for the pinned model that reports reasoning options, with the
        // model's own labels.
        *factory.catalog.write().expect("catalog lock poisoned") =
            Some(snapshot_from_native(&native_body(), &pid()).expect("parsed"));
        let warm = factory.superpowers();
        assert_eq!(warm.len(), 2);
        let model = warm
            .iter()
            .find(|d| d.id.as_str() == "lmstudio:model")
            .unwrap();
        let labels: Vec<&str> = model
            .allowed_values
            .iter()
            .filter_map(|v| match v {
                SuperpowerValue::Choice { value } => Some(value.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"google/gemma-4-26b-a4b"));
        assert!(labels.contains(&"deepseek-r1"));
        let thinking = warm
            .iter()
            .find(|d| d.id.as_str() == "lmstudio:reasoning")
            .unwrap();
        let thinking_labels: Vec<&str> = thinking
            .allowed_values
            .iter()
            .filter_map(|v| match v {
                SuperpowerValue::Choice { value } => Some(value.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            thinking_labels,
            vec!["on"],
            "the pinned model's own options only"
        );
    }

    #[test]
    fn fail_closed_when_capabilities_are_absent() {
        let body = serde_json::json!({
            "models": [
                {"type": "llm", "key": "bare-model", "display_name": "Bare"}
            ]
        });
        let snapshot = snapshot_from_native(&body, &pid()).expect("parsed");
        let bare = &snapshot.models[0];
        assert!(matches!(bare.capabilities.vision, SupportLevel::Unknown));
        assert!(matches!(bare.capabilities.tools, SupportLevel::Unknown));
        assert!(matches!(bare.capabilities.reasoning, SupportLevel::Unknown));
        assert!(matches!(bare.capabilities.limits, SupportLevel::Unknown));
    }
}
