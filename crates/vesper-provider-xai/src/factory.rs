use crate::{DEFAULT_MODEL, XaiCatalog, XaiSession, credentials::Credentials, error, provider_id};
use std::sync::Arc;
use vesper_domain::{
    BoundedString, ExtensionMap, ProviderId, SchemaVersion, VersionedExtensionEnvelope,
};
use vesper_provider::*;

#[derive(Clone)]
pub struct XaiFactory {
    id: ProviderId,
    pub(crate) availability: Arc<std::sync::RwLock<Option<crate::AvailableModels>>>,
    pub(crate) credentials: Credentials,
    #[cfg(feature = "integration-test-harness")]
    pub(crate) test_route: Option<String>,
}
impl Default for XaiFactory {
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
impl XaiFactory {
    pub fn default_configuration() -> ProviderConfiguration {
        let mut values = ExtensionMap::default();
        values
            .insert("xai:model", serde_json::json!(DEFAULT_MODEL))
            .expect("static");
        values
            .insert("xai:reasoning-effort", serde_json::json!("high"))
            .expect("static");
        values
            .insert("xai:transport", serde_json::json!("http"))
            .expect("static");
        values
            .insert("xai:region", serde_json::json!("global"))
            .expect("static");
        ProviderConfiguration {
            provider_id: provider_id(),
            values: VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.xai").expect("static"),
                version: SchemaVersion::new(1).expect("static"),
                values,
            },
        }
    }
    pub fn superpowers_for(
        &self,
        available: &crate::AvailableModels,
        model: &str,
    ) -> Vec<SuperpowerDescriptor> {
        let selected = if available.contains(model) {
            model
        } else {
            available
                .models
                .first()
                .map(|entry| entry.model.model_id.as_str())
                .unwrap_or(DEFAULT_MODEL)
        };
        vec![
            choice(
                "xai:model",
                "Model",
                "model",
                selected,
                available
                    .models
                    .iter()
                    .map(|entry| entry.model.model_id.as_str())
                    .collect(),
                None,
            ),
            choice(
                "xai:reasoning",
                if selected == "grok-4.20-multi-agent-0309" {
                    "Multi-agent scale"
                } else {
                    "Reasoning effort"
                },
                "thinking",
                XaiCatalog::default_effort(selected).unwrap_or("high"),
                XaiCatalog::reasoning_levels(selected),
                Some(if selected == "grok-4.20-multi-agent-0309" {
                    "For this beta model, effort controls xAI-side agent count rather than thinking depth."
                } else {
                    "Reasoning choices verified for the selected xAI model."
                }),
            ),
            choice(
                "xai:region",
                "API region",
                "region",
                "global",
                vec!["global", "us"],
                Some(
                    "US regional processing currently limits model availability to Grok 4.7 and 4.6.",
                ),
            ),
        ]
    }
    #[cfg(feature = "integration-test-harness")]
    #[allow(clippy::result_large_err)]
    pub fn for_loopback(endpoint: &str) -> Result<Self, ProviderError> {
        let url = url::Url::parse(endpoint).map_err(|_| crate::wire::invalid())?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(crate::wire::invalid());
        }
        Ok(Self {
            test_route: Some(endpoint.to_owned()),
            ..Self::default()
        })
    }
}
impl ProviderSuperpowers for XaiFactory {
    fn superpowers(&self) -> Vec<SuperpowerDescriptor> {
        self.superpowers_for(
            &crate::AvailableModels {
                models: XaiCatalog::snapshot().models,
                ..Default::default()
            },
            DEFAULT_MODEL,
        )
    }
}

fn choice(
    id: &str,
    name: &str,
    alias: &str,
    default: &str,
    values: Vec<&str>,
    help: Option<&str>,
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
            .map(|value| SuperpowerValue::Choice {
                value: BoundedString::new(value).expect("static"),
            })
            .collect(),
        command_alias: Some(BoundedString::new(alias).expect("static")),
        help: help.map(|value| BoundedString::new(value).expect("static")),
    }
}
impl ProviderFactory for XaiFactory {
    type Session = XaiSession;
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: self.id.clone(),
            display_name: BoundedString::new("xAI / Grok").expect("static"),
            authentication_methods: vec![
                AuthenticationMethodDescriptor {
                    method_id: BoundedString::new("xai-grok-session").expect("static"),
                    display_name: BoundedString::new(
                        "Grok account / SuperGrok (account allowance)",
                    )
                    .expect("static"),
                    secret_reference_fields: vec![],
                    external_runtime_owned: false,
                    key_url: Some(BoundedString::new("https://accounts.x.ai/").expect("static")),
                },
                AuthenticationMethodDescriptor {
                    method_id: BoundedString::new("xai-api-key").expect("static"),
                    display_name: BoundedString::new("xAI API key (usage-based API billing)")
                        .expect("static"),
                    secret_reference_fields: vec![
                        BoundedString::new("XAI_API_KEY").expect("static"),
                    ],
                    external_runtime_owned: false,
                    key_url: Some(BoundedString::new("https://console.x.ai/").expect("static")),
                },
            ],
            hosted_tools: hosted_tools(),
            configuration: None,
            metadata: ExtensionMap::default(),
        }
    }
    fn create_session<'a>(
        &'a self,
        config: &'a ProviderConfiguration,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<XaiSession, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(error(
                    "xAI request cancelled",
                    vesper_domain::ErrorCategory::Cancellation,
                    false,
                ));
            }
            let model = config
                .values
                .values
                .get("xai:model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(DEFAULT_MODEL);
            let effort = config
                .values
                .values
                .get("xai:reasoning-effort")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("high");
            let region = match config
                .values
                .values
                .get("xai:region")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("global")
            {
                "global" => crate::transport::XaiRegion::Global,
                "us" => crate::transport::XaiRegion::Us,
                _ => {
                    return Err(error(
                        "Invalid xAI endpoint region",
                        vesper_domain::ErrorCategory::InvalidRequest,
                        false,
                    ));
                }
            };
            let transport = match config
                .values
                .values
                .get("xai:transport")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("http")
            {
                "http" => crate::transport::XaiTransport::Http,
                "websocket" => crate::transport::XaiTransport::WebSocket,
                _ => {
                    return Err(error(
                        "Invalid xAI transport selection",
                        vesper_domain::ErrorCategory::InvalidRequest,
                        false,
                    ));
                }
            };
            if config.provider_id != self.id
                || XaiCatalog::find(model).is_none()
                || !XaiCatalog::reasoning_levels(model).contains(&effort)
                || !region.supports(model)
            {
                return Err(error(
                    "Invalid xAI model or reasoning selection",
                    vesper_domain::ErrorCategory::InvalidRequest,
                    false,
                ));
            }
            let session = XaiSession::new(self.credentials.clone(), effort.to_owned(), region)?
                .with_availability(self.availability.clone())
                .with_transport(transport);
            #[cfg(feature = "integration-test-harness")]
            let session = session
                .with_test_route(self.test_route.clone())
                .with_test_auth_mode(crate::credentials::AuthenticationMode::ApiKey);
            Ok(session)
        })
    }
}

fn hosted_tools() -> Vec<HostedToolDescriptor> {
    let tool = |id, name, description, egress_class, separately_billed, configuration_schema| {
        HostedToolDescriptor {
            tool_id: BoundedString::new(id).expect("static"),
            display_name: BoundedString::new(name).expect("static"),
            description: BoundedString::new(description).expect("static"),
            egress_class,
            separately_billed,
            configuration_schema,
        }
    };
    vec![
        tool(
            "web-search",
            "xAI Web Search",
            "Runs web search on xAI infrastructure; separate provider egress and charges may apply.",
            HostedToolEgressClass::RemoteSearch,
            true,
            None,
        ),
        tool(
            "x-search",
            "xAI X Search",
            "Searches X on xAI infrastructure; separate provider egress and charges may apply.",
            HostedToolEgressClass::RemoteSearch,
            true,
            None,
        ),
        tool(
            "code-execution",
            "xAI Code Execution",
            "Runs Python on xAI infrastructure; it is separate from Vesper run_command.",
            HostedToolEgressClass::RemoteExecution,
            true,
            None,
        ),
        tool(
            "attachment-search",
            "xAI Attachment Search",
            "Sends explicitly selected file IDs or public URLs to xAI for provider-side document search.",
            HostedToolEgressClass::ProviderStorage,
            true,
            Some(
                serde_json::json!({"type":"object","properties":{"xai:file-ids":{"type":"array","items":{"type":"string"}},"xai:file-urls":{"type":"array","items":{"type":"string","format":"uri"}}},"additionalProperties":false}),
            ),
        ),
        tool(
            "collections-search",
            "xAI Collections Search",
            "Searches explicitly selected xAI collections on provider infrastructure.",
            HostedToolEgressClass::ProviderStorage,
            true,
            Some(
                serde_json::json!({"type":"object","properties":{"xai:collection-ids":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"string"}},"xai:max-results":{"type":"integer","minimum":1,"maximum":50}},"required":["xai:collection-ids"],"additionalProperties":false}),
            ),
        ),
        tool(
            "remote-mcp",
            "xAI Remote MCP",
            "Allows xAI servers to connect to one explicitly selected HTTPS MCP endpoint; it is separate from Vesper MCP.",
            HostedToolEgressClass::RemoteMcp,
            false,
            Some(
                serde_json::json!({"type":"object","properties":{"xai:server-url":{"type":"string","format":"uri"},"xai:server-label":{"type":"string"},"xai:server-description":{"type":"string"},"xai:allowed-tools":{"type":"array","items":{"type":"string"}}},"required":["xai:server-url","xai:server-label"],"additionalProperties":false}),
            ),
        ),
    ]
}
impl ModelCatalog for XaiFactory {
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
impl ProviderCredentialPort for XaiFactory {
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
    fn device_login<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
        on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
    ) -> ProviderFuture<'a, Result<(), CredentialError>> {
        Box::pin(async move { self.credentials.device_login(cancel, on_challenge).await })
    }
    fn browser_login<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
        on_url: Arc<dyn Fn(String) + Send + Sync>,
    ) -> ProviderFuture<'a, Result<(), CredentialError>> {
        Box::pin(async move { self.credentials.browser_login(cancel, on_url).await })
    }
    fn logout(&self) -> Result<(), CredentialError> {
        self.credentials.logout()
    }
}
