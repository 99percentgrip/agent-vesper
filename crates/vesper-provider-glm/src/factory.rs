use std::sync::Arc;

use serde_json::json;
use vesper_domain::{
    BoundedString, ExtensionMap, ProviderId, SchemaVersion, VersionedExtensionEnvelope,
};
use vesper_provider::{
    AuthenticationMethodDescriptor, CancellationSignal, ModelCatalog, ModelCatalogSnapshot,
    ProviderConfigContribution, ProviderConfiguration, ProviderDescriptor, ProviderError,
    ProviderFactory, ProviderFuture,
};

use crate::{
    EnvironmentCredentialSource, GlmCatalog, GlmConfig, GlmCredentialSource, GlmSession,
    error::{adapter_error, cancelled_error},
    provider_id, resolve_credential,
};

/// Production GLM provider factory with injectable secret resolution.
pub struct GlmFactory {
    credentials: Arc<dyn GlmCredentialSource>,
}

impl std::fmt::Debug for GlmFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GlmFactory")
            .field("credentials", &"<credential-source>")
            .finish()
    }
}

impl Default for GlmFactory {
    fn default() -> Self {
        Self::new(Arc::new(EnvironmentCredentialSource))
    }
}

impl GlmFactory {
    /// Creates a factory with an explicit credential source.
    #[must_use]
    pub fn new(credentials: Arc<dyn GlmCredentialSource>) -> Self {
        Self { credentials }
    }

    /// Stable provider descriptor and non-secret configuration contribution.
    #[must_use]
    pub fn descriptor() -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: provider_id(),
            display_name: BoundedString::new("Z.ai GLM").expect("bounded display name"),
            authentication_methods: vec![AuthenticationMethodDescriptor {
                method_id: BoundedString::new("zai-api-key").expect("bounded auth ID"),
                display_name: BoundedString::new("Z.ai API key").expect("bounded auth name"),
                secret_reference_fields: vec![
                    BoundedString::new("ZAI_API_KEY").expect("bounded field"),
                    BoundedString::new("Z_AI_API_KEY").expect("bounded field"),
                ],
                external_runtime_owned: false,
            }],
            configuration: Some(ProviderConfigContribution {
                provider_id: provider_id(),
                schema_version: 1,
                config_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "zai:model": {"type": "string"},
                        "zai:endpoint-plan": {
                            "enum": ["coding", "standard", "bigmodel", "custom"]
                        },
                        "zai:base-url": {"type": "string"},
                        "zai:allow-insecure-http": {"type": "boolean"},
                        "zai:attach-inference-auth": {"type": "boolean"},
                        "zai:reasoning-mode": {
                            "enum": ["disabled", "enabled", "high", "max"]
                        },
                        "zai:generation-profile": {
                            "enum": ["balanced", "precise", "exploratory"]
                        },
                        "zai:continuation-limit": {
                            "type": "integer", "minimum": 0, "maximum": 20
                        }
                    }
                }),
                secret_reference_fields: vec![
                    BoundedString::new("ZAI_API_KEY").expect("bounded field"),
                    BoundedString::new("Z_AI_API_KEY").expect("bounded field"),
                ],
            }),
            metadata: ExtensionMap::default(),
        }
    }

    /// Creates a default versioned configuration envelope.
    #[must_use]
    pub fn default_configuration() -> ProviderConfiguration {
        let mut values = ExtensionMap::default();
        values
            .insert("zai:model", json!("glm-5.2"))
            .expect("bounded config");
        values
            .insert("zai:endpoint-plan", json!("coding"))
            .expect("bounded config");
        values
            .insert("zai:reasoning-mode", json!("enabled"))
            .expect("bounded config");
        values
            .insert("zai:generation-profile", json!("balanced"))
            .expect("bounded config");
        ProviderConfiguration {
            provider_id: provider_id(),
            values: VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.zai")
                    .expect("static namespace"),
                version: SchemaVersion::new(1).expect("static schema"),
                values,
            },
        }
    }
}

impl ProviderFactory for GlmFactory {
    type Session = GlmSession;

    fn provider_id(&self) -> &ProviderId {
        static ID: std::sync::OnceLock<ProviderId> = std::sync::OnceLock::new();
        ID.get_or_init(provider_id)
    }

    fn create_session<'a>(
        &'a self,
        config: &'a ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(cancelled_error(false));
            }
            let parsed = GlmConfig::from_provider_configuration(config)
                .map_err(|error| adapter_error(&error, false))?;
            let credential =
                resolve_credential(self.credentials.as_ref()).map_err(|error| *error)?;
            GlmSession::from_config(parsed, credential)
                .map_err(|error| adapter_error(&error, false))
        })
    }
}

impl ModelCatalog for GlmFactory {
    fn models<'a>(
        &'a self,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        <GlmCatalog as ModelCatalog>::models(&GlmCatalog, cancellation)
    }
}
