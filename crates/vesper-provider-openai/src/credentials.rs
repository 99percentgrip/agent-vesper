//! Vesper-owned credentials: never reads another application's authentication.
use crate::auth::{AuthenticationMode, NativeAuthClient, SubscriptionTokens};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use vesper_auth::{CredentialId, SecureCredentialStore};
use vesper_provider::{CancellationSignal, CredentialError};
use vesper_security::{SecretScope, SecretValue};
use zeroize::{Zeroize, Zeroizing};

struct Stored(Value);
impl std::ops::Deref for Stored {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
impl Drop for Stored {
    fn drop(&mut self) {
        fn clear(value: &mut Value) {
            match value {
                Value::String(text) => text.zeroize(),
                Value::Array(values) => values.iter_mut().for_each(clear),
                Value::Object(values) => values.values_mut().for_each(clear),
                _ => {}
            }
        }
        clear(&mut self.0);
    }
}

const ID: CredentialId = CredentialId::new("openai", "native-auth");
static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();

/// Provider-owned storage and refresh coordinator shared across sessions.
#[derive(Clone)]
pub struct Credentials {
    store: SecureCredentialStore,
    lock_path: PathBuf,
    lock: Arc<Mutex<()>>,
    #[cfg(test)]
    test_store: Option<vesper_auth::PrivateFileCredentialStore>,
}

/// Dispatch-only credentials; formatting is redacted.
#[derive(Clone, Debug)]
pub(crate) struct DispatchAuth {
    pub mode: AuthenticationMode,
    pub bearer: SecretValue,
    pub account: Option<SecretValue>,
}

impl Default for Credentials {
    fn default() -> Self {
        let path = std::env::var_os("AGENT_VESPER_OPENAI_CREDENTIALS_PATH")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(|p| PathBuf::from(p).join("agent-vesper/openai-credentials.json"))
            })
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .map(|p| PathBuf::from(p).join("agent-vesper/openai-credentials.json"))
            })
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".config/agent-vesper/openai-credentials.json")
            });
        Self {
            lock_path: path.with_extension("lock"),
            store: SecureCredentialStore::new("agent-vesper", path),
            lock: LOCK.get_or_init(|| Arc::new(Mutex::new(()))).clone(),
            #[cfg(test)]
            test_store: None,
        }
    }
}

impl Credentials {
    async fn operation_lock(
        &self,
        cancel: &dyn CancellationSignal,
    ) -> Result<tokio::sync::MutexGuard<'_, ()>, CredentialError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if cancel.is_cancelled() || tokio::time::Instant::now() >= deadline {
                return Err(CredentialError::Unavailable);
            }
            if let Ok(guard) = self.lock.try_lock() {
                return Ok(guard);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    // OS advisory locks survive neither process exit nor a dropped operation;
    // TUI and ACP cannot rotate the same refresh token concurrently.
    fn try_process_lock(&self) -> Result<Option<std::fs::File>, CredentialError> {
        #[cfg(test)]
        let path = self
            .test_store
            .as_ref()
            .map(|store| store.path().with_extension("lock"))
            .unwrap_or_else(|| self.lock_path.clone());
        #[cfg(not(test))]
        let path = self.lock_path.clone();
        let parent = path.parent().ok_or(CredentialError::Unavailable)?;
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|_| CredentialError::Unavailable)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| CredentialError::Unavailable)?;
            }
        }
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.is_file()) {
            return Err(CredentialError::Unavailable);
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|_| CredentialError::Unavailable)?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(_) => Err(CredentialError::Unavailable),
        }
    }
    fn process_lock(&self) -> Result<std::fs::File, CredentialError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(file) = self.try_process_lock()? {
                return Ok(file);
            }
            if std::time::Instant::now() >= deadline {
                return Err(CredentialError::Unavailable);
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    async fn process_lock_async(
        &self,
        cancel: &dyn CancellationSignal,
    ) -> Result<std::fs::File, CredentialError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if cancel.is_cancelled() || tokio::time::Instant::now() >= deadline {
                return Err(CredentialError::Unavailable);
            }
            let this = self.clone();
            if let Some(file) = tokio::task::spawn_blocking(move || this.try_process_lock())
                .await
                .map_err(|_| CredentialError::Unavailable)??
            {
                return Ok(file);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    fn read(&self) -> Result<Stored, CredentialError> {
        #[cfg(test)]
        if let Some(store) = &self.test_store {
            return Self::decode(store.load(ID).map_err(|_| CredentialError::Unavailable)?);
        }
        Self::decode(
            self.store
                .load(ID)
                .map_err(|_| CredentialError::Unavailable)?,
        )
    }
    fn decode(value: Option<SecretValue>) -> Result<Stored, CredentialError> {
        match value {
            Some(value) => serde_json::from_str(value.expose().as_str())
                .map(Stored)
                .map_err(|_| CredentialError::InvalidSecret),
            None => Ok(Stored(Value::Null)),
        }
    }
    fn write(&self, value: Value) -> Result<(), CredentialError> {
        let value = Stored(value);
        let encoded =
            Zeroizing::new(serde_json::to_string(&value.0).map_err(|_| CredentialError::Failed)?);
        #[cfg(test)]
        if let Some(store) = &self.test_store {
            store
                .store(ID, &encoded)
                .map_err(|_| CredentialError::Unavailable)?;
            return Ok(());
        }
        self.store
            .store(ID, &encoded)
            .map_err(|_| CredentialError::Unavailable)?;
        Ok(())
    }
    /// Locally checks only the selected mode; never opens a network connection.
    pub fn authentication_method(&self) -> Result<Option<String>, CredentialError> {
        let value = self.read()?;
        match value.get("mode").and_then(Value::as_str) {
            Some("chatgpt") => Ok(Some("openai-chatgpt".into())),
            Some("api-key") => Ok(Some("openai-api-key".into())),
            Some("signed-out") | None => Ok(None),
            _ => Err(CredentialError::InvalidSecret),
        }
    }
    /// Locally checks only the selected mode; never opens a network connection.
    pub fn present(&self) -> Result<bool, CredentialError> {
        let value = self.read()?;
        match value.get("mode").and_then(Value::as_str) {
            Some("chatgpt") => Ok(tokens(&value).is_ok()),
            Some("signed-out") => Ok(false),
            Some("api-key") | None => Ok(api_key(&value).is_some()),
            _ => Err(CredentialError::InvalidSecret),
        }
    }
    /// Explicitly selects API billing and stores the key in Vesper's vault.
    pub fn store_api_key(&self, key: &str) -> Result<(), CredentialError> {
        vesper_auth::validate_secret(key).map_err(|_| CredentialError::InvalidSecret)?;
        let _guard = self.lock.blocking_lock();
        let _process = self.process_lock()?;
        self.write(json!({"mode":"api-key","api_key":key}))
    }
    /// Writes a tombstone so an environment key cannot silently re-enable billing.
    pub fn logout(&self) -> Result<(), CredentialError> {
        let _guard = self.lock.blocking_lock();
        let _process = self.process_lock()?;
        self.write(json!({"mode":"signed-out"}))
    }
    /// Completes native user-initiated sign-in, persisting only after success.
    pub async fn login(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
    ) -> Result<(), CredentialError> {
        // Hold the operation lock so a logout or concurrent rotation cannot be
        // overwritten by an older in-flight login completion.
        let _guard = self.operation_lock(cancel.as_ref()).await?;
        let _process = self.process_lock_async(cancel.as_ref()).await?;
        let client = NativeAuthClient::new().map_err(|_| CredentialError::Failed)?;
        let login = client
            .begin_device_login(cancel.as_ref())
            .await
            .map_err(|_| CredentialError::Failed)?;
        on_challenge(login.verification_url(), login.user_code());
        let result = client
            .complete_device_login(login, cancel.as_ref())
            .await
            .map_err(|_| CredentialError::Failed)?;
        if cancel.is_cancelled() {
            return Err(CredentialError::Failed);
        }
        account(&result)?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.write(token_value(&result)))
            .await
            .map_err(|_| CredentialError::Failed)?
    }
    pub(crate) async fn dispatch(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        force_refresh: bool,
    ) -> Result<DispatchAuth, CredentialError> {
        let _guard = self.operation_lock(cancel.as_ref()).await?;
        let _process = self.process_lock_async(cancel.as_ref()).await?;
        if cancel.is_cancelled() {
            return Err(CredentialError::Failed);
        }
        let this = self.clone();
        let value = tokio::task::spawn_blocking(move || this.read())
            .await
            .map_err(|_| CredentialError::Failed)??;
        if value.get("mode").and_then(Value::as_str) != Some("chatgpt") {
            if value.get("mode").and_then(Value::as_str) == Some("signed-out") {
                return Err(CredentialError::Absent);
            }
            if !value.is_null() && value.get("mode").and_then(Value::as_str) != Some("api-key") {
                return Err(CredentialError::InvalidSecret);
            }
            return Ok(DispatchAuth {
                mode: AuthenticationMode::ApiKey,
                bearer: api_key(&value).ok_or(CredentialError::Absent)?,
                account: None,
            });
        }
        let mut saved = tokens(&value)?;
        let expiry = jwt(saved.access_token()).and_then(|v| v.get("exp").and_then(Value::as_u64));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CredentialError::Failed)?
            .as_secs();
        if force_refresh || expiry.is_none_or(|exp| exp <= now.saturating_add(60)) {
            saved = NativeAuthClient::new()
                .map_err(|_| CredentialError::Failed)?
                .refresh(&saved, cancel.as_ref())
                .await
                .map_err(|_| CredentialError::Failed)?;
            let next = token_value(&saved);
            let this = self.clone();
            tokio::task::spawn_blocking(move || this.write(next))
                .await
                .map_err(|_| CredentialError::Failed)??;
        }
        Ok(DispatchAuth {
            mode: AuthenticationMode::ChatGpt,
            bearer: saved.access_token.clone(),
            account: Some(account(&saved)?),
        })
    }
}

fn api_key(value: &Value) -> Option<SecretValue> {
    SecretScope::current("OPENAI_API_KEY")
        .ok()
        .filter(|v| vesper_auth::validate_secret(v.expose().as_str()).is_ok())
        .or_else(|| {
            value
                .get("api_key")
                .and_then(Value::as_str)
                .filter(|s| vesper_auth::validate_secret(s).is_ok())
                .map(SecretValue::new)
        })
}
fn tokens(value: &Value) -> Result<SubscriptionTokens, CredentialError> {
    let get = |key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| vesper_auth::validate_secret(s).is_ok())
            .map(SecretValue::new)
            .ok_or(CredentialError::InvalidSecret)
    };
    Ok(SubscriptionTokens {
        access_token: get("access_token")?,
        refresh_token: get("refresh_token")?,
        id_token: get("id_token")?,
    })
}
fn token_value(tokens: &SubscriptionTokens) -> Value {
    json!({"mode":"chatgpt","access_token":tokens.access_token().expose().as_str(),"refresh_token":tokens.refresh_token().expose().as_str(),"id_token":tokens.id_token().expose().as_str()})
}
fn jwt(token: &SecretValue) -> Option<Value> {
    let exposed = token.expose();
    let part = exposed.as_str().split('.').nth(1)?;
    if part.len() > 16_384 {
        return None;
    }
    let bytes = Zeroizing::new(URL_SAFE_NO_PAD.decode(part).ok()?);
    serde_json::from_slice(&bytes).ok()
}
fn account(tokens: &SubscriptionTokens) -> Result<SecretValue, CredentialError> {
    let id = jwt(tokens.id_token())
        .or_else(|| jwt(tokens.access_token()))
        .ok_or(CredentialError::InvalidSecret)?;
    let value = id
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .ok_or(CredentialError::InvalidSecret)?;
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(CredentialError::InvalidSecret);
    }
    Ok(SecretValue::new(value))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn isolated_storage_switches_modes_and_logout_cannot_restore_a_key() {
        let temp = tempfile::tempdir().unwrap();
        let credentials = Credentials {
            test_store: Some(vesper_auth::PrivateFileCredentialStore::new(
                temp.path().join("credentials.json"),
            )),
            lock: Arc::new(Mutex::new(())),
            ..Credentials::default()
        };
        credentials.store_api_key("fixture-key").unwrap();
        let process_lock = credentials.try_process_lock().unwrap().unwrap();
        assert!(credentials.try_process_lock().unwrap().is_none());
        drop(process_lock);
        assert!(credentials.try_process_lock().unwrap().is_some());
        assert!(credentials.present().unwrap());
        assert_eq!(credentials.read().unwrap()["mode"], "api-key");
        credentials.write(json!({"mode":"chatgpt","access_token":"fixture-access","refresh_token":"fixture-refresh","id_token":"fixture-id"})).unwrap();
        assert!(credentials.present().unwrap());
        assert!(credentials.read().unwrap().get("api_key").is_none());
        credentials.logout().unwrap();
        assert!(!credentials.present().unwrap());
        let vault = std::fs::read_to_string(temp.path().join("credentials.json")).unwrap();
        for old in [
            "fixture-key",
            "fixture-access",
            "fixture-refresh",
            "fixture-id",
        ] {
            assert!(!vault.contains(old));
        }
        credentials
            .store_api_key("replacement-fixture-key")
            .unwrap();
        assert!(credentials.present().unwrap());
    }

    #[test]
    fn account_claims_are_bounded_and_dispatch_debug_is_redacted() {
        let claim = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}});
        let jwt = format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(claim.to_string()));
        let tokens = SubscriptionTokens {
            access_token: SecretValue::new("fixture-access"),
            refresh_token: SecretValue::new("fixture-refresh"),
            id_token: SecretValue::new(jwt),
        };
        let auth = DispatchAuth {
            mode: AuthenticationMode::ChatGpt,
            bearer: tokens.access_token.clone(),
            account: Some(account(&tokens).unwrap()),
        };
        let debug = format!("{auth:?}");
        assert!(!debug.contains("fixture-access"));
        assert!(!debug.contains("fixture-account"));
    }
}
