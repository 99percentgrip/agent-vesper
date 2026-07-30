use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::RwLock;
use vesper_domain::ProviderId;
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderFactory, ProviderFuture, ProviderSession,
};

use crate::RuntimeError;

trait ErasedProviderFactory: Send + Sync {
    fn create<'a>(
        &'a self,
        configuration: &'a ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Arc<dyn ProviderSession>, vesper_provider::ProviderError>>;
}

struct FactoryAdapter<F>(F);

impl<F> ErasedProviderFactory for FactoryAdapter<F>
where
    F: ProviderFactory + 'static,
    F::Session: 'static,
{
    fn create<'a>(
        &'a self,
        configuration: &'a ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Arc<dyn ProviderSession>, vesper_provider::ProviderError>> {
        Box::pin(async move {
            self.0
                .create_session(configuration, cancellation)
                .await
                .map(|session| Arc::new(session) as Arc<dyn ProviderSession>)
        })
    }
}

/// Heterogeneous provider-factory registry with no concrete provider enum.
#[derive(Default)]
pub struct ProviderRegistry {
    factories: RwLock<BTreeMap<ProviderId, Arc<dyn ErasedProviderFactory>>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRegistry")
            .finish_non_exhaustive()
    }
}

impl ProviderRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one provider factory, rejecting duplicate identities.
    pub async fn register<F>(&self, factory: F) -> Result<(), RuntimeError>
    where
        F: ProviderFactory + 'static,
        F::Session: 'static,
    {
        let id = factory.provider_id().clone();
        let mut factories = self.factories.write().await;
        if factories.contains_key(&id) {
            return Err(RuntimeError::DuplicateProvider);
        }
        factories.insert(id, Arc::new(FactoryAdapter(factory)));
        Ok(())
    }

    pub(crate) async fn create_session(
        &self,
        provider_id: &ProviderId,
        configuration: &ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> Result<Arc<dyn ProviderSession>, RuntimeError> {
        let factory = self
            .factories
            .read()
            .await
            .get(provider_id)
            .cloned()
            .ok_or(RuntimeError::UnknownProvider)?;
        factory
            .create(configuration, cancellation)
            .await
            .map_err(|_| RuntimeError::Provider)
    }

    /// Returns whether a provider is registered.
    pub async fn contains(&self, provider_id: &ProviderId) -> bool {
        self.factories.read().await.contains_key(provider_id)
    }
}

#[cfg(test)]
mod tests {
    //! Multi-provider dispatch proof: the registry is provider-agnostic and
    //! routes `create_session` purely by `ProviderId`. These tests use inline
    //! recording factories so the proof carries no concrete-provider dependency.

    use std::sync::{Arc, Mutex};

    use vesper_domain::{
        ErrorCategory, ErrorInfo, ProviderId, RedactedDiagnostics, Retryability, SafeMessage,
    };
    use vesper_provider::{
        CancellationSignal, ProviderConfiguration, ProviderError, ProviderEventStream,
        ProviderFactory, ProviderFuture, ProviderRequest, ProviderSession,
    };

    use super::ProviderRegistry;
    use crate::RuntimeError;

    /// Session whose `start` is never exercised by the dispatch tests.
    struct TrivialSession;

    impl ProviderSession for TrivialSession {
        fn start<'a>(
            &'a self,
            _: ProviderRequest,
            _: Arc<dyn CancellationSignal>,
        ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
            // The dispatch tests never start a stream; returning a classified
            // error keeps this trivial and avoids pulling in a stream impl.
            Box::pin(async { Err(trivial_error()) })
        }
    }

    /// Records its identity every time `create_session` is dispatched to it.
    struct RecordingFactory {
        id: ProviderId,
        calls: Arc<Mutex<Vec<ProviderId>>>,
    }

    impl ProviderFactory for RecordingFactory {
        type Session = TrivialSession;

        fn provider_id(&self) -> &ProviderId {
            &self.id
        }

        fn create_session<'a>(
            &'a self,
            _: &'a ProviderConfiguration,
            _: Arc<dyn CancellationSignal>,
        ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
            let id = self.id.clone();
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                calls.lock().expect("calls lock poisoned").push(id);
                Ok(TrivialSession)
            })
        }
    }

    fn trivial_error() -> ProviderError {
        ProviderError {
            provider_id: ProviderId::new("test.dispatch").expect("static provider ID"),
            provider_code: None,
            http_status: None,
            continuation_possible: false,
            info: ErrorInfo {
                category: ErrorCategory::InvalidRequest,
                retryability: Retryability::Never,
                retry_after_ms: None,
                visible_output_emitted: false,
                safe_message: SafeMessage::new("dispatch probe").expect("bounded message"),
                diagnostics: RedactedDiagnostics::default(),
                provider_code: None,
                causes: Vec::new(),
            },
            metadata: vesper_domain::ExtensionMap::default(),
        }
    }

    struct NeverCancelled;
    impl CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    fn empty_configuration(provider_id: &ProviderId) -> ProviderConfiguration {
        ProviderConfiguration {
            provider_id: provider_id.clone(),
            values: vesper_domain::VersionedExtensionEnvelope {
                namespace: vesper_domain::ExtensionNamespace::new("provider.dispatch").unwrap(),
                version: vesper_domain::SchemaVersion::new(1).unwrap(),
                values: vesper_domain::ExtensionMap::default(),
            },
        }
    }

    #[tokio::test]
    async fn registry_dispatches_to_the_correct_provider_by_identity() {
        let registry = ProviderRegistry::new();
        let alpha_calls = Arc::new(Mutex::new(Vec::new()));
        let beta_calls = Arc::new(Mutex::new(Vec::new()));
        let alpha_id = ProviderId::new("alpha").unwrap();
        let beta_id = ProviderId::new("beta").unwrap();

        registry
            .register(RecordingFactory {
                id: alpha_id.clone(),
                calls: Arc::clone(&alpha_calls),
            })
            .await
            .unwrap();
        registry
            .register(RecordingFactory {
                id: beta_id.clone(),
                calls: Arc::clone(&beta_calls),
            })
            .await
            .unwrap();

        assert!(registry.contains(&alpha_id).await);
        assert!(registry.contains(&beta_id).await);

        let cancellation = Arc::new(NeverCancelled) as Arc<dyn CancellationSignal>;
        registry
            .create_session(
                &alpha_id,
                &empty_configuration(&alpha_id),
                Arc::clone(&cancellation),
            )
            .await
            .unwrap();
        registry
            .create_session(
                &beta_id,
                &empty_configuration(&beta_id),
                Arc::clone(&cancellation),
            )
            .await
            .unwrap();

        let alpha_recorded = alpha_calls.lock().unwrap().clone();
        assert_eq!(
            alpha_recorded.len(),
            1,
            "alpha session must dispatch to the alpha factory only"
        );
        assert_eq!(alpha_recorded[0], alpha_id);
        let beta_recorded = beta_calls.lock().unwrap().clone();
        assert_eq!(
            beta_recorded.len(),
            1,
            "beta session must dispatch to the beta factory only"
        );
        assert_eq!(beta_recorded[0], beta_id);
    }

    #[tokio::test]
    async fn unknown_provider_and_duplicate_registration_are_rejected() {
        let registry = ProviderRegistry::new();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let alpha_id = ProviderId::new("alpha").unwrap();

        registry
            .register(RecordingFactory {
                id: alpha_id.clone(),
                calls: Arc::clone(&calls),
            })
            .await
            .unwrap();

        // Duplicate registration is rejected without disturbing the first.
        let duplicate = registry
            .register(RecordingFactory {
                id: alpha_id.clone(),
                calls: Arc::clone(&calls),
            })
            .await;
        assert_eq!(duplicate, Err(RuntimeError::DuplicateProvider));

        // Unknown provider dispatch surfaces a classified error.
        let unknown = ProviderId::new("missing").unwrap();
        let cancellation = Arc::new(NeverCancelled) as Arc<dyn CancellationSignal>;
        let missing = registry
            .create_session(&unknown, &empty_configuration(&unknown), cancellation)
            .await;
        assert!(
            matches!(missing, Err(RuntimeError::UnknownProvider)),
            "unknown provider must surface a classified error"
        );
    }
}
