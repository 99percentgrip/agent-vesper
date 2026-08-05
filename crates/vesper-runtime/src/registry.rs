use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::RwLock;
use vesper_domain::ProviderId;
use vesper_provider::{
    CancellationSignal, CredentialError, PermissiveSuperpowerPolicy, ProviderConfiguration,
    ProviderCredentialPort, ProviderDescriptor, ProviderFactory, ProviderFuture, ProviderSession,
    ProviderSuperpowers, SuperpowerDescriptor, SuperpowerPolicy,
};

use crate::RuntimeError;

trait ErasedProviderFactory: Send + Sync {
    fn create<'a>(
        &'a self,
        configuration: &'a ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Arc<dyn ProviderSession>, vesper_provider::ProviderError>>;

    /// Advertised provider descriptor (identity + auth methods + config).
    fn descriptor(&self) -> ProviderDescriptor;
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

    fn descriptor(&self) -> ProviderDescriptor {
        ProviderFactory::descriptor(&self.0)
    }
}

/// One registered provider: its factory plus optional superpowers surface.
struct RegistryEntry {
    factory: Arc<dyn ErasedProviderFactory>,
    superpowers: Option<Arc<dyn ProviderSuperpowers>>,
    credentials: Option<Arc<dyn ProviderCredentialPort>>,
    policy: Option<Arc<dyn SuperpowerPolicy>>,
}

/// Heterogeneous provider-factory registry with no concrete provider enum.
#[derive(Default)]
pub struct ProviderRegistry {
    factories: RwLock<BTreeMap<ProviderId, RegistryEntry>>,
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
        self.register_inner(factory, None, None, None).await
    }

    /// Registers one provider factory together with its [`ProviderSuperpowers`]
    /// surface so the runtime can advertise provider-native controls to the
    /// composition boundary.
    pub async fn register_with_superpowers<F, S>(
        &self,
        factory: F,
        superpowers: S,
    ) -> Result<(), RuntimeError>
    where
        F: ProviderFactory + 'static,
        F::Session: 'static,
        S: ProviderSuperpowers + 'static,
    {
        self.register_inner(factory, Some(Arc::new(superpowers)), None, None)
            .await
    }

    /// Registers one provider factory together with its
    /// [`ProviderCredentialPort`] so hosts can route credential checks and
    /// storage through the provider instead of hardcoding match arms.
    pub async fn register_with_credentials<F, C>(
        &self,
        factory: F,
        credentials: C,
    ) -> Result<(), RuntimeError>
    where
        F: ProviderFactory + 'static,
        F::Session: 'static,
        C: ProviderCredentialPort + 'static,
    {
        self.register_inner(factory, None, Some(Arc::new(credentials)), None)
            .await
    }

    /// Registers one provider factory together with both its
    /// [`ProviderSuperpowers`] surface and its [`ProviderCredentialPort`].
    pub async fn register_with_superpowers_and_credentials<F, S, C>(
        &self,
        factory: F,
        superpowers: S,
        credentials: C,
    ) -> Result<(), RuntimeError>
    where
        F: ProviderFactory + 'static,
        F::Session: 'static,
        S: ProviderSuperpowers + 'static,
        C: ProviderCredentialPort + 'static,
    {
        self.register_inner(
            factory,
            Some(Arc::new(superpowers)),
            Some(Arc::new(credentials)),
            None,
        )
        .await
    }

    /// Registers one provider factory together with its
    /// [`ProviderSuperpowers`] surface and its [`SuperpowerPolicy`] (the
    /// provider-routed model/plan/reasoning logic), so hosts never hardcode a
    /// concrete provider's behavior.
    pub async fn register_with_superpowers_and_policy<F, S, P>(
        &self,
        factory: F,
        superpowers: S,
        policy: P,
    ) -> Result<(), RuntimeError>
    where
        F: ProviderFactory + 'static,
        F::Session: 'static,
        S: ProviderSuperpowers + 'static,
        P: SuperpowerPolicy + 'static,
    {
        self.register_inner(
            factory,
            Some(Arc::new(superpowers)),
            None,
            Some(Arc::new(policy)),
        )
        .await
    }

    async fn register_inner<F>(
        &self,
        factory: F,
        superpowers: Option<Arc<dyn ProviderSuperpowers>>,
        credentials: Option<Arc<dyn ProviderCredentialPort>>,
        policy: Option<Arc<dyn SuperpowerPolicy>>,
    ) -> Result<(), RuntimeError>
    where
        F: ProviderFactory + 'static,
        F::Session: 'static,
    {
        let id = factory.provider_id().clone();
        let mut factories = self.factories.write().await;
        if factories.contains_key(&id) {
            return Err(RuntimeError::DuplicateProvider);
        }
        factories.insert(
            id,
            RegistryEntry {
                factory: Arc::new(FactoryAdapter(factory)),
                superpowers,
                credentials,
                policy,
            },
        );
        Ok(())
    }

    /// Creates a scoped provider session for direct turn dispatch.
    ///
    /// Public as the Tier C composition seam (ADR 0010): the `vesper-agent`
    /// crate composes the runtime's provider dispatch into its multi-turn
    /// tool-executing loop. The runtime itself stays single-turn — repeated
    /// dispatch and tool-result feedback live in `vesper-agent`, not here.
    pub async fn create_session(
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
            .map(|entry| entry.factory.clone())
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

    /// Returns the advertised descriptor for `provider_id` (identity, auth
    /// methods, configuration contribution), or `None` when unregistered.
    /// Hosts use this to route auth from advertised descriptors instead of
    /// hardcoding provider match arms.
    pub async fn descriptor(&self, provider_id: &ProviderId) -> Option<ProviderDescriptor> {
        self.factories
            .read()
            .await
            .get(provider_id)
            .map(|entry| entry.factory.descriptor())
    }

    /// Whether the provider has a locally valid credential. Routes through the
    /// provider's [`ProviderCredentialPort`]; the secret stays adapter-internal.
    /// Blocking credential I/O runs on a Tokio blocking thread.
    pub async fn credential_present(
        &self,
        provider_id: &ProviderId,
    ) -> Result<bool, CredentialError> {
        let port = self
            .factories
            .read()
            .await
            .get(provider_id)
            .and_then(|entry| entry.credentials.clone())
            .ok_or(CredentialError::Unavailable)?;
        tokio::task::spawn_blocking(move || port.credential_present())
            .await
            .map_err(|_| CredentialError::Failed)?
    }

    /// Persists a credential for `provider_id`, routing through the provider's
    /// [`ProviderCredentialPort`]. Blocking credential I/O runs on a Tokio
    /// blocking thread.
    pub async fn store_credential(
        &self,
        provider_id: &ProviderId,
        secret: String,
    ) -> Result<(), CredentialError> {
        let port = self
            .factories
            .read()
            .await
            .get(provider_id)
            .and_then(|entry| entry.credentials.clone())
            .ok_or(CredentialError::Unavailable)?;
        tokio::task::spawn_blocking(move || port.store_credential(&secret))
            .await
            .map_err(|_| CredentialError::Failed)?
    }

    /// Lists every registered provider identity in stable order.
    pub async fn provider_ids(&self) -> Vec<ProviderId> {
        self.factories.read().await.keys().cloned().collect()
    }

    /// Returns the superpower descriptors advertised by `provider_id`, or an
    /// empty vector when the provider registered none (or is unknown).
    pub async fn superpowers(&self, provider_id: &ProviderId) -> Vec<SuperpowerDescriptor> {
        self.factories
            .read()
            .await
            .get(provider_id)
            .and_then(|entry| entry.superpowers.as_ref())
            .map(|surface| surface.superpowers())
            .unwrap_or_default()
    }

    /// Returns the [`SuperpowerPolicy`] for `provider_id` (the provider-routed
    /// model/plan/reasoning logic), or a permissive default when the provider
    /// registered none (or is unknown). Hosts route through this instead of
    /// hardcoding provider-specific behavior.
    pub async fn superpower_policy(&self, provider_id: &ProviderId) -> Arc<dyn SuperpowerPolicy> {
        self.factories
            .read()
            .await
            .get(provider_id)
            .and_then(|entry| entry.policy.clone())
            .unwrap_or_else(|| Arc::new(PermissiveSuperpowerPolicy))
    }

    /// Returns the superpower descriptors for every registered provider, in
    /// stable order. Providers without superpowers are omitted.
    pub async fn all_superpowers(&self) -> Vec<(ProviderId, Vec<SuperpowerDescriptor>)> {
        self.factories
            .read()
            .await
            .iter()
            .filter_map(|(id, entry)| {
                entry
                    .superpowers
                    .as_ref()
                    .map(|surface| (id.clone(), surface.superpowers()))
            })
            .collect()
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

    /// Recording factory that also advertises one Choice superpower so the
    /// registry's superpowers surface can be exercised without taking a
    /// dependency on a concrete adapter.
    struct SuperpoweredFactory {
        id: ProviderId,
        descriptor: vesper_provider::SuperpowerDescriptor,
    }

    impl ProviderFactory for SuperpoweredFactory {
        type Session = TrivialSession;

        fn provider_id(&self) -> &ProviderId {
            &self.id
        }

        fn create_session<'a>(
            &'a self,
            _: &'a ProviderConfiguration,
            _: Arc<dyn CancellationSignal>,
        ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
            Box::pin(async { Ok(TrivialSession) })
        }
    }

    impl vesper_provider::ProviderSuperpowers for SuperpoweredFactory {
        fn superpowers(&self) -> Vec<vesper_provider::SuperpowerDescriptor> {
            vec![self.descriptor.clone()]
        }
    }

    fn sample_descriptor(provider_id: &ProviderId) -> vesper_provider::SuperpowerDescriptor {
        use vesper_domain::BoundedString;
        use vesper_provider::{SuperpowerKind, SuperpowerScope, SuperpowerValue};
        vesper_provider::SuperpowerDescriptor {
            id: BoundedString::new("test:effort").unwrap(),
            provider_id: provider_id.clone(),
            display_name: BoundedString::new("Effort").unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Request,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("high").unwrap(),
            },
            allowed_values: Vec::new(),
            command_alias: Some(BoundedString::new("effort").unwrap()),
            help: Some(BoundedString::new("Per-request effort.").unwrap()),
        }
    }

    #[tokio::test]
    async fn superpowers_surface_is_queryable_per_provider() {
        let registry = ProviderRegistry::new();
        let powered_id = ProviderId::new("powered").unwrap();
        let plain_id = ProviderId::new("plain").unwrap();

        registry
            .register_with_superpowers(
                SuperpoweredFactory {
                    id: powered_id.clone(),
                    descriptor: sample_descriptor(&powered_id),
                },
                SuperpoweredFactory {
                    id: powered_id.clone(),
                    descriptor: sample_descriptor(&powered_id),
                },
            )
            .await
            .unwrap();
        registry
            .register(RecordingFactory {
                id: plain_id.clone(),
                calls: Arc::new(Mutex::new(Vec::new())),
            })
            .await
            .unwrap();

        // The powered provider exposes exactly its descriptor.
        let powered = registry.superpowers(&powered_id).await;
        assert_eq!(powered.len(), 1);
        assert_eq!(powered[0].id, sample_descriptor(&powered_id).id);

        // The plain provider exposes none.
        assert!(registry.superpowers(&plain_id).await.is_empty());

        // Unknown providers expose none as well.
        let unknown = ProviderId::new("missing").unwrap();
        assert!(registry.superpowers(&unknown).await.is_empty());

        // `all_superpowers` reports only providers with descriptors, in order.
        let all = registry.all_superpowers().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, powered_id);
        assert_eq!(all[0].1.len(), 1);

        // `provider_ids` returns every registered identity in stable order.
        let ids = registry.provider_ids().await;
        assert_eq!(ids, vec![plain_id, powered_id]);
    }

    /// Stub credential port that records stores and reports a fixed presence.
    struct StubCredentialPort {
        present: bool,
        stored: Arc<Mutex<Vec<String>>>,
    }

    impl vesper_provider::ProviderCredentialPort for StubCredentialPort {
        fn credential_present(&self) -> Result<bool, vesper_provider::CredentialError> {
            Ok(self.present)
        }
        fn store_credential(&self, secret: &str) -> Result<(), vesper_provider::CredentialError> {
            self.stored
                .lock()
                .expect("stored lock poisoned")
                .push(secret.to_owned());
            Ok(())
        }
    }

    #[tokio::test]
    async fn credential_check_and_store_route_through_the_registry() {
        // Multi-provider proof: credential check and storage dispatch purely
        // by ProviderId through the registered credential port — no hardcoded
        // provider match arm, and blocking I/O runs on a Tokio blocking thread.
        let registry = ProviderRegistry::new();
        let id = ProviderId::new("cred").unwrap();
        let stored = Arc::new(Mutex::new(Vec::new()));
        let port = StubCredentialPort {
            present: false,
            stored: Arc::clone(&stored),
        };
        registry
            .register_with_credentials(
                RecordingFactory {
                    id: id.clone(),
                    calls: Arc::new(Mutex::new(Vec::new())),
                },
                port,
            )
            .await
            .unwrap();

        // No credential present -> hub would open.
        assert!(!registry.credential_present(&id).await.unwrap());
        // Store routes to the port.
        registry
            .store_credential(&id, "secret-canary".to_owned())
            .await
            .unwrap();
        assert_eq!(
            stored.lock().unwrap().as_slice(),
            &["secret-canary".to_owned()]
        );
        // Unknown provider (or a provider registered without a port) surfaces
        // Unavailable rather than dispatching to a default.
        let unknown = ProviderId::new("missing").unwrap();
        assert_eq!(
            registry.credential_present(&unknown).await,
            Err(vesper_provider::CredentialError::Unavailable)
        );
    }
}
