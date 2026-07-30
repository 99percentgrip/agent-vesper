use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use vesper_domain::{ProviderId, ReasoningRetentionMode, SchemaVersion};
use vesper_security::{EnvironmentScrubber, SecretReference};

use crate::ProfileName;

/// Versioned application configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplicationConfig {
    /// Configuration schema.
    pub version: SchemaVersion,
    /// Active isolated profile.
    pub active_profile: ProfileName,
    /// Initial GLM parity default remains `Persist`.
    #[serde(default)]
    pub reasoning_retention: ReasoningRetentionMode,
    /// Provider-owned configuration envelopes.
    #[serde(default)]
    pub providers: BTreeMap<ProviderId, ProviderConfigEnvelope>,
}

/// Opaque provider configuration plus references to separately managed secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "RawProviderConfigEnvelope",
    into = "RawProviderConfigEnvelope"
)]
pub struct ProviderConfigEnvelope {
    schema_version: SchemaVersion,
    provider_id: ProviderId,
    config: Value,
    secret_references: Vec<SecretReference>,
}

impl ProviderConfigEnvelope {
    /// Creates an envelope after rejecting secret-shaped raw configuration keys.
    pub fn new(
        schema_version: SchemaVersion,
        provider_id: ProviderId,
        config: Value,
        secret_references: Vec<SecretReference>,
    ) -> Result<Self, ProviderConfigError> {
        if contains_sensitive_key(&config) {
            return Err(ProviderConfigError::RawSecretField);
        }
        Ok(Self {
            schema_version,
            provider_id,
            config,
            secret_references,
        })
    }

    /// Provider identity.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Opaque non-secret provider configuration.
    #[must_use]
    pub fn config(&self) -> &Value {
        &self.config
    }

    /// Secret lookup references.
    #[must_use]
    pub fn secret_references(&self) -> &[SecretReference] {
        &self.secret_references
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawProviderConfigEnvelope {
    schema_version: SchemaVersion,
    provider_id: ProviderId,
    config: Value,
    #[serde(default)]
    secret_references: Vec<SecretReference>,
}

impl TryFrom<RawProviderConfigEnvelope> for ProviderConfigEnvelope {
    type Error = ProviderConfigError;

    fn try_from(value: RawProviderConfigEnvelope) -> Result<Self, Self::Error> {
        Self::new(
            value.schema_version,
            value.provider_id,
            value.config,
            value.secret_references,
        )
    }
}

impl From<ProviderConfigEnvelope> for RawProviderConfigEnvelope {
    fn from(value: ProviderConfigEnvelope) -> Self {
        Self {
            schema_version: value.schema_version,
            provider_id: value.provider_id,
            config: value.config,
            secret_references: value.secret_references,
        }
    }
}

fn contains_sensitive_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            EnvironmentScrubber::is_sensitive_key(key) || contains_sensitive_key(value)
        }),
        Value::Array(values) => values.iter().any(contains_sensitive_key),
        _ => false,
    }
}

/// Provider configuration violated the secret-reference boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderConfigError {
    /// Raw secret-shaped fields belong behind a `SecretReference`.
    #[error("provider configuration contains a raw secret-shaped field")]
    RawSecretField,
}

/// Configuration precedence source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigSource {
    /// Command-line/session override.
    Explicit,
    /// Process environment.
    Environment,
    /// Active profile.
    Profile,
    /// Built-in default.
    Default,
}

/// A typed value paired with its precedence provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedValue<T> {
    /// Resolved value.
    pub value: T,
    /// Winning source.
    pub source: ConfigSource,
}

impl<T> ResolvedValue<T> {
    /// Applies explicit > environment > profile > default precedence.
    #[must_use]
    pub fn resolve(
        explicit: Option<T>,
        environment: Option<T>,
        profile: Option<T>,
        default: T,
    ) -> Self {
        if let Some(value) = explicit {
            Self {
                value,
                source: ConfigSource::Explicit,
            }
        } else if let Some(value) = environment {
            Self {
                value,
                source: ConfigSource::Environment,
            }
        } else if let Some(value) = profile {
            Self {
                value,
                source: ConfigSource::Profile,
            }
        } else {
            Self {
                value: default,
                source: ConfigSource::Default,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vesper_security::SecretSource;

    use super::*;

    #[test]
    fn raw_secret_fields_are_rejected_but_references_serialize() {
        let version = SchemaVersion::new(1).unwrap();
        let provider = ProviderId::new("zai").unwrap();
        assert!(
            ProviderConfigEnvelope::new(
                version,
                provider.clone(),
                json!({"api_key": "canary"}),
                vec![],
            )
            .is_err()
        );
        let envelope = ProviderConfigEnvelope::new(
            version,
            provider,
            json!({"endpoint": "coding"}),
            vec![SecretReference::new(SecretSource::Environment, "ZAI_API_KEY").unwrap()],
        )
        .unwrap();
        let encoded = serde_json::to_string(&envelope).unwrap();
        assert!(!encoded.contains("canary"));
        assert!(encoded.contains("ZAI_API_KEY"));
    }

    #[test]
    fn environment_precedence_is_explicit() {
        let resolved = ResolvedValue::resolve(None, Some(2), Some(1), 0);
        assert_eq!(resolved.value, 2);
        assert_eq!(resolved.source, ConfigSource::Environment);
    }
}
