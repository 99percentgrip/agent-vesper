//! Vesper-owned xAI API-key credentials. Never reads Grok Build or browser state.
use std::path::PathBuf;
#[cfg(test)]
use vesper_auth::PrivateFileCredentialStore;
use vesper_auth::{CredentialId, SecureCredentialStore};
use vesper_provider::CredentialError;
use vesper_security::{SecretScope, SecretValue};

const ID: CredentialId = CredentialId::new("xai", "api-key");

#[derive(Clone)]
enum Store {
    Secure(SecureCredentialStore),
    #[cfg(test)]
    Private(PrivateFileCredentialStore),
}

#[derive(Clone)]
pub(crate) struct Credentials {
    store: Store,
}

impl Default for Credentials {
    fn default() -> Self {
        let path = std::env::var_os("AGENT_VESPER_XAI_CREDENTIALS_PATH")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(|p| PathBuf::from(p).join("agent-vesper/xai-credentials.json"))
            })
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .map(|p| PathBuf::from(p).join("agent-vesper/xai-credentials.json"))
            })
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".config/agent-vesper/xai-credentials.json")
            });
        Self {
            store: Store::Secure(SecureCredentialStore::new("agent-vesper", path)),
        }
    }
}

impl Credentials {
    fn load_saved(&self) -> Result<Option<SecretValue>, CredentialError> {
        match &self.store {
            Store::Secure(store) => store.load(ID).map_err(|_| CredentialError::Unavailable),
            #[cfg(test)]
            Store::Private(store) => store.load(ID).map_err(|_| CredentialError::Unavailable),
        }
    }
    pub(crate) fn load(&self) -> Result<SecretValue, CredentialError> {
        SecretScope::current("XAI_API_KEY")
            .ok()
            .filter(|key| vesper_auth::validate_secret(key.expose().as_str()).is_ok())
            .or(self.load_saved()?)
            .ok_or(CredentialError::Absent)
    }
    pub(crate) fn present(&self) -> Result<bool, CredentialError> {
        match self.load() {
            Ok(_) => Ok(true),
            Err(CredentialError::Absent) => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub(crate) fn store(&self, secret: &str) -> Result<(), CredentialError> {
        vesper_auth::validate_secret(secret).map_err(|_| CredentialError::InvalidSecret)?;
        match &self.store {
            Store::Secure(store) => store
                .store(ID, secret)
                .map(|_| ())
                .map_err(|_| CredentialError::Unavailable),
            #[cfg(test)]
            Store::Private(store) => store
                .store(ID, secret)
                .map(|_| ())
                .map_err(|_| CredentialError::Unavailable),
        }
    }
    #[cfg(test)]
    pub(crate) fn isolated(path: PathBuf) -> Self {
        Self {
            store: Store::Private(PrivateFileCredentialStore::new(path)),
        }
    }
}
