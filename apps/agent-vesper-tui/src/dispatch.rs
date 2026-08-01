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
//! reasoning. Under ADR 0010 (Tier C Phase 5) the model drives the
//! `PLANNING → REVIEW` transition by emitting the `update_plan` tool; the
//! agent loop surfaces the rendered plan and [`apply_model_plan`] finalizes
//! it for review. The human-authored `/review <body>` placeholder is retired.
//! Free text typed while a question is pending is routed to
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
    /// Phase 7 (ADR 0010): a workflow command (`/security-review`, `/smart`,
    /// `/release`, `/insights`, `/diff`) built a prompt that should drive the
    /// next background `AgentLoop` turn. The binary drains this after dispatch
    /// alongside free-text prompts; `None` means no workflow prompt is
    /// pending. Keeping the prompt here (rather than returning it from
    /// `dispatch`) preserves the existing `dispatch -> DispatchOutcome`
    /// signature and matches the `pending_reasoning` drain pattern.
    pub pending_prompt: Option<String>,
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

/// Phase 5 bridge (ADR 0010): drives `PLANNING → REVIEW` from a **model-
/// generated** plan body.
///
/// The agent loop's `update_plan` executor writes `.agent/plan.md` and the loop
/// surfaces the rendered plan in `AgentTurnOutcome::plan`. The TUI's event loop
/// calls this with that body so the human reviews the model-authored plan
/// (retiring the human-authored `/review` placeholder). Returns the Plan Mode
/// transition so the caller can render it.
pub fn apply_model_plan(state: &mut SessionState, plan_body: &str) -> crate::PlanTransition {
    let SessionState {
        plan,
        transcript,
        status,
        ..
    } = state;
    match plan.finalize(plan_body) {
        Ok(transition) => {
            transcript.push(format!(
                "plan: REVIEW (model plan, {} bytes)",
                plan_body.len()
            ));
            *status = Some("Plan under review — /approve to execute, /cancel to abort.".into());
            transition
        }
        Err(error) => {
            *status = Some(error.to_string());
            crate::PlanTransition::Notice(
                vesper_domain::SafeMessage::new("plan could not be finalized")
                    .expect("static notice is bounded"),
            )
        }
    }
}

/// Applies a resolved [`CommandOutcome`] to `state`.
///
/// Pure: no I/O, no async, no terminal. The Plan Mode transitions, override
/// store, transcript, and status line are all updated here so the loop body in
/// the binary stays a thin key→intent→dispatch shim.
fn apply_outcome(
    outcome: CommandOutcome,
    surface: &ProviderSuperpowerSurface,
    state: &mut SessionState,
) {
    let SessionState {
        plan,
        overrides,
        transcript,
        status,
        pending_reasoning,
        pending_prompt,
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
                *status = Some("Plan Mode active — the model plans via update_plan.".into());
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
        CommandOutcome::Version => {
            transcript.push(format!("agent-vesper {}", env!("CARGO_PKG_VERSION")));
            *status = Some(format!("v{}", env!("CARGO_PKG_VERSION")));
        }
        CommandOutcome::ClearView => {
            transcript.clear();
            *status = Some("Transcript cleared.".into());
        }

        // === Phase 7 (ADR 0010) — context mutations ===
        CommandOutcome::ClearPlan => {
            plan.cancel();
            transcript.push("plan: cleared (back to NORMAL).".into());
            *status = Some("Plan cleared.".into());
        }
        CommandOutcome::Compact { keep } => {
            let dropped = transcript.len().saturating_sub(keep);
            if keep == 0 {
                transcript.clear();
            } else if transcript.len() > keep {
                let drain_from = transcript.len() - keep;
                transcript.drain(0..drain_from);
            }
            transcript.push(format!("compact: dropped {dropped} older line(s)."));
            *status = Some(format!("Compacted — {keep} recent line(s) kept."));
        }

        // === Phase 7 (ADR 0010) — context views ===
        // Each view inspects the live SessionState and pushes a one-line
        // summary to the transcript (the TUI's "display surface"). The
        // oracle's richer dashboards depend on subsystems Vesper does not
        // surface yet (token counters, live quota, queue state); the TUI
        // surfaces what it actually has.
        CommandOutcome::ContextView(view_kind) => {
            let line = render_context_view(view_kind, plan, overrides, transcript);
            transcript.push(line);
            *status = None;
        }

        // === Phase 7 (ADR 0010) — workflow prompts ===
        // Stash the constructed prompt on the session; the binary drains it
        // after dispatch and spawns the AgentLoop. The display text lands in
        // the transcript so the driver sees what was sent.
        CommandOutcome::Workflow { display, prompt } => {
            transcript.push(format!("workflow: {display}"));
            *pending_prompt = Some(prompt);
            *status = Some("WORKING... (workflow agent turn)".into());
        }

        // === Phase 7 (ADR 0010) — deferred subsystem commands ===
        CommandOutcome::Deferred { command, reason } => {
            transcript.push(format!("/{command}: deferred — {reason}"));
            *status = Some(format!("/{command} is deferred: {reason}"));
        }

        CommandOutcome::Quit => {}
    }
}

/// Renders one [`ViewKind`] line for the transcript. Pure: reads state only.
fn render_context_view(
    view_kind: crate::commands::ViewKind,
    plan: &PlanState,
    overrides: &SuperpowerOverrides,
    transcript: &[String],
) -> String {
    use crate::commands::ViewKind;
    let phase = plan.phase();
    let line_count = transcript.len();
    let override_count = overrides.len();
    match view_kind {
        ViewKind::Recap => {
            let phase_label = phase_label(phase);
            format!(
                "recap: {phase_label}, {line_count} transcript line(s), {override_count} active override(s)."
            )
        }
        ViewKind::Context => format!(
            "context: {line_count} transcript line(s); Phase={phase_label}. \
             (Token-count view is deferred — needs runtime token accounting.)",
            phase_label = phase_label(phase)
        ),
        ViewKind::Status => format!(
            "status: Phase={phase_label}, transcript={line_count} lines, overrides={override_count}.",
            phase_label = phase_label(phase)
        ),
        ViewKind::Tasks => format!(
            "tasks: dashboard — Phase={phase_label}, {line_count} transcript line(s). \
             (Queue / token / model views need runtime integration.)",
            phase_label = phase_label(phase)
        ),
        ViewKind::MaxIterations => {
            // The cap lives in vesper_agent::DEFAULT_MAX_TOOL_ITERATIONS (50).
            // The TUI surfaces it as a fixed value; the oracle's `/max-iterations`
            // also lets the driver SET it, which needs runtime integration.
            "max-iterations: per-turn tool-call cap is 50 (DEFAULT_MAX_TOOL_ITERATIONS). \
             Setting a per-session cap is deferred — needs runtime integration."
                .to_string()
        }
        ViewKind::Usage => {
            "usage: live quota / API-plan view is deferred — needs provider-quota integration."
                .to_string()
        }
    }
}

/// Lower-cased phase label for view rendering.
fn phase_label(phase: PlanPhase) -> &'static str {
    match phase {
        PlanPhase::Normal => "NORMAL",
        PlanPhase::Planning => "PLANNING",
        PlanPhase::Review => "REVIEW",
        PlanPhase::Executing => "EXECUTING",
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
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("PLANNING"))
        );
    }

    #[test]
    fn plan_then_review_drives_into_review_and_waits() {
        // ADR 0010 Phase 5: /plan … then a model-generated plan (via
        // `update_plan`) lands in REVIEW; the next step must be /approve.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(
            &mut state,
            &registry,
            &surface,
            "/plan build a REST gateway",
        );
        assert_eq!(state.phase(), PlanPhase::Planning);

        // The agent loop's update_plan surfaces the plan here (Phase 5 bridge).
        let _ = apply_model_plan(&mut state, "1. scaffold\n2. routes\n3. tests");
        assert_eq!(state.phase(), PlanPhase::Review);
        // REVIEW must surface a confirmation prompt in the status line.
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("/approve"))
        );
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
        let _ = apply_model_plan(&mut state, "the plan body");
        assert_eq!(state.phase(), PlanPhase::Review);
        step(&mut state, &registry, &surface, "/approve");
        assert_eq!(state.phase(), PlanPhase::Executing);
        // EXECUTING → NORMAL via complete() (in a real run the runtime drives
        // this; here we exercise the state machine directly to close the loop).
        let transition = state.plan.complete().unwrap();
        assert_eq!(
            transition,
            crate::plan_mode::PlanTransition::Entered(PlanPhase::Normal)
        );
        assert_eq!(state.phase(), PlanPhase::Normal);
    }

    #[test]
    fn cancel_aborts_an_in_flight_plan() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/plan PRD");
        let _ = apply_model_plan(&mut state, "body");
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
        assert!(
            state
                .overrides
                .get("zai:reasoning", None)
                .is_some_and(|value| matches!(
                    value,
                    SuperpowerValue::Choice { ref value } if value.as_str() == "max"
                ))
        );
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
        assert!(
            state.pending_reasoning.is_none(),
            "no runtime update for invented modes"
        );
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| !status.is_empty()),
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
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("Answer recorded"))
        );
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

        let _ = apply_model_plan(&mut state, "body");
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
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| !status.is_empty())
        );
    }

    #[test]
    fn version_command_surfaces_the_workspace_version() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/version");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("agent-vesper")),
            "version must surface the agent identity"
        );
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains(&env!("CARGO_PKG_VERSION").to_string()))
        );
    }

    #[test]
    fn clear_view_command_empties_the_transcript() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Seed some transcript + a status.
        step(&mut state, &registry, &surface, "hello");
        assert!(!state.transcript.is_empty());

        step(&mut state, &registry, &surface, "/clear-view");
        assert!(state.transcript.is_empty(), "transcript must be cleared");
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("cleared"))
        );
    }

    // ===================================================================
    // Phase 7 (ADR 0010) — full-surface dispatch integration tests.
    // ===================================================================

    #[test]
    fn phase7_clear_plan_resets_plan_mode_to_normal() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Enter PLANNING, then clear-plan should drop back to NORMAL.
        step(&mut state, &registry, &surface, "/plan ship the matrix");
        assert_eq!(state.phase(), PlanPhase::Planning);
        step(&mut state, &registry, &surface, "/clear-plan");
        assert_eq!(state.phase(), PlanPhase::Normal);
        assert!(state.pending_prompt.is_none());
    }

    #[test]
    fn phase7_compact_keeps_only_the_last_n_lines() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Seed 5 transcript lines.
        for n in 1..=5 {
            step(&mut state, &registry, &surface, &format!("line {n}"));
        }
        assert_eq!(state.transcript.len(), 5);
        // /compact 2 keeps the last 2 user lines.
        step(&mut state, &registry, &surface, "/compact 2");
        // The compact itself pushes a "dropped N lines" notice, so the final
        // length is 2 (kept) + 1 (notice) = 3.
        assert!(
            state.transcript.len() <= 3,
            "compact must drop older lines; got {}",
            state.transcript.len()
        );
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("compact") && line.contains("dropped")),
            "compact must push a dropped-lines notice"
        );
    }

    #[test]
    fn phase7_context_views_push_a_summary_line() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        // Seed one line so the recap has something to summarize.
        step(&mut state, &registry, &surface, "hello");
        step(&mut state, &registry, &surface, "/recap");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.starts_with("recap:")),
            "/recap must push a recap line"
        );

        step(&mut state, &registry, &surface, "/status");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.starts_with("status:")),
            "/status must push a status line"
        );

        step(&mut state, &registry, &surface, "/max-iterations");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.starts_with("max-iterations:")),
            "/max-iterations must push its line"
        );
    }

    #[test]
    fn phase7_workflow_command_stashes_a_pending_prompt() {
        // The decisive Phase 7 contract for workflow commands: dispatch
        // must stash the constructed prompt on SessionState.pending_prompt
        // so the binary can drain it into a background AgentLoop turn.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        assert!(state.pending_prompt.is_none());
        step(&mut state, &registry, &surface, "/security-review");
        let prompt = state
            .pending_prompt
            .as_ref()
            .expect("/security-review must stash a pending prompt");
        assert!(
            prompt.to_lowercase().contains("security"),
            "the workflow prompt must mention security: {prompt}"
        );
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("workflow") && line.contains("security-review")),
            "/security-review must push a workflow display line"
        );
    }

    #[test]
    fn phase7_smart_pr_stashes_the_pr_prompt() {
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/smart pr");
        let prompt = state
            .pending_prompt
            .as_ref()
            .expect("/smart pr must stash a pending prompt");
        assert!(
            prompt.contains("gh pr create"),
            "/smart pr must expand to a gh-pr-create prompt: {prompt}"
        );
    }

    #[test]
    fn phase7_deferred_command_pushes_a_clear_warning() {
        // Deferred commands must NOT silently drop; they push a clear, named
        // warning so the driver understands the command is recognized but
        // its subsystem is not built yet.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "/mobile");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("/mobile") && line.contains("deferred")),
            "/mobile must push a deferred notice"
        );
        assert!(
            state
                .status
                .as_ref()
                .is_some_and(|status| status.contains("deferred")),
            "/mobile status must mention deferred"
        );
        // A deferred command must NOT stash a pending prompt.
        assert!(
            state.pending_prompt.is_none(),
            "/mobile must not trigger an agent turn"
        );
    }

    #[test]
    fn phase7_free_text_still_works_alongside_workflow_commands() {
        // Regression: free-text prompts must still flow through dispatch and
        // land in the transcript. The Phase 7 workflow path is additive.
        let registry = registry();
        let surface = surface();
        let mut state = SessionState::new();

        step(&mut state, &registry, &surface, "hello agent");
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("user: hello agent")),
            "free text must still hit the transcript"
        );
        // Free text does not stash a workflow prompt (the binary treats it
        // as a free-text prompt directly).
        assert!(state.pending_prompt.is_none());
    }
}
