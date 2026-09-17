//! Provider-neutral Bridge error contract (VB-PRD-001 §6.6).

use serde::{Deserialize, Serialize};

/// Retry eligibility attached to every error (§6.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryEligibility {
    /// Never retry automatically (ambiguous mutation, denial, unknown).
    Never,
    /// Safe transport-level retry within the NF-10 bound (≤2).
    SafeTransportRetry { used: u8 },
}

/// The stable, provider-neutral error taxonomy. A generic driver exception
/// must be mapped to one of these; it may never erase whether a mutation
/// could already have happened (`unknown_outcome` preserves that).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeError {
    #[error("target_ambiguous: multiple candidates match; exact selection required")]
    TargetAmbiguous,
    #[error("target_changed: bound generation/window/document no longer matches")]
    TargetChanged,
    #[error(
        "capability_unavailable: the installed application/edition/driver does not expose this operation"
    )]
    CapabilityUnavailable,
    #[error("permission_required: authority grant needed before dispatch")]
    PermissionRequired,
    #[error("permission_denied: authority was denied; no route may bypass this")]
    PermissionDenied,
    #[error("stale_observation: a fresh observation is required before this action")]
    StaleObservation,
    #[error("lease_conflict: another writer/input owner holds the resource")]
    LeaseConflict,
    #[error("schema_mismatch: arguments do not validate against the capability schema")]
    SchemaMismatch,
    #[error("transport_unavailable: the driver/transport is not reachable")]
    TransportUnavailable,
    /// H3: the request is invalid for the session's current state (e.g.
    /// connect() on an already-attached session) — distinct from a
    /// transport failure so the caller learns the truth instead of
    /// retrying a healthy transport.
    #[error("invalid_state: the request does not match the session's current state")]
    InvalidState,
    #[error("resource_limit: a bounded budget was exceeded")]
    ResourceLimit,
    /// Adapter-scope failures carrying a bounded, already-escaped detail.
    #[error("dependency_missing: {detail}")]
    DependencyMissing { detail: String },
    #[error("invalid_arguments: {detail}")]
    InvalidArguments { detail: String },
    #[error("transport: {detail}")]
    Transport { detail: String },
    #[error("timeout: {detail}")]
    Timeout { detail: String },
    #[error("verification_failed: the postcondition did not hold")]
    VerificationFailed,
    #[error("unknown_outcome: dispatch may or may not have committed; reconcile before any retry")]
    UnknownOutcome,
    #[error("cleanup_unconfirmed: session effects could not be fully settled; quarantined")]
    CleanupUnconfirmed,
}

impl BridgeError {
    /// Retry guidance per §6.6 and NF-10.
    #[must_use]
    pub fn retry_eligibility(&self, used_retries: u8) -> RetryEligibility {
        match self {
            Self::TransportUnavailable if used_retries < 2 => {
                RetryEligibility::SafeTransportRetry { used: used_retries }
            }
            _ => RetryEligibility::Never,
        }
    }

    /// Denial outranks route fallback (BR-08): a denial can never be
    /// re-attempted through another route.
    #[must_use]
    pub fn blocks_fallback(&self) -> bool {
        matches!(
            self,
            Self::PermissionDenied | Self::TargetAmbiguous | Self::TargetChanged
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failures_allow_at_most_two_safe_retries() {
        assert_eq!(
            BridgeError::TransportUnavailable.retry_eligibility(0),
            RetryEligibility::SafeTransportRetry { used: 0 }
        );
        assert_eq!(
            BridgeError::TransportUnavailable.retry_eligibility(1),
            RetryEligibility::SafeTransportRetry { used: 1 }
        );
        assert_eq!(
            BridgeError::TransportUnavailable.retry_eligibility(2),
            RetryEligibility::Never
        );
    }

    #[test]
    fn ambiguous_mutations_never_retry_automatically() {
        assert_eq!(
            BridgeError::UnknownOutcome.retry_eligibility(0),
            RetryEligibility::Never
        );
        assert_eq!(
            BridgeError::VerificationFailed.retry_eligibility(0),
            RetryEligibility::Never
        );
    }

    #[test]
    fn denial_blocks_route_fallback() {
        assert!(BridgeError::PermissionDenied.blocks_fallback());
        assert!(BridgeError::TargetAmbiguous.blocks_fallback());
        assert!(BridgeError::TargetChanged.blocks_fallback());
        assert!(
            !BridgeError::CapabilityUnavailable.blocks_fallback(),
            "missing capability may fall back within policy"
        );
        assert!(!BridgeError::TransportUnavailable.blocks_fallback());
    }

    #[test]
    fn unknown_outcome_preserves_commit_possibility_in_its_message() {
        let message = BridgeError::UnknownOutcome.to_string();
        assert!(message.contains("may or may not have committed"));
    }
}
