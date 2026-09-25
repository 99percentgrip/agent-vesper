#![forbid(unsafe_code)]
//! Native xAI adapter. API-key Responses transport is implemented directly;
//! Grok-account authentication remains a separately gated VRO-18 phase.

mod auth;
mod catalog;
mod credentials;
mod discovery;
mod factory;
mod http_error;
mod transport;
mod wire;

pub use catalog::{DEFAULT_MODEL, XaiCatalog};
pub use discovery::AvailableModels;
pub use factory::XaiFactory;
pub use transport::XaiSession;

#[cfg(test)]
mod tests;

/// Stable provider identity independent of authentication and endpoint mode.
pub fn provider_id() -> vesper_domain::ProviderId {
    vesper_domain::ProviderId::new("xai").expect("static provider id")
}

pub(crate) fn error(
    message: &'static str,
    category: vesper_domain::ErrorCategory,
    visible: bool,
) -> vesper_provider::ProviderError {
    vesper_provider::ProviderError {
        provider_id: provider_id(),
        provider_code: None,
        http_status: None,
        continuation_possible: false,
        info: vesper_domain::ErrorInfo {
            category,
            retryability: vesper_domain::Retryability::Never,
            retry_after_ms: None,
            visible_output_emitted: visible,
            safe_message: vesper_domain::SafeMessage::new(message).expect("static safe message"),
            diagnostics: Default::default(),
            provider_code: None,
            causes: vec![],
        },
        metadata: Default::default(),
    }
}
