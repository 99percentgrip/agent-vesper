use serde::{Deserialize, Serialize};
use thiserror::Error;

use vesper_domain::{
    ErrorInfo, ExtensionMap, ModelRequirement, ProviderId, Retryability, SafeProviderCode,
};

const UNSUPPORTED_REQUIREMENT_KEY: &str = "vesper:unsupported-requirement";

/// Provider-classified error with only safe diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
#[error("{info}")]
pub struct ProviderError {
    /// Provider identity.
    pub provider_id: ProviderId,
    /// Provider code after redaction.
    pub provider_code: Option<SafeProviderCode>,
    /// HTTP status when transport is HTTP and disclosure is safe.
    pub http_status: Option<u16>,
    /// Whether adapter state may still permit continuation.
    pub continuation_possible: bool,
    /// Shared stable classification.
    pub info: ErrorInfo,
    /// Namespaced safe metadata.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

/// Retry/replay decision derived from error and stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// No retry.
    DoNotRetry,
    /// Retry a request that emitted no visible output.
    RetryBeforeVisibleOutput,
    /// Resume using an explicit deduplicating protocol cursor.
    ResumeWithCursor,
}

impl ProviderError {
    /// Attaches the provider-classified content requirement that was rejected.
    #[must_use]
    pub fn with_unsupported_requirement(mut self, requirement: &ModelRequirement) -> Self {
        if let Ok(value) = serde_json::to_value(requirement) {
            let _ = self.metadata.insert(UNSUPPORTED_REQUIREMENT_KEY, value);
        }
        self
    }

    /// Returns an adapter-classified unsupported-content requirement, if any.
    #[must_use]
    pub fn unsupported_requirement(&self) -> Option<ModelRequirement> {
        self.metadata
            .get(UNSUPPORTED_REQUIREMENT_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    /// Applies the partial-output no-replay invariant.
    #[must_use]
    pub const fn retry_decision(&self) -> RetryDecision {
        match (self.info.visible_output_emitted, self.info.retryability) {
            (_, Retryability::Never) | (true, Retryability::BeforeVisibleOutput) => {
                RetryDecision::DoNotRetry
            }
            (false, Retryability::BeforeVisibleOutput) => RetryDecision::RetryBeforeVisibleOutput,
            (_, Retryability::WithDeduplicatingCursor) => RetryDecision::ResumeWithCursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vesper_domain::{ErrorCategory, RedactedDiagnostics, SafeMessage};

    use super::*;

    fn error(visible: bool, retryability: Retryability) -> ProviderError {
        let mut metadata = ExtensionMap::default();
        metadata
            .insert("future:opaque", json!({"preserve": true}))
            .unwrap();
        ProviderError {
            provider_id: ProviderId::new("fake").unwrap(),
            provider_code: Some(SafeProviderCode::new("cancelled").unwrap()),
            http_status: None,
            continuation_possible: false,
            info: ErrorInfo {
                category: ErrorCategory::Cancellation,
                retryability,
                retry_after_ms: None,
                visible_output_emitted: visible,
                safe_message: SafeMessage::new("request cancelled").unwrap(),
                diagnostics: RedactedDiagnostics::default(),
                provider_code: Some(SafeProviderCode::new("cancelled").unwrap()),
                causes: Vec::new(),
            },
            metadata,
        }
    }

    #[test]
    fn cancellation_stays_classified_and_metadata_round_trips() {
        let original = error(false, Retryability::Never);
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: ProviderError = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.info.category, ErrorCategory::Cancellation);
        assert_eq!(
            decoded.metadata.get("future:opaque"),
            Some(&json!({"preserve": true}))
        );
    }

    #[test]
    fn visible_partial_output_cannot_be_plainly_replayed() {
        assert_eq!(
            error(true, Retryability::BeforeVisibleOutput).retry_decision(),
            RetryDecision::DoNotRetry
        );
        assert_eq!(
            error(false, Retryability::BeforeVisibleOutput).retry_decision(),
            RetryDecision::RetryBeforeVisibleOutput
        );
        assert_eq!(
            error(true, Retryability::WithDeduplicatingCursor).retry_decision(),
            RetryDecision::ResumeWithCursor
        );
    }

    #[test]
    fn unsupported_requirement_round_trips_as_typed_safe_metadata() {
        let requirement = ModelRequirement::VisionImage {
            media_type: vesper_domain::BoundedString::new("image/png").unwrap(),
        };
        let original = error(false, Retryability::Never).with_unsupported_requirement(&requirement);
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: ProviderError = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.unsupported_requirement(), Some(requirement));
    }
}
