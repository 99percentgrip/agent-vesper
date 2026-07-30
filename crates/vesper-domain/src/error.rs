use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{BoundedString, ExtensionMap, SafeMessage};

/// Bounded provider error code already classified as safe by an adapter.
pub type SafeProviderCode = BoundedString<256>;

/// Stable error taxonomy shared by adapters and frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCategory {
    /// Authentication failed or is unavailable.
    Authentication,
    /// Authenticated caller lacks authority.
    Authorization,
    /// An authority or permission was missing.
    Permission,
    /// Provider quota or rate limit.
    QuotaOrRate,
    /// Invalid request.
    InvalidRequest,
    /// Requested capability is unsupported.
    UnsupportedCapability,
    /// Context limit.
    ContextLimit,
    /// Output limit.
    OutputLimit,
    /// Safety refusal.
    Safety,
    /// Retryable HTTP status.
    TransientHttp,
    /// Retryable provider failure not tied to HTTP.
    TransientProvider,
    /// Network/process transport failure.
    Transport,
    /// Malformed protocol data.
    MalformedProtocol,
    /// Bounded operation timeout.
    Timeout,
    /// Explicit cancellation.
    Cancellation,
    /// Persistence failure.
    Persistence,
    /// Frozen-format compatibility failure.
    Compatibility,
    /// Fixture or schema contract failure.
    FixtureOrSchema,
    /// Security invariant failure.
    Security,
    /// Policy denial.
    Policy,
}

/// Retry classification independent of a concrete provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Retryability {
    /// Never retry the operation.
    Never,
    /// Retry only before visible output.
    BeforeVisibleOutput,
    /// Retry is possible using a deduplicating provider cursor.
    WithDeduplicatingCursor,
}

/// Diagnostics already scrubbed for logs and UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RedactedDiagnostics {
    /// Namespaced, redacted diagnostic fields.
    #[serde(default)]
    pub fields: ExtensionMap,
}

/// One sanitized source-chain entry without a raw external error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeErrorCause {
    /// Stable cause category.
    pub category: ErrorCategory,
    /// Bounded safe cause message.
    pub safe_message: SafeMessage,
}

/// Stable safe error record. Display intentionally exposes only `safe_message`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
#[error("{safe_message}")]
pub struct ErrorInfo {
    /// Error category.
    pub category: ErrorCategory,
    /// Retry policy.
    pub retryability: Retryability,
    /// Optional provider/server delay in milliseconds.
    pub retry_after_ms: Option<u64>,
    /// Whether any user-visible output was emitted first.
    pub visible_output_emitted: bool,
    /// Bounded user-safe message.
    pub safe_message: SafeMessage,
    /// Redacted diagnostics.
    #[serde(default)]
    pub diagnostics: RedactedDiagnostics,
    /// Optional namespaced provider error code already classified as safe.
    pub provider_code: Option<SafeProviderCode>,
    /// Sanitized cause chain; raw source objects are never serialized.
    #[serde(default)]
    pub causes: Vec<SafeErrorCause>,
}

impl ErrorInfo {
    /// Returns whether replay is forbidden because visible output already escaped.
    #[must_use]
    pub const fn forbids_plain_replay(&self) -> bool {
        self.visible_output_emitted
            && !matches!(self.retryability, Retryability::WithDeduplicatingCursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_never_includes_diagnostics() {
        let mut diagnostics = RedactedDiagnostics::default();
        diagnostics
            .fields
            .insert("test:detail", serde_json::json!("redacted-host"))
            .unwrap();
        let error = ErrorInfo {
            category: ErrorCategory::Transport,
            retryability: Retryability::Never,
            retry_after_ms: None,
            visible_output_emitted: false,
            safe_message: SafeMessage::new("connection failed").unwrap(),
            diagnostics,
            provider_code: None,
            causes: Vec::new(),
        };
        assert_eq!(error.to_string(), "connection failed");
        assert!(!error.to_string().contains("redacted-host"));
    }
}
