//! VRO status event vocabulary (VRO-10, PRD §16).
//!
//! PRD §16 enumerates 13 internal events the Vesper Reasoning Orchestrator
//! may emit during a turn (`reasoning.profiled`,
//! `reasoning.strategy_selected`, `reasoning.plan_created`, …). These events
//! must reach upstream clients — not just the TUI host — so external tools
//! (Zed, IDEs, telemetry collectors) can render standardized orchestrator
//! state changes.
//!
//! ACP does not have dedicated VRO event types in v1. PRD §16 therefore
//! mandates: "Where ACP has no dedicated event, VRO events should be
//! translated into existing session update or status mechanisms without
//! changing required top-level wire fields." This module:
//!
//! 1. Defines the discrete [`VroEvent`] enum mirroring the 13 PRD §16
//!    events, plus the per-event payload fields (branch id, verifier id,
//!    sequence number, …) PRD §16 also requires.
//! 2. Translates each [`VroEvent`] into the closest-fit ACP
//!    [`SessionNotification`] — typically an `AgentMessageChunk` carrying a
//!    user-facing markdown summary line — so existing clients surface the
//!    phase transition without protocol changes.
//! 3. Exposes a [`VroEventSink`] trait the orchestrator can push events
//!    through; the ACP adapter implements a sink that owns a
//!    [`ConnectionTo<Client>`] and translates per emission.
//!
//! ## Event ordering (PRD §16)
//!
//! "Event ordering must remain deterministic within a session. Parallel
//! branch events must carry branch identifiers and monotonic sequence
//! numbers." Each [`VroEvent`] carries a `session_seq: u64` monotonic
//! counter and branch-specific events carry `branch_id: Option<String>`.

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionId, SessionNotification, SessionUpdate, TextContent,
};

/// A VRO status event (PRD §16). Each variant maps to a distinct phase
/// transition the orchestrator emits during a turn.
///
/// The fields are the **minimum** PRD §16 requires:
/// - `session_seq` — monotonic per-session sequence number.
/// - `branch_id` — branch identifier when the event pertains to a single
///   parallel branch (PRD §16: "Parallel branch events must carry branch
///   identifiers").
/// - `verifier_id` — verifier identifier for verifier-specific events.
/// - `summary` — a user-safe markdown summary line (PRD §8.2: "concise,
///   user-safe summaries"). The translator surfaces this verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VroEvent {
    /// `reasoning.profiled` — the Task Profiler finished.
    Profiled { session_seq: u64, summary: String },
    /// `reasoning.strategy_selected` — the Policy Engine picked a strategy.
    StrategySelected {
        session_seq: u64,
        strategy: String,
        summary: String,
    },
    /// `reasoning.plan_created` — the Workflow Planner produced a plan.
    PlanCreated {
        session_seq: u64,
        step_count: u32,
        summary: String,
    },
    /// `reasoning.branch_started` — a parallel branch began executing.
    BranchStarted {
        session_seq: u64,
        branch_id: String,
        summary: String,
    },
    /// `reasoning.branch_pruned` — a parallel branch was pruned (early-stop
    /// or cancellation).
    BranchPruned {
        session_seq: u64,
        branch_id: String,
        summary: String,
    },
    /// `reasoning.tool_requested` — a tool invocation was issued.
    ToolRequested {
        session_seq: u64,
        tool: String,
        summary: String,
    },
    /// `reasoning.tool_completed` — a tool invocation returned.
    ToolCompleted {
        session_seq: u64,
        tool: String,
        success: bool,
        summary: String,
    },
    /// `reasoning.verification_started` — a verifier started running.
    VerificationStarted {
        session_seq: u64,
        verifier_id: String,
        summary: String,
    },
    /// `reasoning.verification_failed` — a verifier failed.
    VerificationFailed {
        session_seq: u64,
        verifier_id: String,
        repairable: bool,
        summary: String,
    },
    /// `reasoning.repair_started` — the Repair Controller began a repair
    /// attempt.
    RepairStarted {
        session_seq: u64,
        attempt: u32,
        summary: String,
    },
    /// `reasoning.candidate_selected` — a candidate was selected as the
    /// answer.
    CandidateSelected {
        session_seq: u64,
        candidate_id: String,
        summary: String,
    },
    /// `reasoning.budget_warning` — a budget ceiling was approached or hit.
    BudgetWarning {
        session_seq: u64,
        ceiling: String,
        remaining: Option<u64>,
        summary: String,
    },
    /// `reasoning.completed` — the orchestrator finished the turn.
    Completed {
        session_seq: u64,
        status: String,
        summary: String,
    },
}

impl VroEvent {
    /// The PRD §16 wire name of this event (e.g. `"reasoning.profiled"`).
    #[must_use]
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Profiled { .. } => "reasoning.profiled",
            Self::StrategySelected { .. } => "reasoning.strategy_selected",
            Self::PlanCreated { .. } => "reasoning.plan_created",
            Self::BranchStarted { .. } => "reasoning.branch_started",
            Self::BranchPruned { .. } => "reasoning.branch_pruned",
            Self::ToolRequested { .. } => "reasoning.tool_requested",
            Self::ToolCompleted { .. } => "reasoning.tool_completed",
            Self::VerificationStarted { .. } => "reasoning.verification_started",
            Self::VerificationFailed { .. } => "reasoning.verification_failed",
            Self::RepairStarted { .. } => "reasoning.repair_started",
            Self::CandidateSelected { .. } => "reasoning.candidate_selected",
            Self::BudgetWarning { .. } => "reasoning.budget_warning",
            Self::Completed { .. } => "reasoning.completed",
        }
    }

    /// The per-session monotonic sequence number.
    #[must_use]
    pub fn session_seq(&self) -> u64 {
        match self {
            Self::Profiled { session_seq, .. }
            | Self::StrategySelected { session_seq, .. }
            | Self::PlanCreated { session_seq, .. }
            | Self::BranchStarted { session_seq, .. }
            | Self::BranchPruned { session_seq, .. }
            | Self::ToolRequested { session_seq, .. }
            | Self::ToolCompleted { session_seq, .. }
            | Self::VerificationStarted { session_seq, .. }
            | Self::VerificationFailed { session_seq, .. }
            | Self::RepairStarted { session_seq, .. }
            | Self::CandidateSelected { session_seq, .. }
            | Self::BudgetWarning { session_seq, .. }
            | Self::Completed { session_seq, .. } => *session_seq,
        }
    }

    /// The branch identifier when this event pertains to a single parallel
    /// branch, or `None`.
    #[must_use]
    pub fn branch_id(&self) -> Option<&str> {
        match self {
            Self::BranchStarted { branch_id, .. } | Self::BranchPruned { branch_id, .. } => {
                Some(branch_id.as_str())
            }
            _ => None,
        }
    }

    /// The user-facing markdown summary line carried by this event.
    #[must_use]
    pub fn summary(&self) -> &str {
        match self {
            Self::Profiled { summary, .. }
            | Self::StrategySelected { summary, .. }
            | Self::PlanCreated { summary, .. }
            | Self::BranchStarted { summary, .. }
            | Self::BranchPruned { summary, .. }
            | Self::ToolRequested { summary, .. }
            | Self::ToolCompleted { summary, .. }
            | Self::VerificationStarted { summary, .. }
            | Self::VerificationFailed { summary, .. }
            | Self::RepairStarted { summary, .. }
            | Self::CandidateSelected { summary, .. }
            | Self::BudgetWarning { summary, .. }
            | Self::Completed { summary, .. } => summary,
        }
    }

    /// Renders the event as a single user-facing markdown line that the
    /// ACP translator pushes through `AgentMessageChunk`. The line bundles
    /// the event name, sequence, optional branch id, and the user-safe
    /// summary so the receiving client renders a self-contained notice.
    ///
    /// Example: `**[reasoning.plan_created #4]** Built a 3-step plan.`.
    #[must_use]
    pub fn render_markdown_line(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "**[{} #{}`]",
            self.event_name(),
            self.session_seq()
        ));
        if let Some(branch) = self.branch_id() {
            out.push_str(&format!(" (branch `{branch}`)"));
        }
        out.push_str("** ");
        out.push_str(self.summary());
        out
    }
}

/// Translates a [`VroEvent`] into an ACP [`SessionNotification`] carrying
/// the event's markdown line through an `AgentMessageChunk`. PRD §16:
/// "Where ACP has no dedicated event, VRO events should be translated into
/// existing session update or status mechanisms without changing required
/// top-level wire fields."
///
/// Returns `None` when the event carries an empty summary (no chunk to
/// emit) — callers should skip the notification in that case.
#[must_use]
pub fn translate_vro_event_to_acp(
    session_id: &str,
    event: &VroEvent,
) -> Option<SessionNotification> {
    if event.summary().is_empty() {
        return None;
    }
    let line = event.render_markdown_line();
    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(line.as_str())));
    let session = SessionId::new(session_id.to_owned());
    Some(SessionNotification::new(
        session,
        SessionUpdate::AgentMessageChunk(chunk),
    ))
}

/// Sink the orchestrator pushes [`VroEvent`]s through. The ACP adapter
/// implements this with a connection-owning sink; tests use a
/// [`RecordingVroEventSink`] that captures events for assertions.
pub trait VroEventSink: Send + Sync {
    /// Pushes one event through the sink. Errors are surfaced to the caller
    /// but must NOT propagate as the orchestrator's outcome — VRO events
    /// are diagnostic and a translation/persistence failure must never
    /// abort a turn (PRD §17: keep operating even when telemetry is
    /// unavailable).
    fn emit(&self, event: VroEvent) -> Result<(), VroEventSinkError>;
}

/// Errors a [`VroEventSink`] may return. None abort the orchestrator.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VroEventSinkError {
    /// The underlying ACP transport rejected the notification.
    #[error("ACP transport rejected the VRO event notification: {0}")]
    Transport(String),
    /// The sink is closed (the session ended).
    #[error("VRO event sink is closed")]
    Closed,
}

/// Test-only sink that records every emitted event for assertions.
#[derive(Debug, Default)]
pub struct RecordingVroEventSink {
    events: std::sync::Mutex<Vec<VroEvent>>,
}

impl RecordingVroEventSink {
    /// Constructs a fresh recording sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of the events recorded so far, in emission order.
    #[must_use]
    pub fn events(&self) -> Vec<VroEvent> {
        self.events.lock().expect("poisoned").clone()
    }
}

impl VroEventSink for RecordingVroEventSink {
    fn emit(&self, event: VroEvent) -> Result<(), VroEventSinkError> {
        self.events.lock().expect("poisoned").push(event);
        Ok(())
    }
}

/// Convenience: builds the canonical 13-event "happy path" sequence for a
/// single Generate-Verify-Repair turn. Useful for tests, ACP adapter smoke
/// tests, and any caller that wants to validate the full vocabulary is
/// wired.
#[must_use]
pub fn sample_happy_path_sequence(session_seq_start: u64) -> Vec<VroEvent> {
    let mut seq = session_seq_start;
    let mut next = || {
        let s = seq;
        seq += 1;
        s
    };
    vec![
        VroEvent::Profiled {
            session_seq: next(),
            summary: "Understanding request.".into(),
        },
        VroEvent::StrategySelected {
            session_seq: next(),
            strategy: "generate_verify_repair".into(),
            summary: "Selected strategy: generate_verify_repair.".into(),
        },
        VroEvent::PlanCreated {
            session_seq: next(),
            step_count: 1,
            summary: "Built a 1-step plan.".into(),
        },
        VroEvent::BranchStarted {
            session_seq: next(),
            branch_id: "branch-0".into(),
            summary: "Branch 0 started.".into(),
        },
        VroEvent::ToolRequested {
            session_seq: next(),
            tool: "read_file".into(),
            summary: "Running tool: read_file.".into(),
        },
        VroEvent::ToolCompleted {
            session_seq: next(),
            tool: "read_file".into(),
            success: true,
            summary: "Tool read_file returned.".into(),
        },
        VroEvent::VerificationStarted {
            session_seq: next(),
            verifier_id: "cargo_test".into(),
            summary: "Verifying with cargo_test.".into(),
        },
        VroEvent::RepairStarted {
            session_seq: next(),
            attempt: 1,
            summary: "Repairing failed check (attempt 1).".into(),
        },
        VroEvent::VerificationFailed {
            session_seq: next(),
            verifier_id: "cargo_test".into(),
            repairable: true,
            summary: "cargo_test failed (repairable).".into(),
        },
        VroEvent::CandidateSelected {
            session_seq: next(),
            candidate_id: "cand-0000".into(),
            summary: "Selected candidate cand-0000.".into(),
        },
        VroEvent::BranchPruned {
            session_seq: next(),
            branch_id: "branch-1".into(),
            summary: "Branch 1 pruned (early-stop).".into(),
        },
        VroEvent::BudgetWarning {
            session_seq: next(),
            ceiling: "max_total_output_tokens".into(),
            remaining: Some(1024),
            summary: "Approaching max_total_output_tokens budget.".into(),
        },
        VroEvent::Completed {
            session_seq: next(),
            status: "succeeded".into(),
            summary: "Turn completed.".into(),
        },
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::SessionUpdate;
    use std::sync::Arc;

    #[test]
    fn all_thirteen_event_names_match_prd_16() {
        // Every PRD §16 event name must be present and distinct.
        let names: Vec<&'static str> = sample_happy_path_sequence(1)
            .iter()
            .map(VroEvent::event_name)
            .collect();
        let expected = [
            "reasoning.profiled",
            "reasoning.strategy_selected",
            "reasoning.plan_created",
            "reasoning.branch_started",
            "reasoning.tool_requested",
            "reasoning.tool_completed",
            "reasoning.verification_started",
            "reasoning.repair_started",
            "reasoning.verification_failed",
            "reasoning.candidate_selected",
            "reasoning.branch_pruned",
            "reasoning.budget_warning",
            "reasoning.completed",
        ];
        // Each expected name appears exactly once.
        for name in expected {
            assert_eq!(
                names.iter().filter(|n| **n == name).count(),
                1,
                "expected exactly one {name}, got {names:?}"
            );
        }
    }

    #[test]
    fn session_seq_monotonic_in_sample_sequence() {
        let events = sample_happy_path_sequence(7);
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.session_seq(), 7 + i as u64);
        }
    }

    #[test]
    fn branch_id_present_only_for_branch_events() {
        let events = sample_happy_path_sequence(1);
        for event in &events {
            match event.event_name() {
                "reasoning.branch_started" | "reasoning.branch_pruned" => {
                    assert!(
                        event.branch_id().is_some(),
                        "{} must carry a branch_id",
                        event.event_name()
                    );
                }
                _ => {
                    assert!(
                        event.branch_id().is_none(),
                        "{} must not carry a branch_id",
                        event.event_name()
                    );
                }
            }
        }
    }

    #[test]
    fn render_markdown_line_includes_event_name_seq_and_summary() {
        let event = VroEvent::PlanCreated {
            session_seq: 42,
            step_count: 3,
            summary: "Built a 3-step plan.".into(),
        };
        let line = event.render_markdown_line();
        assert!(
            line.contains("reasoning.plan_created"),
            "line must contain the event name: {line}"
        );
        assert!(line.contains("#42"), "line must contain the seq: {line}");
        assert!(
            line.contains("3-step plan"),
            "line must contain the summary: {line}"
        );
    }

    #[test]
    fn render_markdown_line_for_branch_event_includes_branch_id() {
        let event = VroEvent::BranchStarted {
            session_seq: 1,
            branch_id: "branch-7".into(),
            summary: "Branch started.".into(),
        };
        let line = event.render_markdown_line();
        assert!(
            line.contains("branch `branch-7`"),
            "branch event line must carry the branch id: {line}"
        );
    }

    #[test]
    fn translate_to_acp_produces_agent_message_chunk() {
        let event = VroEvent::StrategySelected {
            session_seq: 1,
            strategy: "direct".into(),
            summary: "Selected direct.".into(),
        };
        let notification = translate_vro_event_to_acp("sess-1", &event).expect("must translate");
        // Round-trip via serde to inspect the wire shape.
        let value = serde_json::to_value(&notification).unwrap();
        assert_eq!(
            value
                .pointer("/update/sessionUpdate")
                .and_then(|v| v.as_str()),
            Some("agent_message_chunk"),
            "VRO events must translate to AgentMessageChunk per PRD §16"
        );
        let text = value
            .pointer("/update/content/text")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(text.contains("reasoning.strategy_selected"));
        assert!(text.contains("Selected direct."));
    }

    #[test]
    fn translate_to_acp_returns_none_for_empty_summary() {
        let event = VroEvent::Profiled {
            session_seq: 1,
            summary: String::new(),
        };
        assert!(translate_vro_event_to_acp("sess-1", &event).is_none());
    }

    #[test]
    fn recording_sink_captures_events_in_emission_order() {
        let sink = RecordingVroEventSink::new();
        let sequence = sample_happy_path_sequence(100);
        for event in sequence.clone() {
            sink.emit(event).expect("emit must succeed");
        }
        let recorded = sink.events();
        assert_eq!(recorded.len(), sequence.len());
        assert_eq!(recorded[0], sequence[0]);
        assert_eq!(recorded[12], sequence[12]);
        // Sequence numbers preserved in emission order.
        for (i, event) in recorded.iter().enumerate() {
            assert_eq!(event.session_seq(), 100 + i as u64);
        }
    }

    #[test]
    fn every_event_variant_translates_to_a_valid_session_notification() {
        // Smoke: every PRD §16 variant produces a non-empty markdown line
        // and a valid SessionNotification. Catches regressions where a
        // future field rename breaks the translator.
        let sequence = sample_happy_path_sequence(1);
        for event in &sequence {
            let notification = translate_vro_event_to_acp("sess-x", event)
                .unwrap_or_else(|| panic!("{:?} must translate", event.event_name()));
            let value = serde_json::to_value(&notification).unwrap();
            assert_eq!(
                value
                    .pointer("/update/sessionUpdate")
                    .and_then(|v| v.as_str()),
                Some("agent_message_chunk")
            );
        }
    }

    #[test]
    fn sink_is_object_safe_via_arc_dyn() {
        // The orchestrator will store the sink as `Arc<dyn VroEventSink>`.
        // Verify the trait is object-safe.
        let sink: Arc<dyn VroEventSink> = Arc::new(RecordingVroEventSink::new());
        sink.emit(VroEvent::Completed {
            session_seq: 1,
            status: "succeeded".into(),
            summary: "done".into(),
        })
        .expect("emit must succeed");
    }

    // Silence unused-import warning if SessionUpdate is not directly referenced.
    #[allow(dead_code)]
    fn _session_update_marker(_: SessionUpdate) {
        // SessionUpdate is re-exported for the translator; ensure it stays
        // imported even if no test names it directly.
    }
}
