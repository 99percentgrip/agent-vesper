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
    pub(crate) availability: Arc<std::sync::RwLock<Option<crate::AvailableModels>>>,
    pub(crate) credentials: Credentials,
    #[cfg(feature = "integration-test-harness")]
    pub(crate) test_route: Option<(String, crate::auth::AuthenticationMode)>,
}
impl Default for OpenAiFactory {
    fn default() -> Self {
        Self {
            id: provider_id(),
            availability: Default::default(),
            credentials: Credentials::default(),
            #[cfg(feature = "integration-test-harness")]
            test_route: None,
        }
    }
}
impl OpenAiFactory {
    fn invalidate_models(&self) -> Result<(), CredentialError> {
        let mut snapshot = self
            .availability
            .write()
            .map_err(|_| CredentialError::Failed)?;
        *snapshot = Some(crate::AvailableModels::unavailable(
            self.control_policy().mode,
        ));
        Ok(())
    }
    /// Project account choices without promoting the capability catalog to availability.
    pub fn superpowers_for(&self, available: &crate::AvailableModels) -> Vec<SuperpowerDescriptor> {
        let mut descriptors = self.superpowers();
        for descriptor in &mut descriptors {
            if descriptor
                .command_alias
                .as_ref()
                .is_some_and(|alias| alias.as_str() == "model")
            {
                descriptor.allowed_values.retain(|value| match value {
                    SuperpowerValue::Choice { value } => available.contains(value.as_str()),
                    _ => false,
                });
                if !descriptor
                    .allowed_values
                    .contains(&descriptor.default_value)
                    && let Some(first) = descriptor.allowed_values.first()
                {
                    descriptor.default_value = first.clone();
                }
                descriptor.help = BoundedString::new(if available.models.is_empty() {
                    "No verified account models available. Check sign-in and reopen Settings to retry."
                } else {
                    "Models returned for this account and authentication method."
                }).ok();
            }
        }
        descriptors
    }
    /// Resolve mode at composition, never during terminal rendering.
    pub fn control_policy(&self) -> crate::OpenAiSuperpowerPolicy {
        crate::OpenAiSuperpowerPolicy {
            available: None,
            mode: if self.authentication_method().ok().flatten().as_deref()
                == Some("openai-api-key")
            {
                crate::auth::AuthenticationMode::ApiKey
            } else {
                crate::auth::AuthenticationMode::ChatGpt
            },
        }
    }
    /// Bounded structured extraction for host-owned memory ports. Runs through
    /// the same native authentication and Responses session as ordinary turns.
    pub async fn extract_memory(
        &self,
        system: &str,
        user: &str,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<String, ProviderError> {
        use vesper_domain::*;
        let available = self.available_models(cancel.clone()).await?;
        let model = available
            .models
            .iter()
            .find(|m| m.model.model_id.as_str() == DEFAULT_MODEL)
            .or_else(|| available.models.first())
            .ok_or_else(|| {
                error(
                    "No verified OpenAI account model is available for memory extraction",
                    ErrorCategory::InvalidRequest,
                    false,
                )
            })?;
        let mut configuration = Self::default_configuration();
        configuration
            .values
            .values
            .insert(
                "openai:model",
                serde_json::json!(model.model.model_id.as_str()),
            )
            .map_err(|_| crate::wire::invalid())?;
        let session = self.create_session(&configuration, cancel.clone()).await?;
        let request = ProviderRequest {
            request_id: ProviderRequestId::new("memory-extraction").expect("static"),
            provider_id: provider_id(),
            model: QualifiedModelId {
                provider_id: provider_id(),
                model_id: model.model.model_id.clone(),
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
            if config.provider_id != self.id
                || !OpenAiCatalog::reasoning_levels_for(model, self.control_policy().mode)
                    .contains(&effort)
            {
                return Err(error(
                    "Invalid OpenAI model or reasoning selection",
                    vesper_domain::ErrorCategory::InvalidRequest,
                    false,
                ));
            }
            let session = OpenAiSession::new(self.credentials.clone(), effort.to_owned())?
                .with_availability(self.availability.clone());
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
        Box::pin(async move {
            let available = self.available_models(cancel).await?;
            Ok(ModelCatalogSnapshot {
                models: available.models,
                provenance: ModelCatalogProvenance::Discovered,
                expires_at_unix_ms: None,
            })
        })
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
        self.credentials.store_api_key(secret)?;
        self.invalidate_models()
    }
    fn authentication_method(&self) -> Result<Option<String>, CredentialError> {
        #[cfg(feature = "integration-test-harness")]
        if let Some((_, mode)) = &self.test_route {
            return Ok(Some(
                match mode {
                    crate::auth::AuthenticationMode::ApiKey => "openai-api-key",
                    crate::auth::AuthenticationMode::ChatGpt => "openai-chatgpt",
                }
                .into(),
            ));
        }
        self.credentials.authentication_method()
    }
    fn logout(&self) -> Result<(), CredentialError> {
        self.credentials.logout()?;
        self.invalidate_models()
    }
    fn device_login<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
        on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
    ) -> ProviderFuture<'a, Result<(), CredentialError>> {
        Box::pin(async move {
            self.credentials.login(cancel, on_challenge).await?;
            self.invalidate_models()
        })
    }
}
impl ProviderSuperpowers for OpenAiFactory {
    fn superpowers(&self) -> Vec<SuperpowerDescriptor> {
        let mut values: Vec<&str> = REASONING_LEVELS.to_vec();
        values.push("max");
        values.insert(0, "none");
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
