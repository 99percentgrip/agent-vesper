use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use vesper_auth::{CredentialId, PrivateFileCredentialStore, SecureCredentialStore};
pub use vesper_auth::{CredentialStoreError as AuthStoreError, StoreReceipt};
use vesper_security::{SecretScope, SecretValue};

use crate::error::authentication_error;

/// Registered secure-storage identity for the real Z.ai adapter.
pub const ZAI_CREDENTIAL_ID: CredentialId = CredentialId::new("zai", "api-key");

/// Injectable credential source. Values remain secret wrappers at the boundary.
pub trait GlmCredentialSource: Send + Sync {
    /// Resolves one environment-compatible credential name.
    fn credential(&self, name: &str) -> Option<SecretValue>;
}

/// Production process-environment source.
///
/// Delegates to [`SecretScope::current`] so credential resolution is
/// scope-aware: when a task-local [`SecretScope`] is installed (multi-profile
/// multiplexing), credentials come from the active scope; in single-profile
/// mode it transparently falls back to `std::env`.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvironmentCredentialSource;

impl GlmCredentialSource for EnvironmentCredentialSource {
    fn credential(&self, name: &str) -> Option<SecretValue> {
        SecretScope::current(name)
            .ok()
            .filter(|secret| vesper_auth::validate_secret(secret.expose().as_str()).is_ok())
            .or_else(|| load_stored_api_key(name))
    }
}

/// Returns the user-only credential file used by `--setup` when no explicit
/// path is configured. The path is descriptive until a caller stores a key.
#[must_use]
pub fn credentials_path() -> PathBuf {
    if let Some(path) = std::env::var_os("AGENT_VESPER_CREDENTIALS_PATH") {
        return PathBuf::from(path);
    }
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(base).join("agent-vesper/credentials.json");
    }
    #[cfg(windows)]
    if let Some(base) = std::env::var_os("APPDATA") {
        return PathBuf::from(base).join("agent-vesper/credentials.json");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/agent-vesper/credentials.json")
}

/// Stores a Z.ai API key in the OS credential manager, with an owner-only
/// Unix vault fallback when the native service is unavailable.
pub fn store_api_key(key: &str) -> Result<StoreReceipt, AuthStoreError> {
    credential_store().store(ZAI_CREDENTIAL_ID, key)
}

/// Testable/path-explicit form of [`store_api_key`].
pub fn store_api_key_at(path: &Path, key: &str) -> Result<PathBuf, AuthStoreError> {
    PrivateFileCredentialStore::new(path.to_path_buf()).store(ZAI_CREDENTIAL_ID, key)?;
    Ok(path.to_path_buf())
}

fn load_stored_api_key(name: &str) -> Option<SecretValue> {
    if name != "ZAI_API_KEY" && name != "Z_AI_API_KEY" {
        return None;
    }
    if let Ok(Some(secret)) = credential_store().load(ZAI_CREDENTIAL_ID) {
        return Some(secret);
    }
    // Backward-compatible read for credentials written before the native
    // credential manager was introduced. New writes use the generic vault.
    let bytes = std::fs::read(credentials_path()).ok()?;
    if bytes.len() > 32 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let key = value.get("zai_api_key")?.as_str()?;
    vesper_auth::validate_secret(key).ok().map(SecretValue::new)
}

fn credential_store() -> SecureCredentialStore {
    SecureCredentialStore::new("agent-vesper", credentials_path())
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
    #[cfg(unix)]
    use tempfile::TempDir;

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

    #[cfg(unix)]
    #[test]
    fn stored_credentials_round_trip_through_private_vault() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("credentials.json");
        store_api_key_at(&path, "stored-canary").unwrap();
        let value = PrivateFileCredentialStore::new(path.clone())
            .load(ZAI_CREDENTIAL_ID)
            .unwrap()
            .unwrap();
        assert_eq!(value.expose().as_str(), "stored-canary");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("stored-canary"));
    }
}
