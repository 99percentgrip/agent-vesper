//! Task-level governance for the hive (VRO-16 PR-1).
//!
//! Bounded human-in-the-loop gates with default-action timeouts, strictly
//! per-task suspension, and an observable-signals-only pause evaluator.
//!
//! Purity contract: this module performs no I/O and reads no clock. All time
//! enters as caller-supplied Unix-epoch milliseconds (`now_ms`), mirroring
//! the ledger's timestamp-source seam. Gate countdowns are **derived** from
//! the `GateOpened` wall-clock record — never from a ticking timer — so a
//! governor rebuilt from its audit trail computes identical remaining time
//! (the restart proof).
//!
//! Ratified decisions (docs/advanced-hive-governance-prd.md §1):
//! - D1: every gate has a bounded countdown and a pre-configured fallback;
//!   expiry fires the fallback automatically. No unbounded waits exist.
//! - D2: suspension is per task. A gate on one task never blocks sibling
//!   tasks; dependents stop at the ordinary dependency wall.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default gate countdown (ratified D1 example: five minutes).
pub const DEFAULT_GATE_TIMEOUT: Duration = Duration::from_secs(300);
/// Shortest permitted countdown: long enough for a human to read a gate.
pub const MIN_GATE_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest permitted countdown: no gate may approach an unbounded wait.
pub const MAX_GATE_TIMEOUT: Duration = Duration::from_secs(3600);
/// Bound on the reason/directive text carried by a gate.
pub const MAX_GATE_TEXT_BYTES: usize = 4096;
/// Bounded in-memory audit stream length.
const MAX_EVENTS: usize = 256;

/// What happens when a gate expires without a host command (D1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum FallbackAction {
    /// The suspended task fails with `gate_timeout`; partial history is
    /// preserved and siblings are unaffected. The default fallback.
    FailTask,
}

/// Host resolutions for an open gate. Both hosts expose exactly this set;
/// semantics are shared and defined here (D2: task-level only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum HostCommand {
    /// Approve; the task resumes unchanged.
    Resume,
    /// Resume with a bounded directive prepended to the task prompt.
    Redirect {
        /// Bounded guidance text (at most [`MAX_GATE_TEXT_BYTES`]).
        directive: String,
    },
    /// The task fails with the given reason; no auto-requeue.
    Fail {
        /// Bounded failure reason.
        reason: String,
    },
    /// The whole goal is cancelled; partial history is preserved, never
    /// replayed.
    Cancel,
}

impl HostCommand {
    /// The canonical verb set shared by both hosts, in stable order.
    /// Cross-host parity tests assert both hosts advertise exactly this.
    #[must_use]
    pub fn verbs() -> &'static [&'static str] {
        &["resume", "redirect", "fail", "cancel"]
    }
}

/// Governance profile selecting which boundaries may open gates
/// (recon M5, adapted). `Auto` keeps VRO-15 behavior unless observable
/// degradation signals appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GovernanceProfile {
    /// SmartPause only: gates appear solely from observable degradation
    /// signals (anomalies, budget pressure). Failed receipts keep the
    /// VRO-15 error semantics.
    #[default]
    Auto,
    /// SmartPause plus fixed gates at the decomposition and synthesis
    /// boundaries.
    Gated,
    /// Caller-selected per-boundary policy.
    Custom,
}

/// Per-boundary gate policy for [`GovernanceProfile::Custom`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BoundaryPolicy {
    /// Gate after decomposition, before any dispatch.
    pub decomposition: bool,
    /// Gate before synthesis.
    pub synthesis: bool,
    /// Gate when a driver receipt reports failure.
    pub failed_receipt: bool,
    /// Apply the SmartPause evaluator to observable signals.
    pub smartpause: bool,
}

impl Default for BoundaryPolicy {
    fn default() -> Self {
        Self {
            decomposition: true,
            synthesis: true,
            failed_receipt: true,
            smartpause: true,
        }
    }
}

/// Validated governance configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceConfig {
    /// Which boundaries may open gates.
    pub profile: GovernanceProfile,
    /// Bounded gate countdown (D1).
    pub gate_timeout: Duration,
    /// Per-boundary policy when `profile` is `Custom`.
    pub custom: BoundaryPolicy,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            profile: GovernanceProfile::Auto,
            gate_timeout: DEFAULT_GATE_TIMEOUT,
            custom: BoundaryPolicy::default(),
        }
    }
}

impl GovernanceConfig {
    /// Rejects unbounded or degenerate countdowns (D1: bounded at both ends).
    #[must_use]
    pub fn validated(&self) -> bool {
        self.gate_timeout >= MIN_GATE_TIMEOUT
            && self.gate_timeout <= MAX_GATE_TIMEOUT
            && self.gate_timeout > Duration::ZERO
    }

    fn policy(&self) -> BoundaryPolicy {
        match self.profile {
            GovernanceProfile::Auto => BoundaryPolicy {
                decomposition: false,
                synthesis: false,
                failed_receipt: false,
                smartpause: true,
            },
            GovernanceProfile::Gated => BoundaryPolicy {
                decomposition: true,
                synthesis: true,
                failed_receipt: false,
                smartpause: true,
            },
            GovernanceProfile::Custom => self.custom.clone(),
        }
    }

    /// Whether a failed driver receipt should open a gate instead of taking
    /// the VRO-15 error path.
    #[must_use]
    pub fn gates_failed_receipt(&self, signals: &ReceiptSignals) -> bool {
        let policy = self.policy();
        (policy.failed_receipt || policy.smartpause) && evaluate_receipt(signals)
    }

    /// Whether the decomposition boundary gates.
    #[must_use]
    pub fn gates_decomposition(&self) -> bool {
        self.policy().decomposition
    }

    /// Whether the synthesis boundary gates.
    #[must_use]
    pub fn gates_synthesis(&self) -> bool {
        self.policy().synthesis
    }
}

/// Observable receipt-boundary signals for the SmartPause evaluator.
/// Every field is machine-observed; **no model-confidence input exists**
/// (root contract: never fabricate values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptSignals {
    /// Whether the receipt reports success.
    pub success: bool,
    /// Consecutive failures observed for this task so far.
    pub consecutive_failures: u32,
    /// Ledger/trace anomalies flagged for this task (dangling refs, digest
    /// mismatches — non-zero means machine-detected trouble).
    pub anomalies: u32,
    /// Remaining hive budget as a percentage (0–100).
    pub budget_remaining_percent: u8,
}

impl ReceiptSignals {
    /// A clean successful receipt.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            success: true,
            consecutive_failures: 0,
            anomalies: 0,
            budget_remaining_percent: 100,
        }
    }
}

/// SmartPause evaluator: pure decision over observable signals (recon M7).
/// Yields when a repeated failure, a machine-flagged anomaly, or budget
/// pressure makes unattended continuation unjustifiable. Deterministic and
/// side-effect free.
#[must_use]
pub fn evaluate_receipt(signals: &ReceiptSignals) -> bool {
    (!signals.success && signals.consecutive_failures >= 2)
        || signals.anomalies > 0
        || signals.budget_remaining_percent <= 20
}

/// One open gate. `opened_at_ms` is the exact caller-supplied wall-clock
/// initialization recorded in the audit event; the countdown is always
/// derived from it, never stored as a ticking value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRecord {
    /// Stable gate identity (`{task_id}-gate`).
    pub gate_id: String,
    /// The suspended task.
    pub task_id: String,
    /// Owning goal.
    pub goal_id: String,
    /// Bounded human-readable reason.
    pub reason: String,
    /// Wall-clock gate initialization (Unix epoch milliseconds).
    pub opened_at_ms: u64,
    /// Bounded countdown (D1).
    pub timeout: Duration,
    /// Pre-configured expiry fallback.
    pub fallback: FallbackAction,
}

impl GateRecord {
    /// Remaining countdown in milliseconds; saturates at zero.
    #[must_use]
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        let opened = u128::from(self.opened_at_ms);
        let now = u128::from(now_ms);
        let elapsed = now.saturating_sub(opened);
        u64::try_from(self.timeout.as_millis().saturating_sub(elapsed)).unwrap_or_default()
    }

    /// Whether the countdown has run out at `now_ms`.
    #[must_use]
    pub fn expired(&self, now_ms: u64) -> bool {
        self.remaining_ms(now_ms) == 0
    }
}

/// Immutable audit events (D4 structured log records). Gate lifecycle
/// (PR-1) and decision verdicts (PR-2) share one append-only stream.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum AuditEvent {
    /// A gate opened; carries exact wall-clock initialization.
    GateOpened {
        /// Gate identity.
        gate_id: String,
        /// Suspended task.
        task_id: String,
        /// Owning goal.
        goal_id: String,
        /// Bounded reason.
        reason: String,
        /// Exact wall-clock initialization (Unix epoch milliseconds).
        opened_at_ms: u64,
        /// Countdown in milliseconds.
        timeout_ms: u64,
        /// Pre-configured fallback.
        fallback: FallbackAction,
    },
    /// A host command resolved the gate before expiry.
    GateResolved {
        /// Gate identity.
        gate_id: String,
        /// The resolving command.
        command: HostCommand,
        /// Resolution wall-clock (Unix epoch milliseconds).
        at_ms: u64,
    },
    /// The countdown expired; the fallback fired.
    GateExpired {
        /// Gate identity.
        gate_id: String,
        /// The fallback that fired.
        fallback: FallbackAction,
        /// Expiry wall-clock (Unix epoch milliseconds).
        at_ms: u64,
    },
    /// A Navigator decision verdict was issued (VRO-16 PR-2). All
    /// `rationale_refs` must resolve to real ledger entries — dangling
    /// references fail closed at issuance.
    DecisionIssued {
        /// Owning goal.
        goal_id: String,
        /// The verdict.
        verdict: DecisionVerdictPayload,
        /// Ledger entry ids backing the verdict.
        rationale_refs: Vec<u64>,
        /// Refine/pivot iteration counts at issuance.
        refine_iterations: u32,
        pivot_iterations: u32,
        /// Wall-clock issuance (Unix epoch milliseconds).
        at_ms: u64,
    },
    /// A deterministic verification gate verdict (VRO-16 PR-3): trace or
    /// digest results on admitted evidence.
    VerificationVerdict {
        /// Owning goal.
        goal_id: String,
        /// What was checked.
        check: VerificationCheck,
        /// The failure description; `None` means the check passed.
        failure: Option<String>,
        /// Ledger entry ids the check covered.
        covered_refs: Vec<u64>,
        /// Wall-clock (Unix epoch milliseconds).
        at_ms: u64,
    },
    /// A budget threshold was crossed (VRO-16 PR-3): 50/80 emit without
    /// stopping; 100 hard-stops the hive.
    BudgetThreshold {
        /// Owning goal.
        goal_id: String,
        /// Threshold level just crossed (50, 80 or 100).
        level: u8,
        /// Cumulative tokens consumed at crossing.
        consumed_tokens: u64,
        /// Token ceiling.
        ceiling_tokens: u64,
        /// Wall-clock (Unix epoch milliseconds).
        at_ms: u64,
    },
}

/// What a verification verdict covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationCheck {
    /// Evidence digest recomputation before synthesis admission.
    Digest,
    /// Synthesis citation trace resolution.
    Trace,
}

impl AuditEvent {
    /// The gate this event belongs to; decision events return the owning
    /// goal id (their stream key in the audit log).
    #[must_use]
    pub fn gate_id(&self) -> &str {
        match self {
            AuditEvent::GateOpened { gate_id, .. }
            | AuditEvent::GateResolved { gate_id, .. }
            | AuditEvent::GateExpired { gate_id, .. } => gate_id,
            AuditEvent::DecisionIssued { goal_id, .. }
            | AuditEvent::VerificationVerdict { goal_id, .. }
            | AuditEvent::BudgetThreshold { goal_id, .. } => goal_id,
        }
    }

    /// The owning goal for ledger persistence keys.
    #[must_use]
    pub fn goal_id(&self) -> &str {
        match self {
            AuditEvent::GateOpened { goal_id, .. }
            | AuditEvent::DecisionIssued { goal_id, .. }
            | AuditEvent::VerificationVerdict { goal_id, .. }
            | AuditEvent::BudgetThreshold { goal_id, .. } => goal_id,
            AuditEvent::GateResolved { gate_id, .. } | AuditEvent::GateExpired { gate_id, .. } => {
                goal_of_gate(gate_id).unwrap_or_default()
            }
        }
    }
}

/// Goal extraction from a `{goal}-task-{n}-gate` identity; gates embed
/// their goal inside the task id. Cuts at the LAST `-task-` occurrence so
/// goal ids containing the substring still resolve.
fn goal_of_gate(gate_id: &str) -> Option<&str> {
    let task = gate_id.strip_suffix("-gate")?;
    let cut = task.rfind("-task-")?;
    Some(&task[..cut])
}

/// One decision verdict and its payload (VRO-16 PR-2). Shared by the
/// decision engine, the audit variant and the review-panel inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum DecisionVerdictPayload {
    /// Accept the synthesis; the goal completes.
    Proceed,
    /// Re-run amended tasks; bounded by the refine cap.
    Refine {
        /// Original decomposition indices to re-run, with amended prompts.
        amended: Vec<AmendedTask>,
        /// Which refine iteration this is (1-based).
        iteration: u32,
    },
    /// Discard pending tasks and restart decomposition; bounded by the
    /// pivot cap.
    Pivot {
        /// The new decomposition to dispatch.
        tasks: Vec<AmendedTask>,
    },
    /// A cap was exhausted; the goal completes with the evidence that
    /// exists and an explicit failure marker. Never silent.
    ProceedWithFailure {
        /// Why the cap path was taken.
        reason: String,
    },
}

/// One amended/new task inside a Refine or Pivot verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmendedTask {
    /// Original decomposition index this task replaces (`None` for new
    /// Pivot tasks).
    pub index: Option<usize>,
    /// The bounded task prompt.
    pub prompt: String,
    /// Required capabilities.
    pub required_capabilities: Vec<String>,
    /// Prerequisite original indices.
    pub depends_on: Vec<usize>,
}

/// A bounded append-only audit stream with goal-keyed ledger persistence.
/// The Governor's in-memory stream stays bounded (256); the ledger copy is
/// durable for the run.
#[derive(Debug, Default)]
pub struct AuditLog {
    events: VecDeque<AuditEvent>,
}

impl AuditLog {
    /// Bounded in-memory length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the stream is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Append one event, bounded.
    pub fn push(&mut self, event: AuditEvent) {
        if self.events.len() >= 256 {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// The bounded audit stream (oldest first).
    #[must_use]
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.iter().cloned().collect()
    }
}

/// What the orchestrator should do with the suspended task after a
/// resolution or expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Re-dispatch the task, optionally with a prepended directive.
    Resume { directive: Option<String> },
    /// Mark the task failed with the given reason.
    FailTask { reason: String },
    /// Cancel the whole goal; retain partial history.
    CancelGoal,
}

/// Host-renderable view of an open gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateView {
    /// Gate identity.
    pub gate_id: String,
    /// Suspended task.
    pub task_id: String,
    /// Bounded reason.
    pub reason: String,
    /// Remaining countdown milliseconds at snapshot time.
    pub remaining_ms: u64,
    /// Fallback action on expiry.
    pub fallback: FallbackAction,
}

/// Per-run gate state machine. All transitions are validated and audited.
#[derive(Debug, Default)]
pub struct Governor {
    config: GovernanceConfig,
    gates: BTreeMap<String, GateRecord>,
    resolved: BTreeSet<String>,
    /// Task ids whose gates already resolved/expired this run (G4: a
    /// boundary gate fires once per task id — a resolved gate does not
    /// reopen when the run resumes).
    resolved_task_ids: BTreeSet<String>,
    events: VecDeque<AuditEvent>,
}

impl Governor {
    /// A governor with the given validated configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for an unbounded countdown (D1 violation).
    pub fn new(config: GovernanceConfig) -> Result<Self, String> {
        if !config.validated() {
            return Err(format!(
                "gate timeout must be bounded between {:?} and {:?}",
                MIN_GATE_TIMEOUT, MAX_GATE_TIMEOUT
            ));
        }
        Ok(Self {
            config,
            ..Self::default()
        })
    }

    /// The active configuration.
    #[must_use]
    pub fn config(&self) -> &GovernanceConfig {
        &self.config
    }

    /// Open a gate for `task_id`. Fails on duplicates or unbounded text.
    ///
    /// # Errors
    ///
    /// Duplicate gate, unbounded reason, or an invalid timeout.
    pub fn open_gate(
        &mut self,
        goal_id: &str,
        task_id: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<AuditEvent, String> {
        let gate_id = format!("{task_id}-gate");
        if self.gates.contains_key(&gate_id) || self.resolved.contains(&gate_id) {
            return Err(format!("gate already exists for task {task_id}"));
        }
        if reason.len() > MAX_GATE_TEXT_BYTES || reason.is_empty() {
            return Err("gate reason must be 1..=4096 bytes".into());
        }
        let record = GateRecord {
            gate_id: gate_id.clone(),
            task_id: task_id.to_owned(),
            goal_id: goal_id.to_owned(),
            reason: reason.to_owned(),
            opened_at_ms: now_ms,
            timeout: self.config.gate_timeout,
            fallback: FallbackAction::FailTask,
        };
        let event = AuditEvent::GateOpened {
            gate_id,
            task_id: record.task_id.clone(),
            goal_id: record.goal_id.clone(),
            reason: record.reason.clone(),
            opened_at_ms: record.opened_at_ms,
            timeout_ms: self.config.gate_timeout.as_millis().min(u64::MAX as u128) as u64,
            fallback: record.fallback.clone(),
        };
        self.gates.insert(record.gate_id.clone(), record);
        self.push_event(event.clone());
        Ok(event)
    }

    /// Apply a host command to an open gate.
    ///
    /// # Errors
    ///
    /// Unknown/closed gate, expired gate, or unbounded command text.
    pub fn resolve(
        &mut self,
        task_id: &str,
        command: HostCommand,
        now_ms: u64,
    ) -> Result<(Resolution, AuditEvent), String> {
        let gate_id = format!("{task_id}-gate");
        let Some(record) = self.gates.get(&gate_id) else {
            return Err(format!("no open gate for task {task_id}"));
        };
        if record.expired(now_ms) {
            return Err(format!(
                "gate for task {task_id} already expired; expiry fallback applies"
            ));
        }
        if let HostCommand::Redirect { directive } = &command
            && (directive.is_empty() || directive.len() > MAX_GATE_TEXT_BYTES)
        {
            return Err("redirect directive must be 1..=4096 bytes".into());
        }
        if let HostCommand::Fail { reason } = &command
            && (reason.is_empty() || reason.len() > MAX_GATE_TEXT_BYTES)
        {
            return Err("fail reason must be 1..=4096 bytes".into());
        }
        let resolution = match &command {
            HostCommand::Resume => Resolution::Resume { directive: None },
            HostCommand::Redirect { directive } => Resolution::Resume {
                directive: Some(directive.clone()),
            },
            HostCommand::Fail { reason } => Resolution::FailTask {
                reason: reason.clone(),
            },
            HostCommand::Cancel => Resolution::CancelGoal,
        };
        let task_id = record.task_id.clone();
        let event = AuditEvent::GateResolved {
            gate_id,
            command,
            at_ms: now_ms,
        };
        self.gates.remove(event.gate_id());
        self.resolved.insert(event.gate_id().to_owned());
        self.resolved_task_ids.insert(task_id);
        self.push_event(event.clone());
        Ok((resolution, event))
    }

    /// Expire every due gate, firing each pre-configured fallback (D1).
    /// Returns the expiries in gate-id order.
    pub fn expire_due(&mut self, now_ms: u64) -> Vec<(String, Resolution, AuditEvent)> {
        let due: Vec<String> = self
            .gates
            .values()
            .filter(|record| record.expired(now_ms))
            .map(|record| record.gate_id.clone())
            .collect();
        let mut out = Vec::new();
        for gate_id in due {
            let Some(record) = self.gates.remove(&gate_id) else {
                continue;
            };
            self.resolved.insert(gate_id.clone());
            self.resolved_task_ids.insert(record.task_id.clone());
            let event = AuditEvent::GateExpired {
                gate_id: gate_id.clone(),
                fallback: record.fallback.clone(),
                at_ms: now_ms,
            };
            self.push_event(event.clone());
            let resolution = match record.fallback {
                FallbackAction::FailTask => Resolution::FailTask {
                    reason: String::from("gate_timeout"),
                },
            };
            out.push((record.task_id.clone(), resolution, event));
        }
        out
    }

    /// Whether any gate is still open.
    #[must_use]
    pub fn has_open_gates(&self) -> bool {
        !self.gates.is_empty()
    }

    /// Every task id whose gate already resolved or expired this run
    /// (G4: boundary gates fire once per task id).
    #[must_use]
    pub fn resolved_gate_ids(&self) -> &BTreeSet<String> {
        &self.resolved_task_ids
    }

    /// Open gates keyed by suspended task id.
    #[must_use]
    pub fn open_gate_task_ids(&self) -> Vec<String> {
        self.gates
            .values()
            .map(|record| record.task_id.clone())
            .collect()
    }

    /// Host-renderable snapshot with derived countdowns.
    #[must_use]
    pub fn snapshot(&self, now_ms: u64) -> Vec<GateView> {
        self.gates
            .values()
            .map(|record| GateView {
                gate_id: record.gate_id.clone(),
                task_id: record.task_id.clone(),
                reason: record.reason.clone(),
                remaining_ms: record.remaining_ms(now_ms),
                fallback: record.fallback.clone(),
            })
            .collect()
    }

    /// The bounded audit stream (oldest first).
    #[must_use]
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.iter().cloned().collect()
    }

    /// Rebuild a governor's open-gate state from an audit trail. The
    /// countdown is recomputed from each `GateOpened.opened_at_ms`, so a
    /// reconstructed governor reports identical remaining time (the restart
    /// proof): elapsed time is never reset by reconstruction.
    #[must_use]
    pub fn replay(config: GovernanceConfig, events: &[AuditEvent]) -> Self {
        let mut governor = Self {
            config,
            ..Self::default()
        };
        for event in events {
            match event {
                AuditEvent::GateOpened {
                    gate_id,
                    task_id,
                    goal_id,
                    reason,
                    opened_at_ms,
                    timeout_ms,
                    fallback,
                } => {
                    governor.gates.insert(
                        gate_id.clone(),
                        GateRecord {
                            gate_id: gate_id.clone(),
                            task_id: task_id.clone(),
                            goal_id: goal_id.clone(),
                            reason: reason.clone(),
                            opened_at_ms: *opened_at_ms,
                            timeout: Duration::from_millis(*timeout_ms),
                            fallback: fallback.clone(),
                        },
                    );
                    governor.resolved.remove(gate_id);
                }
                AuditEvent::GateResolved { gate_id, .. }
                | AuditEvent::GateExpired { gate_id, .. } => {
                    governor.gates.remove(gate_id);
                    governor.resolved.insert(gate_id.clone());
                }
                // Decision, verification and budget events carry no
                // open-gate state; replay keeps them in the stream only.
                AuditEvent::DecisionIssued { .. }
                | AuditEvent::VerificationVerdict { .. }
                | AuditEvent::BudgetThreshold { .. } => {}
            }
            governor.events.push_back(event.clone());
        }
        governor
    }

    fn push_event(&mut self, event: AuditEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

#[cfg(test)]
#[path = "governance_tests.rs"]
mod tests;
