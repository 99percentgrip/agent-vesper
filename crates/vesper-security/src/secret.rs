use std::fmt;

use serde::{Deserialize, Serialize, Serializer, ser};
use thiserror::Error;
use zeroize::Zeroizing;

/// Raw secret bytes with explicit exposure and zeroizing drop behavior.
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// Wraps secret text.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Explicitly exposes the secret for an authorized boundary.
    #[must_use]
    pub fn expose(&self) -> SecretExposure<'_> {
        SecretExposure(&self.0)
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Serialize for SecretValue {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(ser::Error::custom("secret values cannot be serialized"))
    }
}

/// Deliberate, short-lived secret exposure.
pub struct SecretExposure<'a>(&'a str);

impl SecretExposure<'_> {
    /// Returns secret text to the authorized caller.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0
    }
}

/// Serializable reference to a secret, never the secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawSecretReference", into = "RawSecretReference")]
pub struct SecretReference {
    /// Secret source.
    source: SecretSource,
    /// Bounded lookup key such as an environment-variable or keyring record name.
    key: String,
}

impl SecretReference {
    /// Constructs a bounded, control-free lookup reference.
    pub fn new(source: SecretSource, key: impl Into<String>) -> Result<Self, SecretReferenceError> {
        let key = key.into();
        if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
            return Err(SecretReferenceError::InvalidKey);
        }
        Ok(Self { source, key })
    }

    /// Reference store.
    #[must_use]
    pub const fn source(&self) -> SecretSource {
        self.source
    }

    /// Non-secret lookup key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawSecretReference {
    source: SecretSource,
    key: String,
}

impl TryFrom<RawSecretReference> for SecretReference {
    type Error = SecretReferenceError;

    fn try_from(value: RawSecretReference) -> Result<Self, Self::Error> {
        Self::new(value.source, value.key)
    }
}

impl From<SecretReference> for RawSecretReference {
    fn from(value: SecretReference) -> Self {
        Self {
            source: value.source,
            key: value.key,
        }
    }
}

/// Invalid non-secret secret-store reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SecretReferenceError {
    /// Lookup key is empty, too long, or contains a control character.
    #[error("secret-reference key is invalid")]
    InvalidKey,
}

/// Supported secret-reference stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretSource {
    /// Process environment.
    Environment,
    /// Operating-system keyring.
    Keyring,
    /// Vesper profile record.
    Profile,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_display_and_serialization_do_not_leak() {
        let secret = SecretValue::new("VESPER_SECRET_CANARY");
        assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");
        assert_eq!(secret.to_string(), "[REDACTED]");
        let error = serde_json::to_string(&secret).unwrap_err();
        assert!(!error.to_string().contains("VESPER_SECRET_CANARY"));
        assert_eq!(secret.expose().as_str(), "VESPER_SECRET_CANARY");
    }

    #[test]
    fn secret_references_validate_during_deserialization() {
        assert!(SecretReference::new(SecretSource::Environment, "").is_err());
        let json = r#"{"source":"environment","key":"line\nbreak"}"#;
        assert!(serde_json::from_str::<SecretReference>(json).is_err());
        let reference = SecretReference::new(SecretSource::Environment, "ZAI_API_KEY").unwrap();
        assert_eq!(reference.key(), "ZAI_API_KEY");
    }
}
