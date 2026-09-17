#![forbid(unsafe_code)]
//! Pure, provider-neutral application-control contracts for Agent Vesper
//! (VB-PRD-001 Phase 1).
//!
//! This crate owns the Bridge **core**: generation-bound identities, the
//! versioned capability manifest, typed operation envelopes and result
//! states, session/operation state machines, fenced mutation leases, the
//! provider-neutral error contract, and the ports adapters/verifiers/
//! journals implement at the composition boundary. It performs **no I/O,
//! process spawning, capture, network, or clock dependence** and names no
//! application, driver, provider or transport.
//!
//! Invariants enforced here (BR-02/03/07/08/11/15/16/17/18/30):
//! - unknown is never supported: a capability with unknown implementation
//!   status cannot be dispatched;
//! - the host, never the model, mints identity/lease/approval references;
//! - denial precedence outranks route fallback: a denied operation cannot
//!   be re-dispatched through an alternate route;
//! - a driver acknowledgment never classifies as a verified postcondition;
//! - stale application generations, expired leases and superseded
//!   observations refuse dispatch before any adapter is consulted;
//! - duplicate request IDs are suppressed locally, and a duplicate of an
//!   unsettled non-idempotent operation returns `unknown_outcome` guidance
//!   rather than a second mutation.

pub mod adapter;
pub mod capability;
pub mod error;
pub mod identity;
pub mod journal;
pub mod lease;
pub mod observation;
pub mod operation;
pub mod session;

pub use adapter::{AdapterOutcome, AdapterPort};
pub use capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
pub use error::{BridgeError, RetryEligibility};
pub use identity::{
    ApplicationBinding, ApplicationInstanceId, BridgeSessionId, CaptureIdentity, Generation,
    ResourceRevision, TargetMatch,
};
pub use journal::{IntentRecord, JournalPort, Settlement};
pub use lease::{FenceState, InputLeaseState, LeaseDecision, LeaseId, LeaseState, ResourceKey};
pub use observation::{
    CoordinateTransform, ImageBounds, Observation, ObservationId, ObservationKind,
};
pub use operation::{
    AuthorityRequest, Idempotency, OperationOutcome, OperationRequestId, OperationSpec,
    Preconditions, TimeoutPolicy,
};
pub use session::{
    Admission, ApplicationSessionState, BridgeSession, DenialReason, OperationRecord, StopOutcome,
};
