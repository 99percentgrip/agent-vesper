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
use vesper_agent::providers::lmstudio::{
    ChatMessage, LmStudioConfig, LmStudioHttpResponse, build_chat_request, build_models_request,
    parse_chat_response, parse_models_response,
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
        Box::pin(async move {
            let messages = provider_request_to_chat_messages(&request);
            let chat_req = build_chat_request(&self.config, &self.model, &messages);

            let resp = self
                .client
                .post(&chat_req.url)
                .headers(self.req_headers_from(&chat_req.headers))
                .body(chat_req.body.unwrap_or_default())
                .send()
                .await
                .map_err(|e| err(format!("HTTP send: {e}")))?;

            let status = resp.status();
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| err(format!("HTTP body: {e}")))?;

            if !status.is_success() {
                return Err(err(format!("HTTP {status}: {body}")));
            }

            let lm_resp = LmStudioHttpResponse {
                status: status.as_u16(),
                body,
            };
            let (content, _tokens) =
                parse_chat_response(&lm_resp).map_err(|e| err(format!("parse: {e}")))?;

            let stream_id = BoundedString::new("content").expect("bounded");
            let text = ContentText::new(content)
                .unwrap_or_else(|_| ContentText::new("(empty response)").expect("bounded"));
            let events = vec![
                Ok(ProviderStreamEvent::ResponseStarted {
                    response_id: None,
                    metadata: ExtensionMap::default(),
                }),
                Ok(ProviderStreamEvent::ContentDelta {
                    stream_id: stream_id.clone(),
                    part: ContentPart::Text(text),
                }),
                Ok(ProviderStreamEvent::Completed {
                    finish: FinishOutcome::Stop,
                    metadata: ExtensionMap::default(),
                }),
            ];

            Ok(Box::pin(futures_util::stream::iter(events)) as ProviderEventStream)
        })
    }
}

impl LmStudioSession {
    fn req_headers_from(&self, headers: &[(String, String)]) -> reqwest::header::HeaderMap {
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
        vec![SuperpowerDescriptor {
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
                BoundedString::new("The model loaded on the LM Studio server.").expect("bounded"),
            ),
        }]
    }
}

// ---------------------------------------------------------------------------
// Credential port (env-var API key)
// ---------------------------------------------------------------------------

impl ProviderCredentialPort for LmStudioFactory {
    fn credential_present(&self) -> Result<bool, CredentialError> {
        Ok(std::env::var("LMSTUDIO_API_KEY").is_ok())
    }
    fn store_credential(&self, _secret: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
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
