//! Plan Mode state machine (Stage 11b).
//!
//! Mirrors the canonical Plan Mode contract from the Python oracle at
//! `/home/alex/Projects/Native GLM-5.2 Provider/glm_acp/agent.py` (`PLAN_MODE_PROMPT`):
//!
//! 1. **Reconnaissance** — Read the user's PRD, identify technologies, and
//!    explore the existing codebase.
//! 2. **Interrogation** — Compare the PRD against research, identify gaps,
//!    and **pause** to ask the user two to four highly specific questions.
//!    Do not proceed until the user answers.
//! 3. **Master Artifact** — Generate the plan, write it to `.agent/plan.md`,
//!    and ask the user to `approve` or edit the file directly.
//! 4. **Execution** — Once the user approves, execute the plan step by step.
//!
//! The Rust runtime does not implement Plan Mode reasoning itself (the runtime
//! stays provider-neutral and free of any agent loop). This state machine owns
//! the *transition discipline*: which phase the session is in, what each phase
//! permits, and which user gestures drive transitions. The reasoning text is
//! produced by the model through the runtime and rendered by the TUI.
//!
//! The state machine is **pure**: no I/O, no async, no global state. Every
//! transition returns a value; the surrounding event loop applies it.

use std::fmt;

use vesper_domain::SafeMessage;

/// Maximum bytes of PRD text retained in the planning record.
const MAX_PRD_BYTES: usize = 4096;

/// One Plan Mode phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanPhase {
    /// Normal interactive session; no plan in flight.
    #[default]
    Normal,
    /// Reconnaissance + interrogation in progress; the runtime is read-only.
    Planning,
    /// Plan written; awaiting `approve` or edit.
    Review,
    /// Plan approved; execution in progress.
    Executing,
}

impl PlanPhase {
    /// Human-readable label shown by the TUI status line.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Planning => "PLANNING",
            Self::Review => "REVIEW",
            Self::Executing => "EXECUTING",
        }
    }

    /// Whether the runtime is restricted to read-only tooling in this phase.
    #[must_use]
    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::Planning | Self::Review)
    }
}

impl fmt::Display for PlanPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// One interrogated user question waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingQuestion {
    /// The question text (bounded by `SafeMessage`).
    pub text: SafeMessage,
}

impl PendingQuestion {
    /// Wraps a question string in a bounded `SafeMessage`.
    pub fn new(text: &str) -> Result<Self, PlanModeError> {
        Ok(Self {
            text: SafeMessage::new(text).map_err(PlanModeError::message_boundary)?,
        })
    }
}

/// Why a transition was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanModeError {
    /// `/plan` was used while another plan is already in flight.
    #[error("a plan is already in flight in phase {0}")]
    AlreadyPlanning(PlanPhase),
    /// A phase-specific action was attempted from the wrong phase.
    #[error("action not permitted in phase {0}")]
    WrongPhase(PlanPhase),
    /// A question or PRD exceeded the bounded message limit.
    #[error("input exceeded the bounded message limit: {0}")]
    MessageBoundary(String),
    /// No plan was approved yet.
    #[error("no approved plan to execute")]
    NoApprovedPlan,
    /// A pending question must be answered before continuing.
    #[error("a pending interrogation question must be answered first")]
    PendingQuestionUnanswered,
}

impl PlanModeError {
    fn message_boundary(error: vesper_domain::BoundedStringError) -> Self {
        Self::MessageBoundary(error.to_string())
    }
}

/// The result of one Plan Mode transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanTransition {
    /// Phase changed; emit the new phase to the UI.
    Entered(PlanPhase),
    /// The runtime must surface a clarifying question to the user and pause.
    Ask(PendingQuestion),
    /// The runtime must record a plan artifact and surface it for review.
    ReviewPlan {
        /// Bounded plan body, ready to persist verbatim under `.agent/plan.md`.
        body: SafeMessage,
    },
    /// The plan was rejected; the session returned to normal.
    Cancelled,
    /// Informational message that did not change the phase.
    Notice(SafeMessage),
}

/// Mutable Plan Mode state owned by the TUI session actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanState {
    phase: PlanPhase,
    /// Original PRD as supplied via `/plan`.
    prd: Option<SafeMessage>,
    /// Open interrogation questions in the order they were raised.
    pending: Vec<PendingQuestion>,
    /// The most recent reviewed plan body.
    plan: Option<SafeMessage>,
}

impl PlanState {
    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> PlanPhase {
        self.phase
    }

    /// PRD supplied for the in-flight plan, if any.
    #[must_use]
    pub fn prd(&self) -> Option<&SafeMessage> {
        self.prd.as_ref()
    }

    /// The plan body under review, if any.
    #[must_use]
    pub fn plan(&self) -> Option<&SafeMessage> {
        self.plan.as_ref()
    }

    /// Currently unanswered interrogation questions.
    #[must_use]
    pub fn pending_questions(&self) -> &[PendingQuestion] {
        &self.pending
    }

    /// Begins a new plan: stores the PRD, enters reconnaissance, and signals
    /// the UI that Plan Mode is active.
    pub fn start(&mut self, prd: &str) -> Result<PlanTransition, PlanModeError> {
        if self.phase != PlanPhase::Normal {
            return Err(PlanModeError::AlreadyPlanning(self.phase));
        }
        let prd_text = prd.trim();
        if prd_text.is_empty() {
            return Err(PlanModeError::MessageBoundary(
                "PRD must not be empty".into(),
            ));
        }
        if prd_text.len() > MAX_PRD_BYTES {
            return Err(PlanModeError::MessageBoundary(format!(
                "PRD exceeded {MAX_PRD_BYTES} bytes"
            )));
        }
        let bounded = SafeMessage::new(prd_text).map_err(PlanModeError::message_boundary)?;
        self.prd = Some(bounded.clone());
        self.phase = PlanPhase::Planning;
        Ok(PlanTransition::Entered(PlanPhase::Planning))
    }

    /// Records a clarifying question the model produced during interrogation.
    /// The runtime must pause until the user supplies an answer.
    pub fn ask(&mut self, question: &str) -> Result<PlanTransition, PlanModeError> {
        if self.phase != PlanPhase::Planning {
            return Err(PlanModeError::WrongPhase(self.phase));
        }
        let pending = PendingQuestion::new(question)?;
        self.pending.push(pending.clone());
        Ok(PlanTransition::Ask(pending))
    }

    /// Accepts a user answer to the oldest open question.
    pub fn answer(&mut self, _answer: &str) -> Result<PlanTransition, PlanModeError> {
        if self.phase != PlanPhase::Planning {
            return Err(PlanModeError::WrongPhase(self.phase));
        }
        if self.pending.is_empty() {
            return Ok(PlanTransition::Notice(
                SafeMessage::new("No pending question to answer.")
                    .map_err(PlanModeError::message_boundary)?,
            ));
        }
        // Drop the oldest answered question; the answer text itself is routed
        // to the model through the normal prompt path by the event loop.
        self.pending.remove(0);
        Ok(PlanTransition::Notice(
            SafeMessage::new("Answer recorded; interrogation continues.")
                .map_err(PlanModeError::message_boundary)?,
        ))
    }

    /// Finalizes the plan and enters REVIEW. Requires every interrogated
    /// question to be answered first.
    pub fn finalize(&mut self, body: &str) -> Result<PlanTransition, PlanModeError> {
        if self.phase != PlanPhase::Planning {
            return Err(PlanModeError::WrongPhase(self.phase));
        }
        if !self.pending.is_empty() {
            return Err(PlanModeError::PendingQuestionUnanswered);
        }
        let bounded = SafeMessage::new(body).map_err(PlanModeError::message_boundary)?;
        self.plan = Some(bounded.clone());
        self.phase = PlanPhase::Review;
        Ok(PlanTransition::ReviewPlan { body: bounded })
    }

    /// Approves the reviewed plan and enters EXECUTING.
    pub fn approve(&mut self) -> Result<PlanTransition, PlanModeError> {
        match self.phase {
            PlanPhase::Review => {
                self.phase = PlanPhase::Executing;
                Ok(PlanTransition::Entered(PlanPhase::Executing))
            }
            PlanPhase::Normal => Err(PlanModeError::NoApprovedPlan),
            other => Err(PlanModeError::WrongPhase(other)),
        }
    }

    /// Cancels any in-flight plan and returns to NORMAL.
    pub fn cancel(&mut self) -> PlanTransition {
        if self.phase == PlanPhase::Normal {
            return PlanTransition::Notice(
                SafeMessage::new("Nothing to cancel.").expect("static notice message is bounded"),
            );
        }
        self.phase = PlanPhase::Normal;
        self.prd = None;
        self.plan = None;
        self.pending.clear();
        PlanTransition::Cancelled
    }

    /// Marks the active execution step done; when the plan is finished the
    /// caller transitions back to NORMAL.
    pub fn complete(&mut self) -> Result<PlanTransition, PlanModeError> {
        if self.phase != PlanPhase::Executing {
            return Err(PlanModeError::WrongPhase(self.phase));
        }
        self.phase = PlanPhase::Normal;
        self.prd = None;
        self.plan = None;
        Ok(PlanTransition::Entered(PlanPhase::Normal))
    }
}

#[cfg(test)]
mod tests {
    //! Plan Mode transition discipline — every legal and illegal move.

    use super::*;

    #[test]
    fn default_phase_is_normal_and_read_write() {
        let state = PlanState::default();
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert!(!PlanPhase::Normal.is_read_only());
        assert!(PlanPhase::Planning.is_read_only());
        assert!(PlanPhase::Review.is_read_only());
        assert!(!PlanPhase::Executing.is_read_only());
    }

    #[test]
    fn start_records_prd_and_enters_planning() {
        let mut state = PlanState::default();
        let transition = state.start("Build a REST gateway.").unwrap();
        assert_eq!(transition, PlanTransition::Entered(PlanPhase::Planning));
        assert_eq!(state.phase(), PlanPhase::Planning);
        assert!(state.prd().is_some());
    }

    #[test]
    fn start_rejects_empty_and_oversized_prd() {
        let mut state = PlanState::default();
        assert!(state.start("   ").is_err());
        let huge = "x".repeat(MAX_PRD_BYTES + 1);
        assert!(state.start(&huge).is_err());
        assert_eq!(state.phase(), PlanPhase::Normal);
    }

    #[test]
    fn start_rejects_a_second_concurrent_plan() {
        let mut state = PlanState::default();
        state.start("First plan").unwrap();
        let err = state.start("Second plan").unwrap_err();
        assert!(matches!(
            err,
            PlanModeError::AlreadyPlanning(PlanPhase::Planning)
        ));
    }

    #[test]
    fn ask_and_answer_pair_drives_interrogation() {
        let mut state = PlanState::default();
        state.start("PRD").unwrap();
        let transition = state.ask("Which framework?").unwrap();
        assert!(matches!(transition, PlanTransition::Ask(_)));
        assert_eq!(state.pending_questions().len(), 1);
        let notice = state.answer("axum").unwrap();
        assert!(matches!(notice, PlanTransition::Notice(_)));
        assert!(state.pending_questions().is_empty());
    }

    #[test]
    fn finalize_requires_all_questions_answered() {
        let mut state = PlanState::default();
        state.start("PRD").unwrap();
        state.ask("Q1").unwrap();
        let err = state.finalize("body").unwrap_err();
        assert_eq!(err, PlanModeError::PendingQuestionUnanswered);
        state.answer("A1").unwrap();
        let transition = state.finalize("Plan body").unwrap();
        assert!(matches!(transition, PlanTransition::ReviewPlan { .. }));
        assert_eq!(state.phase(), PlanPhase::Review);
    }

    #[test]
    fn approve_only_works_in_review() {
        let mut state = PlanState::default();
        assert_eq!(state.approve().unwrap_err(), PlanModeError::NoApprovedPlan);
        state.start("PRD").unwrap();
        state.finalize("body").unwrap();
        let transition = state.approve().unwrap();
        assert_eq!(transition, PlanTransition::Entered(PlanPhase::Executing));
        assert_eq!(state.phase(), PlanPhase::Executing);
    }

    #[test]
    fn cancel_resets_to_normal() {
        let mut state = PlanState::default();
        state.start("PRD").unwrap();
        state.ask("Q1").unwrap();
        let transition = state.cancel();
        assert_eq!(transition, PlanTransition::Cancelled);
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert!(state.prd().is_none());
        assert!(state.pending_questions().is_empty());

        // Cancelling from NORMAL is a no-op notice.
        let again = state.cancel();
        assert!(matches!(again, PlanTransition::Notice(_)));
    }

    #[test]
    fn complete_returns_to_normal() {
        let mut state = PlanState::default();
        state.start("PRD").unwrap();
        state.finalize("body").unwrap();
        state.approve().unwrap();
        let transition = state.complete().unwrap();
        assert_eq!(transition, PlanTransition::Entered(PlanPhase::Normal));
        assert!(state.plan().is_none());
    }

    #[test]
    fn full_four_phase_lifecycle() {
        let mut state = PlanState::default();
        // NORMAL -> PLANNING
        assert_eq!(
            state.start("PRD").unwrap(),
            PlanTransition::Entered(PlanPhase::Planning)
        );
        // interrogation
        assert!(matches!(state.ask("Q1").unwrap(), PlanTransition::Ask(_)));
        assert!(matches!(
            state.answer("A1").unwrap(),
            PlanTransition::Notice(_)
        ));
        // PLANNING -> REVIEW
        assert!(matches!(
            state.finalize("body").unwrap(),
            PlanTransition::ReviewPlan { .. }
        ));
        // REVIEW -> EXECUTING
        assert_eq!(
            state.approve().unwrap(),
            PlanTransition::Entered(PlanPhase::Executing)
        );
        // EXECUTING -> NORMAL
        assert_eq!(
            state.complete().unwrap(),
            PlanTransition::Entered(PlanPhase::Normal)
        );
    }

    #[test]
    fn wrong_phase_actions_are_rejected() {
        let mut state = PlanState::default();
        // ask/finalize require PLANNING.
        assert_eq!(
            state.ask("Q").unwrap_err(),
            PlanModeError::WrongPhase(PlanPhase::Normal)
        );
        assert_eq!(
            state.finalize("body").unwrap_err(),
            PlanModeError::WrongPhase(PlanPhase::Normal)
        );
        // complete requires EXECUTING.
        assert_eq!(
            state.complete().unwrap_err(),
            PlanModeError::WrongPhase(PlanPhase::Normal)
        );
    }
}
