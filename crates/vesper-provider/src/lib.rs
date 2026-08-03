#![forbid(unsafe_code)]
//! Cohesive ports and capability contracts implemented by future provider adapters.

mod capability;
mod error;
mod ports;
mod request;
mod stream;
mod superpowers;

pub use capability::{
    AuthenticationCapability, CapabilityResolution, ContinuationCapability,
    ExternalRuntimeCapability, MediaCapability, ModelLimits, PreservedReasoningCapability,
    PromptCacheCapability, ProviderCapabilities, ReasoningCapability, StreamedReasoningCapability,
    StructuredOutputCapability, SupportLevel, ToolCapability, ToolChoiceCapability,
    resolve_support,
};
pub use error::{ProviderError, RetryDecision};
pub use ports::{
    AuthenticationMethodDescriptor, AuxiliaryRequestPort, CancellationSignal, CredentialError,
    EndpointConfiguration, ModelCatalog, ModelCatalogProvenance, ModelCatalogSnapshot,
    ModelDescriptor, ProviderConfiguration, ProviderCredentialPort, ProviderDescriptor,
    ProviderEventStream, ProviderFactory, ProviderFuture, ProviderSession,
};
pub use request::{
    AuxiliaryRequestIntent, ContinuationContext, ContinuationReason, ContinuationStrategy,
    FallbackDecision, FallbackPolicy, ProviderConfigContribution, ProviderRequest, ReasoningIntent,
    RequestValidationError, SamplingIntent, StructuredOutputIntent, ToolChoice,
};
pub use stream::{
    ProviderStreamContract, ProviderStreamContractError, ProviderStreamEvent, QuotaUpdate,
    RateLimitUpdate,
};
pub use superpowers::{
    ProviderSuperpowers, SuperpowerDescriptor, SuperpowerKind, SuperpowerScope, SuperpowerValue,
};
