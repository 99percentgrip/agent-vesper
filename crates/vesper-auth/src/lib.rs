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

#[cfg(any(windows, test))]
mod native_records;

// Composite provider records can contain several independently bounded tokens.
const MAX_SECRET_BYTES: usize = 16 * 1024;
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_VAULT_BYTES: u64 = 1024 * 1024;
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
    validate_bounded_value(value, MAX_SECRET_BYTES)
}

fn validate_record(value: &str) -> Result<&str, CredentialStoreError> {
    validate_bounded_value(value, MAX_RECORD_BYTES)
}

fn validate_bounded_value(value: &str, maximum: usize) -> Result<&str, CredentialStoreError> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
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

    /// A retained fallback is authoritative: native writes remove it, while
    /// a newer fallback write must not resurrect an older keyring credential.
    pub fn load(&self, id: CredentialId) -> Result<Option<SecretValue>, CredentialStoreError> {
        validate_identity(id)?;
        // Preserve read-only compatibility and retained-vault precedence.
        // Windows still cannot create or permission-verify a fallback write.
        if let Some(value) = self.fallback.load(id)? {
            return Ok(Some(value));
        }
        #[cfg(windows)]
        {
            let _lease = self.native_lease()?;
            native_records::load(&NativeBackend(self.service), &keyring_account(id))
                .map(|value| value.map(|value| SecretValue::new(value.as_str())))
        }
        #[cfg(not(windows))]
        {
            let account = keyring_account(id);
            if let Ok(entry) = Entry::new(self.service, &account)
                && let Ok(value) = entry.get_password()
            {
                let value = Zeroizing::new(value);
                return Ok(Some(SecretValue::new(validate_record(&value)?)));
            }
            Ok(None)
        }
    }

    /// Saves to the OS manager, falling back only where file permissions can
    /// be verified by this crate.
    pub fn store(
        &self,
        id: CredentialId,
        secret: &str,
    ) -> Result<StoreReceipt, CredentialStoreError> {
        validate_identity(id)?;
        let secret = validate_record(secret)?;
        let account = keyring_account(id);
        #[cfg(windows)]
        {
            // Refuse before native mutation if a retained legacy vault would
            // remain authoritative and cannot be safely rewritten on Windows.
            if self.fallback.load(id)?.is_some() {
                return Err(CredentialStoreError::Unavailable);
            }
            let _lease = self.native_lease()?;
            native_records::store(&NativeBackend(self.service), &account, secret)?;
            Ok(StoreReceipt {
                backend: StorageBackend::NativeKeyring,
            })
        }
        #[cfg(not(windows))]
        {
            if let Ok(entry) = Entry::new(self.service, &account)
                && entry.set_password(secret).is_ok()
            {
                // Prevent an older fallback from resurfacing when keyring is down.
                self.fallback.remove(id)?;
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

    /// Removes one credential from the native store and the fallback vault.
    /// A missing entry is success. An entry that is still readable after a
    /// failed native delete is reported as unavailable.
    pub fn remove(&self, id: CredentialId) -> Result<(), CredentialStoreError> {
        validate_identity(id)?;
        let account = keyring_account(id);
        #[cfg(windows)]
        {
            if self.fallback.load(id)?.is_some() {
                return Err(CredentialStoreError::Unavailable);
            }
            let _lease = self.native_lease()?;
            native_records::remove(&NativeBackend(self.service), &account)
        }
        #[cfg(not(windows))]
        {
            if let Ok(entry) = Entry::new(self.service, &account)
                && entry.delete_credential().is_err()
                && entry.get_password().is_ok()
            {
                return Err(CredentialStoreError::Unavailable);
            }
            self.fallback.remove(id)
        }
    }

    #[cfg(windows)]
    fn native_lease(&self) -> Result<std::fs::File, CredentialStoreError> {
        let path = self.fallback.path().with_extension("native-lock");
        let parent = path.parent().ok_or(CredentialStoreError::InvalidPath)?;
        std::fs::create_dir_all(parent).map_err(|_| CredentialStoreError::Io)?;
        if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
            return Err(CredentialStoreError::Unavailable);
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| CredentialStoreError::Io)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(file),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
                {
                    if std::time::Instant::now() >= deadline {
                        return Err(CredentialStoreError::Unavailable);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(_) => return Err(CredentialStoreError::Unavailable),
            }
        }
    }
}

#[cfg(windows)]
struct NativeBackend(&'static str);

#[cfg(windows)]
impl native_records::Backend for NativeBackend {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, CredentialStoreError> {
        let entry = Entry::new(self.0, account).map_err(|_| CredentialStoreError::Unavailable)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialStoreError::Unavailable),
        }
    }
    fn write(&self, account: &str, value: &str) -> Result<(), CredentialStoreError> {
        Entry::new(self.0, account)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| CredentialStoreError::Unavailable)
    }
    fn delete(&self, account: &str) -> Result<(), CredentialStoreError> {
        let entry = Entry::new(self.0, account).map_err(|_| CredentialStoreError::Unavailable)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialStoreError::Unavailable),
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

    /// Removes only this credential, without creating an absent vault.
    pub fn remove(&self, id: CredentialId) -> Result<(), CredentialStoreError> {
        validate_identity(id)?;
        if !self.path.exists() {
            return Ok(());
        }
        let mut vault = self.load_vault()?;
        let removed = vault
            .credentials
            .get_mut(id.provider)
            .and_then(|entries| entries.remove(id.account));
        if let Some(mut removed) = removed {
            removed.zeroize();
            write_private_vault(&self.path, &vault)?;
        }
        Ok(())
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
            .and_then(|value| validate_record(value).ok())
            .map(SecretValue::new))
    }

    /// Atomically merges one credential into the private vault.
    pub fn store(
        &self,
        id: CredentialId,
        secret: &str,
    ) -> Result<StoreReceipt, CredentialStoreError> {
        validate_identity(id)?;
        let secret = validate_record(secret)?;
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
    if payload.len().saturating_add(1) as u64 > MAX_VAULT_BYTES {
        return Err(CredentialStoreError::Unavailable);
    }
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
    #[cfg(unix)]
    use tempfile::TempDir;

    #[cfg(unix)]
    const ZAI: CredentialId = CredentialId::new("zai", "api-key");

    #[cfg(unix)]
    #[test]
    fn removing_one_credential_preserves_neighbors_and_does_not_create_a_vault() {
        let temp = TempDir::new().unwrap();
        let store = PrivateFileCredentialStore::new(temp.path().join("vault.json"));
        store.remove(ZAI).unwrap();
        assert!(!store.path().exists());
        let other = CredentialId::new("other", "auth");
        store.store(ZAI, "old-key").unwrap();
        store.store(other, "neighbor-key").unwrap();
        store.remove(ZAI).unwrap();
        assert!(store.load(ZAI).unwrap().is_none());
        assert_eq!(
            store.load(other).unwrap().unwrap().expose().as_str(),
            "neighbor-key"
        );
    }

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
        assert!(validate_secret(&"x".repeat(MAX_SECRET_BYTES + 1)).is_err());
        assert!(validate_record(&"x".repeat(MAX_SECRET_BYTES + 1)).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn private_vault_large_composite_records_and_overflow_preserve_previous_value() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("vault.json");
        let store = PrivateFileCredentialStore::new(path.clone());
        let record = "synthetic-composite".repeat(2000);
        store.store(ZAI, &record).unwrap();
        assert_eq!(store.load(ZAI).unwrap().unwrap().expose().as_str(), record);
        let before = std::fs::read(&path).unwrap();
        let mut vault = store.load_vault().unwrap();
        // Each record is valid, but the aggregate serialized vault is bounded.
        for index in 0..9 {
            vault
                .credentials
                .entry("fixture".into())
                .or_default()
                .insert(format!("key-{index}"), "x".repeat(MAX_RECORD_BYTES));
        }
        assert!(write_private_vault(&path, &vault).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(store.load(ZAI).unwrap().unwrap().expose().as_str(), record);
    }

    #[cfg(windows)]
    #[test]
    fn native_lease_waits_for_contended_windows_lock_and_releases_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let store =
            SecureCredentialStore::new("fixture-no-keyring-access", temp.path().join("vault.json"));
        let held = store.native_lease().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            sender.send(store.native_lease()).unwrap();
        });
        assert!(
            receiver
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        drop(held);
        let acquired = receiver
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap();
        drop(acquired);
        thread.join().unwrap();
        assert!(!temp.path().join("vault.json").exists());
    }

    #[cfg(windows)]
    #[test]
    fn retained_windows_vault_is_read_only_and_blocks_unsettleable_mutations() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.json");
        let bytes = br#"{"credentials":{"fixture":{"auth":"retained-synthetic-key"}}}"#;
        std::fs::write(&path, bytes).unwrap();
        let id = CredentialId::new("fixture", "auth");
        let store = SecureCredentialStore::new("fixture-no-native-access", path.clone());
        assert_eq!(
            store.load(id).unwrap().unwrap().expose().as_str(),
            "retained-synthetic-key"
        );
        assert!(store.store(id, "replacement").is_err());
        assert!(store.remove(id).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!path.with_extension("native-lock").exists());
    }
}
