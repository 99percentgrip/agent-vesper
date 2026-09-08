//! Device-login protocol checked against openai/codex commit
//! 8e694e955ae02ca737230a5468c55d5847074072, login/src/device_code_auth.rs
//! and login/src/auth/manager.rs. This is a native HTTP implementation, not
//! a dependency on that application's runtime or credential files.

use std::{fmt, future::Future, time::Duration};

use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use thiserror::Error;
use tokio::time::Instant;
use vesper_provider::CancellationSignal;
use vesper_security::SecretValue;
use zeroize::Zeroizing;

const AUTH_ORIGIN: &str = "https://auth.openai.com";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const MAX_BODY: usize = 64 * 1024;
const MAX_TOKEN: usize = 16 * 1024;
const LOGIN_LIFETIME: Duration = Duration::from_secs(15 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors intentionally exclude response bodies, request URLs and secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AuthError {
    /// The user cancelled the operation.
    #[error("OpenAI sign-in cancelled")]
    Cancelled,
    /// The login or HTTP operation exceeded its bound.
    #[error("OpenAI sign-in timed out; start sign-in again")]
    Timeout,
    /// Transport failed without exposing its credential-bearing request.
    #[error("OpenAI authentication connection failed")]
    Transport,
    /// Unexpected or oversized authentication response.
    #[error("OpenAI authentication returned an invalid response")]
    InvalidResponse,
    /// Initial endpoint is unavailable or account policy disables device login.
    #[error("OpenAI device sign-in is unavailable; check account device-login permissions")]
    DeviceLoginUnavailable,
    /// The service refused the request.
    #[error("OpenAI authentication was rejected; sign in again")]
    Rejected,
    /// OAuth refresh is no longer valid. Never automatically retry a rotation.
    #[error("OpenAI subscription session expired or was revoked; sign in again")]
    SessionExpired,
}

/// Explicit authentication choice; modes must never silently fall back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationMode {
    /// Usage-based API billing.
    ApiKey,
    /// Account-entitled ChatGPT subscription usage.
    ChatGpt,
}

/// Login challenge. Only explicit UI getters expose the user-facing code.
/// Consuming this object prevents accidental reuse of a completed flow.
pub struct DeviceLogin {
    origin: String,
    user_code: SecretValue,
    device_auth_id: SecretValue,
    interval: Duration,
    deadline: Instant,
}

impl DeviceLogin {
    /// Official browser destination; not taken from an untrusted response.
    #[must_use]
    pub fn verification_url(&self) -> String {
        format!("{}/codex/device", self.origin)
    }

    /// One-time code for the user who explicitly started this login.
    #[must_use]
    pub fn user_code(&self) -> String {
        self.user_code.expose().as_str().to_owned()
    }
}

impl fmt::Debug for DeviceLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceLogin([REDACTED])")
    }
}

/// Credentials returned to the host's secure-storage boundary. They cannot
/// be serialized implicitly and do not implement Display with raw values.
#[derive(Debug, Clone)]
pub struct SubscriptionTokens {
    pub(crate) access_token: SecretValue,
    pub(crate) refresh_token: SecretValue,
    pub(crate) id_token: SecretValue,
}

impl SubscriptionTokens {
    /// Explicit access for authenticated transport, never for UI/logging.
    #[must_use]
    pub fn access_token(&self) -> &SecretValue {
        &self.access_token
    }

    /// Explicit access for secure persistence.
    #[must_use]
    pub fn refresh_token(&self) -> &SecretValue {
        &self.refresh_token
    }

    /// Identity claims originate from the authenticated token endpoint, not
    /// an arbitrary JWT supplied as an authorization decision.
    #[must_use]
    pub fn id_token(&self) -> &SecretValue {
        &self.id_token
    }
}

/// Bounded direct OAuth client. Production callers cannot override its origin
/// or attach subscription secrets to arbitrary endpoints.
#[derive(Clone)]
pub struct NativeAuthClient {
    client: Client,
    origin: String,
}

impl fmt::Debug for NativeAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeAuthClient")
    }
}

impl NativeAuthClient {
    /// Constructs the native TLS client without I/O, installation or login.
    pub fn new() -> Result<Self, AuthError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("agent-vesper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| AuthError::Transport)?;
        Ok(Self {
            client,
            origin: AUTH_ORIGIN.to_owned(),
        })
    }

    /// Starts a user-requested login. Does not launch a browser or store data.
    pub async fn begin_device_login(
        &self,
        cancel: &dyn CancellationSignal,
    ) -> Result<DeviceLogin, AuthError> {
        let deadline = Instant::now() + LOGIN_LIFETIME;
        let request = self
            .client
            .post(format!("{}/api/accounts/deviceauth/usercode", self.origin))
            .json(&serde_json::json!({"client_id": CLIENT_ID}));
        let (status, body) = self.request(request, deadline, cancel).await?;
        if status == StatusCode::NOT_FOUND {
            return Err(AuthError::DeviceLoginUnavailable);
        }
        if !status.is_success() {
            return Err(AuthError::Rejected);
        }
        let wire: UserCode = decode(&body)?;
        let user_code = wire.user_code.expose();
        if user_code.as_str().len() > 64
            || !user_code
                .as_str()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b' '))
            || wire.device_auth_id.expose().as_str().len() > 256
        {
            return Err(AuthError::InvalidResponse);
        }
        let seconds = match wire.interval {
            Interval::Number(value) => value,
            Interval::Text(value) => value
                .trim()
                .parse::<u64>()
                .map_err(|_| AuthError::InvalidResponse)?,
        };
        if seconds > LOGIN_LIFETIME.as_secs() {
            return Err(AuthError::InvalidResponse);
        }
        Ok(DeviceLogin {
            origin: self.origin.clone(),
            user_code: wire.user_code,
            device_auth_id: wire.device_auth_id,
            interval: Duration::from_secs(seconds.max(1)),
            deadline,
        })
    }

    /// Polls at the server's bounded interval, exchanges the approved code,
    /// and returns tokens for transactional host persistence.
    pub async fn complete_device_login(
        &self,
        login: DeviceLogin,
        cancel: &dyn CancellationSignal,
    ) -> Result<SubscriptionTokens, AuthError> {
        if login.origin != self.origin {
            return Err(AuthError::InvalidResponse);
        }
        loop {
            let request = self
                .client
                .post(format!("{}/api/accounts/deviceauth/token", self.origin))
                .json(&serde_json::json!({
                    "device_auth_id": login.device_auth_id.expose().as_str(),
                    "user_code": login.user_code.expose().as_str(),
                }));
            let (status, body) = self.request(request, login.deadline, cancel).await?;
            if matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND) {
                bounded(tokio::time::sleep(login.interval), login.deadline, cancel).await?;
                continue;
            }
            if !status.is_success() {
                return Err(AuthError::Rejected);
            }
            let code: AuthorizedCode = decode(&body)?;
            let redirect = format!("{}/deviceauth/callback", self.origin);
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("grant_type", "authorization_code"),
                    ("client_id", CLIENT_ID),
                    ("code", code.authorization_code.expose().as_str()),
                    ("code_verifier", code.code_verifier.expose().as_str()),
                    ("redirect_uri", redirect.as_str()),
                ])
                .finish();
            let request = self
                .client
                .post(format!("{}/oauth/token", self.origin))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(body);
            let (status, body) = self.request(request, login.deadline, cancel).await?;
            if !status.is_success() {
                return Err(AuthError::Rejected);
            }
            let tokens: TokenResponse = decode(&body)?;
            return Ok(SubscriptionTokens {
                access_token: tokens.access_token,
                refresh_token: tokens.refresh_token.ok_or(AuthError::InvalidResponse)?,
                id_token: tokens.id_token.ok_or(AuthError::InvalidResponse)?,
            });
        }
    }

    /// Performs one refresh, with no hidden retry of a possibly rotated token.
    /// Hosts must serialize refresh and persist the result before publishing it.
    pub async fn refresh(
        &self,
        previous: &SubscriptionTokens,
        cancel: &dyn CancellationSignal,
    ) -> Result<SubscriptionTokens, AuthError> {
        let request = self
            .client
            .post(format!("{}/oauth/token", self.origin))
            .json(&serde_json::json!({
                "client_id": CLIENT_ID,
                "grant_type": "refresh_token",
                "refresh_token": previous.refresh_token.expose().as_str(),
            }));
        let (status, body) = self
            .request(request, Instant::now() + REQUEST_TIMEOUT, cancel)
            .await?;
        if matches!(status, StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED) {
            return Err(AuthError::SessionExpired);
        }
        if !status.is_success() {
            return Err(AuthError::Rejected);
        }
        let tokens: TokenResponse = decode(&body)?;
        Ok(SubscriptionTokens {
            access_token: tokens.access_token,
            refresh_token: tokens
                .refresh_token
                .unwrap_or_else(|| previous.refresh_token.clone()),
            id_token: tokens.id_token.unwrap_or_else(|| previous.id_token.clone()),
        })
    }

    async fn request(
        &self,
        request: RequestBuilder,
        deadline: Instant,
        cancel: &dyn CancellationSignal,
    ) -> Result<(StatusCode, Zeroizing<Vec<u8>>), AuthError> {
        bounded(
            async {
                let mut response = request.send().await.map_err(|_| AuthError::Transport)?;
                let status = response.status();
                if response
                    .content_length()
                    .is_some_and(|size| size > MAX_BODY as u64)
                {
                    return Err(AuthError::InvalidResponse);
                }
                let mut body = Zeroizing::new(Vec::new());
                while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::Transport)? {
                    if body.len().saturating_add(chunk.len()) > MAX_BODY {
                        return Err(AuthError::InvalidResponse);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok((status, body))
            },
            deadline.min(Instant::now() + REQUEST_TIMEOUT),
            cancel,
        )
        .await?
    }
}

async fn bounded<T>(
    future: impl Future<Output = T>,
    deadline: Instant,
    cancel: &dyn CancellationSignal,
) -> Result<T, AuthError> {
    tokio::pin!(future);
    loop {
        if cancel.is_cancelled() {
            return Err(AuthError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(AuthError::Timeout);
        }
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => return Err(AuthError::Timeout),
            _ = tokio::time::sleep(Duration::from_millis(25)) => {},
            value = &mut future => {
                if cancel.is_cancelled() { return Err(AuthError::Cancelled); }
                return Ok(value);
            }
        }
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, AuthError> {
    serde_json::from_slice(body).map_err(|_| AuthError::InvalidResponse)
}

fn secret<'de, D: Deserializer<'de>>(d: D) -> Result<SecretValue, D::Error> {
    let value = Zeroizing::new(String::deserialize(d)?);
    if value.is_empty() || value.len() > MAX_TOKEN || value.chars().any(char::is_control) {
        return Err(serde::de::Error::custom("invalid authentication value"));
    }
    Ok(SecretValue::new(value.as_str()))
}

fn optional_secret<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SecretValue>, D::Error> {
    // Present fields must contain valid tokens; null/empty fields fail closed.
    secret(d).map(Some)
}

#[derive(Deserialize)]
struct UserCode {
    #[serde(deserialize_with = "secret")]
    device_auth_id: SecretValue,
    #[serde(alias = "usercode", deserialize_with = "secret")]
    user_code: SecretValue,
    #[serde(default)]
    interval: Interval,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Interval {
    Text(String),
    Number(u64),
}

impl Default for Interval {
    fn default() -> Self {
        Self::Number(5)
    }
}

#[derive(Deserialize)]
struct AuthorizedCode {
    #[serde(deserialize_with = "secret")]
    authorization_code: SecretValue,
    #[serde(deserialize_with = "secret")]
    code_verifier: SecretValue,
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(deserialize_with = "secret")]
    access_token: SecretValue,
    #[serde(default, deserialize_with = "optional_secret")]
    refresh_token: Option<SecretValue>,
    #[serde(default, deserialize_with = "optional_secret")]
    id_token: Option<SecretValue>,
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
