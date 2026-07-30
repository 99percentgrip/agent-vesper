use std::{future::Future, pin::Pin, sync::Arc};

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use vesper_domain::{
    BoundedString, EndpointId, ExtensionMap, ProviderId, QualifiedModelId,
    VersionedExtensionEnvelope,
};

use crate::{
    AuxiliaryRequestIntent, ProviderCapabilities, ProviderConfigContribution, ProviderError,
    ProviderRequest, ProviderStreamEvent,
};

/// Boxed provider future without choosing an async runtime.
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Ordered provider stream without choosing HTTP/process transport.
///
/// Backpressure is consumer-driven: adapters must not require the consumer to
/// buffer an unbounded number of events between polls. Transport-specific
/// bounded channels and read limits belong to the adapter stage.
pub type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<ProviderStreamEvent, ProviderError>> + Send + 'static>>;

/// Hierarchical cancellation view supplied by the future runtime.
pub trait CancellationSignal: Send + Sync + 'static {
    /// Whether cancellation has been requested.
    fn is_cancelled(&self) -> bool;
}

/// Authentication method metadata; secret values remain external references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationMethodDescriptor {
    /// Stable adapter-owned method ID.
    pub method_id: BoundedString<128>,
    /// Safe display label.
    pub display_name: BoundedString<256>,
    /// Secret-reference field IDs required by the method.
    pub secret_reference_fields: Vec<BoundedString<128>>,
    /// Whether an external runtime owns authentication.
    pub external_runtime_owned: bool,
}

/// Stable provider descriptor independent of a configured session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    /// Provider identity.
    pub provider_id: ProviderId,
    /// Safe display name.
    pub display_name: BoundedString<256>,
    /// Authentication methods.
    pub authentication_methods: Vec<AuthenticationMethodDescriptor>,
    /// Configuration schema contribution.
    pub configuration: Option<ProviderConfigContribution>,
    /// Safe provider metadata.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

/// Opaque, versioned configuration validated by its provider adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfiguration {
    /// Provider owner.
    pub provider_id: ProviderId,
    /// Adapter-owned values.
    pub values: VersionedExtensionEnvelope,
}

/// Endpoint selection/configuration without transport-client types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndpointConfiguration {
    /// Endpoint identity.
    pub endpoint_id: EndpointId,
    /// Safe endpoint class (`remote`, `local`, `process`, or adapter-defined).
    pub endpoint_class: BoundedString<128>,
    /// Adapter-owned endpoint values; sensitive URLs must already be redacted.
    pub values: VersionedExtensionEnvelope,
}

/// Model catalog record with opaque provider metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// Model identity.
    pub model: QualifiedModelId,
    /// User-facing display name.
    pub display_name: BoundedString<256>,
    /// Capability snapshot.
    pub capabilities: ProviderCapabilities,
    /// Provider data core does not interpret.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

/// Origin of one model catalog snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelCatalogProvenance {
    /// Static adapter registry.
    Static,
    /// Provider discovery endpoint.
    Discovered,
    /// Explicit user configuration.
    UserConfigured,
    /// Cached copy of a prior source.
    Cached,
}

/// Catalog payload with explicit cache expiry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalogSnapshot {
    /// Models.
    pub models: Vec<ModelDescriptor>,
    /// Origin.
    pub provenance: ModelCatalogProvenance,
    /// Unix epoch milliseconds when the snapshot expires, if cacheable.
    pub expires_at_unix_ms: Option<u64>,
}

/// Model discovery/listing port.
pub trait ModelCatalog: Send + Sync {
    /// Returns models with provenance handled by the adapter.
    fn models<'a>(
        &'a self,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>>;
}

/// Provider factory that validates opaque adapter configuration.
pub trait ProviderFactory: Send + Sync {
    /// Session type owned by the adapter.
    type Session: ProviderSession;

    /// Stable provider identity.
    fn provider_id(&self) -> &ProviderId;

    /// Creates a scoped provider session. Authentication remains adapter-owned.
    fn create_session<'a>(
        &'a self,
        config: &'a ProviderConfiguration,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>>;
}

/// Scoped provider transport/session port.
pub trait ProviderSession: Send + Sync {
    /// Starts one ordered response stream.
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>>;
}

/// Optional bounded auxiliary request port, separate from streaming turns.
pub trait AuxiliaryRequestPort: Send + Sync {
    /// Executes one bounded provider request for a declared harness purpose.
    fn execute_auxiliary<'a>(
        &'a self,
        intent: AuxiliaryRequestIntent,
        request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<vesper_domain::ContentPart, ProviderError>>;
}
