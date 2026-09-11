//! Account-scoped model choices. Capabilities remain adapter-owned.
use crate::{OpenAiCatalog, OpenAiFactory, OpenAiSession, auth::AuthenticationMode, error};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use vesper_domain::ErrorCategory;
use vesper_provider::*;

// Subscription discovery gates rows on the upstream protocol client version,
// not Vesper's independent release number. This floor matches the newest model
// in our capability catalog (upstream models.json at 8e694e955ae02ca737230a5468c55d5847074072).
const SUBSCRIPTION_CATALOG_VERSION: &str = "0.153.0";

/// One explicit discovery result, never a process-global or cross-account cache.
#[derive(Clone)]
pub struct AvailableModels {
    pub mode: AuthenticationMode,
    pub models: Vec<ModelDescriptor>,
}
impl AvailableModels {
    pub fn unavailable(mode: AuthenticationMode) -> Self {
        Self {
            mode,
            models: Vec::new(),
        }
    }
    pub fn contains(&self, id: &str) -> bool {
        self.models.iter().any(|m| m.model.model_id.as_str() == id)
    }
    pub fn policy(&self) -> crate::OpenAiSuperpowerPolicy {
        crate::OpenAiSuperpowerPolicy {
            mode: self.mode,
            available: Some(
                self.models
                    .iter()
                    .map(|m| m.model.model_id.as_str().to_owned())
                    .collect(),
            ),
        }
    }
}
#[allow(clippy::result_large_err)]
fn parse(
    value: &serde_json::Value,
    mode: AuthenticationMode,
) -> Result<AvailableModels, ProviderError> {
    let key = if mode == AuthenticationMode::ApiKey {
        "data"
    } else {
        "models"
    };
    let rows = value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(crate::wire::invalid)?;
    if rows.len() > 4096 {
        return Err(crate::wire::invalid());
    }
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for row in rows {
        let key = if mode == AuthenticationMode::ApiKey {
            "id"
        } else {
            "slug"
        };
        let id = row
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(crate::wire::invalid)?;
        if id.len() > 256 || !seen.insert(id) {
            return Err(crate::wire::invalid());
        }
        if mode == AuthenticationMode::ChatGpt
            && row.get("visibility").and_then(serde_json::Value::as_str) != Some("list")
        {
            continue;
        }
        // Unknown identifiers cannot establish tool, vision or reasoning support.
        if let Some(model) = OpenAiCatalog::find(id) {
            models.push(model);
        }
    }
    // API model order is unspecified; retain the adapter's stable order.
    if mode == AuthenticationMode::ApiKey {
        let order = OpenAiCatalog::snapshot().models;
        models.sort_by_key(|m| order.iter().position(|known| known.model == m.model));
    }
    Ok(AvailableModels { mode, models })
}
impl OpenAiFactory {
    /// Native read-only discovery, after authentication and outside rendering.
    pub async fn available_models(
        &self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<AvailableModels, ProviderError> {
        // Invalidate before awaiting: cancellation by dropping this future must
        // not leave the previous account's choices authoritative.
        *self
            .availability
            .write()
            .map_err(|_| crate::wire::invalid())? =
            Some(AvailableModels::unavailable(self.control_policy().mode));
        let session = OpenAiSession::new(self.credentials.clone(), "medium".into())?;
        #[cfg(feature = "integration-test-harness")]
        let session = session.with_test_route(self.test_route.clone());
        let result = session.available_models(cancel).await;
        let snapshot = result
            .as_ref()
            .cloned()
            .unwrap_or_else(|_| AvailableModels::unavailable(self.control_policy().mode));
        *self
            .availability
            .write()
            .map_err(|_| crate::wire::invalid())? = Some(snapshot);
        result
    }
}
impl OpenAiSession {
    pub(crate) async fn available_models(
        &self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<AvailableModels, ProviderError> {
        let operation = async {
            let mut auth = self
                .resolve_auth(cancel.clone(), false)
                .await
                .map_err(|_| {
                    error(
                        "Sign in to OpenAI before loading models",
                        ErrorCategory::Authentication,
                        false,
                    )
                })?;
            let endpoint = match auth.mode {
                AuthenticationMode::ApiKey => "https://api.openai.com/v1/models",
                AuthenticationMode::ChatGpt => "https://chatgpt.com/backend-api/codex/models",
            };
            let mut url = url::Url::parse(endpoint).expect("fixed URL");
            #[cfg(feature = "integration-test-harness")]
            if let Some((route, _)) = &self.test_route {
                url = url::Url::parse(route).map_err(|_| crate::wire::invalid())?;
                url.set_path("/models");
                url.set_query(None);
            }
            if auth.mode == AuthenticationMode::ChatGpt {
                url.query_pairs_mut()
                    .append_pair("client_version", SUBSCRIPTION_CATALOG_VERSION);
            }
            for attempt in 0..2 {
                let mut request = self
                    .client
                    .get(url.clone())
                    .bearer_auth(auth.bearer.expose().as_str())
                    .header("Accept", "application/json");
                if let Some(account) = &auth.account {
                    request = request
                        .header("ChatGPT-Account-Id", account.expose().as_str())
                        .header("originator", "agent-vesper");
                }
                let mut response = request.send().await.map_err(|_| {
                    error(
                        "OpenAI model discovery connection failed; reopen Settings to retry",
                        ErrorCategory::Transport,
                        false,
                    )
                })?;
                if response.status().as_u16() == 401
                    && auth.mode == AuthenticationMode::ChatGpt
                    && attempt == 0
                {
                    auth = self.resolve_auth(cancel.clone(), true).await.map_err(|_| {
                        error(
                            "OpenAI session expired; sign in again",
                            ErrorCategory::Authentication,
                            false,
                        )
                    })?;
                    continue;
                }
                if !response.status().is_success() {
                    return Err(crate::http_error::rejection(response, cancel.as_ref()).await);
                }
                let mut bytes = Vec::new();
                while let Some(chunk) =
                    response.chunk().await.map_err(|_| crate::wire::invalid())?
                {
                    if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                        return Err(crate::wire::invalid());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                return parse(
                    &serde_json::from_slice(&bytes).map_err(|_| crate::wire::invalid())?,
                    auth.mode,
                );
            }
            Err(crate::wire::invalid())
        };
        tokio::select! {
            biased;
            _ = async { while !cancel.is_cancelled() { tokio::time::sleep(Duration::from_millis(25)).await; } } => Err(error("OpenAI model discovery cancelled", ErrorCategory::Cancellation, false)),
            result = tokio::time::timeout(Duration::from_secs(10), operation) => result.map_err(|_| error("OpenAI model discovery timed out; reopen Settings to retry", ErrorCategory::Transport, false))?,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn subscription_excludes_hidden_unknown_and_missing_visibility() {
        let got = parse(&json!({"models":[{"slug":"gpt-5.4","visibility":"hide"},{"slug":"gpt-5.5","visibility":"list"},{"slug":"invented","visibility":"list"},{"slug":"gpt-5.2"}]}), AuthenticationMode::ChatGpt).unwrap();
        assert_eq!(got.models.len(), 1);
        assert!(got.contains("gpt-5.5"));
        assert!(!got.contains("gpt-5.4"));
    }
    #[test]
    fn api_uses_only_returned_verified_ids_and_failures_never_restore_static_list() {
        let got = parse(
            &json!({"data":[{"id":"gpt-5.4"},{"id":"unknown"}]}),
            AuthenticationMode::ApiKey,
        )
        .unwrap();
        assert_eq!(got.models.len(), 1);
        assert!(got.contains("gpt-5.4"));
        for bad in [
            json!({}),
            json!({"data":[{}]}),
            json!({"data":[{"id":"gpt-5.4"},{"id":"gpt-5.4"}]}),
        ] {
            assert!(parse(&bad, AuthenticationMode::ApiKey).is_err());
        }
        assert!(
            parse(&json!({"data":[]}), AuthenticationMode::ApiKey)
                .unwrap()
                .models
                .is_empty()
        );
        assert!(!AvailableModels::unavailable(AuthenticationMode::ChatGpt).contains("gpt-5.4"));
    }
}

#[cfg(all(test, feature = "integration-test-harness"))]
mod transport_tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    struct Never;
    impl CancellationSignal for Never {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    async fn serve(listener: TcpListener, mode: AuthenticationMode, bodies: Vec<String>) {
        for body in bodies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            let request = String::from_utf8(bytes).unwrap().to_lowercase();
            assert!(request.starts_with("get /models"));
            assert!(request.contains("authorization: bearer fixture-openai-key"));
            assert_eq!(
                request.contains("chatgpt-account-id: fixture-account"),
                mode == AuthenticationMode::ChatGpt
            );
            assert_eq!(
                request.contains("client_version="),
                mode == AuthenticationMode::ChatGpt
            );
            if mode == AuthenticationMode::ChatGpt {
                // The service version-gates account rows. Vesper's 0.21.x
                // application version silently produced an empty live catalog.
                assert!(request.starts_with("get /models?client_version=0.153.0 "));
            }
            socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
    }
    #[tokio::test]
    async fn both_modes_filter_menus_block_dispatch_and_discard_failed_refresh() {
        for mode in [AuthenticationMode::ApiKey, AuthenticationMode::ChatGpt] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let factory = OpenAiFactory::for_loopback(
                &format!("http://{}/responses", listener.local_addr().unwrap()),
                mode,
            )
            .unwrap();
            let body = if mode == AuthenticationMode::ApiKey {
                r#"{"data":[{"id":"gpt-5.5"}]}"#
            } else {
                r#"{"models":[{"slug":"gpt-5.5","visibility":"list"},{"slug":"gpt-5.4","visibility":"hide"}]}"#
            };
            let server = tokio::spawn(serve(
                listener,
                mode,
                vec![body.into(), "{}".into(), body.into()],
            ));
            let available = factory.available_models(Arc::new(Never)).await.unwrap();
            let policy = available.policy();
            let values = factory.superpowers_for(&available);
            let model = &values[0];
            assert_eq!(model.allowed_values.len(), 1);
            assert_eq!(model.default_value, model.allowed_values[0]);
            assert!(
                policy
                    .validate("model", &model.allowed_values[0], "", "")
                    .is_ok()
            );
            let session = factory
                .create_session(&OpenAiFactory::default_configuration(), Arc::new(Never))
                .await
                .unwrap();
            let rejected = session
                .start(crate::tests::fixture_request(), Arc::new(Never))
                .await;
            assert!(rejected.is_err()); // hidden/missing gpt-5.4 never reaches Responses.
            assert!(factory.available_models(Arc::new(Never)).await.is_err());
            assert!(
                factory
                    .availability
                    .read()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .models
                    .is_empty()
            );
            let mut request = crate::tests::fixture_request();
            request.model.model_id = vesper_domain::ModelId::new("gpt-5.5").unwrap();
            assert!(session.start(request, Arc::new(Never)).await.is_err());
            let recovered = factory.available_models(Arc::new(Never)).await.unwrap();
            assert!(recovered.contains("gpt-5.5"));
            assert_eq!(
                factory.superpowers_for(&recovered)[0].allowed_values.len(),
                1
            );
            assert!(
                recovered
                    .policy()
                    .validate("model", &model.allowed_values[0], "", "")
                    .is_ok()
            );
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn cancellation_does_not_connect_or_restore_choices() {
        struct Cancelled;
        impl CancellationSignal for Cancelled {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let factory = OpenAiFactory::for_loopback(
            &format!("http://{}/responses", listener.local_addr().unwrap()),
            AuthenticationMode::ChatGpt,
        )
        .unwrap();
        let error = factory
            .available_models(Arc::new(Cancelled))
            .await
            .err()
            .unwrap();
        assert_eq!(error.info.category, ErrorCategory::Cancellation);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), listener.accept())
                .await
                .is_err()
        );
    }
}

#[cfg(all(test, feature = "integration-test-harness"))]
mod bounds_tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    struct Never;
    impl CancellationSignal for Never {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    #[tokio::test]
    async fn rejects_redirects_and_oversized_bodies_without_echoing_content() {
        for response in [
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/secret-canary\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", 4 * 1024 * 1024 + 1, "x".repeat(4 * 1024 * 1024 + 1)),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let factory = OpenAiFactory::for_loopback(&format!("http://{}/responses", listener.local_addr().unwrap()), AuthenticationMode::ApiKey).unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                assert!(socket.read(&mut bytes).await.unwrap() > 0);
                let _ = socket.write_all(response.as_bytes()).await;
            });
            let error = factory.available_models(Arc::new(Never)).await.err().unwrap();
            assert!(!format!("{error:?}").contains("secret-canary"));
            assert!(factory.availability.read().unwrap().as_ref().unwrap().models.is_empty());
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn stalled_body_obeys_the_discovery_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let factory = OpenAiFactory::for_loopback(
            &format!("http://{}/responses", listener.local_addr().unwrap()),
            AuthenticationMode::ApiKey,
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            assert!(socket.read(&mut bytes).await.unwrap() > 0);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\n\r\n{")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let started = std::time::Instant::now();
        let error = factory
            .available_models(Arc::new(Never))
            .await
            .err()
            .unwrap();
        assert!(error.info.safe_message.as_str().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(15));
        server.abort();
        let _ = server.await;
    }
}
