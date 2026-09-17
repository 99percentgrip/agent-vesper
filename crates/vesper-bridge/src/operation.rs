//! Typed operation envelope and result classification (VB-PRD-001 §6.3/6.4, §8.1).
//!
//! `OperationSpec` is what the model may *propose*: a capability id and
//! JSON arguments. Everything authoritative — request id, session,
//! application binding, capability generation, authority generation, lease
//! token, preconditions — is host-minted in `AuthorityRequest` and
//! revalidated immediately before dispatch (§6, BR-08/15).

use serde::{Deserialize, Serialize};

use crate::capability::Mutability;
use crate::identity::{ApplicationInstanceId, BridgeSessionId, ResourceRevision};
use crate::observation::ObservationId;

/// Host-minted request identity (duplicate suppression key, BR-16/AT-18).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationRequestId(pub String);

impl OperationRequestId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 {
            return Err("operation request id must be 1–128 chars".into());
        }
        Ok(Self(value))
    }
}

/// What the model proposes. No shell, code, eval, URL or opaque command
/// strings exist here — only a namespaced capability reference and JSON
/// arguments validated against that capability's schema version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationSpec {
    pub capability: crate::capability::CapabilityId,
    pub arguments: serde_json::Value,
}

/// Idempotency class of the operation (§8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    ReadOnly,
    NaturallyIdempotent,
    ConditionallyIdempotent,
    NonIdempotent,
}

/// Preconditions binding a dispatch to the world it was planned against.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preconditions {
    /// Observation the plan was derived from; superseded ⇒ refuse.
    pub observation: ObservationId,
    /// Application-side resource revision at planning time.
    pub resource_revision: ResourceRevision,
    /// Application instance generation at planning time.
    pub application_generation: crate::identity::Generation,
}

/// Deadline policy. Ordinary actions default to 15 s (NF-09); renders and
/// other long work are jobs with their own deadlines, not global raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    pub timeout_ms: u64,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self { timeout_ms: 15_000 }
    }
}

/// The authoritative, host-minted execution envelope (§6.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthorityRequest {
    pub request_id: OperationRequestId,
    pub bridge_session: BridgeSessionId,
    pub application: ApplicationInstanceId,
    /// Capability manifest generation this dispatch was validated against.
    pub capability_generation: u32,
    /// Authority (approval) generation this dispatch is authorized by.
    pub authority_generation: u32,
    /// Opaque host-issued lease reference.
    pub lease_token: String,
    pub spec: OperationSpec,
    pub preconditions: Preconditions,
    pub timeout: TimeoutPolicy,
    /// Mutability as resolved from the capability record at validation time.
    pub resolved_mutability: Mutability,
    /// Idempotency class resolved at validation time.
    pub idempotency: Idempotency,
}

/// Result state machine for one operation (§8.1). A driver ack is
/// `Dispatched`; only independent verification (or an explicitly
/// acknowledgment-only task) reaches `Verified`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    /// Sent to the adapter; effect not yet established.
    Dispatched,
    /// Adapter reports the effect applied; postcondition not yet verified.
    Applied,
    /// Postcondition independently verified (BR-12, NF-13).
    Verified,
    /// Some requested effects hold, others do not; evidence retained.
    Partial,
    /// Terminal failure with evidence.
    Failed(String),
    /// Cancelled by the host/stop path.
    Cancelled,
    /// Dispatch may or may not have committed (timeout/disconnect after send).
    UnknownOutcome,
}

impl OperationOutcome {
    /// Truthful success for completion reporting (NF-13).
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Verified)
    }

    /// Terminal states settle the operation record.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(
            self,
            Self::Verified | Self::Partial | Self::Failed(_) | Self::Cancelled
        )
    }

    /// States that may still mutate after the fact (reconcile required).
    #[must_use]
    pub fn may_have_committed(&self) -> bool {
        matches!(
            self,
            Self::Dispatched | Self::Applied | Self::UnknownOutcome
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityId;

    #[test]
    fn only_verified_counts_as_success() {
        assert!(!OperationOutcome::Dispatched.is_success());
        assert!(!OperationOutcome::Applied.is_success());
        assert!(OperationOutcome::Verified.is_success());
        assert!(!OperationOutcome::UnknownOutcome.is_success());
        assert!(!OperationOutcome::Partial.is_success());
        assert!(!OperationOutcome::Failed("x".into()).is_success());
    }

    #[test]
    fn driver_ack_is_not_settled_and_may_have_committed() {
        assert!(!OperationOutcome::Applied.is_settled());
        assert!(OperationOutcome::Applied.may_have_committed());
        assert!(OperationOutcome::UnknownOutcome.may_have_committed());
        assert!(!OperationOutcome::Verified.may_have_committed());
    }

    #[test]
    fn default_timeout_is_15s_ordinary_actions() {
        assert_eq!(TimeoutPolicy::default().timeout_ms, 15_000);
    }

    #[test]
    fn operation_spec_carries_no_execution_strings() {
        let spec = OperationSpec {
            capability: CapabilityId::new("media.clip.trim").unwrap(),
            arguments: serde_json::json!({"clip": "c1", "duration_frames": 24}),
        };
        let serialized = serde_json::to_string(&spec).unwrap();
        for forbidden in ["\"shell\"", "\"code\"", "\"eval\"", "\"command\""] {
            assert!(
                !serialized.contains(forbidden),
                "spec must not carry {forbidden}"
            );
        }
    }
}
