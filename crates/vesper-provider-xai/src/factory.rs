use crate::{
    DEFAULT_MODEL, REASONING_LEVELS, XaiCatalog, XaiSession, credentials::Credentials, error,
    provider_id,
};
use std::sync::Arc;
use vesper_domain::{
    BoundedString, ExtensionMap, ProviderId, SchemaVersion, VersionedExtensionEnvelope,
};
use vesper_provider::*;

#[derive(Clone)]
pub struct XaiFactory {
    id: ProviderId,
    pub(crate) credentials: Credentials,
    #[cfg(feature = "integration-test-harness")]
    pub(crate) test_route: Option<String>,
}
impl Default for XaiFactory {
    fn default() -> Self {
        Self {
            id: provider_id(),
            credentials: Credentials::default(),
            #[cfg(feature = "integration-test-harness")]
            test_route: None,
        }
    }
}
impl XaiFactory {
    pub fn default_configuration() -> ProviderConfiguration {
        let mut values = ExtensionMap::default();
        values
            .insert("xai:model", serde_json::json!(DEFAULT_MODEL))
            .expect("static");
        values
            .insert("xai:reasoning-effort", serde_json::json!("high"))
            .expect("static");
        ProviderConfiguration {
            provider_id: provider_id(),
            values: VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.xai").expect("static"),
                version: SchemaVersion::new(1).expect("static"),
                values,
            },
        }
    }
    #[cfg(feature = "integration-test-harness")]
    #[allow(clippy::result_large_err)]
    pub fn for_loopback(endpoint: &str) -> Result<Self, ProviderError> {
        let url = url::Url::parse(endpoint).map_err(|_| crate::wire::invalid())?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(crate::wire::invalid());
        }
        Ok(Self {
            test_route: Some(endpoint.to_owned()),
            ..Self::default()
        })
    }
}
impl ProviderFactory for XaiFactory {
    type Session = XaiSession;
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider_id: self.id.clone(),
            display_name: BoundedString::new("xAI / Grok").expect("static"),
            authentication_methods: vec![AuthenticationMethodDescriptor {
                method_id: BoundedString::new("xai-api-key").expect("static"),
                display_name: BoundedString::new("xAI API key (usage-based API billing)")
                    .expect("static"),
                secret_reference_fields: vec![BoundedString::new("XAI_API_KEY").expect("static")],
                external_runtime_owned: false,
                key_url: Some(BoundedString::new("https://console.x.ai/").expect("static")),
            }],
            configuration: None,
            metadata: ExtensionMap::default(),
        }
    }
    fn create_session<'a>(
        &'a self,
        config: &'a ProviderConfiguration,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<XaiSession, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(error(
                    "xAI request cancelled",
                    vesper_domain::ErrorCategory::Cancellation,
                    false,
                ));
            }
            let model = config
                .values
                .values
                .get("xai:model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(DEFAULT_MODEL);
            let effort = config
                .values
                .values
                .get("xai:reasoning-effort")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("high");
            if config.provider_id != self.id
                || XaiCatalog::find(model).is_none()
                || !REASONING_LEVELS.contains(&effort)
            {
                return Err(error(
                    "Invalid xAI model or reasoning selection",
                    vesper_domain::ErrorCategory::InvalidRequest,
                    false,
                ));
            }
            let session = XaiSession::new(self.credentials.clone(), effort.to_owned())?;
            #[cfg(feature = "integration-test-harness")]
            let session = session.with_test_route(self.test_route.clone());
            Ok(session)
        })
    }
}
impl ModelCatalog for XaiFactory {
    fn models<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        XaiCatalog.models(cancel)
    }
}
impl ProviderCredentialPort for XaiFactory {
    fn credential_present(&self) -> Result<bool, CredentialError> {
        #[cfg(feature = "integration-test-harness")]
        if self.test_route.is_some() {
            return Ok(true);
        }
        self.credentials.present()
    }
    fn store_credential(&self, secret: &str) -> Result<(), CredentialError> {
        self.credentials.store(secret)
    }
    fn authentication_method(&self) -> Result<Option<String>, CredentialError> {
        Ok(self.credential_present()?.then(|| "xai-api-key".into()))
    }
}
