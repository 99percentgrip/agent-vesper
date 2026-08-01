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

    // ADR 0009: a single session-scoped reasoning dial matching the Python
    // oracle's `thought_level` scale `{disabled, enabled, high, max}`. The
    // former separate `zai:effort` (request-scoped) and
    // `zai:interleaved-thinking` (toggle) controls are collapsed into this one
    // dial; `low`/`medium` are intentionally absent — the oracle never emits
    // them as `reasoning_effort` (only `high`/`max`).
    vec![
        SuperpowerDescriptor {
            id: BoundedString::new("zai:reasoning").expect("bounded superpower id"),
            provider_id: provider_id.clone(),
            display_name: BoundedString::new("Thinking").expect("bounded display"),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("enabled").expect("bounded value"),
            },
            allowed_values: ["disabled", "enabled", "high", "max"]
                .into_iter()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(raw).expect("bounded value"),
                })
                .collect(),
            command_alias: Some(BoundedString::new("thinking").expect("bounded alias")),
            help: Some(
                BoundedString::new("Session reasoning depth (disabled/enabled/high/max).")
                    .expect("bounded help"),
            ),
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
            allowed_values: GlmCatalog::snapshot()
                .models
                .into_iter()
                .map(|model| SuperpowerValue::Choice {
                    value: BoundedString::new(model.model.model_id.as_str())
                        .expect("catalog model ids are bounded"),
                })
                .collect(),
            command_alias: Some(BoundedString::new("model").expect("bounded alias")),
            help: Some(BoundedString::new("Switch the active Z.ai model.").expect("bounded help")),
        },
    ]
}

/// The canonical reasoning-mode labels accepted by the reconciled
/// `zai:reasoning` dial (ADR 0009). Mirrors the Python oracle's
/// `THOUGHT_LEVELS` scale.
pub const GLM_REASONING_MODES: [&str; 4] = ["disabled", "enabled", "high", "max"];

/// Maps a `SuperpowerValue` resolved against the `zai:reasoning` descriptor into
/// the opaque reasoning-mode label carried by `UpdateSessionReasoning` and
/// ultimately by `ProviderRequest.reasoning.mode`.
///
/// Composition boundaries (e.g. the TUI) call this to translate a parsed
/// `/thinking <level>` into the runtime command's mode field. Only the four
/// oracle-faithful labels are accepted; `low`/`medium` are rejected.
pub fn reasoning_mode_for_superpower(
    value: &vesper_provider::SuperpowerValue,
) -> Result<BoundedString<128>, crate::error::GlmAdapterError> {
    let vesper_provider::SuperpowerValue::Choice { value } = value else {
        return Err(crate::error::GlmAdapterError::Configuration(
            "reasoning superpower expects a choice value",
        ));
    };
    if !GLM_REASONING_MODES.contains(&value.as_str()) {
        return Err(crate::error::GlmAdapterError::Configuration(
            "reasoning mode must be one of disabled/enabled/high/max",
        ));
    }
    Ok(value.clone())
}

impl vesper_provider::ProviderSuperpowers for GlmFactory {
    fn superpowers(&self) -> Vec<vesper_provider::SuperpowerDescriptor> {
        glm_superpowers()
    }
}

#[cfg(test)]
mod tests {
    //! ADR 0009: the reconciled reasoning dial and its `SuperpowerValue →
    //! mode` mapper. Asserts the oracle-faithful `{disabled, enabled, high,
    //! max}` scale and that `low`/`medium` are rejected.

    use super::*;
    use vesper_domain::BoundedString;
    use vesper_provider::{SuperpowerKind, SuperpowerScope, SuperpowerValue};

    #[test]
    fn reasoning_dial_is_single_session_scoped_with_oracle_scale() {
        let descriptors = glm_superpowers();
        let reasoning = descriptors
            .iter()
            .find(|descriptor| descriptor.id.as_str() == "zai:reasoning")
            .expect("reconciled reasoning dial must be advertised");
        assert_eq!(reasoning.kind, SuperpowerKind::Choice);
        assert_eq!(reasoning.scope, SuperpowerScope::Session);
        assert_eq!(
            reasoning.command_alias.as_ref().map(|alias| alias.as_str()),
            Some("thinking")
        );
        let allowed: Vec<&str> = reasoning
            .allowed_values
            .iter()
            .filter_map(|value| match value {
                SuperpowerValue::Choice { value } => Some(value.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(allowed, ["disabled", "enabled", "high", "max"]);
    }

    #[test]
    fn former_effort_and_thinking_descriptors_are_gone() {
        let descriptors = glm_superpowers();
        assert!(
            descriptors
                .iter()
                .all(|descriptor| descriptor.id.as_str() != "zai:effort"
                    && descriptor.id.as_str() != "zai:interleaved-thinking"),
            "the split effort/thinking controls must be collapsed (ADR 0009)"
        );
    }

    #[test]
    fn mapper_accepts_each_oracle_mode() {
        for raw in GLM_REASONING_MODES {
            let value = SuperpowerValue::Choice {
                value: BoundedString::new(raw).unwrap(),
            };
            let mode = reasoning_mode_for_superpower(&value).unwrap();
            assert_eq!(mode.as_str(), raw);
        }
    }

    #[test]
    fn mapper_rejects_invalid_modes_and_non_choice_values() {
        // `low`/`medium` are invented values, not in the oracle.
        for raw in ["low", "medium", "turbo"] {
            let value = SuperpowerValue::Choice {
                value: BoundedString::new(raw).unwrap(),
            };
            assert!(reasoning_mode_for_superpower(&value).is_err());
        }
        // A toggle/flag is the wrong value shape.
        let flag = SuperpowerValue::Flag { value: true };
        assert!(reasoning_mode_for_superpower(&flag).is_err());
    }
}
