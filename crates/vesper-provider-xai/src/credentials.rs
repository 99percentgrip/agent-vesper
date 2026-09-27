//! Vesper-owned xAI credentials. Never reads Grok Build, browser, or foreign app state.
use crate::auth::{NativeAuthClient, SessionTokens};
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

const ID: CredentialId = CredentialId::new("xai", "native-auth");
static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();

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
                Value::String(v) => v.zeroize(),
                Value::Array(v) => v.iter_mut().for_each(clear),
                Value::Object(v) => v.values_mut().for_each(clear),
                _ => {}
            }
        }
        clear(&mut self.0);
    }
}

#[derive(Clone)]
pub(crate) struct Credentials {
    store: SecureCredentialStore,
    lock_path: PathBuf,
    lock: Arc<Mutex<()>>,
    #[cfg(test)]
    test_store: Option<TestCredentialStore>,
}

#[cfg(test)]
#[derive(Clone)]
struct TestCredentialStore {
    path: PathBuf,
    value: Arc<std::sync::Mutex<Option<Zeroizing<String>>>>,
}

#[cfg(test)]
impl TestCredentialStore {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            value: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn load(&self) -> Result<Option<SecretValue>, CredentialError> {
        let value = self
            .value
            .lock()
            .map_err(|_| CredentialError::Unavailable)?;
        Ok(value
            .as_ref()
            .map(|secret| SecretValue::new(secret.as_str())))
    }

    fn store(&self, secret: &str) -> Result<(), CredentialError> {
        let mut value = self
            .value
            .lock()
            .map_err(|_| CredentialError::Unavailable)?;
        *value = Some(Zeroizing::new(secret.to_owned()));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthenticationMode {
    ApiKey,
    GrokSession,
}

#[derive(Clone, Debug)]
pub(crate) struct DispatchAuth {
    pub mode: AuthenticationMode,
    pub bearer: SecretValue,
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
    fn lock_path(&self) -> PathBuf {
        #[cfg(test)]
        if let Some(store) = &self.test_store {
            return store.path.with_extension("lock");
        }
        self.lock_path.clone()
    }
    fn try_process_lock(&self) -> Result<Option<std::fs::File>, CredentialError> {
        let path = self.lock_path();
        let parent = path.parent().ok_or(CredentialError::Unavailable)?;
        std::fs::create_dir_all(parent).map_err(|_| CredentialError::Unavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| CredentialError::Unavailable)?;
        }
        if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
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
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
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
            return decode(store.load()?);
        }
        decode(
            self.store
                .load(ID)
                .map_err(|_| CredentialError::Unavailable)?,
        )
    }
    fn write(&self, value: Value) -> Result<(), CredentialError> {
        let value = Stored(value);
        let encoded =
            Zeroizing::new(serde_json::to_string(&value.0).map_err(|_| CredentialError::Failed)?);
        #[cfg(test)]
        if let Some(store) = &self.test_store {
            store.store(&encoded)?;
            return Ok(());
        }
        self.store
            .store(ID, &encoded)
            .map_err(|_| CredentialError::Unavailable)?;
        Ok(())
    }
    pub(crate) fn authentication_method(&self) -> Result<Option<String>, CredentialError> {
        let value = self.read()?;
        match value.get("mode").and_then(Value::as_str) {
            Some("grok-session") => {
                session_tokens(&value)?;
                Ok(Some("xai-grok-session".into()))
            }
            Some("api-key") => {
                if api_key(&value).is_some() || scoped_key().is_some() {
                    Ok(Some("xai-api-key".into()))
                } else {
                    Err(CredentialError::InvalidSecret)
                }
            }
            Some("signed-out") => Ok(None),
            None if scoped_key().is_some() => Ok(Some("xai-api-key".into())),
            None => Ok(None),
            _ => Err(CredentialError::InvalidSecret),
        }
    }
    pub(crate) fn present(&self) -> Result<bool, CredentialError> {
        Ok(self.authentication_method()?.is_some())
    }
    pub(crate) fn store_api_key(&self, secret: &str) -> Result<(), CredentialError> {
        self.store_method("xai-api-key", secret)
    }
    pub(crate) fn store_method(
        &self,
        method_id: &str,
        secret: &str,
    ) -> Result<(), CredentialError> {
        if method_id != "xai-api-key" {
            return Err(CredentialError::Unavailable);
        }
        vesper_auth::validate_secret(secret).map_err(|_| CredentialError::InvalidSecret)?;
        let _guard = self.lock.blocking_lock();
        let _process = self.process_lock()?;
        let mut value = object(self.read()?.0.clone());
        value["mode"] = json!("api-key");
        value["api_key"] = json!(secret);
        self.write(value)
    }
    pub(crate) fn select_method(&self, method_id: &str) -> Result<(), CredentialError> {
        let _guard = self.lock.blocking_lock();
        let _process = self.process_lock()?;
        let mut value = object(self.read()?.0.clone());
        match method_id {
            "xai-api-key" => {
                if api_key(&value).is_none() && scoped_key().is_none() {
                    return Err(CredentialError::Absent);
                }
                value["mode"] = json!("api-key");
            }
            "xai-grok-session" => {
                session_tokens(&value)?;
                value["mode"] = json!("grok-session");
            }
            _ => return Err(CredentialError::Unavailable),
        }
        self.write(value)
    }
    pub(crate) fn clear_method(&self, method_id: &str) -> Result<(), CredentialError> {
        let _guard = self.lock.blocking_lock();
        let _process = self.process_lock()?;
        let mut value = object(self.read()?.0.clone());
        match method_id {
            "xai-api-key" => {
                value.as_object_mut().map(|map| map.remove("api_key"));
                if value.get("mode").and_then(Value::as_str) == Some("api-key")
                    && scoped_key().is_none()
                {
                    value["mode"] = json!("signed-out");
                }
            }
            "xai-grok-session" => {
                if let Some(map) = value.as_object_mut() {
                    map.remove("access_token");
                    map.remove("refresh_token");
                    map.remove("expires_at_unix");
                }
                if value.get("mode").and_then(Value::as_str) == Some("grok-session") {
                    value["mode"] = json!("signed-out");
                }
            }
            _ => return Err(CredentialError::Unavailable),
        }
        self.write(value)
    }
    pub(crate) fn inventory(
        &self,
    ) -> Result<vesper_provider::AuthenticationInventory, CredentialError> {
        use vesper_provider::{AuthenticationMethodState, CredentialSource};
        let value = self.read()?;
        let stored_key = api_key(&value).is_some();
        let environment_key = scoped_key().is_some();
        let session = session_tokens(&value);
        let session_source = match &session {
            Ok(tokens) if tokens.expires_at_unix <= unix_now()? => CredentialSource::Expired,
            Ok(_) => CredentialSource::Stored,
            Err(_) => CredentialSource::Absent,
        };
        let api_source = if stored_key {
            CredentialSource::Stored
        } else if environment_key {
            CredentialSource::Environment
        } else {
            CredentialSource::Absent
        };
        let selected = match value.get("mode").and_then(Value::as_str) {
            Some("grok-session")
                if matches!(
                    session_source,
                    CredentialSource::Stored | CredentialSource::Expired
                ) =>
            {
                Some("xai-grok-session".to_owned())
            }
            Some("api-key") if api_source != CredentialSource::Absent => {
                Some("xai-api-key".to_owned())
            }
            Some("signed-out") => None,
            None if api_source != CredentialSource::Absent => Some("xai-api-key".to_owned()),
            _ => None,
        };
        Ok(vesper_provider::AuthenticationInventory {
            selected_method: selected,
            methods: vec![
                AuthenticationMethodState {
                    method_id: "xai-grok-session".into(),
                    source: session_source,
                },
                AuthenticationMethodState {
                    method_id: "xai-api-key".into(),
                    source: api_source,
                },
            ],
        })
    }
    pub(crate) fn logout(&self) -> Result<(), CredentialError> {
        let _guard = self.lock.blocking_lock();
        let _process = self.process_lock()?;
        self.write(json!({"mode":"signed-out"}))
    }
    pub(crate) async fn device_login(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
    ) -> Result<(), CredentialError> {
        let client = self.auth_client()?;
        self.login_with(cancel.clone(), client.device_login(cancel, on_challenge))
            .await
    }
    pub(crate) async fn browser_login(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        on_url: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), CredentialError> {
        let client = self.auth_client()?;
        self.login_with(cancel.clone(), client.browser_login(cancel, on_url))
            .await
    }
    async fn login_with<F>(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        future: F,
    ) -> Result<(), CredentialError>
    where
        F: std::future::Future<Output = Result<SessionTokens, crate::auth::AuthError>>,
    {
        let _guard = self.operation_lock(cancel.as_ref()).await?;
        let _process = self.process_lock_async(cancel.as_ref()).await?;
        let tokens = future.await.map_err(|_| CredentialError::Failed)?;
        if cancel.is_cancelled() {
            return Err(CredentialError::Failed);
        }
        let existing = self.read()?;
        let value = merge_session(&existing.0, &tokens);
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.write(value))
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
        let this = self.clone();
        let value = tokio::task::spawn_blocking(move || this.read())
            .await
            .map_err(|_| CredentialError::Failed)??;
        match value.get("mode").and_then(Value::as_str) {
            Some("signed-out") => Err(CredentialError::Absent),
            Some("api-key") => Ok(DispatchAuth {
                mode: AuthenticationMode::ApiKey,
                bearer: api_key(&value)
                    .or_else(scoped_key)
                    .ok_or(CredentialError::Absent)?,
            }),
            None => Ok(DispatchAuth {
                mode: AuthenticationMode::ApiKey,
                bearer: scoped_key().ok_or(CredentialError::Absent)?,
            }),
            Some("grok-session") => {
                let mut tokens = session_tokens(&value)?;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| CredentialError::Failed)?
                    .as_secs();
                if force_refresh || tokens.expires_at_unix <= now.saturating_add(60) {
                    tokens = self
                        .auth_client()?
                        .refresh(&tokens.refresh, cancel.as_ref())
                        .await
                        .map_err(|_| CredentialError::Failed)?;
                    let next = token_value(&tokens);
                    let this = self.clone();
                    tokio::task::spawn_blocking(move || this.write(next))
                        .await
                        .map_err(|_| CredentialError::Failed)??;
                }
                Ok(DispatchAuth {
                    mode: AuthenticationMode::GrokSession,
                    bearer: tokens.access,
                })
            }
            _ => Err(CredentialError::InvalidSecret),
        }
    }
    fn auth_client(&self) -> Result<NativeAuthClient, CredentialError> {
        NativeAuthClient::production().map_err(|_| CredentialError::Failed)
    }
    #[cfg(test)]
    pub(crate) fn isolated(path: PathBuf) -> Self {
        Self {
            test_store: Some(TestCredentialStore::new(path.clone())),
            lock_path: path.with_extension("lock"),
            lock: Arc::new(Mutex::new(())),
            ..Self::default()
        }
    }
}

fn decode(value: Option<SecretValue>) -> Result<Stored, CredentialError> {
    match value {
        Some(v) => serde_json::from_str(v.expose().as_str())
            .map(Stored)
            .map_err(|_| CredentialError::InvalidSecret),
        None => Ok(Stored(Value::Null)),
    }
}
fn scoped_key() -> Option<SecretValue> {
    SecretScope::current("XAI_API_KEY")
        .ok()
        .filter(|v| vesper_auth::validate_secret(v.expose().as_str()).is_ok())
}
fn api_key(value: &Value) -> Option<SecretValue> {
    value
        .get("api_key")
        .and_then(Value::as_str)
        .filter(|v| vesper_auth::validate_secret(v).is_ok())
        .map(SecretValue::new)
}
fn session_tokens(value: &Value) -> Result<SessionTokens, CredentialError> {
    let get = |name| {
        value
            .get(name)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 64 * 1024)
            .map(SecretValue::new)
            .ok_or(CredentialError::InvalidSecret)
    };
    Ok(SessionTokens {
        access: get("access_token")?,
        refresh: get("refresh_token")?,
        expires_at_unix: value
            .get("expires_at_unix")
            .and_then(Value::as_u64)
            .ok_or(CredentialError::InvalidSecret)?,
    })
}
fn token_value(tokens: &SessionTokens) -> Value {
    merge_session(&Value::Null, tokens)
}
fn object(value: Value) -> Value {
    if value.is_object() { value } else { json!({}) }
}
fn unix_now() -> Result<u64, CredentialError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| CredentialError::Failed)
}
fn merge_session(existing: &Value, tokens: &SessionTokens) -> Value {
    let mut value = object(existing.clone());
    value["mode"] = json!("grok-session");
    value["access_token"] = json!(tokens.access.expose().as_str());
    value["refresh_token"] = json!(tokens.refresh.expose().as_str());
    value["expires_at_unix"] = json!(tokens.expires_at_unix);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn selected_session_and_signout_never_fall_through_to_environment_api_billing() {
        let temp = tempfile::tempdir().unwrap();
        let credentials = Credentials::isolated(temp.path().join("credentials.json"));
        SecretScope::empty().with("XAI_API_KEY", SecretValue::new("environment-key")).install(async {
            assert_eq!(credentials.authentication_method().unwrap().as_deref(), Some("xai-api-key"));
            credentials.write(json!({"mode":"grok-session","access_token":"session","refresh_token":"refresh","expires_at_unix":u64::MAX})).unwrap();
            let auth = credentials.dispatch(Arc::new(Never), false).await.unwrap();
            assert_eq!(auth.mode, AuthenticationMode::GrokSession);
            assert_eq!(auth.bearer.expose().as_str(), "session");
            let copy = credentials.clone();
            tokio::task::spawn_blocking(move || copy.logout()).await.unwrap().unwrap();
            assert_eq!(credentials.authentication_method().unwrap(), None);
            assert!(matches!(credentials.dispatch(Arc::new(Never), false).await, Err(CredentialError::Absent)));
        }).await;
    }

    #[tokio::test]
    async fn failed_interactive_login_preserves_signed_out_billing_barrier() {
        let temp = tempfile::tempdir().unwrap();
        let credentials = Credentials::isolated(temp.path().join("credentials.json"));
        credentials.write(json!({"mode":"signed-out"})).unwrap();
        SecretScope::empty()
            .with("XAI_API_KEY", SecretValue::new("environment-key"))
            .install(async {
                for failure in [
                    crate::auth::AuthError::Denied,
                    crate::auth::AuthError::Protocol,
                ] {
                    assert!(matches!(
                        credentials
                            .login_with(Arc::new(Never), std::future::ready(Err(failure)))
                            .await,
                        Err(CredentialError::Failed)
                    ));
                    assert_eq!(credentials.authentication_method().unwrap(), None);
                    assert!(matches!(
                        credentials.dispatch(Arc::new(Never), false).await,
                        Err(CredentialError::Absent)
                    ));
                }
            })
            .await;
    }

    #[tokio::test]
    async fn explicit_api_key_mode_never_resolves_a_grok_session() {
        let temp = tempfile::tempdir().unwrap();
        let credentials = Credentials::isolated(temp.path().join("credentials.json"));
        credentials
            .write(json!({"mode":"grok-session","access_token":"session","refresh_token":"refresh","expires_at_unix":u64::MAX}))
            .unwrap();
        let copy = credentials.clone();
        tokio::task::spawn_blocking(move || copy.store_api_key("selected-api-key"))
            .await
            .unwrap()
            .unwrap();
        let auth = credentials.dispatch(Arc::new(Never), false).await.unwrap();
        assert_eq!(auth.mode, AuthenticationMode::ApiKey);
        assert_eq!(auth.bearer.expose().as_str(), "selected-api-key");
        assert_eq!(credentials.read().unwrap()["access_token"], "session");
        let copy = credentials.clone();
        tokio::task::spawn_blocking(move || copy.select_method("xai-grok-session"))
            .await
            .unwrap()
            .unwrap();
        let auth = credentials.dispatch(Arc::new(Never), false).await.unwrap();
        assert_eq!(auth.mode, AuthenticationMode::GrokSession);
        assert_eq!(auth.bearer.expose().as_str(), "session");
        let copy = credentials.clone();
        tokio::task::spawn_blocking(move || copy.select_method("xai-api-key"))
            .await
            .unwrap()
            .unwrap();
        let auth = credentials.dispatch(Arc::new(Never), false).await.unwrap();
        assert_eq!(auth.bearer.expose().as_str(), "selected-api-key");
    }

    struct Never;
    impl CancellationSignal for Never {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
}
