#![forbid(unsafe_code)]
//! Provider-neutral secure credential persistence.

use std::{
    collections::BTreeMap,
    fmt,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use keyring::v1::Entry;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use vesper_security::SecretValue;
use zeroize::{Zeroize, Zeroizing};

const MAX_SECRET_BYTES: usize = 16 * 1024;
const MAX_VAULT_BYTES: u64 = 64 * 1024;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Stable identity of one provider credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CredentialId {
    /// Provider-neutral registry identity.
    pub provider: &'static str,
    /// OS-keyring account name and fallback-vault field.
    pub account: &'static str,
}

impl CredentialId {
    /// Creates a statically registered credential identity.
    #[must_use]
    pub const fn new(provider: &'static str, account: &'static str) -> Self {
        Self { provider, account }
    }
}

/// Where a credential was durably saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageBackend {
    /// The operating system's native credential manager.
    NativeKeyring,
    /// An atomic owner-only local vault.
    PrivateFile(PathBuf),
}

/// Secret-safe result of a successful store operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreReceipt {
    /// Backend that accepted the credential.
    pub backend: StorageBackend,
}

/// Bounded, secret-safe storage failure.
#[derive(Debug, Error)]
pub enum CredentialStoreError {
    /// Secret failed local structural validation.
    #[error("credential value is invalid")]
    InvalidSecret,
    /// Provider/account identity is unsuitable for a storage key.
    #[error("credential identity is invalid")]
    InvalidIdentity,
    /// Vault path has no safe parent.
    #[error("credential vault path is invalid")]
    InvalidPath,
    /// Neither native storage nor a permission-verifiable fallback is available.
    #[error("secure credential storage is unavailable")]
    Unavailable,
    /// A bounded filesystem operation failed.
    #[error("credential vault operation failed")]
    Io,
    /// Vault serialization failed.
    #[error("credential vault serialization failed")]
    Serialize,
}

/// Validates a secret without imposing provider-invented formatting rules.
pub fn validate_secret(value: &str) -> Result<&str, CredentialStoreError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_SECRET_BYTES || value.chars().any(char::is_control) {
        return Err(CredentialStoreError::InvalidSecret);
    }
    Ok(value)
}

/// Native-first production credential store.
#[derive(Clone)]
pub struct SecureCredentialStore {
    service: &'static str,
    fallback: PrivateFileCredentialStore,
}

impl fmt::Debug for SecureCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecureCredentialStore")
            .field("service", &self.service)
            .field("fallback", &self.fallback)
            .finish()
    }
}

impl SecureCredentialStore {
    /// Creates a native-first store with one explicit fallback vault.
    #[must_use]
    pub fn new(service: &'static str, fallback_path: PathBuf) -> Self {
        Self {
            service,
            fallback: PrivateFileCredentialStore::new(fallback_path),
        }
    }

    /// Loads from the native store first, then the compatibility fallback.
    pub fn load(&self, id: CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
        validate_identity(id)?;
        let account = keyring_account(id);
        if let Ok(entry) = Entry::new(self.service, &account)
            && let Ok(value) = entry.get_password()
        {
            let value = Zeroizing::new(value);
            return Ok(Some(SecretValue::new(validate_secret(&value)?)));
        }
        self.fallback.load(id)
    }

    /// Saves to the OS manager, falling back only where file permissions can
    /// be verified by this crate.
    pub fn store(
        &self,
        id: CredentialId,
        secret: &str,
    ) -> Result<StoreReceipt, CredentialStoreError> {
        validate_identity(id)?;
        let secret = validate_secret(secret)?;
        let account = keyring_account(id);
        if let Ok(entry) = Entry::new(self.service, &account)
            && entry.set_password(secret).is_ok()
        {
            return Ok(StoreReceipt {
                backend: StorageBackend::NativeKeyring,
            });
        }
        #[cfg(unix)]
        {
            self.fallback.store(id, secret)
        }
        #[cfg(not(unix))]
        {
            Err(CredentialStoreError::Unavailable)
        }
    }
}

/// Path-explicit private store used for Unix fallback and deterministic tests.
#[derive(Clone, Debug)]
pub struct PrivateFileCredentialStore {
    path: PathBuf,
}

impl PrivateFileCredentialStore {
    /// Creates a private store. No filesystem state changes until `store`.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns the descriptive vault path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads one bounded credential from the fallback vault.
    pub fn load(&self, id: CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
        validate_identity(id)?;
        let metadata = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CredentialStoreError::Io),
        };
        if metadata.len() > MAX_VAULT_BYTES {
            return Err(CredentialStoreError::Io);
        }
        let bytes = std::fs::read(&self.path).map_err(|_| CredentialStoreError::Io)?;
        let vault: Vault =
            serde_json::from_slice(&bytes).map_err(|_| CredentialStoreError::Serialize)?;
        Ok(vault
            .credentials
            .get(id.provider)
            .and_then(|entries| entries.get(id.account))
            .and_then(|value| validate_secret(value).ok())
            .map(SecretValue::new))
    }

    /// Atomically merges one credential into the private vault.
    pub fn store(
        &self,
        id: CredentialId,
        secret: &str,
    ) -> Result<StoreReceipt, CredentialStoreError> {
        validate_identity(id)?;
        let secret = validate_secret(secret)?;
        let mut vault = self.load_vault()?;
        vault
            .credentials
            .entry(id.provider.to_owned())
            .or_default()
            .insert(id.account.to_owned(), secret.to_owned());
        write_private_vault(&self.path, &vault)?;
        Ok(StoreReceipt {
            backend: StorageBackend::PrivateFile(self.path.clone()),
        })
    }

    fn load_vault(&self) -> Result<Vault, CredentialStoreError> {
        if !self.path.exists() {
            return Ok(Vault::default());
        }
        let metadata = std::fs::metadata(&self.path).map_err(|_| CredentialStoreError::Io)?;
        if metadata.len() > MAX_VAULT_BYTES {
            return Err(CredentialStoreError::Io);
        }
        serde_json::from_slice(&std::fs::read(&self.path).map_err(|_| CredentialStoreError::Io)?)
            .map_err(|_| CredentialStoreError::Serialize)
    }
}

#[derive(Default, Deserialize, Serialize)]
struct Vault {
    #[serde(default)]
    credentials: BTreeMap<String, BTreeMap<String, String>>,
}

impl Drop for Vault {
    fn drop(&mut self) {
        for entries in self.credentials.values_mut() {
            for secret in entries.values_mut() {
                secret.zeroize();
            }
        }
    }
}

fn validate_identity(id: CredentialId) -> Result<(), CredentialStoreError> {
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if valid(id.provider) && valid(id.account) {
        Ok(())
    } else {
        Err(CredentialStoreError::InvalidIdentity)
    }
}

fn keyring_account(id: CredentialId) -> String {
    format!("{}:{}", id.provider, id.account)
}

fn write_private_vault(path: &Path, vault: &Vault) -> Result<(), CredentialStoreError> {
    let parent = path.parent().ok_or(CredentialStoreError::InvalidPath)?;
    let parent_existed = parent.exists();
    std::fs::create_dir_all(parent).map_err(|_| CredentialStoreError::Io)?;
    if !parent_existed {
        set_private_directory_mode(parent)?;
    }
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".credentials-{}-{sequence}.tmp",
        std::process::id()
    ));
    let payload =
        Zeroizing::new(serde_json::to_vec(vault).map_err(|_| CredentialStoreError::Serialize)?);
    let write_result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| CredentialStoreError::Io)?;
        file.write_all(&payload)
            .map_err(|_| CredentialStoreError::Io)?;
        file.write_all(b"\n")
            .map_err(|_| CredentialStoreError::Io)?;
        file.sync_all().map_err(|_| CredentialStoreError::Io)?;
        set_private_file_mode(&temporary)?;
        std::fs::rename(&temporary, path).map_err(|_| CredentialStoreError::Io)?;
        set_private_file_mode(path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

fn set_private_directory_mode(path: &Path) -> Result<(), CredentialStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| CredentialStoreError::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(CredentialStoreError::Unavailable)
    }
}

fn set_private_file_mode(path: &Path) -> Result<(), CredentialStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| CredentialStoreError::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(CredentialStoreError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const ZAI: CredentialId = CredentialId::new("zai", "api-key");

    #[cfg(unix)]
    #[test]
    fn private_vault_round_trips_and_never_formats_secret() {
        let temp = TempDir::new().unwrap();
        let store = PrivateFileCredentialStore::new(temp.path().join("vault.json"));
        let receipt = store.store(ZAI, "secret-canary").unwrap();
        assert_eq!(
            store.load(ZAI).unwrap().unwrap().expose().as_str(),
            "secret-canary"
        );
        assert!(!format!("{store:?} {receipt:?}").contains("secret-canary"));
    }

    #[cfg(unix)]
    #[test]
    fn private_vault_and_parent_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("auth");
        let path = parent.join("vault.json");
        PrivateFileCredentialStore::new(path.clone())
            .store(ZAI, "permission-canary")
            .unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn invalid_secrets_are_rejected() {
        assert!(validate_secret("").is_err());
        assert!(validate_secret("line\nbreak").is_err());
    }
}
