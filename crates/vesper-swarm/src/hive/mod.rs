//! Hive orchestration semantics (VRO-15 PR-5).
//!
//! This module owns the pure decision layer between the bus (PR-4), the
//! pool (PR-3), and the topology (PR-2): which worker should take a task
//! ([`assignment`]), what bus tier a task's urgency maps to
//! ([`assignment`]), and how a dispatched turn is bounded in time and
//! cancellable end to end ([`timeout`]). The orchestrator composes mutable
//! coordination state and async execution ports; no filesystem/network I/O
//! or provider implementation lives here.

pub mod assignment;
pub mod timeout;

pub mod decision;
pub mod governance;
pub mod panel;
pub mod verify;

pub mod orchestrator;

pub mod decomposition;
mod routing;

pub use decision::{
    DEFAULT_PIVOT_CAP, DEFAULT_REFINE_CAP, DecisionConfig, DecisionEngine, DecisionVerdict,
};
pub use governance::{
    AmendedTask, AuditEvent, AuditLog, DecisionVerdictPayload, FallbackAction, GateRecord,
    GateView, GovernanceConfig, GovernanceProfile, Governor, HostCommand, Resolution,
    VerificationCheck,
};
pub use verify::{
    BUDGET_LEVELS, BudgetCeiling, BudgetReading, BudgetState, BudgetWatchdog, EvidenceArtifact,
    EvidenceBook, VerificationFailure, artifact_digest, extract_citations, fnv64, verify_traces,
};
