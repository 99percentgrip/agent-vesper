use std::{
    collections::BTreeMap,
    fmt,
    io::Write,
    path::{Path, PathBuf},
};

use thiserror::Error;
use vesper_security::{SecretScope, SecretValue};

use crate::error::authentication_error;

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
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/agent-vesper/credentials.json")
}

/// Stores a Z.ai API key with a bounded atomic write and user-only mode.
pub fn store_api_key(key: &str) -> Result<PathBuf, AuthStoreError> {
    store_api_key_at(&credentials_path(), key)
}

/// Testable/path-explicit form of [`store_api_key`].
pub fn store_api_key_at(path: &Path, key: &str) -> Result<PathBuf, AuthStoreError> {
    let key = key.trim();
    if key.is_empty() || key.len() > 16 * 1024 || key.chars().any(char::is_control) {
        return Err(AuthStoreError::InvalidKey);
    }
    let parent = path.parent().ok_or(AuthStoreError::InvalidPath)?;
    std::fs::create_dir_all(parent).map_err(|_| AuthStoreError::Io)?;
    let temporary = parent.join(format!(".credentials-{}.tmp", std::process::id()));
    let payload = serde_json::to_vec(&serde_json::json!({"zai_api_key": key}))
        .map_err(|_| AuthStoreError::Serialize)?;
    {
        let mut file = std::fs::File::create(&temporary).map_err(|_| AuthStoreError::Io)?;
        file.write_all(&payload).map_err(|_| AuthStoreError::Io)?;
        file.write_all(b"\n").map_err(|_| AuthStoreError::Io)?;
        file.sync_all().map_err(|_| AuthStoreError::Io)?;
    }
    set_private_mode(&temporary);
    std::fs::rename(&temporary, path).map_err(|_| AuthStoreError::Io)?;
    set_private_mode(path);
    Ok(path.to_path_buf())
}

/// Safe setup-store failures; no path or key is included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AuthStoreError {
    #[error("credential key is invalid")]
    InvalidKey,
    #[error("credential path is invalid")]
    InvalidPath,
    #[error("credential storage failed")]
    Io,
    #[error("credential record could not be serialized")]
    Serialize,
}

fn load_stored_api_key(name: &str) -> Option<SecretValue> {
    if name != "ZAI_API_KEY" && name != "Z_AI_API_KEY" {
        return None;
    }
    let bytes = std::fs::read(credentials_path()).ok()?;
    if bytes.len() > 32 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let key = value.get("zai_api_key")?.as_str()?.trim();
    (!key.is_empty()).then(|| SecretValue::new(key))
}

fn set_private_mode(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
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

    #[test]
    fn stored_credentials_round_trip_without_serializing_the_secret() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("credentials.json");
        store_api_key_at(&path, "stored-canary").unwrap();
        let value = load_stored_api_key_from(&path, "ZAI_API_KEY").unwrap();
        assert_eq!(value.expose().as_str(), "stored-canary");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("stored-canary"));
    }

    fn load_stored_api_key_from(path: &Path, name: &str) -> Option<SecretValue> {
        if name != "ZAI_API_KEY" && name != "Z_AI_API_KEY" {
            return None;
        }
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        Some(SecretValue::new(value.get("zai_api_key")?.as_str()?))
    }
}
