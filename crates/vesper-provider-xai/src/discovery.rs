//! Account-scoped API model discovery intersected with verified capabilities.
use crate::{XaiCatalog, XaiFactory, XaiSession, error, transport::XaiRegion};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use vesper_domain::ErrorCategory;
use vesper_provider::*;

#[derive(Clone, Default)]
pub struct AvailableModels {
    pub models: Vec<ModelDescriptor>,
    pub unverified: Vec<String>,
    pub endpoint_excluded: Vec<String>,
    /// Selected non-secret billing/authentication mode for capability projection.
    pub authentication_method: Option<String>,
}
impl AvailableModels {
    pub fn contains(&self, id: &str) -> bool {
        self.models
            .iter()
            .any(|model| model.model.model_id.as_str() == id)
    }
}
#[allow(clippy::result_large_err)]
pub(crate) fn parse(
    value: &serde_json::Value,
    region: XaiRegion,
) -> Result<AvailableModels, ProviderError> {
    let rows = value
        .get("models")
        .or_else(|| value.get("data"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(crate::wire::invalid)?;
    if rows.len() > 4096 {
        return Err(crate::wire::invalid());
    }
    let mut seen = BTreeSet::new();
    let mut canonical_seen = BTreeSet::new();
    let mut models = Vec::new();
    let mut unverified = Vec::new();
    let mut endpoint_excluded = Vec::new();
    for row in rows {
        let id = row
            .get("id")
            .or_else(|| row.get("model"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(crate::wire::invalid)?;
        if id.is_empty() || id.len() > 256 || !seen.insert(id) {
            return Err(crate::wire::invalid());
        }
        match XaiCatalog::resolve_current_alias(id) {
            Some(canonical) if !canonical_seen.insert(canonical) => {}
            Some(canonical) if region.supports(canonical) => {
                models.push(XaiCatalog::find(canonical).expect("resolved catalog entry"))
            }
            Some(canonical) => endpoint_excluded.push(canonical.to_owned()),
            None => unverified.push(id.to_owned()),
        }
    }
    let order = XaiCatalog::snapshot().models;
    models.sort_by_key(|model| order.iter().position(|known| known.model == model.model));
    Ok(AvailableModels {
        models,
        unverified,
        endpoint_excluded,
        authentication_method: None,
    })
}
impl XaiFactory {
    pub async fn available_models(
        &self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<AvailableModels, ProviderError> {
        self.available_models_in(XaiRegion::Global, cancel).await
    }
    pub(crate) async fn available_models_in(
        &self,
        region: XaiRegion,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<AvailableModels, ProviderError> {
        *self
            .availability
            .write()
            .map_err(|_| crate::wire::invalid())? = Some(AvailableModels::default());
        let session = XaiSession::new(self.credentials.clone(), "high".into(), region)?;
        #[cfg(feature = "integration-test-harness")]
        let session = session.with_test_route(self.test_route.clone());
        let result = session.available_models(cancel).await;
        let snapshot = result.as_ref().cloned().unwrap_or_default();
        *self
            .availability
            .write()
            .map_err(|_| crate::wire::invalid())? = Some(snapshot);
        result
    }
}
impl XaiSession {
    pub(crate) async fn available_models(
        &self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> Result<AvailableModels, ProviderError> {
        let operation = async {
            let auth = self
                .resolve_auth(false, cancel.clone())
                .await
                .map_err(|_| {
                    error(
                        "Configure an xAI API key before loading models",
                        ErrorCategory::Authentication,
                        false,
                    )
                })?;
            if auth.mode == crate::credentials::AuthenticationMode::GrokSession
                && self.region != XaiRegion::Global
            {
                return Err(error(
                    "Grok account model discovery uses the global subscription endpoint",
                    ErrorCategory::InvalidRequest,
                    false,
                ));
            }
            let endpoint = match (auth.mode, self.region) {
                (crate::credentials::AuthenticationMode::GrokSession, _) => {
                    "https://cli-chat-proxy.grok.com/v1/models"
                }
                (crate::credentials::AuthenticationMode::ApiKey, XaiRegion::Global) => {
                    "https://api.x.ai/v1/language-models"
                }
                (crate::credentials::AuthenticationMode::ApiKey, XaiRegion::Us) => {
                    "https://us.api.x.ai/v1/language-models"
                }
            };
            let url = url::Url::parse(endpoint).expect("fixed URL");
            #[cfg(feature = "integration-test-harness")]
            let url = if let Some(route) = &self.test_route {
                let mut test_url = url::Url::parse(route).map_err(|_| crate::wire::invalid())?;
                test_url.set_path("/language-models");
                test_url.set_query(None);
                test_url
            } else {
                url
            };
            let mut builder = self
                .client
                .get(url)
                .bearer_auth(auth.bearer.expose().as_str())
                .header("Accept", "application/json");
            if auth.mode == crate::credentials::AuthenticationMode::GrokSession {
                builder = builder.header("X-XAI-Token-Auth", "xai-grok-cli");
            }
            let mut response = builder.send().await.map_err(|_| {
                error(
                    "xAI model discovery connection failed; reopen Settings to retry",
                    ErrorCategory::Transport,
                    false,
                )
            })?;
            if !response.status().is_success() {
                return Err(crate::http_error::rejection(response, cancel.as_ref()).await);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| crate::wire::invalid())? {
                if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                    return Err(crate::wire::invalid());
                }
                bytes.extend_from_slice(&chunk);
            }
            let mut available = parse(
                &serde_json::from_slice(&bytes).map_err(|_| crate::wire::invalid())?,
                self.region,
            )?;
            available.authentication_method = Some(
                match auth.mode {
                    crate::credentials::AuthenticationMode::GrokSession => "xai-grok-session",
                    crate::credentials::AuthenticationMode::ApiKey => "xai-api-key",
                }
                .to_owned(),
            );
            Ok(available)
        };
        tokio::select! {biased;
            _=async{while !cancel.is_cancelled(){tokio::time::sleep(Duration::from_millis(25)).await;}}=>Err(error("xAI model discovery cancelled",ErrorCategory::Cancellation,false)),
            result=tokio::time::timeout(Duration::from_secs(10),operation)=>result.map_err(|_|error("xAI model discovery timed out; reopen Settings to retry",ErrorCategory::Transport,false))?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unknown_discovered_models_are_retained_as_unverified_but_not_executable() {
        let result = parse(
            &json!({"models":[{"id":"grok-4.7"},{"id":"future-grok"}]}),
            XaiRegion::Global,
        )
        .unwrap();
        assert!(result.contains("grok-4.7"));
        assert!(!result.contains("future-grok"));
        assert_eq!(result.unverified, ["future-grok"]);
    }
    #[test]
    fn malformed_duplicate_and_empty_catalogs_fail_or_stay_empty() {
        assert!(parse(&json!({}), XaiRegion::Global).is_err());
        assert!(
            parse(
                &json!({"models":[{"id":"grok-4.7"},{"id":"grok-4.7"}]}),
                XaiRegion::Global
            )
            .is_err()
        );
        assert!(parse(&json!({"models":[{}]}), XaiRegion::Global).is_err());
        assert!(
            parse(&json!({"models":[]}), XaiRegion::Global)
                .unwrap()
                .models
                .is_empty()
        );
    }
    #[test]
    fn us_region_excludes_models_outside_the_documented_endpoint_set() {
        let result = parse(
            &json!({"models":[{"id":"grok-4.7"},{"id":"grok-4.5"}]}),
            XaiRegion::Us,
        )
        .unwrap();
        assert!(result.contains("grok-4.7"));
        assert!(!result.contains("grok-4.5"));
        assert!(result.unverified.is_empty());
        assert_eq!(result.endpoint_excluded, ["grok-4.5"]);
    }
    #[test]
    fn moving_aliases_collapse_to_canonical_and_retired_aliases_stay_unverified() {
        let result = parse(
            &json!({"models":[{"id":"grok-4.7-latest"},{"id":"grok-4.7"},{"id":"grok-code-fast-1"}]}),
            XaiRegion::Global,
        )
        .unwrap();
        assert_eq!(result.models.len(), 1);
        assert!(result.contains("grok-4.7"));
        assert_eq!(result.unverified, ["grok-code-fast-1"]);
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
    #[tokio::test]
    async fn loopback_discovery_authenticates_and_intersects_verified_catalog() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let factory = XaiFactory::for_loopback(&format!(
            "http://{}/responses",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(bytes).unwrap().to_lowercase();
            assert!(request.starts_with("get /language-models http/1.1"));
            assert!(request.contains("authorization: bearer fixture-xai-key"));
            let body = r#"{"models":[{"id":"future-grok"},{"id":"grok-4.6"},{"id":"grok-4.7"}]}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let available = factory.available_models(Arc::new(Never)).await.unwrap();
        assert_eq!(
            available
                .models
                .iter()
                .map(|model| model.model.model_id.as_str())
                .collect::<Vec<_>>(),
            ["grok-4.7", "grok-4.6"]
        );
        assert_eq!(available.unverified, ["future-grok"]);
        server.await.unwrap();
    }
}
