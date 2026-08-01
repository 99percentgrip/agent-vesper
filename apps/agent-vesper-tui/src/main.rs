#![forbid(unsafe_code)]
//! `agent-vesper-tui` binary entry point.
//!
//! Stage 11b shipped a minimal interactive loop. Tier C Phase 6 (ADR 0010)
//! completes the parity surface: the binary now drives the full multi-turn
//! [`vesper_agent::AgentLoop`] end-to-end.
//!
//! 1. Select a provider via `AGENT_VESPER_PROVIDER` (default `zai`).
//! 2. Query the runtime registry for that provider's advertised superpowers.
//! 3. Build an `AgentLoop` over the same shared registry that backs the
//!    reasoning-override supervisor.
//! 4. Enter a crossterm/ratatui event loop. Free-text prompts in NORMAL phase
//!    spawn the agent loop in a background tokio task; the event loop stays
//!    non-blocking (it `try_recv`s the result each iteration) so the UI
//!    remains responsive while the model thinks / runs tools.
//! 5. When the loop returns `AgentTurnOutcome::Completed { plan: Some(body),
//!    .. }`, the binary routes the model-authored plan through
//!    [`agent_vesper_tui::dispatch::apply_model_plan`] to drive
//!    `PLANNING → REVIEW`.
//!
//! The architectural surface (Plan Mode, commands, superpowers,
//! TerminalRenderer) lives in the library and is unit-tested there. The
//! binary's stdout stays free of any ACP/JSON-RPC contract — it writes only
//! terminal escapes via crossterm.

use std::io::{self, stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_vesper_tui::{
    CommandIntent, CommandRegistry, DispatchOutcome, PlanPhase, ProviderSuperpowerSurface,
    SessionState, ViewModel, apply_model_plan, dispatch, query_startup_view, render_to_frame,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;
use tracing::{error, warn};
use vesper_agent::{
    AgentLoop, AgentLoopConfig, AgentLoopError, AgentTurnOutcome, DEFAULT_MAX_TOOL_ITERATIONS,
    ToolRegistry,
};
use vesper_domain::{
    BoundedString, CommandId, CommandInitiator, CommandSchemaVersion, ContentPart, ContentText,
    ConversationMessage, CorrelationId, EndpointId, ExtensionMap, HarnessCommand,
    HarnessCommandPayload, MessageId, MessageRole, ModelId, ProviderId, QualifiedModelId, Revision,
    SessionId, SessionOperatingMode, SessionPermissionMode, SystemInstruction, WorkspaceRoot,
};
use vesper_provider::ProviderConfiguration;

/// Default provider identity when `AGENT_VESPER_PROVIDER` is unset.
const DEFAULT_PROVIDER: &str = "zai";

type Backend = CrosstermBackend<io::Stdout>;

#[tokio::main]
async fn main() -> io::Result<()> {
    // Tracing goes to stderr only; stdout is reserved for terminal escapes.
    let _ = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    if let Err(message) = run().await {
        error!("agent-vesper-tui exited with error: {message}");
        return Err(io::Error::other(message));
    }
    Ok(())
}

async fn run() -> Result<(), String> {
    let provider_id = ProviderId::new(provider_name_from_env().as_str())
        .map_err(|error| format!("invalid provider id: {error}"))?;

    let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
    register_default_providers(&registry)
        .await
        .map_err(|error| format!("provider registration failed: {error:?}"))?;

    let startup = query_startup_view(&registry, &provider_id).await;
    let surface = ProviderSuperpowerSurface::new(startup.provider_id.clone(), startup.superpowers);

    // Runtime supervisor owns the live session state. Building it, creating a
    // session, and applying reasoning overrides are all credential-free: the
    // GLM provider is only contacted on an actual prompt dispatch, which the
    // TUI binary does not perform (driving prompts requires live credentials).
    let supervisor = Arc::new(vesper_runtime::RuntimeSupervisor::new(
        Arc::clone(&registry),
        runtime_defaults(&provider_id),
    ));
    // Initialize + create the session the override bridge mutates.
    let runtime_session_id = init_runtime_session(&supervisor)
        .await
        .map_err(|error| format!("runtime session creation failed: {error:?}"))?;

    let registry_commands = CommandRegistry::stage_11b();
    // Phase 6 (ADR 0010): build the multi-turn agent loop over the same
    // shared registry that backs the supervisor. The loop is wrapped in an
    // `Arc` so the spawned background task can own a clone without moving
    // the loop off the composition boundary.
    let agent = Arc::new(
        build_agent_loop(Arc::clone(&registry), &provider_id)
            .map_err(|error| format!("agent loop construction failed: {error}"))?,
    );

    let mut session = TuiSession {
        // Pure dispatch state lives in the library so the full Plan Mode
        // lifecycle is unit-testable; the binary only owns the input buffer
        // and the in-flight agent-turn channel.
        state: SessionState::new(),
        input: String::new(),
        agent_rx: None,
        agent_running: false,
    };

    enter_raw_mode().map_err(|error| format!("failed to enter raw mode: {error}"))?;
    let result = drive_loop(
        &provider_id,
        &registry_commands,
        &surface,
        &mut session,
        &supervisor,
        &runtime_session_id,
        &agent,
    )
    .await;
    let _ = leave_raw_mode();
    result
}

/// Builds provider-neutral runtime defaults seeded from the GLM composition
/// boundary (ADR 0009). No reasoning default: the session override drives it.
fn runtime_defaults(provider_id: &ProviderId) -> vesper_runtime::RuntimeDefaults {
    vesper_runtime::RuntimeDefaults {
        provider_configuration: vesper_provider_glm::GlmFactory::default_configuration(),
        model: QualifiedModelId {
            provider_id: provider_id.clone(),
            model_id: ModelId::new("glm-5.2").expect("static model id"),
        },
        endpoint: EndpointId::new("zai-coding").expect("static endpoint id"),
        system_instructions: Vec::new(),
        reasoning: None,
        sampling: None,
        maximum_output_tokens: None,
    }
}

/// Initializes the runtime and creates one session, returning its identity.
/// Credential-free: neither step dispatches to a provider.
async fn init_runtime_session(
    supervisor: &vesper_runtime::RuntimeSupervisor,
) -> Result<SessionId, vesper_runtime::RuntimeError> {
    let ids = AtomicU64::new(100);
    let next = || ids.fetch_add(1, Ordering::Relaxed);
    let _ = supervisor
        .execute(runtime_command(
            next(),
            HarnessCommandPayload::InitializeRuntime(vesper_domain::RuntimeInitialization {
                client_name: BoundedString::new("agent-vesper-tui").expect("bounded name"),
                workspace_roots: Vec::new(),
                client_capabilities: std::collections::BTreeSet::new(),
                authentication_methods: Vec::new(),
                frontend: None,
            }),
        ))
        .await?;
    let response = supervisor
        .execute(runtime_command(
            next(),
            HarnessCommandPayload::CreateSession {
                workspace_roots: vec![WorkspaceRoot {
                    name: BoundedString::new("workspace").expect("bounded name"),
                    path: BoundedString::new(".").expect("bounded path"),
                    primary: true,
                }],
                requested_session_id: Some(SessionId::new("vesper-tui").expect("bounded id")),
            },
        ))
        .await?;
    let vesper_runtime::RuntimeResponse::Session(snapshot) = response else {
        return Err(vesper_runtime::RuntimeError::UnsupportedCommand);
    };
    Ok(snapshot.session_id)
}

/// Builds a correlated runtime command for the supervisor.
fn runtime_command(number: u64, payload: HarnessCommandPayload) -> HarnessCommand {
    HarnessCommand {
        schema_version: CommandSchemaVersion::CURRENT,
        command_id: CommandId::new(format!("tui-command-{number}")).expect("bounded command id"),
        correlation_id: CorrelationId::new(format!("tui-correlation-{number}"))
            .expect("bounded correlation id"),
        initiator: CommandInitiator::Acp,
        expected_revision: None,
        payload,
    }
}

/// Monotonic sequence for runtime commands issued from the event loop.
fn reasoning_seq() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(2000);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

// `Revision` is re-exported so the public command surface stays complete even
// though the TUI never sets an expected revision.
#[allow(dead_code)]
const _: Option<Revision> = None;

fn provider_name_from_env() -> String {
    std::env::var("AGENT_VESPER_PROVIDER").unwrap_or_else(|_| DEFAULT_PROVIDER.to_string())
}

async fn register_default_providers(
    registry: &vesper_runtime::ProviderRegistry,
) -> Result<(), vesper_runtime::RuntimeError> {
    // GLM factory + synthetic factory, each with its declared superpowers.
    // `register_with_superpowers` consumes the factory and the superpowers
    // surface; since `GlmFactory` is not `Clone`, register two fresh instances
    // so one drives session creation and the other answers superpower queries.
    let glm = vesper_provider_glm::GlmFactory::default();
    let glm_superpowers = vesper_provider_glm::GlmFactory::default();
    registry
        .register_with_superpowers(glm, glm_superpowers)
        .await?;
    let synthetic = vesper_provider_synthetic::SyntheticFactory::default();
    let synthetic_superpowers = vesper_provider_synthetic::SyntheticFactory::default();
    registry
        .register_with_superpowers(synthetic, synthetic_superpowers)
        .await?;
    Ok(())
}

/// Mutable per-session state held across the event loop.
///
/// Wraps the library-owned [`SessionState`] (pure Plan Mode + override +
/// transcript state, fully unit-tested) together with the `input` buffer that
/// never crosses the dispatch boundary. Only the binary owns the terminal; all
/// transition discipline lives in [`agent_vesper_tui::dispatch`].
struct TuiSession {
    /// Pure dispatch state (plan, overrides, transcript, status).
    state: SessionState,
    /// In-progress input line being typed by the driver.
    input: String,
    /// Phase 6 (ADR 0010): receiver for an in-flight agent turn. `Some` while
    /// a `tokio::spawn`-ed `AgentLoop::run_prompt` is running; the receiver
    /// yields exactly one [`AgentEvent`]. The event loop drains it via
    /// `try_recv` each iteration so the UI stays responsive while the model
    /// thinks and tools execute.
    agent_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    /// `true` while an agent turn is in flight — drives the "WORKING..."
    /// status banner. Cleared as soon as the receiver yields (or aborts).
    agent_running: bool,
}

/// What a spawned agent-loop task reports back to the event loop.
///
/// The task sends exactly one of these through the per-turn mpsc channel.
#[derive(Debug)]
enum AgentEvent {
    /// The loop returned a terminal outcome.
    Completed(AgentTurnOutcome),
    /// The provider boundary classified an error.
    Failed(AgentLoopError),
}

async fn drive_loop(
    provider_id: &ProviderId,
    registry_commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    session: &mut TuiSession,
    supervisor: &vesper_runtime::RuntimeSupervisor,
    runtime_session_id: &SessionId,
    agent: &Arc<AgentLoop>,
) -> Result<(), String> {
    let mut terminal = Terminal::new(Backend::new(stdout()))
        .map_err(|error| format!("terminal init failed: {error}"))?;

    loop {
        // Phase 6: drain any completed agent turn BEFORE redrawing so the
        // "WORKING..." banner clears the moment the result lands. The drain
        // is non-blocking (`try_recv`); if the turn is still running we just
        // fall through and render the in-flight banner.
        drain_agent_event(session);

        let model = ViewModel {
            plan: session.state.plan.clone(),
            superpowers: Some(surface.clone()),
            overrides: session.state.overrides.clone(),
            transcript: session.state.transcript.clone(),
            input: session.input.clone(),
            status: session.state.status.clone(),
        };
        if let Err(error) = terminal.draw(|frame| {
            render_to_frame(frame, &model);
        }) {
            return Err(format!("redraw failed: {error}"));
        }

        if !event::poll(std::time::Duration::from_millis(250))
            .map_err(|error| format!("event poll failed: {error}"))?
        {
            continue;
        }
        let event = event::read().map_err(|error| format!("event read failed: {error}"))?;
        let Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = event
        else {
            continue;
        };

        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Char('c') if ctrl => {
                session.state.transcript.push("Interrupted.".into());
                break;
            }
            KeyCode::Char('d') if ctrl => {
                session.state.transcript.push("EOF.".into());
                break;
            }
            KeyCode::Enter => {
                let intent = CommandIntent::parse(&session.input);
                // Capture whether this was a free-text prompt BEFORE dispatch
                // clears the input buffer; Phase 6 needs the text to drive
                // the agent loop after the pure dispatch state mutates.
                let prompt_text = match &intent {
                    CommandIntent::Prompt(text) => Some(text.clone()),
                    _ => None,
                };
                // Single integration point with the pure dispatch surface:
                // resolve the intent and mutate session state in place. The
                // Quit decision short-circuits the loop.
                let outcome = dispatch(
                    &intent,
                    registry_commands,
                    surface,
                    provider_id,
                    &mut session.state,
                );
                if outcome == DispatchOutcome::Quit {
                    session.state.transcript.push("bye.".into());
                    break;
                }
                session.input.clear();
                // ADR 0009 / Tier A: drain any pending reasoning update into
                // the runtime session. `dispatch` stays pure and produces the
                // command intent here; the binary owns the async runtime call.
                if let Some(mode) = session.state.pending_reasoning.take() {
                    let payload = HarnessCommandPayload::UpdateSessionReasoning {
                        session_id: runtime_session_id.clone(),
                        mode: Some(mode.clone()),
                    };
                    match supervisor
                        .execute(runtime_command(reasoning_seq(), payload))
                        .await
                    {
                        Ok(_) => session
                            .state
                            .transcript
                            .push(format!("runtime: session reasoning → {mode}")),
                        Err(error) => {
                            warn!("reasoning override rejected by runtime: {error:?}");
                            session.state.status =
                                Some(format!("reasoning update failed: {error:?}"));
                        }
                    }
                }
                // Phase 6 (ADR 0010): drive the multi-turn agent loop for
                // free-text prompts submitted in NORMAL phase when no turn is
                // already in flight. PLANNING-phase free text is the driver
                // answering an inline question (handled by `dispatch`); we
                // never spawn the loop there. The loop runs in a background
                // tokio task; the result is drained at the top of the next
                // iteration, so the UI keeps redrawing the WORKING banner.
                if let Some(text) = prompt_text
                    && !session.agent_running
                    && session.state.phase() == PlanPhase::Normal
                {
                    spawn_agent_turn(agent, text, session);
                }
            }
            KeyCode::Backspace => {
                session.input.pop();
            }
            KeyCode::Char(ch) => {
                session.input.push(ch);
            }
            KeyCode::Esc if session.state.phase() != PlanPhase::Normal => {
                // Esc cancels any in-flight plan directly through the state
                // machine; the dispatch surface is also reachable via /cancel.
                session.state.plan.cancel();
                session.state.status = Some("Plan cancelled.".into());
            }
            _ => {}
        }
    }
    Ok(())
}

fn enter_raw_mode() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Ok(())
}

fn leave_raw_mode() -> io::Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 6 (ADR 0010): the multi-turn agent-loop bridge.
//
// `build_agent_loop` constructs the loop at composition time. `spawn_agent_turn`
// owns the per-prompt `tokio::spawn`. `drain_agent_event` / `apply_agent_event`
// are the non-blocking result handlers the event loop calls each iteration.
// ---------------------------------------------------------------------------

/// Builds the [`AgentLoop`] over the shared registry, with provider-aware
/// configuration. Mirrors `runtime_defaults`'s composition-boundary
/// convention: GLM (`zai`) gets the GLM factory's default configuration +
/// `glm-5.2`; the synthetic provider gets its deterministic defaults.
///
/// Returns `Err` for an unknown provider id so a misconfigured
/// `AGENT_VESPER_PROVIDER` fails fast at startup instead of mid-prompt.
fn build_agent_loop(
    registry: Arc<vesper_runtime::ProviderRegistry>,
    provider_id: &ProviderId,
) -> Result<AgentLoop, String> {
    let config = build_agent_config(provider_id)?;
    Ok(AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config,
    ))
}

/// Builds the agent-loop configuration for one provider. Pure: no registry,
/// no I/O. Split out so the composition boundary is unit-testable without
/// standing up a real [`vesper_runtime::ProviderRegistry`].
fn build_agent_config(provider_id: &ProviderId) -> Result<AgentLoopConfig, String> {
    Ok(AgentLoopConfig {
        provider_id: provider_id.clone(),
        provider_configuration: provider_configuration_for(provider_id)?,
        model: QualifiedModelId {
            provider_id: provider_id.clone(),
            model_id: model_id_for_provider(provider_id)?,
        },
        // The binary ships no system instructions; the agent loop's tools are
        // advertised through `ToolDefinition`s and the model discovers them.
        system_instructions: Vec::<SystemInstruction>::new(),
        // Same single primary root the runtime session was initialized with.
        // The executors confine every read/write/run under it.
        workspace_roots: vec![primary_workspace_root()],
        max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
    })
}

/// Resolves the provider's typed configuration at the composition boundary.
fn provider_configuration_for(provider_id: &ProviderId) -> Result<ProviderConfiguration, String> {
    match provider_id.as_str() {
        // The GLM adapter registers under the stable `zai` identity.
        "zai" => Ok(vesper_provider_glm::GlmFactory::default_configuration()),
        // The deterministic in-process reference adapter.
        "vesper-synthetic" => {
            Ok(vesper_provider_synthetic::SyntheticFactory::default_configuration())
        }
        other => Err(format!("unsupported provider id: {other}")),
    }
}

/// Resolves the provider's primary model id at the composition boundary.
fn model_id_for_provider(provider_id: &ProviderId) -> Result<ModelId, String> {
    let id = match provider_id.as_str() {
        "zai" => "glm-5.2",
        "vesper-synthetic" => "synthetic-1",
        other => return Err(format!("unsupported provider id: {other}")),
    };
    ModelId::new(id).map_err(|error| format!("invalid model id {id:?}: {error}"))
}

/// The primary workspace root the agent loop confines every tool under.
///
/// Mirrors the root the runtime session was created with (`init_runtime_session`)
/// so the loop and the supervisor agree on the boundary.
fn primary_workspace_root() -> WorkspaceRoot {
    WorkspaceRoot {
        name: BoundedString::new("workspace").expect("bounded name"),
        path: BoundedString::new(".").expect("bounded path"),
        primary: true,
    }
}

/// Spawns one agent turn in a background tokio task and stores the receiver
/// on `session`. The task owns a clone of the [`AgentLoop`] `Arc` and sends
/// exactly one [`AgentEvent`] through a fresh mpsc channel.
///
/// Drives the "WORKING..." banner until the receiver yields.
fn spawn_agent_turn(agent: &Arc<AgentLoop>, user_text: String, session: &mut TuiSession) {
    let (tx, rx) = mpsc::unbounded_channel::<AgentEvent>();
    let agent = Arc::clone(agent);
    tokio::spawn(async move {
        let user = build_user_message(&user_text);
        let result = agent
            .run_prompt(user, SessionOperatingMode::Code, SessionPermissionMode::Ask)
            .await;
        let event = match result {
            Ok(outcome) => AgentEvent::Completed(outcome),
            Err(error) => AgentEvent::Failed(error),
        };
        // `send` only fails if the receiver was dropped (the binary exited
        // before the turn finished). Discarding the result is safe: there is
        // no one left to observe it.
        let _ = tx.send(event);
    });
    session.agent_rx = Some(rx);
    session.agent_running = true;
    session.state.status = Some("WORKING... (agent loop running)".into());
}

/// Drains a completed agent turn non-blockingly.
///
/// Called at the top of every event-loop iteration. If the receiver is empty
/// the in-flight banner stays up and the loop continues to redraw; if it has
/// yielded, the result is applied and the banner clears. A dropped sender
/// (task panicked or was cancelled) clears the in-flight flag and surfaces a
/// status notice rather than wedging the UI.
fn drain_agent_event(session: &mut TuiSession) {
    let Some(rx) = session.agent_rx.as_mut() else {
        return;
    };
    match rx.try_recv() {
        Ok(event) => {
            session.agent_running = false;
            session.agent_rx = None;
            apply_agent_event(event, &mut session.state);
        }
        Err(mpsc::error::TryRecvError::Empty) => {
            // Still running; the renderer shows "WORKING...".
        }
        Err(mpsc::error::TryRecvError::Disconnected) => {
            session.agent_running = false;
            session.agent_rx = None;
            session
                .state
                .status
                .replace("agent loop task aborted before completion.".into());
            session
                .state
                .transcript
                .push("agent: task aborted (sender dropped).".into());
        }
    }
}

/// Applies a terminal [`AgentEvent`] to the pure dispatch state.
///
/// The model-authored plan, when present, drives `PLANNING → REVIEW` through
/// [`apply_model_plan`]; otherwise the assistant content is appended to the
/// transcript and the status is set to a brief completion notice.
fn apply_agent_event(event: AgentEvent, state: &mut SessionState) {
    match event {
        AgentEvent::Completed(outcome) => match outcome {
            AgentTurnOutcome::Completed {
                assistant_content,
                iterations,
                tool_results,
                plan,
            } => {
                // Surface the assistant's final text — the model's answer.
                for part in &assistant_content {
                    if let ContentPart::Text(text) = part {
                        state.transcript.push(format!("assistant: {text}"));
                    }
                }
                state.transcript.push(format!(
                    "agent: {iterations} turn(s), {} tool result(s)",
                    tool_results.len()
                ));
                // Phase 5/6 bridge (ADR 0010): if the model emitted
                // `update_plan`, drive PLANNING → REVIEW with the model-
                // authored body. The human reviews it via /approve or
                // /cancel; the binary no longer authors the plan.
                if let Some(body) = plan {
                    let _ = apply_model_plan(state, &body);
                } else {
                    state.status = Some("agent turn complete.".into());
                }
            }
            AgentTurnOutcome::MaxIterationsReached { iterations } => {
                state
                    .status
                    .replace(format!("agent hit the {iterations}-iteration safety cap."));
                state.transcript.push(format!(
                    "agent: stopped at the {iterations}-iteration safety cap."
                ));
            }
        },
        AgentEvent::Failed(error) => {
            // Provider errors typically mean missing credentials or a network
            // failure; surface the message rather than wedging the UI.
            let message = error.to_string();
            state.status = Some(format!("agent loop error: {message}"));
            state.transcript.push(format!("agent error: {message}"));
        }
    }
}

/// Builds a user-role [`ConversationMessage`] for one agent turn.
///
/// `MessageId` is bounded; the binary uses a monotonic counter scoped to this
/// process so collisions across prompts are impossible.
fn build_user_message(text: &str) -> ConversationMessage {
    static SEQ: AtomicU64 = AtomicU64::new(3000);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let id = MessageId::new(format!("tui-prompt-{n}"))
        .expect("bounded message id derived from a small monotonic counter");
    let content = vec![ContentPart::Text(ContentText::new(text).unwrap_or_else(
        |_| ContentText::new("[prompt too large]").expect("bounded"),
    ))];
    ConversationMessage {
        id,
        role: MessageRole::User,
        content,
        extensions: ExtensionMap::default(),
    }
}

#[cfg(test)]
mod tests {
    //! Phase 6 (ADR 0010) wiring tests.
    //!
    //! The Plan Mode / dispatch / renderer surface lives in the library and
    //! is unit-tested there. These tests cover the binary-only composition
    //! glue: provider-aware configuration resolution, `AgentLoop`
    //! construction, and the `AgentEvent → SessionState` mapper. They never
    //! touch crossterm or a real terminal.

    use super::*;

    #[test]
    fn provider_configuration_resolves_for_glm_and_synthetic() {
        let zai = ProviderId::new("zai").unwrap();
        let cfg = provider_configuration_for(&zai).expect("zai configuration");
        assert_eq!(cfg.provider_id.as_str(), "zai");

        let synthetic = ProviderId::new("vesper-synthetic").unwrap();
        let cfg = provider_configuration_for(&synthetic).expect("synthetic configuration");
        assert_eq!(cfg.provider_id.as_str(), "vesper-synthetic");
    }

    #[test]
    fn provider_configuration_rejects_unknown_providers() {
        let bogus = ProviderId::new("acme").unwrap();
        assert!(provider_configuration_for(&bogus).is_err());
    }

    #[test]
    fn model_id_resolves_per_provider() {
        let zai = ProviderId::new("zai").unwrap();
        assert_eq!(model_id_for_provider(&zai).unwrap().as_str(), "glm-5.2");

        let synthetic = ProviderId::new("vesper-synthetic").unwrap();
        assert_eq!(
            model_id_for_provider(&synthetic).unwrap().as_str(),
            "synthetic-1"
        );
    }

    #[test]
    fn primary_workspace_root_is_marked_primary() {
        let root = primary_workspace_root();
        assert!(root.primary);
        assert_eq!(root.name.as_str(), "workspace");
    }

    #[test]
    fn build_agent_loop_constructs_for_both_providers() {
        // The agent loop must construct over a real ProviderRegistry for
        // both supported providers without contacting any provider endpoint
        // (construction is credential-free; only `run_prompt` dispatches).
        for id_str in ["zai", "vesper-synthetic"] {
            let provider_id = ProviderId::new(id_str).unwrap();
            let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
            let _agent = build_agent_loop(registry, &provider_id)
                .unwrap_or_else(|error| panic!("build_agent_loop({id_str}) failed: {error}"));
        }
    }

    #[test]
    fn build_agent_config_targets_the_requested_provider_with_a_primary_root() {
        // Pure, registry-free check of the composition-boundary config: the
        // loop must target the requested provider id, the matching model,
        // and ship exactly one primary workspace root for tool confinement.
        for (id_str, expected_model) in [("zai", "glm-5.2"), ("vesper-synthetic", "synthetic-1")] {
            let provider_id = ProviderId::new(id_str).unwrap();
            let config = build_agent_config(&provider_id)
                .unwrap_or_else(|error| panic!("build_agent_config({id_str}) failed: {error}"));
            assert_eq!(config.provider_id, provider_id);
            assert_eq!(config.model.provider_id, provider_id);
            assert_eq!(config.model.model_id.as_str(), expected_model);
            assert_eq!(
                config.workspace_roots.len(),
                1,
                "exactly one workspace root for the loop"
            );
            assert!(config.workspace_roots[0].primary);
            assert_eq!(config.max_tool_iterations, DEFAULT_MAX_TOOL_ITERATIONS);
        }
    }

    #[test]
    fn build_agent_config_rejects_unknown_providers() {
        let bogus = ProviderId::new("acme").unwrap();
        assert!(build_agent_config(&bogus).is_err());
    }

    #[test]
    fn apply_agent_event_routes_plan_through_apply_model_plan() {
        // The decisive Phase 6 contract: a Completed outcome with a plan
        // body drives the pure dispatch state PLANNING -> REVIEW. We start
        // in PLANNING (the only phase `apply_model_plan` will finalize from)
        // and assert REVIEW + a populated pending plan body afterwards.
        let mut state = SessionState::new();
        state
            .plan
            .start("build a Tier C agent loop")
            .expect("enter PLANNING");
        assert_eq!(state.phase(), PlanPhase::Planning);

        let event = AgentEvent::Completed(AgentTurnOutcome::Completed {
            assistant_content: vec![ContentPart::Text(
                ContentText::new("Planning now.").unwrap(),
            )],
            iterations: 1,
            tool_results: Vec::new(),
            plan: Some("# Plan\n1. wire the loop\n2. ship it\n".to_string()),
        });
        apply_agent_event(event, &mut state);

        assert_eq!(
            state.phase(),
            PlanPhase::Review,
            "model-authored plan must drive PLANNING -> REVIEW"
        );
        assert!(
            state
                .status
                .as_deref()
                .unwrap_or_default()
                .contains("/approve"),
            "REVIEW status must point the driver at /approve: {:?}",
            state.status
        );
    }

    #[test]
    fn apply_agent_event_with_no_plan_records_completion() {
        // A turn that produces text without an update_plan must surface the
        // assistant text and a completion notice, leaving Plan Mode alone.
        let mut state = SessionState::new();
        let event = AgentEvent::Completed(AgentTurnOutcome::Completed {
            assistant_content: vec![ContentPart::Text(
                ContentText::new("Hello, agent.").unwrap(),
            )],
            iterations: 1,
            tool_results: Vec::new(),
            plan: None,
        });
        apply_agent_event(event, &mut state);
        assert_eq!(state.phase(), PlanPhase::Normal, "no plan => no transition");
        assert_eq!(state.status.as_deref(), Some("agent turn complete."));
        assert!(
            state
                .transcript
                .iter()
                .any(|line| line.contains("Hello, agent.")),
            "assistant text must hit the transcript"
        );
    }

    #[test]
    fn apply_agent_event_surfaces_iteration_cap_and_errors() {
        let mut state = SessionState::new();
        apply_agent_event(
            AgentEvent::Completed(AgentTurnOutcome::MaxIterationsReached { iterations: 50 }),
            &mut state,
        );
        assert!(state.status.as_deref().unwrap_or_default().contains("50"));

        state = SessionState::new();
        apply_agent_event(
            AgentEvent::Failed(AgentLoopError::StreamWithoutTerminal),
            &mut state,
        );
        assert!(
            state
                .status
                .as_deref()
                .unwrap_or_default()
                .contains("agent loop error")
        );
    }

    #[test]
    fn drain_agent_event_handles_aborted_sender() {
        // If the spawned task's sender is dropped without sending (e.g. the
        // task panicked), the drain must clear the in-flight flag and surface
        // an abort notice instead of wedging the UI on WORKING... forever.
        let mut session = TuiSession {
            state: SessionState::new(),
            input: String::new(),
            agent_rx: None,
            agent_running: true,
        };
        let (_tx, rx): (mpsc::UnboundedSender<AgentEvent>, _) = mpsc::unbounded_channel();
        drop(_tx);
        session.agent_rx = Some(rx);
        drain_agent_event(&mut session);
        assert!(
            !session.agent_running,
            "an aborted sender must clear the in-flight flag"
        );
        assert!(session.agent_rx.is_none());
        assert!(state_status_contains(&session, "aborted"));
    }

    #[test]
    fn drain_agent_event_passes_through_when_still_running() {
        // While the channel is still empty (the task is still running), the
        // drain must NOT clear the in-flight flag — the WORKING banner stays.
        let mut session = TuiSession {
            state: SessionState::new(),
            input: String::new(),
            agent_rx: None,
            agent_running: true,
        };
        let (tx, rx): (mpsc::UnboundedSender<AgentEvent>, _) = mpsc::unbounded_channel();
        session.agent_rx = Some(rx);
        drain_agent_event(&mut session);
        assert!(session.agent_running, "still-running turn keeps the banner");
        assert!(session.agent_rx.is_some());
        drop(tx); // quiet unused-tx warning cleanly
    }

    fn state_status_contains(session: &TuiSession, needle: &str) -> bool {
        session
            .state
            .status
            .as_deref()
            .unwrap_or_default()
            .contains(needle)
    }
}
