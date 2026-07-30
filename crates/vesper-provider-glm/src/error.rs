use serde_json::json;
use thiserror::Error;
use vesper_domain::{
    ErrorCategory, ErrorInfo, ExtensionMap, RedactedDiagnostics, Retryability, SafeMessage,
    SafeProviderCode,
};
use vesper_provider::ProviderError;

use crate::provider_id;

/// GLM adapter configuration or wire-contract failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GlmAdapterError {
    /// Provider configuration was invalid or incompatible.
    #[error("GLM provider configuration is invalid: {0}")]
    Configuration(&'static str),
    /// The selected model/endpoint combination is unsupported.
    #[error("GLM model is not available for the selected endpoint plan")]
    ModelPlanMismatch,
    /// The selected model is not in the frozen catalog.
    #[error("GLM model is unknown")]
    UnknownModel,
    /// A neutral request cannot be represented on the GLM wire.
    #[error("GLM request is unsupported: {0}")]
    UnsupportedRequest(&'static str),
    /// A bounded wire field exceeded its adapter limit.
    #[error("GLM wire field exceeds its configured bound: {0}")]
    Limit(&'static str),
    /// GLM protocol data was structurally invalid.
    #[error("GLM protocol data is malformed")]
    MalformedProtocol,
    /// Checked usage arithmetic failed.
    #[error("GLM usage arithmetic overflow")]
    UsageOverflow,
}

pub(crate) fn provider_error(
    category: ErrorCategory,
    retryability: Retryability,
    visible: bool,
    message: &'static str,
    provider_code: Option<&str>,
    http_status: Option<u16>,
    retry_after_ms: Option<u64>,
) -> ProviderError {
    let safe_code = provider_code.and_then(|code| SafeProviderCode::new(code).ok());
    let mut fields = ExtensionMap::default();
    if let Some(status) = http_status {
        fields
            .insert("zai:http-status", json!(status))
            .expect("bounded static diagnostic");
    }
    ProviderError {
        provider_id: provider_id(),
        provider_code: safe_code.clone(),
        http_status,
        continuation_possible: false,
        info: ErrorInfo {
            category,
            retryability,
            retry_after_ms,
            visible_output_emitted: visible,
            safe_message: SafeMessage::new(message).expect("bounded static message"),
            diagnostics: RedactedDiagnostics { fields },
            provider_code: safe_code,
            causes: Vec::new(),
        },
        metadata: ExtensionMap::default(),
    }
}

pub(crate) fn cancelled_error(visible: bool) -> ProviderError {
    provider_error(
        ErrorCategory::Cancellation,
        Retryability::Never,
        visible,
        "GLM request was cancelled",
        Some("cancelled"),
        None,
        None,
    )
}

pub(crate) fn adapter_error(error: &GlmAdapterError, visible: bool) -> ProviderError {
    let (category, code) = match error {
        GlmAdapterError::Configuration(_)
        | GlmAdapterError::ModelPlanMismatch
        | GlmAdapterError::UnknownModel => (ErrorCategory::InvalidRequest, "configuration"),
        GlmAdapterError::UnsupportedRequest(_) => {
            (ErrorCategory::UnsupportedCapability, "unsupported")
        }
        GlmAdapterError::Limit(_) => (ErrorCategory::Security, "wire-limit"),
        GlmAdapterError::MalformedProtocol => {
            (ErrorCategory::MalformedProtocol, "malformed-protocol")
        }
        GlmAdapterError::UsageOverflow => (ErrorCategory::MalformedProtocol, "usage-overflow"),
    };
    provider_error(
        category,
        Retryability::Never,
        visible,
        match error {
            GlmAdapterError::Configuration(_) => "GLM provider configuration is invalid",
            GlmAdapterError::ModelPlanMismatch => {
                "GLM model is unavailable on the selected endpoint plan"
            }
            GlmAdapterError::UnknownModel => "GLM model is unknown",
            GlmAdapterError::UnsupportedRequest(_) => {
                "GLM cannot satisfy the requested provider capability"
            }
            GlmAdapterError::Limit(_) => "GLM response exceeded a safety limit",
            GlmAdapterError::MalformedProtocol => "GLM returned malformed protocol data",
            GlmAdapterError::UsageOverflow => "GLM usage counters overflowed",
        },
        Some(code),
        None,
        None,
    )
}

pub(crate) fn authentication_error() -> ProviderError {
    provider_error(
        ErrorCategory::Authentication,
        Retryability::Never,
        false,
        "Z.ai API credentials are required",
        Some("missing-credentials"),
        None,
        None,
    )
}
