//! Deterministic verification gates (VRO-16 PR-3).
//!
//! Two machine-checked gates protect the synthesis input:
//!
//! 1. **Digest gate** — every admitted evidence artifact is content-hashed
//!    (FNV-1a 64, matching the crate's inline-hash discipline) at
//!    admission. Before a receipt's output may enter the synthesis input,
//!    the digest is recomputed: a tampered artifact is inadmissible, the
//!    mismatch is audited as a `VerificationVerdict` failure, and the
//!    evidence never reaches synthesis.
//! 2. **Trace gate** — an accepted synthesis must cite the ledger entry ids
//!    that back its factual assertions. Every cited id must resolve to a
//!    recorded artifact; a dangling citation fails closed (the goal errors
//!    rather than accepting hallucinated provenance).
//!
//! The **budget watchdog** tracks cumulative token/time consumption against
//! a ceiling: it emits `BudgetThreshold` audit events at 50% and 80%
//! (without stopping) and at 100% requires an immediate hard stop that
//!    completes only already-dispatched bounded turns, preserves truthful
//!    partial state, and surfaces termination. Consumption beyond the
//!    ceiling is structurally refused.
//!
//! Purity: no I/O, no clock — thresholds evaluate against caller-supplied
//! consumption readings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::governance::AuditEvent;

/// Digest byte: separator inside the evidence key to prevent
/// task-id/output concatenation ambiguity.
const DIGEST_SEPARATOR: u8 = 0x1f;

/// Deterministic FNV-1a 64 content digest.
#[must_use]
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    state
}

/// The digest over one admitted evidence artifact: task id and content.
#[must_use]
pub fn artifact_digest(task_id: &str, output: &str) -> u64 {
    let mut bytes =
        Vec::with_capacity(task_id.len() + output.len() + DIGEST_SEPARATOR as usize + 8);
    bytes.extend_from_slice(task_id.as_bytes());
    bytes.push(DIGEST_SEPARATOR);
    bytes.extend_from_slice(output.as_bytes());
    bytes.extend_from_slice(&task_id.len().to_le_bytes());
    fnv64(&bytes)
}

/// One admitted evidence artifact with its recorded digest and ledger
/// entry id (assigned when the trajectory is written).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceArtifact {
    /// The task that produced this artifact.
    pub task_id: String,
    /// The recorded content digest at admission.
    pub digest: u64,
    /// Ledger entry id backing this artifact (trace target).
    pub ledger_entry_id: u64,
}

/// The digest book: admitted artifacts keyed by task id. Recomputation
/// against the live receipt output detects tampering between admission
/// and synthesis.
#[derive(Debug, Default)]
pub struct EvidenceBook {
    artifacts: BTreeMap<String, EvidenceArtifact>,
}

impl EvidenceBook {
    /// Record one admitted artifact. Overwriting an existing task id is a
    /// verification failure (double admission), refused loudly.
    ///
    /// # Errors
    ///
    /// Duplicate task admission.
    pub fn admit(
        &mut self,
        task_id: &str,
        output: &str,
        ledger_entry_id: u64,
    ) -> Result<u64, String> {
        if self.artifacts.contains_key(task_id) {
            return Err(format!("task {task_id} already admitted to evidence"));
        }
        let digest = artifact_digest(task_id, output);
        self.artifacts.insert(
            task_id.to_owned(),
            EvidenceArtifact {
                task_id: task_id.to_owned(),
                digest,
                ledger_entry_id,
            },
        );
        Ok(digest)
    }

    /// Admitted artifacts in task-id order (digest re-verification input).
    pub fn artifacts_list(&self) -> Vec<EvidenceArtifact> {
        self.artifacts.values().cloned().collect()
    }

    /// Bind a ledger entry id to an admitted artifact (after the
    /// trajectory write publishes the trace target).
    pub fn bind_ledger_entry(&mut self, task_id: &str, ledger_entry_id: u64) {
        if let Some(artifact) = self.artifacts.get_mut(task_id) {
            artifact.ledger_entry_id = ledger_entry_id;
        }
    }

    /// Whether a verdict event records a failure.
    #[must_use]
    pub fn verdict_failed(event: &crate::hive::governance::AuditEvent) -> bool {
        matches!(
            event,
            crate::hive::governance::AuditEvent::VerificationVerdict {
                failure: Some(_),
                ..
            }
        )
    }

    /// Verify one receipt's output against its recorded digest. A mismatch
    /// (tampered output between admission and synthesis) is inadmissible.
    pub fn verify(&self, task_id: &str, output: &str) -> Result<u64, VerificationFailure> {
        let Some(artifact) = self.artifacts.get(task_id) else {
            return Err(VerificationFailure::UnknownTask {
                task_id: task_id.to_owned(),
            });
        };
        let recomputed = artifact_digest(task_id, output);
        if recomputed != artifact.digest {
            return Err(VerificationFailure::DigestMismatch {
                task_id: task_id.to_owned(),
                recorded: artifact.digest,
                recomputed,
            });
        }
        Ok(artifact.digest)
    }

    /// The ledger entry ids backing all admitted evidence (the legal
    /// citation universe for the trace gate).
    #[must_use]
    pub fn citable_ledger_ids(&self) -> Vec<u64> {
        self.artifacts.values().map(|a| a.ledger_entry_id).collect()
    }

    /// Number of admitted artifacts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    /// Whether no artifact is admitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }
}

/// Why evidence or a citation was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "reason")]
pub enum VerificationFailure {
    /// The task has no admitted artifact.
    UnknownTask {
        /// Offending task id.
        task_id: String,
    },
    /// The recomputed digest differs from the recorded one: tampered or
    /// corrupted output between admission and synthesis.
    DigestMismatch {
        /// Offending task id.
        task_id: String,
        /// Digest recorded at admission.
        recorded: u64,
        /// Digest recomputed at verification.
        recomputed: u64,
    },
    /// A synthesis citation resolves to no recorded artifact.
    DanglingCitation {
        /// Offending ledger entry id.
        ledger_entry_id: u64,
    },
}

impl std::fmt::Display for VerificationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTask { task_id } => write!(f, "no admitted artifact for task {task_id}"),
            Self::DigestMismatch {
                task_id,
                recorded,
                recomputed,
            } => write!(
                f,
                "digest mismatch for task {task_id}: recorded {recorded:#x}, recomputed {recomputed:#x}"
            ),
            Self::DanglingCitation { ledger_entry_id } => {
                write!(
                    f,
                    "citation {ledger_entry_id} resolves to no recorded artifact"
                )
            }
        }
    }
}

/// Extract `[evidence:<id>]` citations from a synthesis. This is the
/// citation contract taught to the Navigator in the synthesis prompt;
/// assertions without citations are not machine-checkable and are handled
/// by profile policy at the caller, not here.
#[must_use]
pub fn extract_citations(synthesis: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut rest = synthesis;
    while let Some(start) = rest.find("[evidence:") {
        let after = &rest[start + "[evidence:".len()..];
        let Some(end) = after.find(']') else {
            break;
        };
        if let Ok(id) = after[..end].trim().parse::<u64>() {
            ids.push(id);
        }
        rest = &after[end + 1..];
    }
    ids
}

/// The trace gate: every citation must resolve to an admitted artifact's
/// ledger entry. Returns the resolved ids in citation order.
///
/// # Errors
///
/// The first dangling citation, failing closed.
pub fn verify_traces(
    synthesis: &str,
    book: &EvidenceBook,
) -> Result<Vec<u64>, VerificationFailure> {
    let citable: std::collections::BTreeSet<u64> = book.citable_ledger_ids().into_iter().collect();
    let citations = extract_citations(synthesis);
    for id in &citations {
        if !citable.contains(id) {
            return Err(VerificationFailure::DanglingCitation {
                ledger_entry_id: *id,
            });
        }
    }
    Ok(citations)
}

/// Budget levels with their threshold fractions.
pub const BUDGET_LEVELS: [u8; 3] = [50, 80, 100];

/// One budget consumption reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetReading {
    /// Cumulative tokens consumed.
    pub tokens: u64,
    /// Cumulative milliseconds spent (caller-supplied).
    pub elapsed_ms: u64,
}

impl BudgetReading {
    /// A reading of nothing consumed.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            tokens: 0,
            elapsed_ms: 0,
        }
    }
}

/// Budget ceiling in tokens and wall-clock milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetCeiling {
    /// Maximum cumulative tokens.
    pub tokens: u64,
    /// Maximum cumulative milliseconds.
    pub elapsed_ms: u64,
}

/// What the watchdog says about a reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum BudgetState {
    /// Below the 50% mark.
    Nominal,
    /// At or beyond 50% but below 80%: emit the audit event, keep going.
    Warning {
        /// Percent consumed (0–100).
        percent: u8,
    },
    /// At or beyond 80% but below 100%: emit, keep going, prepare to stop.
    Critical {
        /// Percent consumed (0–100).
        percent: u8,
    },
    /// At or beyond 100%: hard stop required. No further consumption is
    /// admissible.
    Exhausted {
        /// Percent consumed (clamped ≥100).
        percent: u8,
    },
}

/// The budget watchdog: evaluates cumulative consumption against a
/// ceiling, remembering which thresholds have already fired (each level
/// emits exactly once).
#[derive(Debug)]
pub struct BudgetWatchdog {
    ceiling: BudgetCeiling,
    fired: BTreeMap<u8, BudgetReading>,
    consumed: BudgetReading,
}

impl BudgetWatchdog {
    /// A watchdog over the given ceiling.
    #[must_use]
    pub fn new(ceiling: BudgetCeiling) -> Self {
        Self {
            ceiling,
            fired: BTreeMap::new(),
            consumed: BudgetReading::zero(),
        }
    }

    /// The configured ceiling.
    #[must_use]
    pub fn ceiling(&self) -> &BudgetCeiling {
        &self.ceiling
    }

    /// Percent consumed for a reading (0–100), by the binding axis: the
    /// axis closest to its ceiling governs.
    ///
    /// Takes the reading explicitly (a caller may project a hypothetical
    /// reading); for the accumulated consumption use
    /// [`Self::percent_consumed`].
    #[must_use]
    pub fn percent(&self, reading: BudgetReading) -> u8 {
        Self::percent_of(&self.ceiling, reading)
    }

    /// Percent of the ceiling already accumulated inside the watchdog.
    #[must_use]
    pub fn percent_consumed(&self) -> u8 {
        Self::percent_of(&self.ceiling, self.consumed)
    }

    fn percent_of(ceiling: &BudgetCeiling, reading: BudgetReading) -> u8 {
        let token_percent = if ceiling.tokens == 0 {
            100
        } else {
            ((reading.tokens.saturating_mul(100)) / ceiling.tokens.max(1)) as u8
        };
        let time_percent = if ceiling.elapsed_ms == 0 {
            100
        } else {
            ((reading.elapsed_ms.saturating_mul(100)) / ceiling.elapsed_ms.max(1)) as u8
        };
        token_percent.max(time_percent).min(100)
    }

    /// Evaluate a reading for a goal: returns the state and, when a new
    /// threshold is crossed, the `BudgetThreshold` audit event to record
    /// (each level fires exactly once; the highest newly crossed level
    /// emits, and lower skipped levels emit retroactively in order).
    pub fn evaluate(
        &mut self,
        goal_id: &str,
        reading: BudgetReading,
    ) -> (BudgetState, Vec<AuditEvent>) {
        let percent = self.percent(reading);
        let state = if percent >= 100 {
            BudgetState::Exhausted { percent }
        } else if percent >= 80 {
            BudgetState::Critical { percent }
        } else if percent >= 50 {
            BudgetState::Warning { percent }
        } else {
            BudgetState::Nominal
        };
        self.consumed = reading;
        let mut events = Vec::new();
        for level in BUDGET_LEVELS {
            if percent >= level && !self.fired.contains_key(&level) {
                self.fired.insert(level, reading);
                events.push(AuditEvent::BudgetThreshold {
                    goal_id: goal_id.to_owned(),
                    level,
                    consumed_tokens: reading.tokens,
                    ceiling_tokens: self.ceiling.tokens,
                    at_ms: reading.elapsed_ms,
                });
            }
        }
        (state, events)
    }

    /// The levels that have fired so far.
    #[must_use]
    pub fn fired_levels(&self) -> Vec<u8> {
        self.fired.keys().copied().collect()
    }
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
