#![forbid(unsafe_code)]
//! Production Z.ai GLM provider adapter.
//!
//! This crate owns GLM wire behavior behind `vesper-provider` ports. It does
//! not implement ACP, an agent loop, tools, persistence, or a frontend.

mod adapter;
mod auth;
mod auxiliary;
mod catalog;
mod compatibility;
mod config;
mod error;
mod factory;
mod policy;
mod quota;
mod request;
mod response;
mod retry;
mod sse;
mod transport;

#[cfg(test)]
mod integration_tests;

use vesper_domain::ProviderId;

pub use adapter::{GlmSession, continuation_message};
pub use auth::{
    AuthStoreError, EnvironmentCredentialSource, GlmCredentialSource, ResolvedCredential,
    StaticCredentialSource, credentials_path, resolve_credential, store_api_key, store_api_key_at,
};
pub use catalog::{GlmCatalog, GlmModelInfo};
pub use compatibility::{LegacyGlmConfiguration, translate_legacy_settings};
pub use config::{GlmConfig, GlmEndpoint, GlmGenerationProfile, GlmPlan, GlmReasoningMode};
pub use error::GlmAdapterError;
pub use factory::{GLM_REASONING_MODES, GlmFactory, reasoning_mode_for_superpower};
pub use policy::GlmSuperpowerPolicy;
pub use quota::{GlmPlanUsage, GlmQuotaWindow};
pub use request::{SerializedGlmRequest, serialize_auxiliary_request, serialize_request};
pub use retry::{JitterSource, RetryPolicy, SystemJitter, parse_retry_after};
pub use sse::{
    MAX_ERROR_BODY_BYTES, MAX_PROVIDER_METADATA_BYTES, MAX_SSE_EVENT_BYTES, MAX_SSE_LINE_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_NAME_BYTES, SseError, SseFrame, SseParser,
};

/// Stable provider identity used by all GLM contributions.
#[must_use]
pub fn provider_id() -> ProviderId {
    ProviderId::new("zai").expect("static provider ID")
}
