use serde::{Deserialize, Serialize};

use crate::{BoundedString, PlanId};

/// Plan lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    /// Proposed but not approved.
    Proposed,
    /// Approved for execution.
    Approved,
    /// Active.
    InProgress,
    /// Completed.
    Completed,
    /// Rejected or abandoned.
    Rejected,
}

/// One ordered plan step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    /// Stable step label.
    pub id: BoundedString<128>,
    /// Bounded description.
    pub description: BoundedString<4096>,
    /// Completion state.
    pub completed: bool,
}

/// Provider-neutral plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Stable plan identity.
    pub id: PlanId,
    /// Plan status.
    pub status: PlanStatus,
    /// Ordered steps.
    pub steps: Vec<PlanStep>,
}
