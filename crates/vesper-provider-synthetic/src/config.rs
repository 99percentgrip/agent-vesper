//! Stable descriptor, configuration, and catalog for the synthetic adapter.
//!
//! These mirror the GLM adapter's convention (static descriptor + default
//! configuration + static catalog) while remaining entirely provider-neutral.
//! No authentication method is advertised because the synthetic adapter needs
//! no credentials; it is a deterministic in-process reference implementation.

use serde_json::json;
use vesper_domain::{
    BoundedString, ExtensionMap, ExtensionNamespace, ModelId, QualifiedModelId, SchemaVersion,
    VersionedExtensionEnvelope,
};
use vesper_provider::{
    ModelCatalogProvenance, ModelCatalogSnapshot, ModelDescriptor, ModelLimits,
    ProviderCapabilities, ProviderConfigContribution, ProviderConfiguration, ProviderDescriptor,
    SupportLevel,
};

use crate::provider_id;

/// Synthesizes the stable, non-secret adapter descriptor.
#[must_use]
pub fn descriptor() -> ProviderDescriptor {
    ProviderDescriptor {
        provider_id: provider_id(),
        display_name: BoundedString::new("Vesper Synthetic").expect("bounded display name"),
        authentication_methods: Vec::new(),
        configuration: Some(configuration_contribution()),
        metadata: ExtensionMap::default(),
    }
}

/// Default versioned configuration envelope for the synthetic adapter.
#[must_use]
pub fn default_configuration() -> ProviderConfiguration {
    let mut values = ExtensionMap::default();
    values
        .insert("synthetic:reply", json!("synthetic-ok"))
        .expect("bounded config value");
    ProviderConfiguration {
        provider_id: provider_id(),
        values: VersionedExtensionEnvelope {
            namespace: ExtensionNamespace::new("provider.synthetic")
                .expect("bounded provider namespace"),
            version: SchemaVersion::new(1).expect("static schema version"),
            values,
        },
    }
}

/// Capabilities the synthetic adapter actually implements: bounded text output
/// with deterministic streaming and a single terminal completion. Everything
/// else stays `Unknown`, which is honest rather than over-advertised.
#[must_use]
pub fn capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        limits: SupportLevel::Native {
            details: ModelLimits {
                context_tokens: Some(8192),
                output_tokens: Some(4096),
                exact: true,
            },
        },
        ..ProviderCapabilities::default()
    }
}

fn configuration_contribution() -> ProviderConfigContribution {
    ProviderConfigContribution {
        provider_id: provider_id(),
        schema_version: 1,
        config_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "synthetic:reply": {"type": "string"}
            }
        }),
        secret_reference_fields: Vec::new(),
    }
}

/// Static catalog handle declaring the single deterministic model.
pub struct SyntheticCatalog;

impl SyntheticCatalog {
    /// Returns the static catalog snapshot.
    #[must_use]
    pub fn snapshot() -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            models: vec![ModelDescriptor {
                model: QualifiedModelId {
                    provider_id: provider_id(),
                    model_id: ModelId::new("synthetic-1").expect("static model ID"),
                },
                display_name: BoundedString::new("Synthetic 1")
                    .expect("bounded model display name"),
                capabilities: capabilities(),
                metadata: ExtensionMap::default(),
            }],
            provenance: ModelCatalogProvenance::Static,
            expires_at_unix_ms: None,
        }
    }
}

/// Lightweight configuration carrier parsed from a [`ProviderConfiguration`].
///
/// The synthetic adapter does not validate secrets; it only extracts the
/// optional deterministic reply text so tests and integrations can customize
/// the streamed response.
#[derive(Debug, Clone)]
pub struct SyntheticConfig {
    /// Reply text streamed for every turn when set.
    pub reply: Option<String>,
}

impl SyntheticConfig {
    /// Extracts a config view from a provider configuration envelope.
    ///
    /// Unknown or missing values are tolerated: the adapter always has a safe
    /// default reply.
    #[must_use]
    pub fn from_configuration(configuration: &ProviderConfiguration) -> Self {
        let reply = configuration
            .values
            .values
            .get("synthetic:reply")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        Self { reply }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::ProviderId;

    #[test]
    fn descriptor_is_provider_neutral_and_auth_free() {
        let descriptor = descriptor();
        assert_eq!(
            descriptor.provider_id,
            ProviderId::new("vesper-synthetic").unwrap()
        );
        assert!(
            descriptor.authentication_methods.is_empty(),
            "synthetic adapter must not advertise authentication"
        );
    }

    #[test]
    fn catalog_declares_one_deterministic_model() {
        let snapshot = SyntheticCatalog::snapshot();
        assert_eq!(snapshot.models.len(), 1);
        assert_eq!(
            snapshot.models[0].model.provider_id,
            ProviderId::new("vesper-synthetic").unwrap()
        );
    }

    #[test]
    fn config_extracts_optional_reply_and_defaults_safely() {
        assert!(
            SyntheticConfig::from_configuration(&default_configuration())
                .reply
                .is_some()
        );
        let empty = ProviderConfiguration {
            provider_id: provider_id(),
            values: VersionedExtensionEnvelope {
                namespace: ExtensionNamespace::new("provider.synthetic").unwrap(),
                version: SchemaVersion::new(1).unwrap(),
                values: ExtensionMap::default(),
            },
        };
        assert!(SyntheticConfig::from_configuration(&empty).reply.is_none());
    }
}
