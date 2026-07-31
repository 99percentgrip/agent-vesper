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

/// Static GLM-specific superpower descriptors advertised to the composition
/// boundary. Kept in a free function so a `GlmFactory` and any test stand-in
/// share one source of truth.
fn glm_superpowers() -> Vec<vesper_provider::SuperpowerDescriptor> {
    use vesper_domain::BoundedString;
    use vesper_provider::{SuperpowerDescriptor, SuperpowerKind, SuperpowerScope, SuperpowerValue};

    let provider_id = provider_id();

    // Order: effort dial, interleaved-thinking toggle, model selector.
    vec![
        // Per-request effort dial exposed by the GLM coding endpoint.
        SuperpowerDescriptor {
            id: BoundedString::new("zai:effort").expect("bounded superpower id"),
            provider_id: provider_id.clone(),
            display_name: BoundedString::new("Effort").expect("bounded display"),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Request,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("high").expect("bounded value"),
            },
            allowed_values: ["low", "medium", "high", "max"]
                .into_iter()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(raw).expect("bounded value"),
                })
                .collect(),
            command_alias: Some(BoundedString::new("effort").expect("bounded alias")),
            help: Some(
                BoundedString::new("Set per-request Z.ai effort (low/medium/high/max).")
                    .expect("bounded help"),
            ),
        },
        // Interleaved thinking toggle (deep reasoning interleaved with tool calls).
        SuperpowerDescriptor {
            id: BoundedString::new("zai:interleaved-thinking").expect("bounded superpower id"),
            provider_id: provider_id.clone(),
            display_name: BoundedString::new("Interleaved Thinking").expect("bounded display"),
            kind: SuperpowerKind::Toggle,
            scope: SuperpowerScope::Both,
            default_value: SuperpowerValue::Flag { value: true },
            allowed_values: Vec::new(),
            command_alias: Some(BoundedString::new("thinking").expect("bounded alias")),
            help: Some(BoundedString::new("Toggle interleaved thinking.").expect("bounded help")),
        },
        // Model selector: applies to the whole session.
        SuperpowerDescriptor {
            id: BoundedString::new("zai:model").expect("bounded superpower id"),
            provider_id,
            display_name: BoundedString::new("Model").expect("bounded display"),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("glm-5.2").expect("bounded value"),
            },
            allowed_values: ["glm-5.2", "glm-5.2-air", "glm-5.2-flash"]
                .into_iter()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(raw).expect("bounded value"),
                })
                .collect(),
            command_alias: Some(BoundedString::new("model").expect("bounded alias")),
            help: Some(BoundedString::new("Switch the active Z.ai model.").expect("bounded help")),
        },
    ]
}

impl vesper_provider::ProviderSuperpowers for GlmFactory {
    fn superpowers(&self) -> Vec<vesper_provider::SuperpowerDescriptor> {
        glm_superpowers()
    }
}
