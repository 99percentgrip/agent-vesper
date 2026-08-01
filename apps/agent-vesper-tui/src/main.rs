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

    // Phase 8 (ADR 0011): open the durable memory subsystem rooted at
    // `AGENT_VESPER_MEMORY_ROOT` (falling back to `.agent-vesper/memory/`
    // under the current directory). The stores handle confinement and
    // atomic writes themselves; the binary owns only the root path. If the
    // root cannot be opened we keep going so the rest of the TUI works —
    // memory commands will surface a clear error in the transcript.
    let memory_stores = MemoryStores::open_default();
    // Phase 9 (ADR 0012): open the durable checkpoints subsystem rooted at
    // `AGENT_VESPER_CHECKPOINT_ROOT` (falling back to
    // `.agent-vesper/checkpoints/`). Same confinement + atomic-write
    // discipline as the memory subsystem; the binary owns the root path.
    let mut checkpoint_stores = CheckpointStores::open_default();

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
        &memory_stores,
        &mut checkpoint_stores,
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

#[allow(clippy::too_many_arguments)] // single-call composition boundary
async fn drive_loop(
    provider_id: &ProviderId,
    registry_commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    session: &mut TuiSession,
    supervisor: &vesper_runtime::RuntimeSupervisor,
    runtime_session_id: &SessionId,
    agent: &Arc<AgentLoop>,
    memory_stores: &MemoryStores,
    checkpoint_stores: &mut CheckpointStores,
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
                //
                // Phase 7 (ADR 0010): workflow commands (`/security-review`,
                // `/smart`, `/release`, `/insights`, `/diff`) build a prompt
                // in `dispatch` and stash it on `SessionState.pending_prompt`.
                // Drain it the same way: it takes precedence over a free-text
                // prompt (only one prompt fires per Enter).
                let workflow_prompt = session.state.pending_prompt.take();
                let prompt_to_spawn = workflow_prompt.or(prompt_text);
                if let Some(text) = prompt_to_spawn
                    && !session.agent_running
                    && session.state.phase() == PlanPhase::Normal
                {
                    spawn_agent_turn(agent, text, session);
                }
                // Phase 8 (ADR 0011): drain any pending memory op against the
                // durable vesper_memory stores. The op was stashed by
                // `dispatch` (Memory(MemoryOp)); the binary owns the real
                // stores and executes the op synchronously (these are local
                // filesystem reads/writes — fast enough not to block the UI).
                if let Some(op) = session.state.pending_memory_op.take() {
                    drain_memory_op(op, memory_stores, &mut session.state);
                }
                // Phase 9 (ADR 0012): drain any pending checkpoint op against
                // the durable vesper_checkpoints stores. Same synchronous
                // execution pattern (local filesystem + scoped /ci subprocess).
                if let Some(op) = session.state.pending_checkpoint_op.take() {
                    drain_checkpoint_op(op, checkpoint_stores, &mut session.state);
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

// ---------------------------------------------------------------------------
// Phase 8 (ADR 0011): the durable memory subsystem bridge.
//
// `MemoryStores` owns one `MemoryStore`, `SkillStore`, `UserProfile`, and
// `AwarenessLedger` rooted at the same directory. `drain_memory_op` is the
// synchronous executor the event loop calls after dispatch; it formats the
// result into one or more transcript lines so the driver sees the outcome
// immediately.
// ---------------------------------------------------------------------------

/// Bundle of the four durable memory stores, all rooted at the same path.
/// The binary owns one `MemoryStores`; the event loop borrows it for the
/// duration of `drive_loop`.
struct MemoryStores {
    memory: Option<vesper_memory::MemoryStore>,
    skills: Option<vesper_memory::SkillStore>,
    profile: Option<vesper_memory::UserProfile>,
    awareness: Option<vesper_memory::AwarenessLedger>,
    /// Human-readable root path used in error notices.
    root_display: String,
}

impl MemoryStores {
    /// Opens the bundle at `AGENT_VESPER_MEMORY_ROOT` (falling back to
    /// `.agent-vesper/memory/` under the current directory). If opening any
    /// store fails the bundle stays `None` for that store and memory
    /// commands surface a clear error rather than crashing the TUI.
    fn open_default() -> Self {
        let root = match std::env::var("AGENT_VESPER_MEMORY_ROOT") {
            Ok(value) => std::path::PathBuf::from(value),
            Err(_) => std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".agent-vesper")
                .join("memory"),
        };
        // Ensure the root directory exists so the stores can open it.
        let _ = std::fs::create_dir_all(&root);
        let root_display = root.display().to_string();
        let memory = vesper_memory::MemoryStore::open(&root).ok();
        let skills = vesper_memory::SkillStore::open(&root).ok();
        let profile = vesper_memory::UserProfile::open(&root).ok();
        let awareness = vesper_memory::AwarenessLedger::open(&root).ok();
        Self {
            memory,
            skills,
            profile,
            awareness,
            root_display,
        }
    }
}

/// Drains one [`MemoryOp`] against the durable stores, pushing the result
/// into the transcript. Pure-with-side-effects: no async, no terminal I/O,
/// only local filesystem reads/writes via `vesper_memory`.
fn drain_memory_op(
    op: agent_vesper_tui::commands::MemoryOp,
    stores: &MemoryStores,
    state: &mut SessionState,
) {
    use agent_vesper_tui::commands::MemoryOp;
    use std::time::{SystemTime, UNIX_EPOCH};
    use vesper_memory::{MemoryEntry, MemoryKind};

    let now = SystemTime::now();
    let fresh_entry = |kind: MemoryKind, summary: String| MemoryEntry {
        id: String::new(),
        kind,
        summary,
        scopes: Vec::new(),
        evidence: Vec::new(),
        created_at: UNIX_EPOCH,
        updated_at: UNIX_EPOCH,
    };

    match op {
        MemoryOp::MemoryList { needle } => {
            let Some(store) = stores.memory.as_ref() else {
                state.transcript.push(format!(
                    "memory: store unavailable (root {})",
                    stores.root_display
                ));
                state.status = Some("memory store could not be opened.".into());
                return;
            };
            let entries = match needle {
                Some(needle) => store.query(&needle),
                None => store.list(None),
            };
            if entries.is_empty() {
                state.transcript.push("memory: (no entries)".into());
            } else {
                let count = entries.len();
                state
                    .transcript
                    .push(format!("memory: {count} entr{{y|ies}}"));
                for entry in entries.iter().take(50) {
                    state.transcript.push(format!(
                        "  [{}] {}: {}",
                        entry.kind.as_str(),
                        entry.id,
                        entry.summary.chars().take(80).collect::<String>()
                    ));
                }
                if count > 50 {
                    state
                        .transcript
                        .push(format!("  … and {} more", count - 50));
                }
            }
            state.status = None;
        }
        MemoryOp::GoalAdd { summary } => {
            let Some(store) = stores.memory.as_ref() else {
                state.transcript.push(format!(
                    "memory: store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match store.append(fresh_entry(MemoryKind::Goal, summary.clone())) {
                Ok(entry) => {
                    state.transcript.push(format!(
                        "goal: persisted as {} — \"{}\"",
                        entry.id,
                        entry.summary.chars().take(80).collect::<String>()
                    ));
                    state.status = Some("Goal recorded to durable memory.".into());
                }
                Err(error) => {
                    state.transcript.push(format!("goal: rejected — {error}"));
                    state.status = Some(format!("goal failed: {error}"));
                }
            }
        }
        MemoryOp::SubgoalAdd { summary } => {
            let Some(store) = stores.memory.as_ref() else {
                state.transcript.push(format!(
                    "memory: store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match store.append(fresh_entry(MemoryKind::Subgoal, summary.clone())) {
                Ok(entry) => {
                    state.transcript.push(format!(
                        "subgoal: persisted as {} — \"{}\"",
                        entry.id,
                        entry.summary.chars().take(80).collect::<String>()
                    ));
                    state.status = Some("Subgoal recorded to durable memory.".into());
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("subgoal: rejected — {error}"));
                    state.status = Some(format!("subgoal failed: {error}"));
                }
            }
        }
        MemoryOp::SkillsList => {
            let Some(store) = stores.skills.as_ref() else {
                state.transcript.push(format!(
                    "skills: store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let summaries = store.list();
            if summaries.is_empty() {
                state.transcript.push("skills: (no learned skills)".into());
            } else {
                state
                    .transcript
                    .push(format!("skills: {} learned skill(s)", summaries.len()));
                for summary in summaries.iter().take(50) {
                    state
                        .transcript
                        .push(format!("  - {}: {}", summary.slug, summary.headline));
                }
            }
            state.status = None;
        }
        MemoryOp::ProfileShow => {
            let Some(profile) = stores.profile.as_ref() else {
                state.transcript.push(format!(
                    "profile: store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let body = profile.read();
            if body.trim().is_empty() {
                state
                    .transcript
                    .push("profile: (no approved cross-project preferences)".into());
            } else {
                let line_count = body.lines().count();
                state.transcript.push(format!(
                    "profile: {line_count} line(s) of approved preferences"
                ));
                for line in body.lines().take(40) {
                    state.transcript.push(format!("  {line}"));
                }
            }
            state.status = None;
        }
        MemoryOp::AwarenessList { kind } => list_awareness(stores, kind, state, "awareness"),
        MemoryOp::MetacognitionList => list_awareness(
            stores,
            Some(MemoryKind::Metacognition),
            state,
            "metacognition",
        ),
        MemoryOp::DeliberationList => list_awareness(
            stores,
            Some(MemoryKind::Deliberation),
            state,
            "deliberation",
        ),
        MemoryOp::RepositoryList => {
            list_awareness(stores, Some(MemoryKind::Repository), state, "repository")
        }
        MemoryOp::MetaLearningList => list_awareness(
            stores,
            Some(MemoryKind::MetaLearning),
            state,
            "meta-learning",
        ),
        MemoryOp::ObservabilityList => list_awareness(
            stores,
            Some(MemoryKind::Observability),
            state,
            "observability",
        ),
        MemoryOp::Curate => {
            let Some(store) = stores.memory.as_ref() else {
                state.transcript.push(format!(
                    "curator: memory store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match store.curate() {
                Ok((duplicates_removed, overflow_trimmed)) => {
                    state.transcript.push(format!(
                        "curator: removed {duplicates_removed} duplicate(s), trimmed {overflow_trimmed} overflow(s)"
                    ));
                    state.status = Some(format!(
                        "Curated: -{duplicates_removed} dupes, -{overflow_trimmed} overflow"
                    ));
                }
                Err(error) => {
                    state.transcript.push(format!("curator: failed — {error}"));
                    state.status = Some(format!("curator failed: {error}"));
                }
            }
        }
        MemoryOp::Journey => {
            // Composite view: chronologically interleave memory entries and
            // learned skills (profile is shown via /profile on its own).
            let memory_count = stores
                .memory
                .as_ref()
                .map(|store| store.list(None).len())
                .unwrap_or(0);
            let skill_count = stores
                .skills
                .as_ref()
                .map(|store| store.list().len())
                .unwrap_or(0);
            state.transcript.push(format!(
                "journey: {memory_count} memory entr{{y|ies}}, {skill_count} learned skill(s)"
            ));
            if let Some(store) = stores.memory.as_ref() {
                let mut entries = store.list(None);
                entries.sort_by_key(|entry| entry.created_at);
                for entry in entries.iter().take(20) {
                    state.transcript.push(format!(
                        "  [{}] {}: {}",
                        entry.kind.as_str(),
                        entry.id,
                        entry.summary.chars().take(80).collect::<String>()
                    ));
                }
            }
            state.status = None;
        }
    }
    // Touch `now` so the binding stays used even on early-return branches.
    let _ = now;
}

/// Helper for the five `/awareness`-family listing commands.
fn list_awareness(
    stores: &MemoryStores,
    kind: Option<vesper_memory::MemoryKind>,
    state: &mut SessionState,
    label: &str,
) {
    let Some(ledger) = stores.awareness.as_ref() else {
        state.transcript.push(format!(
            "{label}: awareness ledger unavailable (root {})",
            stores.root_display
        ));
        return;
    };
    let records = ledger.list(kind);
    if records.is_empty() {
        state.transcript.push(format!("{label}: (no records)"));
    } else {
        state
            .transcript
            .push(format!("{label}: {} record(s)", records.len()));
        for record in records.iter().take(50) {
            state.transcript.push(format!(
                "  [{}] {} ({:?}): {}",
                record.kind.as_str(),
                record.id,
                record.status,
                record.summary.chars().take(80).collect::<String>()
            ));
        }
    }
    state.status = None;
}

// ---------------------------------------------------------------------------
// Phase 9 (ADR 0012): the durable checkpoints subsystem bridge.
//
// `CheckpointStores` owns one `CheckpointsLedger`, `SessionLineage`,
// `CronRegistry`, `SessionExporter`, and `ClipboardPort` rooted at the same
// directory, plus the `CiStatusReader` (which is process-scoped). The binary
// owns one bundle; the event loop borrows it for the duration of `drive_loop`.
// `drain_checkpoint_op` is the synchronous executor the event loop calls
// after dispatch; it formats the result into one or more transcript lines so
// the driver sees the outcome immediately.
// ---------------------------------------------------------------------------

/// Bundle of the durable checkpoint subsystem stores.
struct CheckpointStores {
    ledger: Option<vesper_checkpoints::CheckpointsLedger>,
    sessions: Option<vesper_checkpoints::SessionLineage>,
    cron: Option<vesper_checkpoints::CronRegistry>,
    exporter: Option<vesper_checkpoints::SessionExporter>,
    clipboard: Option<vesper_checkpoints::ClipboardPort>,
    /// Workspace root snapshots and restores are confined to.
    workspace_root: std::path::PathBuf,
    /// Human-readable root path used in error notices.
    root_display: String,
    /// Active session id (used by /lineage, /branch, /rename). Defaults to
    /// `sess-1` so the very first session works without an explicit
    /// `/sessions-new`.
    active_session_id: String,
}

impl CheckpointStores {
    /// Opens the bundle at `AGENT_VESPER_CHECKPOINT_ROOT` (falling back to
    /// `.agent-vesper/checkpoints/`).
    fn open_default() -> Self {
        let root = match std::env::var("AGENT_VESPER_CHECKPOINT_ROOT") {
            Ok(value) => std::path::PathBuf::from(value),
            Err(_) => std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".agent-vesper")
                .join("checkpoints"),
        };
        let _ = std::fs::create_dir_all(&root);
        let workspace_root = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
        let root_display = root.display().to_string();
        let ledger = vesper_checkpoints::CheckpointsLedger::open(&root).ok();
        let sessions = vesper_checkpoints::SessionLineage::open(&root).ok();
        let cron = vesper_checkpoints::CronRegistry::open(&root).ok();
        let exporter = vesper_checkpoints::SessionExporter::open(&root).ok();
        let clipboard = vesper_checkpoints::ClipboardPort::open(&root).ok();
        Self {
            ledger,
            sessions,
            cron,
            exporter,
            clipboard,
            workspace_root,
            root_display,
            active_session_id: "sess-1".to_string(),
        }
    }
}

/// Drains one [`CheckpointOp`] against the durable stores, pushing the
/// result into the transcript. Pure-with-side-effects: no async, no
/// terminal I/O, only local filesystem reads/writes via
/// `vesper_checkpoints` (and a scoped `gh` subprocess for `/ci`).
fn drain_checkpoint_op(
    op: agent_vesper_tui::commands::CheckpointOp,
    stores: &mut CheckpointStores,
    state: &mut SessionState,
) {
    use agent_vesper_tui::commands::CheckpointOp;
    use vesper_checkpoints::CheckpointKind;

    match op {
        CheckpointOp::SessionCreate { name } => {
            let Some(sessions) = stores.sessions.as_ref() else {
                state.transcript.push(format!(
                    "sessions-new: lineage store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match sessions.create(None, name.as_deref(), &stores.workspace_root) {
                Ok(record) => {
                    stores.active_session_id = record.id.clone();
                    state.transcript.push(format!(
                        "sessions-new: created `{}` ({})",
                        record.name, record.id
                    ));
                    state.status = Some(format!("Active session: {} ({})", record.name, record.id));
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("sessions-new: failed — {error}"));
                    state.status = Some(format!("sessions-new failed: {error}"));
                }
            }
        }
        CheckpointOp::SessionList => {
            let Some(sessions) = stores.sessions.as_ref() else {
                state.transcript.push(format!(
                    "sessions: lineage store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let records = sessions.list();
            if records.is_empty() {
                state
                    .transcript
                    .push("sessions: (no sessions recorded)".into());
            } else {
                state
                    .transcript
                    .push(format!("sessions: {} session(s)", records.len()));
                for record in records.iter().take(50) {
                    state.transcript.push(format!(
                        "  {} `{}` ({:?}) parent={:?}",
                        record.id, record.name, record.status, record.parent_id
                    ));
                }
            }
            state.status = None;
        }
        CheckpointOp::LineageShow => {
            let Some(sessions) = stores.sessions.as_ref() else {
                state.transcript.push(format!(
                    "lineage: lineage store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let chain = sessions.lineage(&stores.active_session_id);
            if chain.is_empty() {
                state.transcript.push(format!(
                    "lineage: (no chain for {})",
                    stores.active_session_id
                ));
            } else {
                state
                    .transcript
                    .push(format!("lineage: {} hop(s)", chain.len()));
                for record in &chain {
                    state.transcript.push(format!(
                        "  {} `{}` ({:?})",
                        record.id, record.name, record.status
                    ));
                }
            }
            state.status = None;
        }
        CheckpointOp::SessionBranch { name } => {
            let Some(sessions) = stores.sessions.as_ref() else {
                state.transcript.push(format!(
                    "branch: lineage store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match sessions.branch(
                &stores.active_session_id,
                name.as_deref(),
                &stores.workspace_root,
            ) {
                Ok(record) => {
                    state.transcript.push(format!(
                        "branch: forked `{}` ({}) from {}",
                        record.name, record.id, stores.active_session_id
                    ));
                    stores.active_session_id = record.id.clone();
                    state.status = Some(format!("Branched to {} ({})", record.name, record.id));
                }
                Err(error) => {
                    state.transcript.push(format!("branch: failed — {error}"));
                    state.status = Some(format!("branch failed: {error}"));
                }
            }
        }
        CheckpointOp::SessionRename { new_name } => {
            let Some(sessions) = stores.sessions.as_ref() else {
                state.transcript.push(format!(
                    "rename: lineage store unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match sessions.rename(&stores.active_session_id, &new_name) {
                Ok(record) => {
                    state
                        .transcript
                        .push(format!("rename: `{}` is now `{}`", record.id, record.name));
                    state.status = Some(format!("Renamed to `{}`", record.name));
                }
                Err(error) => {
                    state.transcript.push(format!("rename: failed — {error}"));
                    state.status = Some(format!("rename failed: {error}"));
                }
            }
        }
        CheckpointOp::CheckpointCreate { label } => {
            let Some(ledger) = stores.ledger.as_ref() else {
                state.transcript.push(format!(
                    "checkpoint: ledger unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            // Find the most recent checkpoint id in this session to chain
            // from (lineage: parent_id).
            let parent_id = ledger
                .list()
                .iter()
                .rev()
                .find(|record| record.session_id == stores.active_session_id)
                .map(|record| record.id.clone());
            match ledger.create(
                &stores.active_session_id,
                parent_id.as_deref(),
                CheckpointKind::Manual,
                label.as_deref(),
                &stores.workspace_root,
            ) {
                Ok(record) => {
                    state.transcript.push(format!(
                        "checkpoint: {} captured {} file(s), {} byte(s){}",
                        record.id,
                        record.files.len(),
                        record.total_bytes,
                        record
                            .label
                            .as_ref()
                            .map(|label| format!(" — `{label}`"))
                            .unwrap_or_default()
                    ));
                    state.status = Some(format!("Snapshot {} saved.", record.id));
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("checkpoint: failed — {error}"));
                    state.status = Some(format!("checkpoint failed: {error}"));
                }
            }
        }
        CheckpointOp::CheckpointRollback { id } | CheckpointOp::CheckpointRewind { id } => {
            let Some(ledger) = stores.ledger.as_ref() else {
                state.transcript.push(format!(
                    "rollback: ledger unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match ledger.restore(&id, &stores.workspace_root) {
                Ok(restored) => {
                    state
                        .transcript
                        .push(format!("rollback: restored {restored} file(s) from {id}"));
                    state.status = Some(format!("Restored from {id}."));
                }
                Err(error) => {
                    state.transcript.push(format!("rollback: failed — {error}"));
                    state.status = Some(format!("rollback failed: {error}"));
                }
            }
        }
        CheckpointOp::CheckpointUndo { count } => {
            let Some(ledger) = stores.ledger.as_ref() else {
                state.transcript.push(format!(
                    "undo: ledger unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let recent = ledger.recent(count);
            // The N-th most recent is the restore target (skip the most
            // recent, which is the current state).
            let target = recent.iter().rev().nth(1).or(recent.last());
            match target {
                Some(record) => match ledger.restore(&record.id, &stores.workspace_root) {
                    Ok(restored) => {
                        state.transcript.push(format!(
                            "undo: rolled back to {} — restored {restored} file(s)",
                            record.id
                        ));
                        state.status = Some(format!("Undid to {}.", record.id));
                    }
                    Err(error) => {
                        state.transcript.push(format!("undo: failed — {error}"));
                        state.status = Some(format!("undo failed: {error}"));
                    }
                },
                None => {
                    state
                        .transcript
                        .push("undo: no prior checkpoint to roll back to".into());
                    state.status = Some("Nothing to undo.".into());
                }
            }
        }
        CheckpointOp::CronRegister { prompt, schedule } => {
            let Some(cron) = stores.cron.as_ref() else {
                state.transcript.push(format!(
                    "loop: cron registry unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            // The name defaults to a slice of the prompt so the registry
            // entry is human-identifiable.
            let name: String = prompt.chars().take(40).collect();
            match cron.register(&name, &prompt, &schedule) {
                Ok(entry) => {
                    state.transcript.push(format!(
                        "loop: registered `{}` ({}) — `{}`",
                        entry.id, entry.schedule, entry.name
                    ));
                    state.status = Some(format!("Cron entry {} saved.", entry.id));
                }
                Err(error) => {
                    state.transcript.push(format!("loop: failed — {error}"));
                    state.status = Some(format!("loop failed: {error}"));
                }
            }
        }
        CheckpointOp::SessionExport => {
            let Some(exporter) = stores.exporter.as_ref() else {
                state.transcript.push(format!(
                    "export: exporter unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            // Build the lineage view (best-effort; absent sessions → empty).
            let lineage = stores
                .sessions
                .as_ref()
                .map(|sessions| sessions.lineage(&stores.active_session_id))
                .unwrap_or_default();
            match exporter.export(&state.transcript, &lineage) {
                Ok(path) => {
                    state
                        .transcript
                        .push(format!("export: wrote {}", path.display()));
                    state.status = Some(format!("Exported to {}.", path.display()));
                }
                Err(error) => {
                    state.transcript.push(format!("export: failed — {error}"));
                    state.status = Some(format!("export failed: {error}"));
                }
            }
        }
        CheckpointOp::ClipboardCopy { target } => {
            let Some(clipboard) = stores.clipboard.as_ref() else {
                state.transcript.push(format!(
                    "copy: clipboard port unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            // Resolve the value to copy: the literal `target` (which
            // defaults to "last-response"), or the most recent assistant
            // transcript line when target == "last-response".
            let value = if target == "last-response" {
                state
                    .transcript
                    .iter()
                    .rev()
                    .find(|line| line.starts_with("assistant:"))
                    .cloned()
                    .unwrap_or_else(|| "(no recent assistant response)".to_string())
            } else {
                target
            };
            match clipboard.copy(&value) {
                Ok(outcome) => {
                    let native_label = if outcome.native {
                        "(native + persisted)"
                    } else {
                        "(persisted; no native clipboard available)"
                    };
                    state.transcript.push(format!(
                        "copy: {} {}",
                        value.chars().take(60).collect::<String>(),
                        native_label
                    ));
                    state.status = Some(format!("Copied {}.", native_label));
                }
                Err(error) => {
                    state.transcript.push(format!("copy: failed — {error}"));
                    state.status = Some(format!("copy failed: {error}"));
                }
            }
        }
        CheckpointOp::CiStatus => {
            let status = vesper_checkpoints::CiStatusReader::status();
            state.transcript.push(format!("ci: {}", status.output));
            state.status = if status.available {
                Some("CI status retrieved.".into())
            } else {
                Some("CI status unavailable.".into())
            };
        }
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
