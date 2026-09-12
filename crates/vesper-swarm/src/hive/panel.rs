//! Driver-role review panel with frozen-snapshot rounds (VRO-16 PR-2).
//!
//! The panel evaluates an artifact against **frozen snapshots of peers'
//! prior-round entries**: within one round, a reviewer sees only what
//! peers wrote in *earlier* rounds — same-round isolation is structural
//! (the input is a snapshot taken before the round starts), not
//! convention. A reviewer whose turn fails twice is dropped; an empty
//! rebuttal keeps the prior position; a panel reduced to zero members is
//! a **verification failure**, never a silent acceptance.
//!
//! D3 judge separation: the panel is composed of Driver-role reviewers
//! with judge-separated role templates; the orchestrator refuses a panel
//! whose reviewer template identity equals any author template identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::governance::AuditEvent;

/// Maximum reviewers on one panel.
pub const MAX_PANEL_SIZE: usize = 8;
/// Maximum rebuttal rounds (round 0 opening + N rebuttals).
pub const MAX_ROUNDS: u32 = 2;
/// Bounded review text.
pub const MAX_REVIEW_BYTES: usize = 65_536;

/// One reviewer's materialized position for a round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelPosition {
    /// Reviewer identity (worker node id).
    pub reviewer: String,
    /// The reviewer's bounded written position.
    pub text: String,
    /// Which round produced this position (0 = opening).
    pub round: u32,
}

/// Panel outcome handed to the decision node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelOutcome {
    /// At least one reviewer survived; carries their latest positions.
    Survived {
        /// Surviving reviewers' final positions, reviewer-id order.
        positions: Vec<PanelPosition>,
        /// Reviewers dropped after double failures.
        dropped: Vec<String>,
        /// Rounds actually executed.
        rounds: u32,
    },
    /// Every reviewer failed twice: verification failure, not acceptance.
    ZeroPanel {
        /// The reviewers that were dropped.
        dropped: Vec<String>,
    },
}

/// Round execution input: the frozen prior-round snapshot each reviewer
/// sees. Round 0 sees only the artifact under review.
#[derive(Debug, Clone)]
pub struct RoundInput {
    /// The artifact under review.
    pub artifact: String,
    /// Frozen prior-round positions of *other* reviewers only (the
    /// reviewer's own prior position is passed separately).
    pub peer_positions: Vec<PanelPosition>,
    /// The reviewer's own prior position, if any.
    pub own_prior: Option<PanelPosition>,
}

impl RoundInput {
    /// Build the frozen input for one reviewer in one round.
    ///
    /// # Errors
    ///
    /// Oversized artifact.
    pub fn frozen(
        artifact: &str,
        prior_round: &[PanelPosition],
        reviewer: &str,
    ) -> Result<Self, String> {
        if artifact.len() > 1_048_576 {
            return Err("artifact under review exceeds byte bound".into());
        }
        Ok(Self {
            artifact: artifact.to_owned(),
            peer_positions: prior_round
                .iter()
                .filter(|position| position.reviewer != reviewer)
                .cloned()
                .collect(),
            own_prior: prior_round
                .iter()
                .find(|position| position.reviewer == reviewer)
                .cloned(),
        })
    }
}

/// One reviewer's attempted turn result for a round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerTurn {
    /// A bounded written position.
    Wrote(String),
    /// The turn failed this attempt.
    Failed,
}

/// Drive the bounded review panel over reviewer turn results.
///
/// `turn` is invoked once per surviving reviewer per round with the frozen
/// input for that reviewer; it returns the reviewer's attempted result.
/// Double failure drops a reviewer; an empty rebuttal (round ≥ 1) keeps
/// the prior position; round-0 empty output counts as a failure attempt.
pub fn run_panel<F>(artifact: &str, reviewers: &[String], rounds: u32, mut turn: F) -> PanelOutcome
where
    F: FnMut(&str, &RoundInput) -> ReviewerTurn,
{
    if reviewers.is_empty() || reviewers.len() > MAX_PANEL_SIZE {
        return PanelOutcome::ZeroPanel {
            dropped: reviewers.to_vec(),
        };
    }
    let rounds = rounds.min(MAX_ROUNDS);
    let mut positions: Vec<PanelPosition> = Vec::new();
    let mut failures: BTreeMap<String, u32> = BTreeMap::new();
    let mut dropped: Vec<String> = Vec::new();
    let mut current: Vec<String> = reviewers.to_vec();
    for round in 0..=rounds {
        if current.len() < 2 && round > 0 {
            // Fewer than two survivors: nothing to rebut.
            break;
        }
        // The frozen snapshot of positions BEFORE this round: same-round
        // peers are structurally invisible.
        let frozen: Vec<PanelPosition> = positions.clone();
        let mut next_round_positions: Vec<PanelPosition> = Vec::new();
        for reviewer in &current {
            let input = match RoundInput::frozen(artifact, &frozen, reviewer) {
                Ok(input) => input,
                Err(_) => {
                    *failures.entry(reviewer.clone()).or_default() += 1;
                    continue;
                }
            };
            match turn(reviewer, &input) {
                ReviewerTurn::Wrote(text) => {
                    if text.trim().is_empty() || text.len() > MAX_REVIEW_BYTES {
                        *failures.entry(reviewer.clone()).or_default() += 1;
                        continue;
                    }
                    *failures.entry(reviewer.clone()).or_default() = 0;
                    next_round_positions.push(PanelPosition {
                        reviewer: reviewer.clone(),
                        text,
                        round,
                    });
                }
                ReviewerTurn::Failed => {
                    *failures.entry(reviewer.clone()).or_default() += 1;
                }
            }
        }
        // Apply drops: double failure removes the reviewer. Reviewers that
        // produced no position this round but had one before KEEP their
        // prior position (empty rebuttal retention) unless double-failed.
        let mut survivors: Vec<String> = Vec::new();
        for reviewer in &current {
            let count = failures.get(reviewer).copied().unwrap_or(0);
            if count >= 2 {
                dropped.push(reviewer.clone());
                positions.retain(|position| position.reviewer != *reviewer);
            } else {
                survivors.push(reviewer.clone());
                if let Some(position) = next_round_positions
                    .iter()
                    .find(|position| &position.reviewer == reviewer)
                {
                    // Replace the reviewer's prior position with the new one.
                    positions.retain(|existing| existing.reviewer != *reviewer);
                    positions.push(position.clone());
                }
                // Else: keep prior position (empty rebuttal retention).
            }
        }
        current = survivors;
        if current.is_empty() {
            return PanelOutcome::ZeroPanel { dropped };
        }
    }
    if current.is_empty() || positions.is_empty() {
        return PanelOutcome::ZeroPanel { dropped };
    }
    PanelOutcome::Survived {
        positions,
        dropped,
        rounds,
    }
}

/// The audit event for a completed panel round set (recorded by the
/// orchestrator; carried in the DecisionIssued rationale chain).
#[must_use]
pub fn panel_audit(goal_id: &str, outcome: &PanelOutcome, at_ms: u64) -> AuditEvent {
    // Panel outcomes are summarized into the decision's rationale; the
    // audit event here records the panel shape for the trail.
    let (survived, dropped) = match outcome {
        PanelOutcome::Survived {
            positions, dropped, ..
        } => (positions.len(), dropped.len()),
        PanelOutcome::ZeroPanel { dropped } => (0, dropped.len()),
    };
    AuditEvent::DecisionIssued {
        goal_id: goal_id.to_owned(),
        verdict: super::governance::DecisionVerdictPayload::ProceedWithFailure {
            reason: format!("panel-summary survived={survived} dropped={dropped}"),
        },
        rationale_refs: Vec::new(),
        refine_iterations: 0,
        pivot_iterations: 0,
        at_ms,
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
