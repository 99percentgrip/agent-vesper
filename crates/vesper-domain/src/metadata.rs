use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use thiserror::Error;

const MAX_EXTENSION_ENTRIES: usize = 128;
const MAX_EXTENSION_BYTES: usize = 64 * 1024;
const MAX_EXTENSION_DEPTH: usize = 16;

/// Namespaced extension values that foundational contracts preserve but do not interpret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionMap(BTreeMap<String, Value>);

/// Bounded owner namespace for versioned opaque metadata.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ExtensionNamespace(String);

impl ExtensionNamespace {
    /// Creates a dotted namespace such as `provider.example`.
    pub fn new(value: impl Into<String>) -> Result<Self, ExtensionError> {
        let value = value.into();
        if value.len() > 128
            || value.trim() != value
            || !value.contains('.')
            || value.chars().any(char::is_control)
        {
            return Err(ExtensionError::InvalidNamespace);
        }
        Ok(Self(value))
    }

    /// Returns the namespace.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ExtensionNamespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Rejected opaque extension content.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExtensionError {
    /// Envelope owner is not a bounded dotted namespace.
    #[error("extension namespace must be bounded and dotted")]
    InvalidNamespace,
    /// A key lacks a namespace or is too long.
    #[error("extension key must be bounded and namespaced")]
    InvalidKey,
    /// The map has too many entries.
    #[error("extension map exceeds {MAX_EXTENSION_ENTRIES} entries")]
    TooManyEntries,
    /// Encoded content is too large.
    #[error("extension map exceeds {MAX_EXTENSION_BYTES} encoded bytes")]
    TooLarge,
    /// Nested input is too deep.
    #[error("extension value exceeds {MAX_EXTENSION_DEPTH} nesting levels")]
    TooDeep,
    /// Secret-shaped keys are forbidden in generic metadata.
    #[error("extension metadata contains a secret-bearing key")]
    SecretBearingKey,
}

impl ExtensionMap {
    /// Inserts an extension. Keys must contain a namespace separator.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Result<(), ExtensionError> {
        let key = key.into();
        let mut candidate = self.0.clone();
        candidate.insert(key, value);
        validate_map(&candidate)?;
        self.0 = candidate;
        Ok(())
    }

    /// Returns a preserved extension value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Iterates in deterministic key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Returns whether the map contains no fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for ExtensionMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExtensionMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<String, Value>::deserialize(deserializer)?;
        validate_map(&values).map_err(de::Error::custom)?;
        Ok(Self(values))
    }
}

fn validate_map(values: &BTreeMap<String, Value>) -> Result<(), ExtensionError> {
    if values.len() > MAX_EXTENSION_ENTRIES {
        return Err(ExtensionError::TooManyEntries);
    }
    for (key, value) in values {
        if !key.contains(':') || key.len() > 256 || key.trim() != key {
            return Err(ExtensionError::InvalidKey);
        }
        if is_secret_key(key.rsplit(':').next().unwrap_or(key)) {
            return Err(ExtensionError::SecretBearingKey);
        }
        validate_value(value, 0)?;
    }
    let encoded = serde_json::to_vec(values).map_err(|_| ExtensionError::TooLarge)?;
    if encoded.len() > MAX_EXTENSION_BYTES {
        return Err(ExtensionError::TooLarge);
    }
    Ok(())
}

fn validate_value(value: &Value, depth: usize) -> Result<(), ExtensionError> {
    if depth > MAX_EXTENSION_DEPTH {
        return Err(ExtensionError::TooDeep);
    }
    match value {
        Value::Array(values) => {
            for value in values {
                validate_value(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if is_secret_key(&normalized) {
                    return Err(ExtensionError::SecretBearingKey);
                }
                validate_value(value, depth + 1)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "password"
            | "secret"
            | "token"
            | "api_key"
            | "authorization"
            | "access_token"
            | "refresh_token"
            | "client_secret"
    ) || ["_password", "_secret", "_token", "_api_key"]
        .iter()
        .any(|suffix| normalized.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn unknown_metadata_round_trips_without_interpretation() {
        let mut metadata = ExtensionMap::default();
        metadata
            .insert("future:field", json!({"nested": true}))
            .unwrap();
        let encoded = serde_json::to_string(&metadata).unwrap();
        let decoded: ExtensionMap = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.get("future:field"), Some(&json!({"nested": true})));
    }

    #[test]
    fn deserialization_cannot_bypass_namespace_or_secret_checks() {
        assert!(serde_json::from_str::<ExtensionMap>(r#"{"plain":true}"#).is_err());
        assert!(
            serde_json::from_str::<ExtensionMap>(
                r#"{"future:value":{"api_token":"must-not-enter-metadata"}}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<ExtensionMap>(r#"{"provider:api-key":"raw"}"#).is_err());
        assert!(
            serde_json::from_str::<ExtensionMap>(
                r#"{"provider.example:usage":{"token_count":42}}"#
            )
            .is_ok()
        );
    }
}
