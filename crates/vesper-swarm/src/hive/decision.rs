//! Navigator decision node: bounded PIVOT/REFINE loops (VRO-16 PR-2).
//!
//! A decision consumes the review panel's ranked verdict over a frozen
//! ledger snapshot and emits one audited [`DecisionVerdict`]:
//! `Proceed`, `Refine` (re-run amended tasks), `Pivot` (new decomposition)
//! or the explicit `ProceedWithFailure` cap-exhaustion verdict.
//!
//! **No silent loops (PR-2 constraint 1):** refine and pivot iterations are
//! capped per goal (`DecisionConfig`, default 3 refines / 2 pivots). A
//! verdict that would exceed a cap is rewritten to the audited
//! cap-exhaustion verdict at issuance — the loop cannot continue past its
//! ceiling, and exhaustion is always an explicit audit event, never a
//! silent acceptance.
//!
//! **Fail-closed rationale (constraint 3):** every `DecisionIssued` event
//! carries `rationale_refs` that must resolve to real ledger entry ids in
//! the frozen snapshot; dangling references refuse issuance.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::governance::{AuditEvent, DecisionVerdictPayload};

/// Default maximum refine iterations per goal.
pub const DEFAULT_REFINE_CAP: u32 = 3;
/// Default maximum pivot iterations per goal.
pub const DEFAULT_PIVOT_CAP: u32 = 2;
/// Hard ceiling on amended tasks per verdict (matches decomposition bound).
pub const MAX_AMENDED_TASKS: usize = 64;
/// Hard ceiling on rationale references per decision.
pub const MAX_RATIONALE_REFS: usize = 64;
/// Bounded prompt/capability text limits (match decomposition bounds).
const MAX_PROMPT_BYTES: usize = 16_384;
const MAX_CAPABILITY_BYTES: usize = 256;

/// Bounded decision configuration (VRO-16 PR-2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionConfig {
    /// Maximum refine iterations per goal.
    pub refine_cap: u32,
    /// Maximum pivot iterations per goal.
    pub pivot_cap: u32,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        Self {
            refine_cap: DEFAULT_REFINE_CAP,
            pivot_cap: DEFAULT_PIVOT_CAP,
        }
    }
}

impl DecisionConfig {
    /// Reject degenerate caps. Zero caps are legal (decisions may only
    /// proceed), but nothing above the hard defaults is accepted so
    /// configuration cannot manufacture an unbounded loop.
    #[must_use]
    pub fn validated(&self) -> bool {
        self.refine_cap <= DEFAULT_REFINE_CAP && self.pivot_cap <= DEFAULT_PIVOT_CAP
    }
}

/// One parsed decision verdict, with the parsed payload resolved against
/// iteration state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionVerdict {
    /// Accept the synthesis; complete the goal.
    Proceed,
    /// Re-run the amended tasks; carries the 1-based iteration number.
    Refine {
        /// Amended task set (validated, bounded).
        amended: Vec<super::governance::AmendedTask>,
        /// 1-based refine iteration this verdict starts.
        iteration: u32,
    },
    /// Restart with a new decomposition.
    Pivot {
        /// New task set (validated, bounded).
        tasks: Vec<super::governance::AmendedTask>,
    },
    /// A cap was exhausted: complete with failure marker, never silently.
    ProceedWithFailure {
        /// Machine-readable exhaustion reason.
        reason: String,
    },
}

/// The decision engine's per-goal iteration ledger. One per goal run.
#[derive(Debug, Default)]
pub struct DecisionEngine {
    config: DecisionConfig,
    refine_iterations: BTreeMap<String, u32>,
    pivot_iterations: BTreeMap<String, u32>,
}

impl DecisionEngine {
    /// An engine with the given validated configuration.
    ///
    /// # Errors
    ///
    /// Refuses caps above the hard defaults (no manufactured unbounded loops).
    pub fn new(config: DecisionConfig) -> Result<Self, String> {
        if !config.validated() {
            return Err(format!(
                "decision caps must not exceed {DEFAULT_REFINE_CAP} refines / {DEFAULT_PIVOT_CAP} pivots"
            ));
        }
        Ok(Self {
            config,
            ..Self::default()
        })
    }

    /// The active configuration.
    #[must_use]
    pub fn config(&self) -> &DecisionConfig {
        &self.config
    }

    /// Iterations consumed so far for a goal.
    #[must_use]
    pub fn iterations(&self, goal_id: &str) -> (u32, u32) {
        (
            self.refine_iterations.get(goal_id).copied().unwrap_or(0),
            self.pivot_iterations.get(goal_id).copied().unwrap_or(0),
        )
    }

    /// Issue a decision from the Navigator's parsed output. The output is
    /// strict JSON mirroring [`DecisionVerdictPayload`]; malformed output,
    /// out-of-bounds tasks, over-cap iterations or dangling rationale
    /// references fail closed.
    ///
    /// `rationale_resolver` must answer whether a ledger entry id exists in
    /// the frozen snapshot the decision is being made against.
    pub fn issue(
        &mut self,
        goal_id: &str,
        navigator_output: &str,
        rationale_refs: &[u64],
        rationale_resolver: &dyn Fn(u64) -> bool,
        now_ms: u64,
    ) -> Result<(DecisionVerdict, AuditEvent), String> {
        if navigator_output.len() > 1_048_576 {
            return Err("decision output byte limit".into());
        }
        // Fail closed on dangling rationale references (constraint 3).
        let refs: Vec<u64> = rationale_refs.to_vec();
        if refs.len() > MAX_RATIONALE_REFS {
            return Err("rationale reference count exceeds bound".into());
        }
        if let Some(dangling) = refs.iter().find(|id| !rationale_resolver(**id)) {
            return Err(format!(
                "dangling rationale reference {dangling}; refusing to issue decision"
            ));
        }
        let payload: DecisionVerdictPayload = serde_json::from_str(navigator_output)
            .map_err(|_| "invalid decision JSON".to_string())?;
        let (refine_used, pivot_used) = self.iterations(goal_id);
        let verdict = match payload {
            DecisionVerdictPayload::Proceed => DecisionVerdict::Proceed,
            DecisionVerdictPayload::ProceedWithFailure { reason } => {
                DecisionVerdict::ProceedWithFailure { reason }
            }
            DecisionVerdictPayload::Refine { amended, .. } => {
                validate_amended(&amended)?;
                if refine_used >= self.config.refine_cap {
                    DecisionVerdict::ProceedWithFailure {
                        reason: format!("refine cap exhausted ({})", self.config.refine_cap),
                    }
                } else {
                    let iteration = refine_used + 1;
                    self.refine_iterations.insert(goal_id.to_owned(), iteration);
                    DecisionVerdict::Refine { amended, iteration }
                }
            }
            DecisionVerdictPayload::Pivot { tasks } => {
                validate_amended(&tasks)?;
                if pivot_used >= self.config.pivot_cap {
                    DecisionVerdict::ProceedWithFailure {
                        reason: format!("pivot cap exhausted ({})", self.config.pivot_cap),
                    }
                } else {
                    self.pivot_iterations
                        .insert(goal_id.to_owned(), pivot_used + 1);
                    DecisionVerdict::Pivot { tasks }
                }
            }
        };
        let (refine_now, pivot_now) = self.iterations(goal_id);
        let audit_payload = match &verdict {
            DecisionVerdict::Proceed => DecisionVerdictPayload::Proceed,
            DecisionVerdict::ProceedWithFailure { reason } => {
                DecisionVerdictPayload::ProceedWithFailure {
                    reason: reason.clone(),
                }
            }
            DecisionVerdict::Refine { amended, iteration } => DecisionVerdictPayload::Refine {
                amended: amended.clone(),
                iteration: *iteration,
            },
            DecisionVerdict::Pivot { tasks } => DecisionVerdictPayload::Pivot {
                tasks: tasks.clone(),
            },
        };
        let event = AuditEvent::DecisionIssued {
            goal_id: goal_id.to_owned(),
            verdict: audit_payload,
            rationale_refs: refs,
            refine_iterations: refine_now,
            pivot_iterations: pivot_now,
            at_ms: now_ms,
        };
        Ok((verdict, event))
    }

    /// The strict decision instruction appended to the synthesis/decision
    /// prompt so the Navigator knows the exact output contract and the
    /// remaining iteration budget.
    #[must_use]
    pub fn instructions(&self, goal_id: &str) -> String {
        let (refine_used, pivot_used) = self.iterations(goal_id);
        format!(
            "After synthesis, return only JSON deciding the next step: \
             {{\"kind\":\"proceed\"}} to accept, \
             {{\"kind\":\"refine\",\"amended\":[{{\"index\":0,\"prompt\":\"...\",\"required_capabilities\":[],\"depends_on\":[]}}],\"iteration\":1}} to re-run amended tasks, or \
             {{\"kind\":\"pivot\",\"tasks\":[...]}} for a new decomposition. \
             Remaining budget: {} refine(s), {} pivot(s).",
            self.config.refine_cap.saturating_sub(refine_used),
            self.config.pivot_cap.saturating_sub(pivot_used),
        )
    }
}

/// Validate an amended/new task set: bounded count, nonempty bounded
/// prompts, bounded capabilities, in-range non-self dependencies, no
/// duplicates.
fn validate_amended(tasks: &[super::governance::AmendedTask]) -> Result<(), String> {
    if tasks.is_empty() || tasks.len() > MAX_AMENDED_TASKS {
        return Err("amended task count outside 1..=64".into());
    }
    let max_index = tasks
        .iter()
        .filter_map(|task| task.index)
        .max()
        .unwrap_or(tasks.len() - 1);
    for task in tasks {
        if task.prompt.trim().is_empty() || task.prompt.len() > MAX_PROMPT_BYTES {
            return Err("amended task prompt outside bounds".into());
        }
        if task.required_capabilities.len() > 32
            || task
                .required_capabilities
                .iter()
                .any(|name| name.is_empty() || name.len() > MAX_CAPABILITY_BYTES)
        {
            return Err("amended task capability bounds".into());
        }
        if task.depends_on.len() > 63
            || task
                .depends_on
                .iter()
                .any(|id| *id == task.index.unwrap_or(usize::MAX) || *id > max_index)
        {
            return Err("amended task dependency bounds".into());
        }
    }
    Ok(())
}

/// Ledger snapshot versioning (VRO-16 PR-2 directive 1): every Refine or
/// Pivot preserves the prior generation so any iteration's evidence stays
/// retrievable. Versions are numbered per goal from 0.
#[derive(Debug, Default)]
pub struct VersionRegistry {
    versions: BTreeMap<String, Vec<crate::ledger::store::LedgerSnapshot>>,
}

impl VersionRegistry {
    /// Record the pre-decision snapshot as a new version for the goal.
    pub fn push_version(
        &mut self,
        goal_id: &str,
        snapshot: crate::ledger::store::LedgerSnapshot,
    ) -> usize {
        let chain = self.versions.entry(goal_id.to_owned()).or_default();
        chain.push(snapshot);
        chain.len() - 1
    }

    /// The number of versions recorded for a goal.
    #[must_use]
    pub fn version_count(&self, goal_id: &str) -> usize {
        self.versions.get(goal_id).map_or(0, Vec::len)
    }

    /// Retrieve a recorded version for evidence inspection. Version 0 is
    /// the pre-first-decision state; the latest is pre-final-decision.
    ///
    /// # Errors
    ///
    /// Unknown goal or out-of-range version.
    pub fn version(
        &self,
        goal_id: &str,
        index: usize,
    ) -> Result<&crate::ledger::store::LedgerSnapshot, String> {
        self.versions
            .get(goal_id)
            .and_then(|chain| chain.get(index))
            .ok_or_else(|| format!("no version {index} for goal {goal_id}"))
    }

    /// Whether an entry id is retrievable in **any** recorded version of
    /// the goal — the rationale resolver for fail-closed decisions.
    #[must_use]
    pub fn resolves_any(&self, goal_id: &str, entry_id: u64) -> bool {
        self.versions
            .get(goal_id)
            .is_some_and(|chain| chain.iter().any(|snapshot| snapshot.has_entry(entry_id)))
    }
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod tests;
