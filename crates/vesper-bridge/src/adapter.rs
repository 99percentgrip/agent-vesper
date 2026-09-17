//! Adapter port: the composition boundary between the pure Bridge core and
//! real application transports (VB-PRD-001 §5).
//!
//! The core stays pure (no I/O, no process spawning, no clock); concrete
//! adapters live in the hosted layer (`vesper-harness`). This trait is the
//! seam: an adapter receives a *typed, already-authorized* operation and
//! returns a typed outcome. It never receives authority decisions, lease
//! minting, or model text to evaluate.
//!
//! Invariants this seam preserves:
//! - an adapter may never report `Verified` without supplying evidence —
//!   a driver acknowledgment classifies as `Applied` at best;
//! - adapter failures are typed, never panics;
//! - every adapter is constructed with its fixed capability manifest, so
//!   `bridge_capabilities` reflects what the transport can actually do.

use crate::capability::CapabilityManifest;
use crate::operation::{OperationRequestId, OperationSpec};
use crate::session::OperationRecord;

/// Result of a dispatched adapter operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterOutcome {
    /// Terminal outcome classification (dispatched/applied/verified/...).
    pub outcome: crate::operation::OperationOutcome,
    /// Bounded human-readable summary (already escaped by the adapter where
    /// it embeds application-originated strings).
    pub summary: String,
    /// Independent evidence backing a `Verified` claim (file path + digest,
    /// measured property, etc.). Empty means "no verification evidence" and
    /// MUST accompany a non-verified outcome.
    pub evidence: String,
    /// True when an asynchronous job was created and remains outstanding.
    pub job_created: bool,
}

impl AdapterOutcome {
    /// An honest "the application accepted the operation" result — applied,
    /// not verified, no evidence, no async job.
    #[must_use]
    pub fn applied(summary: impl Into<String>) -> Self {
        Self {
            outcome: crate::operation::OperationOutcome::Applied,
            summary: summary.into(),
            evidence: String::new(),
            job_created: false,
        }
    }

    /// A failure with a bounded reason.
    #[must_use]
    pub fn failed(summary: impl Into<String>) -> Self {
        let summary = summary.into();
        Self {
            outcome: crate::operation::OperationOutcome::Failed(summary.clone()),
            summary,
            evidence: String::new(),
            job_created: false,
        }
    }
}

/// The transport seam. Implemented by hosted-layer adapters.
pub trait AdapterPort: Send + Sync {
    /// The fixed capability manifest describing what this transport can do.
    fn manifest(&self) -> &CapabilityManifest;

    /// Dispatch one typed, already-authorized operation.
    ///
    /// `request_id` is host-minted; adapters must record it if the target
    /// application supports idempotency keys, and must never re-derive it.
    fn dispatch(
        &self,
        request_id: &OperationRequestId,
        spec: &OperationSpec,
    ) -> Result<AdapterOutcome, crate::error::BridgeError>;

    /// Poll an outstanding asynchronous job (render queue, export, ...).
    /// Returns `None` when the job id is unknown to this adapter.
    fn job_status(&self, job_id: &str) -> Option<crate::session::StopOutcome>;

    /// Release driver-owned inputs (emergency path). Must be callable after
    /// ordinary leases were revoked (NF-03/§8.4). Returns a bounded report.
    fn release_inputs(&self) -> String;

    /// Human-readable transport identity for reports (never used for
    /// authority decisions).
    fn describe(&self) -> String;

    /// Optional probe of live readiness (app running, channel alive).
    /// `None` = not applicable.
    fn health(&self) -> Option<bool> {
        None
    }
}

/// A no-op record helper adapters can use to describe what they did.
#[allow(dead_code)]
pub fn record_note(record: &OperationRecord) -> String {
    format!("{}:{:?}", record.request_id.0, record.outcome)
}
