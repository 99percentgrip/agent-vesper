//! First-party xAI public-client OAuth/OIDC and RFC 8628 device authorization.
//! Derived from xai-org/grok-build at f0e3be1100ef5252488e3be8bb0e91cf68d8c305.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vesper_provider::CancellationSignal;
use vesper_security::SecretValue;

pub(crate) const ISSUER: &str = "https://auth.x.ai";
pub(crate) const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub(crate) const SCOPES: &[&str] = &[
    "openid",
    "profile",
    "email",
    "offline_access",
    "grok-cli:access",
    "api:access",
];
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuthError {
    #[error("authentication was cancelled")]
    Cancelled,
    #[error("authentication timed out")]
    Timeout,
    #[error("authentication was denied")]
    Denied,
    #[error("authentication protocol failed")]
    Protocol,
    #[error("authentication transport failed")]
    Transport,
}

#[derive(Clone, Debug)]
pub(crate) struct SessionTokens {
    pub access: SecretValue,
    pub refresh: SecretValue,
    pub expires_at_unix: u64,
}

#[derive(Clone)]
pub(crate) struct NativeAuthClient {
    client: reqwest::Client,
    issuer: String,
}

#[derive(Clone, Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: i64,
    interval: Option<i64>,
}

#[derive(Deserialize)]
struct OAuthError {
    error: String,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: serde_json::Value,
    exp: u64,
    nonce: String,
}

impl NativeAuthClient {
    pub(crate) fn production() -> Result<Self, AuthError> {
        Self::new(ISSUER)
    }

    #[cfg(test)]
    pub(crate) fn loopback(origin: &str) -> Result<Self, AuthError> {
        let parsed = url::Url::parse(origin).map_err(|_| AuthError::Protocol)?;
        if parsed.scheme() != "http"
            || parsed.host_str() != Some("127.0.0.1")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(AuthError::Protocol);
        }
        Self::new(origin.trim_end_matches('/'))
    }

    fn new(issuer: &str) -> Result<Self, AuthError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("agent-vesper/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| AuthError::Transport)?;
        Ok(Self {
            client,
            issuer: issuer.to_owned(),
        })
    }

    pub(crate) async fn device_login(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
    ) -> Result<SessionTokens, AuthError> {
        let url = format!("{}/oauth2/device/code", self.issuer);
        let scope = SCOPES.join(" ");
        let response = self
            .send(
                cancel.as_ref(),
                self.client
                    .post(url)
                    .header("x-grok-client-version", env!("CARGO_PKG_VERSION"))
                    .header("x-grok-client-surface", "ui")
                    .form(&[
                        ("client_id", CLIENT_ID),
                        ("scope", scope.as_str()),
                        ("referrer", "agent-vesper"),
                    ]),
            )
            .await?;
        let device: DeviceResponse = decode_response(response).await?;
        if device.device_code.is_empty()
            || device.device_code.len() > 4096
            || device.user_code.is_empty()
            || device.user_code.len() > 64
            || !device
                .user_code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !(60..=1800).contains(&device.expires_in)
        {
            return Err(AuthError::Protocol);
        }
        let verification = device
            .verification_uri_complete
            .as_deref()
            .unwrap_or(&device.verification_uri);
        self.validate_verification_url(verification)?;
        on_challenge(verification.to_owned(), device.user_code.clone());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(device.expires_in as u64);
        let mut interval = Duration::from_secs(device.interval.unwrap_or(5).clamp(1, 30) as u64);
        let token_url = format!("{}/oauth2/token", self.issuer);
        loop {
            wait(interval, cancel.as_ref()).await?;
            if tokio::time::Instant::now() >= deadline {
                return Err(AuthError::Timeout);
            }
            let response = self
                .send(
                    cancel.as_ref(),
                    self.client
                        .post(&token_url)
                        .header("x-grok-client-version", env!("CARGO_PKG_VERSION"))
                        .header("x-grok-client-surface", "ui")
                        .form(&[
                            ("grant_type", DEVICE_GRANT),
                            ("device_code", device.device_code.as_str()),
                            ("client_id", CLIENT_ID),
                        ]),
                )
                .await?;
            if response.status().is_success() {
                return tokens(decode_json(response).await?, None);
            }
            let error: OAuthError = decode_json(response).await?;
            match error.error.as_str() {
                "authorization_pending" => {}
                "slow_down" => {
                    interval = (interval + Duration::from_secs(5)).min(Duration::from_secs(60))
                }
                "access_denied" => return Err(AuthError::Denied),
                "expired_token" => return Err(AuthError::Timeout),
                _ => return Err(AuthError::Protocol),
            }
        }
    }

    pub(crate) async fn browser_login(
        &self,
        cancel: Arc<dyn CancellationSignal>,
        on_url: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<SessionTokens, AuthError> {
        let discovery = self.discovery(cancel.as_ref()).await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| AuthError::Transport)?;
        let redirect = format!(
            "http://127.0.0.1:{}/callback",
            listener
                .local_addr()
                .map_err(|_| AuthError::Transport)?
                .port()
        );
        let verifier = random_token(32);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_token(32);
        let nonce = random_token(32);
        let mut authorize =
            url::Url::parse(&discovery.authorization_endpoint).map_err(|_| AuthError::Protocol)?;
        authorize
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", &redirect)
            .append_pair("scope", &SCOPES.join(" "))
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("referrer", "agent-vesper");
        on_url(authorize.to_string());
        let (mut socket, _) = tokio::select! {
            _ = cancelled(cancel.as_ref()) => return Err(AuthError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(600), listener.accept()) => result.map_err(|_| AuthError::Timeout)?.map_err(|_| AuthError::Transport)?,
        };
        let mut raw = Vec::new();
        loop {
            let mut chunk = [0; 2048];
            let count = tokio::select! {
                _ = cancelled(cancel.as_ref()) => return Err(AuthError::Cancelled),
                result = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk)) => result.map_err(|_| AuthError::Timeout)?.map_err(|_| AuthError::Transport)?,
            };
            if count == 0 || raw.len().saturating_add(count) > 16 * 1024 {
                return Err(AuthError::Protocol);
            }
            raw.extend_from_slice(&chunk[..count]);
            if raw.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        let first = std::str::from_utf8(&raw)
            .map_err(|_| AuthError::Protocol)?
            .lines()
            .next()
            .ok_or(AuthError::Protocol)?;
        let target = first
            .strip_prefix("GET ")
            .and_then(|line| line.split_once(' ').map(|v| v.0))
            .ok_or(AuthError::Protocol)?;
        let callback = url::Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|_| AuthError::Protocol)?;
        let values: BTreeMap<_, _> = callback.query_pairs().into_owned().collect();
        let accepted = values.get("state") == Some(&state) && values.contains_key("code");
        let (status, message) = if accepted {
            ("200 OK", "Authorization complete. Return to Vesper.")
        } else {
            ("400 Bad Request", "Authorization validation failed.")
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
            message.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        if !accepted {
            return Err(AuthError::Protocol);
        }
        let token = self
            .send(
                cancel.as_ref(),
                self.client.post(&discovery.token_endpoint).form(&[
                    ("grant_type", "authorization_code"),
                    (
                        "code",
                        values.get("code").ok_or(AuthError::Protocol)?.as_str(),
                    ),
                    ("redirect_uri", redirect.as_str()),
                    ("client_id", CLIENT_ID),
                    ("code_verifier", verifier.as_str()),
                ]),
            )
            .await?;
        let wire: TokenResponse = decode_response(token).await?;
        let id_token = wire.id_token.as_deref().ok_or(AuthError::Protocol)?;
        self.validate_id_token(&discovery, id_token, &nonce, cancel.as_ref())
            .await?;
        tokens(wire, None)
    }

    pub(crate) async fn refresh(
        &self,
        refresh: &SecretValue,
        cancel: &dyn CancellationSignal,
    ) -> Result<SessionTokens, AuthError> {
        let discovery = self.discovery(cancel).await?;
        let response = self
            .send(
                cancel,
                self.client.post(&discovery.token_endpoint).form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh.expose().as_str()),
                    ("client_id", CLIENT_ID),
                ]),
            )
            .await?;
        tokens(decode_response(response).await?, Some(refresh))
    }

    async fn discovery(&self, cancel: &dyn CancellationSignal) -> Result<Discovery, AuthError> {
        let response = self
            .send(
                cancel,
                self.client
                    .get(format!("{}/.well-known/openid-configuration", self.issuer)),
            )
            .await?;
        let discovery: Discovery = decode_response(response).await?;
        self.validate_authorization_endpoint(&discovery.authorization_endpoint)?;
        self.validate_auth_endpoint(&discovery.token_endpoint)?;
        self.validate_auth_endpoint(&discovery.jwks_uri)?;
        Ok(discovery)
    }

    async fn validate_id_token(
        &self,
        discovery: &Discovery,
        token: &str,
        nonce: &str,
        cancel: &dyn CancellationSignal,
    ) -> Result<(), AuthError> {
        jsonwebtoken::crypto::CryptoProvider::install_default(
            &jsonwebtoken::crypto::aws_lc::DEFAULT_PROVIDER,
        )
        .ok();
        let header = decode_header(token).map_err(|_| AuthError::Protocol)?;
        if !matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::EdDSA
        ) || !discovery
            .id_token_signing_alg_values_supported
            .iter()
            .any(|alg| alg == &format!("{:?}", header.alg))
        {
            return Err(AuthError::Protocol);
        }
        let response = self
            .send(cancel, self.client.get(&discovery.jwks_uri))
            .await?;
        let set: JwkSet = decode_response(response).await?;
        let key = set
            .find(header.kid.as_deref().ok_or(AuthError::Protocol)?)
            .ok_or(AuthError::Protocol)?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[CLIENT_ID]);
        validation.required_spec_claims = ["sub", "iss", "aud", "exp"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let claims = decode::<Claims>(
            token,
            &DecodingKey::from_jwk(key).map_err(|_| AuthError::Protocol)?,
            &validation,
        )
        .map_err(|_| AuthError::Protocol)?
        .claims;
        if claims.nonce != nonce
            || claims.iss != self.issuer
            || claims.sub.is_empty()
            || claims.exp == 0
            || claims.aud.is_null()
        {
            return Err(AuthError::Protocol);
        }
        Ok(())
    }

    async fn send(
        &self,
        cancel: &dyn CancellationSignal,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, AuthError> {
        tokio::select! {
            _ = cancelled(cancel) => Err(AuthError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(15), request.send()) => result.map_err(|_| AuthError::Timeout)?.map_err(|_| AuthError::Transport),
        }
    }

    fn validate_auth_endpoint(&self, value: &str) -> Result<(), AuthError> {
        let url = url::Url::parse(value).map_err(|_| AuthError::Protocol)?;
        let issuer = url::Url::parse(&self.issuer).map_err(|_| AuthError::Protocol)?;
        if url.scheme() != issuer.scheme()
            || url.host_str() != issuer.host_str()
            || url.port_or_known_default() != issuer.port_or_known_default()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AuthError::Protocol);
        }
        if self.issuer == ISSUER && url.scheme() != "https" {
            return Err(AuthError::Protocol);
        }
        Ok(())
    }

    fn validate_authorization_endpoint(&self, value: &str) -> Result<(), AuthError> {
        if self.issuer != ISSUER {
            return self.validate_auth_endpoint(value);
        }
        let url = url::Url::parse(value).map_err(|_| AuthError::Protocol)?;
        if url.scheme() != "https"
            || !matches!(url.host_str(), Some("auth.x.ai" | "accounts.x.ai"))
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AuthError::Protocol);
        }
        Ok(())
    }

    fn validate_verification_url(&self, value: &str) -> Result<(), AuthError> {
        let url = url::Url::parse(value).map_err(|_| AuthError::Protocol)?;
        let production = self.issuer == ISSUER
            && url.scheme() == "https"
            && url.host_str() == Some("accounts.x.ai")
            && url.port().is_none();
        let test =
            self.issuer != ISSUER && url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
        if !production && !test {
            return Err(AuthError::Protocol);
        }
        Ok(())
    }
}

fn tokens(
    wire: TokenResponse,
    previous_refresh: Option<&SecretValue>,
) -> Result<SessionTokens, AuthError> {
    if wire.access_token.is_empty() || wire.access_token.len() > 64 * 1024 {
        return Err(AuthError::Protocol);
    }
    let refresh = match wire.refresh_token {
        Some(value) if !value.is_empty() && value.len() <= 64 * 1024 => SecretValue::new(value),
        Some(_) => return Err(AuthError::Protocol),
        None => previous_refresh.cloned().ok_or(AuthError::Protocol)?,
    };
    let ttl = wire.expires_in.unwrap_or(900).clamp(60, 86_400);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AuthError::Protocol)?
        .as_secs();
    Ok(SessionTokens {
        access: SecretValue::new(wire.access_token),
        refresh,
        expires_at_unix: now.saturating_add(ttl),
    })
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, AuthError> {
    if !response.status().is_success() {
        return Err(
            if response.status() == reqwest::StatusCode::UNAUTHORIZED
                || response.status() == reqwest::StatusCode::FORBIDDEN
            {
                AuthError::Denied
            } else {
                AuthError::Protocol
            },
        );
    }
    decode_json(response).await
}

async fn decode_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, AuthError> {
    let bytes = response.bytes().await.map_err(|_| AuthError::Transport)?;
    if bytes.len() > MAX_BODY {
        return Err(AuthError::Protocol);
    }
    serde_json::from_slice(&bytes).map_err(|_| AuthError::Protocol)
}

fn random_token(bytes: usize) -> String {
    let mut value = vec![0; bytes];
    rand::thread_rng().fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

async fn cancelled(cancel: &dyn CancellationSignal) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait(duration: Duration, cancel: &dyn CancellationSignal) -> Result<(), AuthError> {
    tokio::select! { _ = cancelled(cancel) => Err(AuthError::Cancelled), _ = tokio::time::sleep(duration) => Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    #[derive(Default)]
    struct Cancel(AtomicBool);
    impl CancellationSignal for Cancel {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Relaxed)
        }
    }

    async fn reply(mut socket: tokio::net::TcpStream, status: &str, body: &str) -> String {
        let mut request = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..n]);
            if request.windows(4).any(|v| v == b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    }

    #[tokio::test]
    async fn device_flow_handles_pending_slow_down_and_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let verification = format!("{origin}/device");
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let body = format!(
                r#"{{"device_code":"private-device","user_code":"ABCD-EFGH","verification_uri":"{verification}","expires_in":60,"interval":1}}"#
            );
            let request = reply(socket, "200 OK", &body).await;
            assert!(request.contains("client_id=b1a00492-073a-47ea-816f-4c329264a828"));
            for body in [
                r#"{"error":"authorization_pending"}"#,
                r#"{"error":"slow_down"}"#,
                r#"{"access_token":"session-access","refresh_token":"session-refresh","expires_in":900}"#,
            ] {
                let (socket, _) = listener.accept().await.unwrap();
                let request = reply(
                    socket,
                    if body.contains("access_token") {
                        "200 OK"
                    } else {
                        "400 Bad Request"
                    },
                    body,
                )
                .await;
                assert!(request.contains("private-device"));
            }
        });
        let client = NativeAuthClient::loopback(&origin).unwrap();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let sink = seen.clone();
        let tokens = client
            .device_login(
                Arc::new(Cancel::default()),
                Arc::new(move |url, code| *sink.lock().unwrap() = Some((url, code))),
            )
            .await
            .unwrap();
        assert_eq!(tokens.access.expose().as_str(), "session-access");
        assert_eq!(seen.lock().unwrap().as_ref().unwrap().1, "ABCD-EFGH");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn browser_flow_rejects_state_mismatch_before_token_exchange() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let discovery_origin = origin.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let body = format!(
                r#"{{"authorization_endpoint":"{discovery_origin}/authorize","token_endpoint":"{discovery_origin}/token","jwks_uri":"{discovery_origin}/jwks","id_token_signing_alg_values_supported":["RS256"]}}"#
            );
            let request = reply(socket, "200 OK", &body).await;
            assert!(request.starts_with("GET /.well-known/openid-configuration"));
        });
        let client = NativeAuthClient::loopback(&origin).unwrap();
        let (tx, rx) = oneshot::channel();
        let sender = std::sync::Mutex::new(Some(tx));
        let task = tokio::spawn(async move {
            client
                .browser_login(
                    Arc::new(Cancel::default()),
                    Arc::new(move |url| {
                        if let Some(tx) = sender.lock().unwrap().take() {
                            let _ = tx.send(url);
                        }
                    }),
                )
                .await
        });
        let authorization = url::Url::parse(&rx.await.unwrap()).unwrap();
        let query: BTreeMap<_, _> = authorization.query_pairs().into_owned().collect();
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert!(query.contains_key("nonce"));
        let redirect = url::Url::parse(query.get("redirect_uri").unwrap()).unwrap();
        let mut stream = tokio::net::TcpStream::connect((
            redirect.host_str().unwrap(),
            redirect.port().unwrap(),
        ))
        .await
        .unwrap();
        stream
            .write_all(
                b"GET /callback?code=fixture&state=wrong HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
            .await
            .unwrap();
        assert!(matches!(task.await.unwrap(), Err(AuthError::Protocol)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn browser_flow_validates_signed_nonce_and_exchanges_pkce() {
        jsonwebtoken::crypto::CryptoProvider::install_default(
            &jsonwebtoken::crypto::aws_lc::DEFAULT_PROVIDER,
        )
        .ok();
        let private = include_bytes!("../testdata/oidc-test-private.pem");
        let modulus = "x0fceJ2rz0xmwOGcQkxVqktX941Zhmy0bUN9FdYDOK4UqoNCrKques2aJFjOoRz2GvPfrTwlYiwOG-HvmS_ec0AOu_QCo_bd3u1GVeWxNw93moH7i62ZhZNEksRTVPu-sgvK0OcVebW4FwJLRbv01GjUFD657mUrmUP3Fc0e66MDgkYO5eX7W9x0ikHNIUKIkmDKaxwR-VFna68Mu6tfPa7t9iK2nLj2rU0DXMXany473enwZIyxXZq_7WPz8gqc0y9m29MaWVhBNrxucycU2xDY8rWgzsRFqx_Fwx7tXG79bWrtCjbsiME71BKrelCFliXdJQRRPtvrQTzjL0MSSw";
        let exponent = "AQAB";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server_origin = origin.clone();
        let auth_url = Arc::new(std::sync::Mutex::new(None::<String>));
        let server_url = auth_url.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let discovery = format!(
                r#"{{"authorization_endpoint":"{server_origin}/authorize","token_endpoint":"{server_origin}/token","jwks_uri":"{server_origin}/jwks","id_token_signing_alg_values_supported":["RS256"]}}"#
            );
            reply(socket, "200 OK", &discovery).await;
            let (socket, _) = listener.accept().await.unwrap();
            let request = reply(socket, "200 OK", &{
                let authorization = url::Url::parse(server_url.lock().unwrap().as_ref().unwrap()).unwrap();
                let query: BTreeMap<_, _> = authorization.query_pairs().into_owned().collect();
                #[derive(Serialize)]
                struct SignedClaims<'a> { sub: &'a str, iss: &'a str, aud: &'a str, exp: u64, nonce: &'a str }
                let claims = SignedClaims { sub: "user-1", iss: &server_origin, aud: CLIENT_ID, exp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 600, nonce: query.get("nonce").unwrap() };
                let mut header = jsonwebtoken::Header::new(Algorithm::RS256); header.kid = Some("fixture-key".into());
                let encoded = jsonwebtoken::encode(&header, &claims, &jsonwebtoken::EncodingKey::from_rsa_pem(private).unwrap()).unwrap();
                format!(r#"{{"access_token":"session-access","refresh_token":"session-refresh","id_token":"{encoded}","expires_in":900}}"#)
            }).await;
            assert!(request.contains("grant_type=authorization_code"));
            assert!(request.contains("code_verifier="));
            let (socket, _) = listener.accept().await.unwrap();
            let jwks = format!(
                r#"{{"keys":[{{"kty":"RSA","kid":"fixture-key","use":"sig","alg":"RS256","n":"{modulus}","e":"{exponent}"}}]}}"#
            );
            reply(socket, "200 OK", &jwks).await;
        });
        let client = NativeAuthClient::loopback(&origin).unwrap();
        let (tx, rx) = oneshot::channel();
        let sender = std::sync::Mutex::new(Some(tx));
        let captured = auth_url.clone();
        let task = tokio::spawn(async move {
            client
                .browser_login(
                    Arc::new(Cancel::default()),
                    Arc::new(move |url| {
                        *captured.lock().unwrap() = Some(url.clone());
                        if let Some(tx) = sender.lock().unwrap().take() {
                            let _ = tx.send(url);
                        }
                    }),
                )
                .await
        });
        let authorization = url::Url::parse(&rx.await.unwrap()).unwrap();
        let query: BTreeMap<_, _> = authorization.query_pairs().into_owned().collect();
        let redirect = url::Url::parse(query.get("redirect_uri").unwrap()).unwrap();
        let mut stream = tokio::net::TcpStream::connect((
            redirect.host_str().unwrap(),
            redirect.port().unwrap(),
        ))
        .await
        .unwrap();
        let callback = format!(
            "GET /callback?code=fixture-code&state={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            query.get("state").unwrap()
        );
        stream.write_all(callback.as_bytes()).await.unwrap();
        let tokens = task.await.unwrap().unwrap();
        assert_eq!(tokens.access.expose().as_str(), "session-access");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_settles_before_network() {
        let cancel = Arc::new(Cancel::default());
        cancel.0.store(true, Ordering::Relaxed);
        let client = NativeAuthClient::loopback("http://127.0.0.1:9").unwrap();
        assert!(matches!(
            client.device_login(cancel, Arc::new(|_, _| {})).await,
            Err(AuthError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn refresh_preserves_or_rotates_the_refresh_token_exactly_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server_origin = origin.clone();
        let server = tokio::spawn(async move {
            for token_body in [
                r#"{"access_token":"next-access","expires_in":900}"#,
                r#"{"access_token":"newer-access","refresh_token":"rotated-refresh","expires_in":900}"#,
            ] {
                let (socket, _) = listener.accept().await.unwrap();
                let discovery = format!(
                    r#"{{"authorization_endpoint":"{server_origin}/authorize","token_endpoint":"{server_origin}/token","jwks_uri":"{server_origin}/jwks","id_token_signing_alg_values_supported":["RS256"]}}"#
                );
                reply(socket, "200 OK", &discovery).await;
                let (socket, _) = listener.accept().await.unwrap();
                let request = reply(socket, "200 OK", token_body).await;
                assert!(request.contains("refresh_token=old-refresh"));
            }
        });
        let client = NativeAuthClient::loopback(&origin).unwrap();
        let old = SecretValue::new("old-refresh");
        let first = client.refresh(&old, &Cancel::default()).await.unwrap();
        assert_eq!(first.refresh.expose().as_str(), "old-refresh");
        let second = client.refresh(&old, &Cancel::default()).await.unwrap();
        assert_eq!(second.refresh.expose().as_str(), "rotated-refresh");
        server.await.unwrap();
    }
}
