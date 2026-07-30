use serde::{Deserialize, Serialize};

use crate::{BoundedString, GoalId};

/// Goal lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GoalStatus {
    /// Work remains.
    Active,
    /// Completion evidence passed.
    Completed,
    /// Work cannot proceed.
    Blocked,
}

/// Explicit user/session goal contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    /// Stable goal identity.
    pub id: GoalId,
    /// Bounded objective.
    pub objective: BoundedString<8192>,
    /// Current lifecycle status.
    pub status: GoalStatus,
    /// Optional token budget.
    pub token_budget: Option<u64>,
}
