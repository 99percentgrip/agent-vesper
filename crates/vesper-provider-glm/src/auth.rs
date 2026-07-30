use std::{collections::BTreeMap, fmt};

use vesper_security::SecretValue;

use crate::error::authentication_error;

/// Injectable credential source. Values remain secret wrappers at the boundary.
pub trait GlmCredentialSource: Send + Sync {
    /// Resolves one environment-compatible credential name.
    fn credential(&self, name: &str) -> Option<SecretValue>;
}

/// Production process-environment source.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvironmentCredentialSource;

impl GlmCredentialSource for EnvironmentCredentialSource {
    fn credential(&self, name: &str) -> Option<SecretValue> {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretValue::new)
    }
}

/// Deterministic source for applications/tests that already hold secret-safe
/// values. This type deliberately does not expose iteration or serialization.
#[derive(Default)]
pub struct StaticCredentialSource {
    values: BTreeMap<String, SecretValue>,
}

impl StaticCredentialSource {
    /// Adds one synthetic or externally resolved value.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: SecretValue) -> Self {
        self.values.insert(name.into(), value);
        self
    }
}

impl fmt::Debug for StaticCredentialSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StaticCredentialSource")
            .field("entries", &self.values.len())
            .finish()
    }
}

impl GlmCredentialSource for StaticCredentialSource {
    fn credential(&self, name: &str) -> Option<SecretValue> {
        self.values
            .get(name)
            .map(|value| SecretValue::new(value.expose().as_str()))
    }
}

/// Resolves `ZAI_API_KEY` before the legacy `Z_AI_API_KEY` alias.
pub fn resolve_credential(
    source: &dyn GlmCredentialSource,
) -> Result<SecretValue, Box<vesper_provider::ProviderError>> {
    source
        .credential("ZAI_API_KEY")
        .or_else(|| source.credential("Z_AI_API_KEY"))
        .ok_or_else(|| Box::new(authentication_error()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_debug_are_secret_safe() {
        let source = StaticCredentialSource::default()
            .with("Z_AI_API_KEY", SecretValue::new("legacy-canary"))
            .with("ZAI_API_KEY", SecretValue::new("primary-canary"));
        let credential = resolve_credential(&source).unwrap();
        assert_eq!(credential.expose().as_str(), "primary-canary");
        let debug = format!("{source:?} {credential:?}");
        assert!(!debug.contains("primary-canary"));
        assert!(!debug.contains("legacy-canary"));
    }
}
