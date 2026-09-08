use crate::{
    DEFAULT_MODEL, OpenAiCatalog, OpenAiSession, REASONING_LEVELS, credentials::Credentials, error,
    provider_id,
};
use std::sync::Arc;
use vesper_domain::{
    BoundedString, ExtensionMap, ProviderId, SchemaVersion, VersionedExtensionEnvelope,
};
use vesper_provider::*;

#[derive(Clone)]
pub struct OpenAiFactory {
    id: ProviderId,
    credentials: Credentials,
    #[cfg(feature = "integration-test-harness")]
    test_route: Option<(String, crate::auth::AuthenticationMode)>,
}
impl Default for OpenAiFactory {
    fn default() -> Self {
        Self {
            id: provider_id(),
            credentials: Credentials::default(),
            #[cfg(feature = "integration-test-harness")]
            test_route: None,
        }
    }
}
impl OpenAiFactory {
    /// Bounded structured extraction for host-owned memory ports. Runs through
    /// the same native authentication and Responses session as ordinary turns.
    pub async fn extract_memory(
        &self,
        system: &str,
        user: &str,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<String, ProviderError> {
        use vesper_domain::*;
        let configuration = Self::default_configuration();
        let session = self.create_session(&configuration, cancel.clone()).await?;
        let request = ProviderRequest {
            request_id: ProviderRequestId::new("memory-extraction").expect("static"),
            provider_id: provider_id(),
            model: QualifiedModelId {
                provider_id: provider_id(),
                model_id: ModelId::new(DEFAULT_MODEL).expect("static"),
            },
            endpoint_id: None,
            system_instructions: vec![SystemInstruction {
                content: vec![ContentPart::Text(
                    ContentText::new(system).map_err(|_| crate::wire::invalid())?,
                )],
                cache_stable: true,
                extensions: Default::default(),
            }],
            messages: vec![ConversationMessage {
                id: MessageId::new("memory-input").expect("static"),
                role: MessageRole::User,
                content: vec![ContentPart::Text(
                    ContentText::new(user).map_err(|_| crate::wire::invalid())?,
                )],
                extensions: Default::default(),
            }],
            tools: vec![],
            tool_choice: ToolChoiceIntent::None,
            capabilities: vec![],
            reasoning: None,
            structured_output: StructuredOutputIntent::JsonObject,
            sampling: None,
            maximum_output_tokens: Some(4096),
            continuation: None,
            fallback_policy: FallbackPolicy::Strict,
            provider_extensions: None,
        };
        let content = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            session.execute_auxiliary(AuxiliaryRequestIntent::MemoryExtraction, request, cancel),
        )
        .await
        .map_err(|_| {
            error(
                "OpenAI memory extraction timed out",
                ErrorCategory::Transport,
                false,
            )
        })??;
        match content {
            ContentPart::Text(text) => Ok(text.as_str().to_owned()),
            _ => Err(crate::wire::invalid()),
        }
    }
    /// Loopback-only fixture route with synthetic credentials. Not available
    /// in release/default builds and never resolves user credentials.
    #[cfg(feature = "integration-test-harness")]
    #[allow(clippy::result_large_err)]
    pub fn for_loopback(
        endpoint: &str,
        mode: crate::auth::AuthenticationMode,
    ) -> Result<Self, ProviderError> {
        let url = url::Url::parse(endpoint).map_err(|_| crate::wire::invalid())?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(crate::wire::invalid());
        }
        Ok(Self {
            test_route: Some((endpoint.to_owned(), mode)),
            ..Self::default()
        })
    }
    pub fn default_configuration() -> ProviderConfiguration {
        let mut values = ExtensionMap::default();
        values
            .insert("openai:model", serde_json::json!(DEFAULT_MODEL))
            .expect("static");
        values
            .insert("openai:reasoning-mode", serde_json::json!("medium"))
            .expect("static");
        ProviderConfiguration {
            provider_id: provider_id(),
            values: VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.openai")
                    .expect("static"),
                version: SchemaVersion::new(1).expect("static"),
                values,
            },
        }
    }
}
impl ProviderFactory for OpenAiFactory {
    type Session = OpenAiSession;
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: self.id.clone(),
            display_name: BoundedString::new("OpenAI").expect("static"),
            authentication_methods: vec![
                AuthenticationMethodDescriptor {
                    method_id: BoundedString::new("openai-api-key").expect("static"),
                    display_name: BoundedString::new("API key (usage-based billing)")
                        .expect("static"),
                    secret_reference_fields: vec![
                        BoundedString::new("OPENAI_API_KEY").expect("static"),
                    ],
                    external_runtime_owned: false,
                    key_url: Some(
                        BoundedString::new("https://platform.openai.com/api-keys").expect("static"),
                    ),
                },
                AuthenticationMethodDescriptor {
                    method_id: BoundedString::new("openai-chatgpt").expect("static"),
                    display_name: BoundedString::new("ChatGPT subscription (device sign-in)")
                        .expect("static"),
                    secret_reference_fields: vec![],
                    external_runtime_owned: false,
                    key_url: Some(
                        BoundedString::new("https://auth.openai.com/codex/device").expect("static"),
                    ),
                },
            ],
            configuration: None,
            metadata: ExtensionMap::default(),
        }
    }
    fn create_session<'a>(
        &'a self,
        config: &'a ProviderConfiguration,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<OpenAiSession, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(error(
                    "OpenAI request cancelled",
                    vesper_domain::ErrorCategory::Cancellation,
                    false,
                ));
            }
            let model = config
                .values
                .values
                .get("openai:model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(DEFAULT_MODEL);
            let effort = config
                .values
                .values
                .get("openai:reasoning-mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("medium");
            if config.provider_id != self.id || !OpenAiCatalog::supports_reasoning(model, effort) {
                return Err(error(
                    "Invalid OpenAI model or reasoning selection",
                    vesper_domain::ErrorCategory::InvalidRequest,
                    false,
                ));
            }
            let session = OpenAiSession::new(self.credentials.clone(), effort.to_owned())?;
            #[cfg(feature = "integration-test-harness")]
            let session = session.with_test_route(self.test_route.clone());
            Ok(session)
        })
    }
}
impl ModelCatalog for OpenAiFactory {
    fn models<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        OpenAiCatalog.models(cancel)
    }
}
impl ProviderCredentialPort for OpenAiFactory {
    fn credential_present(&self) -> Result<bool, CredentialError> {
        #[cfg(feature = "integration-test-harness")]
        if self.test_route.is_some() {
            return Ok(true);
        }
        self.credentials.present()
    }
    fn store_credential(&self, secret: &str) -> Result<(), CredentialError> {
        self.credentials.store_api_key(secret)
    }
    fn authentication_method(&self) -> Result<Option<String>, CredentialError> {
        self.credentials.authentication_method()
    }
    fn logout(&self) -> Result<(), CredentialError> {
        self.credentials.logout()
    }
    fn device_login<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
        on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
    ) -> ProviderFuture<'a, Result<(), CredentialError>> {
        Box::pin(self.credentials.login(cancel, on_challenge))
    }
}
impl ProviderSuperpowers for OpenAiFactory {
    fn superpowers(&self) -> Vec<SuperpowerDescriptor> {
        let mut values: Vec<&str> = REASONING_LEVELS.to_vec();
        values.push("max");
        vec![
            choice(
                "openai:model",
                "Model",
                "model",
                DEFAULT_MODEL,
                OpenAiCatalog::snapshot()
                    .models
                    .iter()
                    .map(|m| m.model.model_id.as_str())
                    .collect(),
            ),
            choice(
                "openai:reasoning",
                "Reasoning effort",
                "thinking",
                "medium",
                values,
            ),
        ]
    }
}
fn choice(
    id: &str,
    name: &str,
    alias: &str,
    default: &str,
    values: Vec<&str>,
) -> SuperpowerDescriptor {
    SuperpowerDescriptor {
        id: BoundedString::new(id).expect("static"),
        provider_id: provider_id(),
        display_name: BoundedString::new(name).expect("static"),
        kind: SuperpowerKind::Choice,
        scope: SuperpowerScope::Session,
        default_value: SuperpowerValue::Choice {
            value: BoundedString::new(default).expect("static"),
        },
        allowed_values: values
            .into_iter()
            .map(|v| SuperpowerValue::Choice {
                value: BoundedString::new(v).expect("static"),
            })
            .collect(),
        command_alias: Some(BoundedString::new(alias).expect("static")),
        help: None,
    }
}
