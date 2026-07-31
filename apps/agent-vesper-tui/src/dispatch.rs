//! Event-loop dispatch — the pure bridge between the command registry, the
//! Plan Mode state machine, the superpower override store, and the transcript.
//!
//! This module owns **no** terminal I/O. The production binary's event loop
//! owns the crossterm input buffer, the raw-mode lifecycle, and the
//! [`ratatui`] frame; it hands each parsed [`CommandIntent`] to [`dispatch`],
//! which mutates a [`SessionState`] in place and reports whether to continue
//! or quit. Keeping the dispatch logic here lets the full Plan Mode lifecycle
//! (`NORMAL → PLANNING → REVIEW → EXECUTING`) be exercised by integration
//! tests under a [`crate::ui::StubRenderer`] without ever touching a terminal.
//!
//! ## Driver/Navigator contract
//!
//! The Plan Mode state machine is pure: it owns *transition discipline*, not
//! reasoning. The model produces plan text through the runtime; until that
//! runtime hook exists, the `/review <body>` command drives the
//! `PLANNING → REVIEW` transition as a sanctioned, testable placeholder. Free
//! text typed while a question is pending is routed to
//! [`PlanState::answer`](crate::plan_mode::PlanState::answer) so the driver
//! can refine the plan inline.

use vesper_domain::{BoundedString, ProviderId};
use vesper_provider::SuperpowerValue;

use crate::commands::{CommandIntent, CommandOutcome, CommandRegistry, PlanGesture};
use crate::plan_mode::{PlanPhase, PlanState};
use crate::superpowers::{ProviderSuperpowerSurface, SuperpowerOverrides};

/// Pure, terminal-free session state mutated by [`dispatch`].
///
/// The binary wraps this together with an `input: String` buffer that never
/// crosses the [`dispatch`] boundary; only the parsed [`CommandIntent`] does.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// Plan Mode state machine.
    pub plan: PlanState,
    /// Active superpower overrides, mutated by `/effort`, `/thinking`,
    /// `/model`. Surfaced to the renderer and (in a future stage) handed to
    /// the runtime as part of the next provider request.
    pub overrides: SuperpowerOverrides,
    /// Visible transcript lines, oldest first.
    pub transcript: Vec<String>,
    /// One-line status / error / notice. `None` clears the status line.
    pub status: Option<String>,
    /// A pending session-reasoning update produced by `/thinking <level>`
    /// (ADR 0009). The binary's async event loop drains this into the runtime
    /// `UpdateSessionReasoning` command after each dispatch; `None` means no
    /// update is pending.
    pub pending_reasoning: Option<BoundedString<128>>,
}

impl SessionState {
    /// Creates an empty session (NORMAL phase, no overrides, no transcript).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current Plan Mode phase — convenience pass-through for the renderer.
    #[must_use]
    pub fn phase(&self) -> PlanPhase {
        self.plan.phase()
    }
}

/// What [`dispatch`] decided after applying one input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// State was mutated in place; keep running the loop.
    Continue,
    /// The user requested to quit (`/quit`, `/exit`).
    Quit,
}

/// Resolves and applies one parsed input line.
///
/// The active provider and its superpower surface come from the composition
/// boundary; superpower commands are resolved against `surface.descriptors()`
/// filtered to `provider_id`. Returns [`DispatchOutcome::Quit`] when the user
/// typed `/quit`; otherwise mutates `state` in place and returns
/// [`DispatchOutcome::Continue`].
#[must_use = "the caller must act on Quit"]
pub fn dispatch(
    intent: &CommandIntent,
    registry: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    provider_id: &ProviderId,
    state: &mut SessionState,
) -> DispatchOutcome {
    let outcome = registry.resolve(intent, &state.plan, provider_id, surface.descriptors());
    match outcome {
        CommandOutcome::Quit => DispatchOutcome::Quit,
        other => {
            apply_outcome(other, surface, state);
            DispatchOutcome::Continue
        }
    }
}

/// Applies a resolved [`CommandOutcome`] to `state`.
///
/// Pure: no I/O, no async, no terminal. The Plan Mode transitions, override
/// store, transcript, and status line are all updated here so the loop body in
/// the binary stays a thin key→intent→dispatch shim.
fn apply_outcome(outcome: CommandOutcome, surface: &ProviderSuperpowerSurface, state: &mut SessionState) {
    let SessionState {
        plan,
        overrides,
        transcript,
        status,
        pending_reasoning,
    } = state;
    match outcome {
        CommandOutcome::Error(message) => {
            *status = Some(message);
        }
        CommandOutcome::Prompt(text) => {
            // During PLANNING the driver answers the oldest pending question
            // inline. Outside PLANNING (or when nothing is pending) the text
            // is a normal free-text utterance routed to the runtime.
            if plan.phase() == PlanPhase::Planning && !plan.pending_questions().is_empty() {
                match plan.answer(&text) {
                    Ok(_) => *status = Some("Answer recorded; interrogation continues.".into()),
                    Err(error) => *status = Some(error.to_string()),
                }
            } else {
                transcript.push(format!("user: {text}"));
                *status = None;
            }
        }
        CommandOutcome::Plan { prd } => match plan.start(&prd) {
            Ok(_) => {
                transcript.push(format!("plan: entered PLANNING ({} bytes)", prd.len()));
                *status = Some("Plan Mode active — answer questions or /review the plan.".into());
            }
            Err(error) => *status = Some(error.to_string()),
        },
        CommandOutcome::FinalizePlan { body } => match plan.finalize(&body) {
            Ok(_) => {
                transcript.push(format!("plan: REVIEW ({} bytes)", body.len()));
                *status =
                    Some("Plan under review — /approve to execute, /cancel to abort.".into());
            }
            Err(error) => *status = Some(error.to_string()),
        },
        CommandOutcome::PlanGesture(gesture) => match gesture {
            PlanGesture::Approve => match plan.approve() {
                Ok(_) => *status = Some("Plan approved — EXECUTING.".into()),
                Err(error) => *status = Some(error.to_string()),
            },
            PlanGesture::Cancel => {
                plan.cancel();
                *status = Some("Plan cancelled.".into());
            }
        },
        CommandOutcome::Superpower {
            descriptor, value, ..
        } => {
            // Persist the override so it surfaces in the renderer.
            overrides.set(descriptor.id.as_str(), value.clone());

            // ADR 0009: when the resolved superpower is the reconciled
            // `zai:reasoning` dial, translate the value into the runtime
            // reasoning-mode label the binary will push through the
            // `UpdateSessionReasoning` command. The mapping lives in the GLM
            // crate (the TUI is a GLM composition boundary); other providers
            // would supply their own mapper when registered.
            if descriptor.id.as_str() == "zai:reasoning" {
                match vesper_provider_glm::reasoning_mode_for_superpower(&value) {
                    Ok(mode) => *pending_reasoning = Some(mode),
                    Err(error) => {
                        *status = Some(format!("invalid reasoning value: {error}"));
                        return;
                    }
                }
            }

            transcript.push(format!(
                "superpower: {} set to {}",
                descriptor.display_name.as_str(),
                format_superpower_value(&value)
            ));
            // Surface the active override set so the driver sees the layer
            // was updated; the renderer also shows every active override.
            let count = surface
                .descriptors()
                .iter()
                .filter(|descriptor| overrides.get(descriptor.id.as_str(), None).is_some())
                .count();
            *status = Some(format!("{count} active superpower override(s)."));
        }
        CommandOutcome::Help(text) => transcript.push(text),
        CommandOutcome::Quit => {}
    }
}

/// Formats a [`SuperpowerValue`] for transcript/status display.
fn format_superpower_value(value: &SuperpowerValue) -> String {
    match value {
        SuperpowerValue::Choice { value } => value.as_str().to_string(),
        SuperpowerValue::Flag { value } => value.to_string(),
        SuperpowerValue::Number { value } => value.to_string(),
    }
}

#[cfg(test)]
mod integration_tests {
    //! End-to-end Plan Mode lifecycle through the dispatch surface.
    //!
    //! These compose the command registry, the Plan Mode state machine, the
    //! superpower surface, the override store, and the [`StubRenderer`] into a
    //! simulated loop — proving the wiring the binary's event loop relies on
    //! without ever touching a real terminal or crossterm.

    use super::*;
    use crate::commands::CommandIntent;
    use crate::plan_mode::PlanPhase;
    use crate::ui::{StubRenderer, TerminalRenderer, ViewModel};
    use vesper_domain::{BoundedString, ProviderId};
    use vesper_provider::{SuperpowerDescriptor, SuperpowerKind, SuperpowerScope, SuperpowerValue};

    fn provider() -> ProviderId {
        ProviderId::new("test").unwrap()
    }

    fn reasoning_descriptor() -> SuperpowerDescriptor {
        // Mirrors the reconciled GLM `zai:reasoning` dial (ADR 0009).
        SuperpowerDescriptor {
            id: BoundedString::new("zai:reasoning").unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new("Thinking").unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("enabled").unwrap(),
            },
            allowed_values: ["disabled", "enabled", "high", "max"]
                .iter()
                .copied()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(raw).unwrap(),
                })
                .collect(),
            command_alias: Some(BoundedString::new("thinking").unwrap()),
            help: Some(BoundedString::new("Session reasoning depth.").unwrap()),
        }
    }

    fn surface() -> ProviderSuperpowerSurface {
        ProviderSuperpowerSurface::new(provider(), vec![reasoning_descriptor()])
    }

    fn registry() -> CommandRegistry {
        CommandRegistry::stage_11b()
    }

    /// Steps a `SessionState` through one input line, mirroring what the
    /// binary's event loop does on `KeyCode::Enter`.
    fn step(
        state: &mut SessionState,
        registry: &CommandRegistry,
        surface: &ProviderSuperpowerSurface,
        raw: &str,
    ) -> DispatchOutcome {
        let intent = CommandIntent::parse(raw);
        dispatch(&intent, registry, surface, &provider(), state)
    }

    #[test]
    fn plan_command_drives_normal_into_planning() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        assert_eq!(state.phase(), PlanPhase::Normal);
        let outcome = step(&mut state, &registry, &surface, "/plan ship the matrix");
        assert_eq!(outcome, DispatchOutcome::Continue);
        assert_eq!(state.phase(), PlanPhase::Planning);
        assert!(state.plan.prd().is_some());
        assert!(state.transcript.iter().any(|line| line.contains("PLANNING")));
    }

    #[test]
    fn plan_then_review_drives_into_review_and_waits() {
        // The directive's core verification: /plan … /review … must land in
        // REVIEW and the next step must be /approve (or /cancel).
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/plan build a REST gateway");
        assert_eq!(state.phase(), PlanPhase::Planning);

        let outcome = step(
            &mut state,
            &registry,
            &surface,
            "/review 1. scaffold\n2. routes\n3. tests",
        );
        assert_eq!(outcome, DispatchOutcome::Continue);
        assert_eq!(state.phase(), PlanPhase::Review);
        // REVIEW must surface a confirmation prompt in the status line.
        assert!(state
            .status
            .as_ref()
            .is_some_and(|status| status.contains("/approve")));
        // The plan body must be retained so the driver can review it.
        assert!(state.plan.plan().is_some());
    }

    #[test]
    fn full_lifecycle_normal_planning_review_executing_normal() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/plan PRD");
        assert_eq!(state.phase(), PlanPhase::Planning);
        step(&mut state, &registry, &surface, "/review the plan body");
        assert_eq!(state.phase(), PlanPhase::Review);
        step(&mut state, &registry, &surface, "/approve");
        assert_eq!(state.phase(), PlanPhase::Executing);
        // EXECUTING → NORMAL via complete() (in a real run the runtime drives
        // this; here we exercise the state machine directly to close the loop).
        let transition = state.plan.complete().unwrap();
        assert_eq!(transition, crate::plan_mode::PlanTransition::Entered(PlanPhase::Normal));
        assert_eq!(state.phase(), PlanPhase::Normal);
    }

    #[test]
    fn cancel_aborts_an_in_flight_plan() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/plan PRD");
        step(&mut state, &registry, &surface, "/review body");
        assert_eq!(state.phase(), PlanPhase::Review);

        step(&mut state, &registry, &surface, "/cancel");
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert!(state.plan.plan().is_none());
    }

    #[test]
    fn thinking_command_sets_a_pending_reasoning_update() {
        // ADR 0009 / Tier A: `/thinking max` must (a) persist into the local
        // override layer for rendering and (b) produce a pending reasoning
        // update the binary drains into the runtime `UpdateSessionReasoning`
        // command. The runtime-side threading is proven in
        // `vesper-runtime/tests/runtime.rs`.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        assert!(state.overrides.is_empty());
        assert!(state.pending_reasoning.is_none());

        step(&mut state, &registry, &surface, "/thinking max");

        // Local override persisted for the renderer.
        assert_eq!(state.overrides.len(), 1);
        assert!(state
            .overrides
            .get("zai:reasoning", None)
            .is_some_and(|value| matches!(
                value,
                SuperpowerValue::Choice { ref value } if value.as_str() == "max"
            )));
        // Pending runtime update carries the oracle-faithful mode label.
        assert_eq!(
            state.pending_reasoning.as_ref().map(|mode| mode.as_str()),
            Some("max")
        );
    }

    #[test]
    fn thinking_command_rejects_invented_modes() {
        // ADR 0009: `low`/`medium` are not in the oracle scale. Defense in
        // depth: the command registry rejects them at the descriptor's
        // `allowed_values` gate before the GLM mapper runs, so no pending
        // reasoning update is ever produced. (The mapper's own rejection path
        // is covered in `vesper-provider-glm`.)
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/thinking medium");
        assert!(state.pending_reasoning.is_none(), "no runtime update for invented modes");
        assert!(
            state.status.as_ref().is_some_and(|status| !status.is_empty()),
            "an invented mode must surface an error status"
        );

        // The oracle-faithful modes do produce an update.
        step(&mut state, &registry, &surface, "/thinking high");
        assert_eq!(
            state.pending_reasoning.as_ref().map(|mode| mode.as_str()),
            Some("high")
        );
    }

    #[test]
    fn free_text_answers_a_pending_question_during_planning() {
        // Directive 2: the driver can type answers directly to refine the plan.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/plan PRD");
        // Simulate the model raising one interrogation question.
        state.plan.ask("Which framework?").unwrap();
        assert_eq!(state.plan.pending_questions().len(), 1);

        // The driver answers inline with free text.
        step(&mut state, &registry, &surface, "axum");
        assert!(state.plan.pending_questions().is_empty());
        assert!(state
            .status
            .as_ref()
            .is_some_and(|status| status.contains("Answer recorded")));
    }

    #[test]
    fn free_text_outside_planning_is_a_normal_prompt() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "hello world");
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert_eq!(state.transcript, vec!["user: hello world".to_string()]);
        assert!(state.status.is_none());
    }

    #[test]
    fn quit_short_circuits_dispatch_without_mutating_state() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/plan PRD");
        let snapshot = state.clone();

        let outcome = step(&mut state, &registry, &surface, "/quit");
        assert_eq!(outcome, DispatchOutcome::Quit);
        // Quit does not touch the plan/override/transcript state.
        assert_eq!(state.plan, snapshot.plan);
        assert_eq!(state.overrides, snapshot.overrides);
        assert_eq!(state.transcript, snapshot.transcript);
    }

    #[test]
    fn stub_renderer_reflects_each_phase_banner() {
        // Directive 3: each phase must render with a clear visual indicator.
        // The StubRenderer records the ViewModel the binary would draw.
        let registry = registry();
        let surface = surface();
        let mut renderer = StubRenderer::new();
        let mut state = SessionState::new();

        let render = |state: &SessionState, renderer: &mut StubRenderer| {
            let model = ViewModel {
                plan: state.plan.clone(),
                superpowers: Some(surface.clone()),
                overrides: state.overrides.clone(),
                transcript: state.transcript.clone(),
                input: String::new(),
                status: state.status.clone(),
            };
            renderer.render(&model);
        };

        render(&state, &mut renderer);
        assert_eq!(
            renderer
                .last_model
                .as_ref()
                .expect("model recorded")
                .plan
                .phase(),
            PlanPhase::Normal
        );

        step(&mut state, &registry, &surface, "/plan PRD");
        render(&state, &mut renderer);
        assert_eq!(
            renderer.last_model.as_ref().unwrap().plan.phase(),
            PlanPhase::Planning
        );

        step(&mut state, &registry, &surface, "/review body");
        render(&state, &mut renderer);
        assert_eq!(
            renderer.last_model.as_ref().unwrap().plan.phase(),
            PlanPhase::Review
        );

        step(&mut state, &registry, &surface, "/approve");
        render(&state, &mut renderer);
        assert_eq!(
            renderer.last_model.as_ref().unwrap().plan.phase(),
            PlanPhase::Executing
        );
    }

    #[test]
    fn empty_input_surfaces_an_error_in_status() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "   ");
        assert!(state.status.as_ref().is_some_and(|status| !status.is_empty()));
    }
}
