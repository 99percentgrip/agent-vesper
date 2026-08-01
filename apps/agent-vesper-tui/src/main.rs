#![forbid(unsafe_code)]
//! `agent-vesper-tui` binary entry point.
//!
//! Stage 11b owns the ratatui/crossterm terminal composition, including the
//! conversation/sidebar/composer layout and the oracle-style slash-command
//! palette. Tier C Phase 6 (ADR 0010) drives the full multi-turn
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
use vesper_provider::{ProviderConfiguration, SuperpowerValue};

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
    // Phase 8 (ADR 0011): open the durable memory subsystem rooted at
    // `AGENT_VESPER_MEMORY_ROOT` (falling back to `.agent-vesper/memory/`
    // under the current directory). The stores handle confinement and
    // atomic writes themselves; the binary owns only the root path. If the
    // root cannot be opened we keep going so the rest of the TUI works —
    // memory commands will surface a clear error in the transcript.
    let memory_stores = Arc::new(MemoryStores::open_default());
    // Phase 9 (ADR 0012): open the durable checkpoints subsystem rooted at
    // `AGENT_VESPER_CHECKPOINT_ROOT` (falling back to
    // `.agent-vesper/checkpoints/`). Same confinement + atomic-write
    // discipline as the memory subsystem; the binary owns the root path.
    let mut checkpoint_stores = CheckpointStores::open_default();
    // Phase 10 (ADR 0013): open the durable MCP + plugins subsystem rooted
    // at `AGENT_VESPER_MCP_ROOT` (falling back to `.agent-vesper/mcp/`).
    // Same confinement + atomic-write discipline; the binary owns the root
    // path and the trusted-publishers registry.
    let mut mcp_stores = McpStores::open_default();

    // Phase 6 (ADR 0010): build the multi-turn agent loop over the same
    // shared registry that backs the supervisor. The durable memory service
    // is injected at this composition boundary so model-facing memory/skill
    // tools use the exact stores rendered by the TUI.
    let worker_factory = Arc::new(WorkerFactory {
        registry: Arc::clone(&registry),
        config: build_agent_config(&provider_id)?,
    });
    let agent_tools = Arc::new(TuiToolService::new(
        Arc::clone(&memory_stores),
        checkpoint_root_path(),
        mcp_root_path(),
        Some(worker_factory),
    ));
    let (approval_port, approval_rx) = vesper_agent::ApprovalBroker::channel();
    let agent = Arc::new(
        build_agent_loop(Arc::clone(&registry), &provider_id, agent_tools)
            .map_err(|error| format!("agent loop construction failed: {error}"))?
            .with_permission_port(approval_port),
    );

    let mut session = TuiSession {
        // Pure dispatch state lives in the library so the full Plan Mode
        // lifecycle is unit-testable; the binary only owns the input buffer
        // and the in-flight agent-turn channel.
        state: SessionState::new(),
        input: String::new(),
        conversation: Vec::new(),
        agent_rx: None,
        agent_running: false,
        approval_rx,
        pending_approval: None,
        command_matches: Vec::new(),
        command_selected: 0,
        session_id: runtime_session_id.as_str().to_owned(),
        telemetry: Arc::new(trajectory_recorder()),
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
        &mut mcp_stores,
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
    /// Conversation history sent to the provider on every agent turn.
    ///
    /// The event loop owns this history so the background agent task remains
    /// stateless between turns while the visible session stays multi-turn.
    conversation: Vec<ConversationMessage>,
    /// Phase 6 (ADR 0010): receiver for an in-flight agent turn. `Some` while
    /// a `tokio::spawn`-ed `AgentLoop::run_prompt` is running; the receiver
    /// yields exactly one [`AgentEvent`]. The event loop drains it via
    /// `try_recv` each iteration so the UI stays responsive while the model
    /// thinks and tools execute.
    agent_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    /// `true` while an agent turn is in flight — drives the "WORKING..."
    /// status banner. Cleared as soon as the receiver yields (or aborts).
    agent_running: bool,
    /// Host-side one-time approval requests emitted by the agent loop.
    approval_rx: mpsc::UnboundedReceiver<vesper_agent::PermissionRequest>,
    /// The request currently displayed to the driver, if any.
    pending_approval: Option<vesper_agent::PermissionRequest>,
    /// Current slash-command palette entries for the composer.
    command_matches: Vec<(String, String)>,
    /// Highlighted slash-command palette entry.
    command_selected: usize,
    /// Stable persisted transcript id used by the local search bridge.
    session_id: String,
    /// Opt-in secret-safe trajectory sink.
    telemetry: Arc<vesper_observability::TrajectoryRecorder>,
}

/// What a spawned agent-loop task reports back to the event loop.
///
/// The task sends exactly one of these through the per-turn mpsc channel.
#[derive(Debug)]
enum AgentEvent {
    /// The loop returned a terminal outcome.
    Completed {
        outcome: AgentTurnOutcome,
        history: Vec<ConversationMessage>,
    },
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
    mcp_stores: &mut McpStores,
) -> Result<(), String> {
    let mut terminal = Terminal::new(Backend::new(stdout()))
        .map_err(|error| format!("terminal init failed: {error}"))?;

    loop {
        // Phase 6: drain any completed agent turn BEFORE redrawing so the
        // "WORKING..." banner clears the moment the result lands. The drain
        // is non-blocking (`try_recv`); if the turn is still running we just
        // fall through and render the in-flight banner.
        drain_agent_event(session);
        drain_permission_request(session);
        refresh_command_menu(session, registry_commands, surface);

        let model = ViewModel {
            plan: session.state.plan.clone(),
            superpowers: Some(surface.clone()),
            overrides: session.state.overrides.clone(),
            transcript: session.state.transcript.clone(),
            input: session.input.clone(),
            status: session.state.status.clone(),
            command_menu: session.command_matches.clone(),
            command_menu_selected: session.command_selected,
            agent_running: session.agent_running,
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
                if let Some(request) = session.pending_approval.take() {
                    match session.input.trim() {
                        "/approve" => {
                            let tool = request.tool.clone();
                            request.approve();
                            session.state.status = Some(format!("Approved `{tool}` once."));
                        }
                        "/cancel" | "/reject" => {
                            let tool = request.tool.clone();
                            request.reject("driver rejected one-time approval");
                            session.state.status = Some(format!("Rejected `{tool}`."));
                        }
                        _ => {
                            session.pending_approval = Some(request);
                            session.state.status =
                                Some("Approval required: type /approve or /cancel.".into());
                        }
                    }
                    session.input.clear();
                    continue;
                }
                if let Some(selected) = selected_command_completion(session) {
                    let typed = session.input.trim_end();
                    if typed != selected || command_expands_to_argument(&selected, surface) {
                        session.input = selected;
                    }
                    if command_expands_to_argument(&session.input, surface) {
                        session.input.push(' ');
                        session.command_selected = 0;
                        refresh_command_menu(session, registry_commands, surface);
                        session.state.status = Some(if session.command_matches.is_empty() {
                            "Type the command argument, then press Enter.".into()
                        } else {
                            "Select a value with ↑/↓, then press Enter.".into()
                        });
                        continue;
                    }
                }
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
                session.command_matches.clear();
                session.command_selected = 0;
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
                    let root =
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    match vesper_agent::expand_references(&root, &text) {
                        Ok(expanded) => spawn_agent_turn(agent, expanded, session),
                        Err(error) => {
                            session.state.status =
                                Some(format!("context expansion failed: {error}"));
                        }
                    }
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
                // Phase 10 (ADR 0013): drain any pending MCP/plugins op
                // against the durable vesper_mcp stores.
                if let Some(op) = session.state.pending_mcp_op.take() {
                    drain_mcp_op(op, mcp_stores, &mut session.state);
                }
            }
            KeyCode::Backspace => {
                session.input.pop();
                refresh_command_menu(session, registry_commands, surface);
            }
            KeyCode::Tab if !session.command_matches.is_empty() => {
                if let Some(command) = selected_command_completion(session) {
                    session.input = command;
                    if command_expands_to_argument(&session.input, surface) {
                        session.input.push(' ');
                    }
                    session.command_selected = 0;
                    refresh_command_menu(session, registry_commands, surface);
                }
            }
            KeyCode::Up if !session.command_matches.is_empty() => {
                session.command_selected = session.command_selected.saturating_sub(1);
            }
            KeyCode::Down if !session.command_matches.is_empty() => {
                session.command_selected = (session.command_selected + 1)
                    .min(session.command_matches.len().saturating_sub(1));
            }
            KeyCode::Esc if !session.command_matches.is_empty() => {
                session.command_matches.clear();
                session.command_selected = 0;
            }
            KeyCode::Char(ch) => {
                session.input.push(ch);
                refresh_command_menu(session, registry_commands, surface);
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

/// Refreshes the oracle-style slash-command palette from the current
/// composer value. This is deliberately kept at the terminal boundary: the
/// registry remains pure and the palette disappears while an agent turn is in
/// flight, matching the Python composer behavior.
fn refresh_command_menu(
    session: &mut TuiSession,
    registry: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
) {
    if session.agent_running || !session.input.trim_start().starts_with('/') {
        session.command_matches.clear();
        session.command_selected = 0;
        return;
    }

    session.command_matches = command_palette_candidates(&session.input, registry, surface);
    if session.command_matches.is_empty() {
        session.command_selected = 0;
    } else {
        session.command_selected = session
            .command_selected
            .min(session.command_matches.len().saturating_sub(1));
    }
}

/// Produces either root slash-command matches or provider-advertised values
/// for a configurable command. This mirrors the oracle composer's two-level
/// palette while keeping values derived from the active provider rather than
/// hard-coding GLM model/reasoning choices in the terminal loop.
fn command_palette_candidates(
    input: &str,
    registry: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
) -> Vec<(String, String)> {
    let trimmed = input.trim_start();
    let Some((command, argument)) = trimmed.split_once(' ') else {
        return registry.completion_candidates(trimmed);
    };
    let alias = match command {
        "/reasoning" => "thinking",
        value => value.trim_start_matches('/'),
    };
    let Some(descriptor) = surface.by_alias(alias) else {
        return Vec::new();
    };
    let query = argument.trim().to_ascii_lowercase();
    descriptor
        .allowed_values
        .iter()
        .map(superpower_value_text)
        .filter(|value| query.is_empty() || value.to_ascii_lowercase().starts_with(&query))
        .map(|value| {
            (
                format!("{command} {value}"),
                descriptor.display_name.as_str().to_string(),
            )
        })
        .collect()
}

fn superpower_value_text(value: &SuperpowerValue) -> String {
    match value {
        SuperpowerValue::Choice { value } => value.as_str().to_string(),
        SuperpowerValue::Flag { value } => value.to_string(),
        SuperpowerValue::Number { value } => value.to_string(),
    }
}

fn selected_command_completion(session: &TuiSession) -> Option<String> {
    session
        .command_matches
        .get(session.command_selected)
        .map(|(command, _)| command.clone())
}

/// Commands which must not be submitted immediately after root-palette
/// selection. Provider configuration commands expand into a second value
/// palette; free-form commands leave the cursor after a trailing space.
fn command_expands_to_argument(command: &str, surface: &ProviderSuperpowerSurface) -> bool {
    let command = command.trim_end();
    if command.contains(' ') {
        return false;
    }
    let name = command.trim_start_matches('/');
    let alias = if name == "reasoning" {
        "thinking"
    } else {
        name
    };
    surface.by_alias(alias).is_some()
        || matches!(
            name,
            "plan"
                | "planmode"
                | "api-plan"
                | "endpoint"
                | "goal"
                | "subgoal"
                | "rename"
                | "rollback"
                | "rewind"
                | "loop"
        )
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
    tool_service: Arc<dyn vesper_agent::ToolService>,
) -> Result<AgentLoop, String> {
    let mut config = build_agent_config(provider_id)?;
    config.system_instructions = vesper_agent::project_instructions(&config.workspace_roots);
    Ok(AgentLoop::new(
        registry,
        ToolRegistry::parity_default().with_service(tool_service),
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
        // Project instructions are loaded at the composition boundary after
        // this pure provider/configuration projection is built.
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
    let user = build_user_message(&user_text);
    session.conversation.push(user.clone());
    let history = session.conversation.clone();
    let (tx, rx) = mpsc::unbounded_channel::<AgentEvent>();
    let agent = Arc::clone(agent);
    tokio::spawn(async move {
        let result = agent
            .run_prompt_with_history(
                history,
                SessionOperatingMode::Code,
                SessionPermissionMode::Ask,
            )
            .await;
        let event = match result {
            Ok((outcome, history)) => AgentEvent::Completed { outcome, history },
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

/// Moves one pending approval request into the visible TUI state. The agent
/// loop emits at most one request at a time because it awaits the decision
/// before executing the tool, so retaining one request is sufficient and
/// keeps the interaction deterministic.
fn drain_permission_request(session: &mut TuiSession) {
    if session.pending_approval.is_some() {
        return;
    }
    match session.approval_rx.try_recv() {
        Ok(request) => {
            session.state.status = Some(format!(
                "APPROVAL REQUIRED: `{}` — type /approve or /cancel",
                request.tool
            ));
            session.pending_approval = Some(request);
        }
        Err(mpsc::error::TryRecvError::Empty) => {}
        Err(mpsc::error::TryRecvError::Disconnected) => {
            if session.agent_running {
                session.state.status = Some("approval channel closed; requests fail closed".into());
            }
        }
    }
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
            if let AgentEvent::Completed { history, .. } = &event {
                session.conversation = history.clone();
                if let Err(error) = persist_tui_conversation(session) {
                    session.state.status = Some(format!("session persistence failed: {error}"));
                }
            }
            record_agent_event(session, &event);
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

fn record_agent_event(session: &TuiSession, event: &AgentEvent) {
    let result = match event {
        AgentEvent::Completed { outcome, .. } => match outcome {
            AgentTurnOutcome::Completed {
                iterations,
                tool_results,
                ..
            } => session.telemetry.record(
                "turn.completed",
                &session.session_id,
                [
                    ("status", "completed".to_owned()),
                    ("iterations", iterations.to_string()),
                    ("tool_count", tool_results.len().to_string()),
                ],
            ),
            AgentTurnOutcome::MaxIterationsReached { iterations } => session.telemetry.record(
                "turn.max_iterations",
                &session.session_id,
                [
                    ("status", "max_iterations".to_owned()),
                    ("iterations", iterations.to_string()),
                ],
            ),
        },
        AgentEvent::Failed(_) => session.telemetry.record(
            "turn.failed",
            &session.session_id,
            [("status", "failed".to_owned())],
        ),
    };
    if let Err(error) = result {
        tracing::debug!("secret-safe telemetry write failed: {error}");
    }
}

/// Persists a bounded, search-oriented transcript projection. Provider
/// opaque data, reasoning blocks, tool arguments, and tool outputs are
/// intentionally omitted; the persisted search contract exposes only user
/// and assistant text.
fn persist_tui_conversation(session: &TuiSession) -> Result<(), String> {
    let root = session_root_path();
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let messages = session
        .conversation
        .iter()
        .take(10_000)
        .filter_map(|message| {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool | MessageRole::ProviderOpaque(_) => return None,
            };
            let text = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then(|| serde_json::json!({"role": role, "content": text}))
        })
        .collect::<Vec<_>>();
    let record = serde_json::json!({
        "session_id": session.session_id,
        "title": "Agent Vesper TUI session",
        "cwd": std::env::current_dir().ok().map(|path| path.display().to_string()),
        "updated_at": format!("{:?}", std::time::SystemTime::now()),
        "messages": messages,
    });
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("transcript exceeded 16 MiB".into());
    }
    let target = root.join(format!("{}.json", session.session_id));
    let temporary = root.join(format!(
        ".{}.tmp-{}",
        session.session_id,
        std::process::id()
    ));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, &target).map_err(|error| error.to_string())
}

/// Applies a terminal [`AgentEvent`] to the pure dispatch state.
///
/// The model-authored plan, when present, drives `PLANNING → REVIEW` through
/// [`apply_model_plan`]; otherwise the assistant content is appended to the
/// transcript and the status is set to a brief completion notice.
fn apply_agent_event(event: AgentEvent, state: &mut SessionState) {
    match event {
        AgentEvent::Completed { outcome, .. } => match outcome {
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
    memory: Option<Arc<vesper_memory::MemoryStore>>,
    skills: Option<Arc<vesper_memory::SkillStore>>,
    profile: Option<Arc<vesper_memory::UserProfile>>,
    awareness: Option<Arc<vesper_memory::AwarenessLedger>>,
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
        let memory = vesper_memory::MemoryStore::open(&root).ok().map(Arc::new);
        let skills = vesper_memory::SkillStore::open(&root).ok().map(Arc::new);
        let profile = vesper_memory::UserProfile::open(&root).ok().map(Arc::new);
        let awareness = vesper_memory::AwarenessLedger::open(&root)
            .ok()
            .map(Arc::new);
        Self {
            memory,
            skills,
            profile,
            awareness,
            root_display,
        }
    }
}

/// Memory/skills tool service injected into the agent loop.
///
/// These tools deliberately use the same durable stores as slash-command
/// dispatch. The agent loop only sees the provider-neutral `ToolService`
/// contract; storage ownership and secret-safe bounds remain in
/// `vesper-memory`.
#[derive(Clone)]
struct WorkerFactory {
    registry: Arc<vesper_runtime::ProviderRegistry>,
    config: vesper_agent::AgentLoopConfig,
}

#[allow(dead_code)]
#[derive(Clone)]
struct LegacyTuiToolService {
    stores: Arc<MemoryStores>,
    /// Core read/write tools used by `batch_read` and `run_workflow`.
    core: Arc<ToolRegistry>,
    cron_root: std::path::PathBuf,
    plugin_loader: Option<Arc<vesper_mcp::PluginLoader>>,
    trusted_publishers: vesper_mcp::TrustedPublishers,
    plugin_root: std::path::PathBuf,
    session_root: std::path::PathBuf,
    worker_factory: Option<Arc<WorkerFactory>>,
}

impl LegacyTuiToolService {
    fn read_only_worker_service(&self) -> Arc<Self> {
        Arc::new(Self {
            stores: Arc::clone(&self.stores),
            core: Arc::clone(&self.core),
            cron_root: self.cron_root.clone(),
            plugin_loader: self.plugin_loader.clone(),
            trusted_publishers: self.trusted_publishers.clone(),
            plugin_root: self.plugin_root.clone(),
            session_root: self.session_root.clone(),
            worker_factory: None,
        })
    }
}

impl vesper_agent::ToolService for LegacyTuiToolService {
    fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
        use vesper_domain::ToolExecutionClass;
        type DefinitionRow = (
            &'static str,
            &'static str,
            ToolExecutionClass,
            &'static [(&'static str, &'static str, bool)],
        );
        let definitions: [DefinitionRow; 30] = [
            (
                "update_awareness",
                "Upsert, resolve, or invalidate bounded evidence-backed awareness records.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("record_id", "string", false),
                    ("summary", "string", false),
                    ("kind", "string", false),
                    ("confidence", "string", false),
                ],
            ),
            (
                "recall_memory",
                "Read opt-in durable project knowledge.",
                ToolExecutionClass::ReadOnly,
                &[("query", "string", false)],
            ),
            (
                "store_memory",
                "Store one stable reusable project fact.",
                ToolExecutionClass::Mutating,
                &[("entry", "string", true)],
            ),
            (
                "recall_user_profile",
                "Read approved cross-project user preferences.",
                ToolExecutionClass::ReadOnly,
                &[],
            ),
            (
                "store_user_profile",
                "Store one explicit durable user preference or environment fact.",
                ToolExecutionClass::Mutating,
                &[("entry", "string", true), ("category", "string", true)],
            ),
            (
                "forget_memory",
                "Remove one exact durable project or user fact.",
                ToolExecutionClass::Mutating,
                &[("scope", "string", true), ("entry", "string", true)],
            ),
            (
                "update_memory",
                "Apply bounded project-memory additions and removals.",
                ToolExecutionClass::Mutating,
                &[("operations", "array", true)],
            ),
            (
                "list_skills",
                "List reusable learned project skills.",
                ToolExecutionClass::ReadOnly,
                &[],
            ),
            (
                "read_skill",
                "Read one learned project skill.",
                ToolExecutionClass::ReadOnly,
                &[("name", "string", true)],
            ),
            (
                "learn_skill",
                "Create or refine a reusable project skill after verification.",
                ToolExecutionClass::Mutating,
                &[
                    ("name", "string", true),
                    ("description", "string", true),
                    ("instructions", "string", true),
                ],
            ),
            (
                "forget_skill",
                "Remove one agent-learned project skill.",
                ToolExecutionClass::Mutating,
                &[("name", "string", true)],
            ),
            (
                "manage_skill",
                "Pin, unpin, archive, or restore one learned skill.",
                ToolExecutionClass::Mutating,
                &[("name", "string", true), ("action", "string", true)],
            ),
            (
                "curate_skills",
                "Run deterministic bounded skill maintenance.",
                ToolExecutionClass::Mutating,
                &[],
            ),
            (
                "list_skill_bundles",
                "List project-local groups of learned skills.",
                ToolExecutionClass::ReadOnly,
                &[],
            ),
            (
                "read_skill_bundle",
                "Read one learned skill bundle.",
                ToolExecutionClass::ReadOnly,
                &[("name", "string", true)],
            ),
            (
                "manage_skill_bundle",
                "Create or delete a project-local skill bundle.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("name", "string", true),
                    ("description", "string", false),
                    ("skills", "array", false),
                    ("instruction", "string", false),
                ],
            ),
            (
                "evolve_skill",
                "Draft, promote, or discard a bounded candidate skill.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("name", "string", true),
                    ("description", "string", false),
                    ("instructions", "string", false),
                ],
            ),
            (
                "update_deliberation",
                "Record a bounded deliberation state update.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("record_id", "string", false),
                    ("summary", "string", false),
                ],
            ),
            (
                "failure_corpus",
                "Record or recall bounded failure-corpus observations.",
                ToolExecutionClass::Mutating,
                &[("action", "string", true), ("summary", "string", false)],
            ),
            (
                "cronjob",
                "Create, list, or remove persistent local scheduled-task definitions.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("name", "string", false),
                    ("prompt", "string", false),
                    ("schedule", "string", false),
                    ("job_id", "string", false),
                ],
            ),
            (
                "session_search",
                "Search the bounded conversation currently visible to this session.",
                ToolExecutionClass::ReadOnly,
                &[("query", "string", false), ("limit", "integer", false)],
            ),
            (
                "delegate_task",
                "Delegate a bounded read-only investigation to an injected worker port.",
                ToolExecutionClass::Mutating,
                &[
                    ("goal", "string", true),
                    ("context", "string", false),
                    ("role", "string", false),
                    ("background", "boolean", false),
                ],
            ),
            (
                "semantic_code",
                "Inspect bounded source symbols and references without editing files.",
                ToolExecutionClass::ReadOnly,
                &[
                    ("action", "string", true),
                    ("path", "string", true),
                    ("query", "string", false),
                    ("line", "integer", false),
                    ("column", "integer", false),
                ],
            ),
            (
                "apply_patch_set",
                "Transactionally apply hash-checked unified diffs to multiple files.",
                ToolExecutionClass::Mutating,
                &[("patches", "array", true)],
            ),
            (
                "batch_read",
                "Run up to twenty bounded read-only core operations and return JSON results.",
                ToolExecutionClass::ReadOnly,
                &[
                    ("operations", "array", true),
                    ("max_chars_per_result", "integer", false),
                ],
            ),
            (
                "run_workflow",
                "Run a validated bounded dependency graph of registered core tools.",
                ToolExecutionClass::Mutating,
                &[("steps", "array", true)],
            ),
            (
                "plugin_package",
                "Verify, install, list, or manage signed declarative plugin packages.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("manifest_path", "string", false),
                    ("publisher", "string", false),
                    ("public_key_path", "string", false),
                ],
            ),
            (
                "worktree_worker",
                "Inspect or manage the lifecycle of a bounded isolated Git worker.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("task", "string", false),
                    ("worker_path", "string", false),
                    ("base_ref", "string", false),
                    ("diff_sha256", "string", false),
                ],
            ),
            (
                "mcp_search",
                "Discover bounded tool descriptors from configured MCP servers.",
                ToolExecutionClass::ReadOnly,
                &[("server", "string", false)],
            ),
            (
                "mcp_call",
                "Call one explicitly selected MCP tool through the configured gateway.",
                ToolExecutionClass::Mutating,
                &[
                    ("server", "string", true),
                    ("tool", "string", true),
                    ("arguments", "object", true),
                ],
            ),
        ];
        definitions
            .into_iter()
            .map(|(name, description, class, properties)| {
                vesper_agent::schema_definition(name, description, class, properties)
            })
            .collect()
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        let stores = Arc::clone(&self.stores);
        let core = Arc::clone(&self.core);
        let cron_root = self.cron_root.clone();
        let plugin_loader = self.plugin_loader.clone();
        let trusted_publishers = self.trusted_publishers.clone();
        let plugin_root = self.plugin_root.clone();
        let session_root = self.session_root.clone();
        let worker_factory = self.worker_factory.clone();
        let worker_service = self.read_only_worker_service();
        let name = call.tool_id.as_str().to_owned();
        let arguments = call.arguments.clone();
        Box::pin(async move {
            match name.as_str() {
                "cronjob" | "session_search" | "delegate_task" | "semantic_code"
                | "apply_patch_set" | "batch_read" | "run_workflow" | "plugin_package"
                | "worktree_worker" | "mcp_search" | "mcp_call" => {
                    execute_extended_tui_tool(
                        &name,
                        &arguments,
                        context,
                        &core,
                        &cron_root,
                        plugin_loader.as_deref(),
                        &trusted_publishers,
                        &plugin_root,
                        &session_root,
                        worker_factory.as_deref(),
                        worker_service,
                    )
                    .await
                }
                _ => execute_tui_tool(&name, &arguments, &stores),
            }
        })
    }
}

/// Frontend adapter over the shared hosted service. The legacy implementation
/// below remains only for the narrow slash-command compatibility tests; all
/// model-facing tool calls use this shared ACP/TUI surface.
#[derive(Clone)]
struct TuiToolService {
    inner: Arc<vesper_harness::HarnessToolService>,
}

impl TuiToolService {
    fn new(
        _stores: Arc<MemoryStores>,
        cron_root: std::path::PathBuf,
        plugin_root: std::path::PathBuf,
        worker_factory: Option<Arc<WorkerFactory>>,
    ) -> Self {
        let worker_factory = worker_factory.map(|factory| {
            Arc::new(vesper_harness::WorkerFactory::new(
                Arc::clone(&factory.registry),
                factory.config.clone(),
            ))
        });
        Self {
            inner: Arc::new(vesper_harness::HarnessToolService::new(
                Arc::new(vesper_harness::MemoryStores::open_default()),
                cron_root,
                plugin_root,
                worker_factory,
            )),
        }
    }
}

impl vesper_agent::ToolService for TuiToolService {
    fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
        self.inner.definitions()
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        self.inner.execute(call, context)
    }
}

fn tui_tool_failure(name: &str, error: impl std::fmt::Display) -> vesper_agent::ToolError {
    vesper_agent::ToolError::Failed(format!("{name} failed: {error}"))
}

/// Executes the tool families that need a workspace/session boundary in
/// addition to the durable memory stores. Keeping this at the composition
/// boundary prevents the provider-neutral agent crate from depending on
/// checkpoints, plugins, or frontend state.
#[allow(clippy::too_many_arguments)]
async fn execute_extended_tui_tool(
    name: &str,
    arguments: &serde_json::Value,
    context: &vesper_agent::ToolContext,
    core: &ToolRegistry,
    cron_root: &std::path::Path,
    plugin_loader: Option<&vesper_mcp::PluginLoader>,
    trusted_publishers: &vesper_mcp::TrustedPublishers,
    plugin_root: &std::path::Path,
    session_root: &std::path::Path,
    worker_factory: Option<&WorkerFactory>,
    worker_service: Arc<LegacyTuiToolService>,
) -> Result<vesper_agent::ToolResult, vesper_agent::ToolError> {
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use vesper_agent::confinement::{confine, primary_root};
    use vesper_domain::{ContentPart, MessageRole, ToolCall, ToolCallId, ToolId};

    let required_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                tool: name.to_owned(),
                reason: format!("missing string argument `{key}`"),
            })
    };
    let optional_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let root = primary_root(context)?;

    match name {
        "mcp_search" => {
            let requested_server = optional_string("server");
            let registry_root = plugin_root.to_path_buf();
            let descriptors = tokio::task::spawn_blocking(move || {
                let registry = vesper_mcp::McpRegistry::open(&registry_root)
                    .map_err(|error| tui_tool_failure("mcp_search", error))?;
                let mut output = Vec::new();
                for server in registry.list() {
                    if requested_server
                        .as_deref()
                        .is_some_and(|requested| requested != server.id)
                    {
                        continue;
                    }
                    let tools = vesper_mcp::McpClient::tools(&server)
                        .map_err(|error| tui_tool_failure("mcp_search", error))?;
                    output.push(serde_json::json!({
                        "server": server.id,
                        "tools": tools,
                    }));
                }
                Ok::<_, vesper_agent::ToolError>(output)
            })
            .await
            .map_err(|_| vesper_agent::ToolError::Failed("mcp discovery task failed".into()))??;
            vesper_agent::ToolResult::new(
                serde_json::to_string(&descriptors)
                    .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "mcp_call" => {
            let server_id = required_string("server")?;
            let tool = required_string("tool")?;
            let arguments = arguments.get("arguments").cloned().ok_or_else(|| {
                vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "missing object argument `arguments`".into(),
                }
            })?;
            if !arguments.is_object() {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "`arguments` must be a JSON object".into(),
                });
            }
            let registry_root = plugin_root.to_path_buf();
            let result = tokio::task::spawn_blocking(move || {
                let registry = vesper_mcp::McpRegistry::open(&registry_root)
                    .map_err(|error| tui_tool_failure("mcp_call", error))?;
                let server = registry.get(&server_id).ok_or_else(|| {
                    vesper_agent::ToolError::Failed("MCP server not found".into())
                })?;
                vesper_mcp::McpClient::call_tool(&server, &tool, arguments)
                    .map_err(|error| tui_tool_failure("mcp_call", error))
            })
            .await
            .map_err(|_| vesper_agent::ToolError::Failed("mcp call task failed".into()))??;
            vesper_agent::ToolResult::new(truncate_text(
                &serde_json::to_string(&result).map_err(|error| tui_tool_failure(name, error))?,
                16_000,
            ))
        }
        "cronjob" => {
            let registry = vesper_checkpoints::CronRegistry::open(cron_root)
                .map_err(|error| tui_tool_failure(name, error))?;
            match required_string("action")?.as_str() {
                "list" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&registry.list())
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "create" => {
                    let entry = registry
                        .register(
                            optional_string("name").as_deref().unwrap_or("vesper-job"),
                            &required_string("prompt")?,
                            &required_string("schedule")?,
                        )
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&entry)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "update" => {
                    let id = required_string("job_id")?;
                    let entry = registry
                        .update(
                            &id,
                            optional_string("name").as_deref(),
                            optional_string("prompt").as_deref(),
                            optional_string("schedule").as_deref(),
                        )
                        .map_err(|error| tui_tool_failure(name, error))?
                        .ok_or_else(|| {
                            vesper_agent::ToolError::Failed(format!("cron job not found: {id}"))
                        })?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&entry)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "pause" | "resume" => {
                    let id = required_string("job_id")?;
                    let enabled = required_string("action")? == "resume";
                    let entry = registry
                        .set_enabled(&id, enabled)
                        .map_err(|error| tui_tool_failure(name, error))?
                        .ok_or_else(|| {
                            vesper_agent::ToolError::Failed(format!("cron job not found: {id}"))
                        })?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&entry)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "run" => {
                    let Some(factory) = worker_factory else {
                        return Err(vesper_agent::ToolError::Failed(
                            "cron run requires a provider-backed worker factory".into(),
                        ));
                    };
                    let id = required_string("job_id")?;
                    let Some(entry) = registry.get(&id) else {
                        return Err(vesper_agent::ToolError::Failed(format!(
                            "cron job not found: {id}"
                        )));
                    };
                    if !entry.enabled {
                        return Err(vesper_agent::ToolError::Failed("cron job is paused".into()));
                    }
                    let report = run_provider_worker(
                        factory,
                        None,
                        entry.prompt,
                        SessionOperatingMode::Code,
                        SessionPermissionMode::Ask,
                        None,
                    )
                    .await?;
                    vesper_agent::ToolResult::new(report)
                }
                "remove" => {
                    let id = required_string("job_id")?;
                    let removed = registry
                        .forget(&id)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("cron job {id} removed: {removed}"))
                }
                action => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: format!("unsupported cronjob action `{action}`"),
                }),
            }
        }
        "session_search" => {
            let query = optional_string("query").unwrap_or_default();
            let limit = arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 20) as usize;
            let persisted_store = vesper_sessions::FilesystemSessionStore::new(
                session_root.to_path_buf(),
                vesper_sessions::SessionSource::AgentVesper,
                vesper_sessions::DiscoveryBounds::default(),
            )
            .map_err(|error| tui_tool_failure(name, error))?;
            let persisted = vesper_sessions::search_sessions(
                &persisted_store,
                vesper_sessions::SessionSearchRequest {
                    query: query.clone(),
                    session_id: None,
                    around_ordinal: None,
                    limit,
                    window: 5,
                },
            )
            .await
            .map_err(|error| tui_tool_failure(name, error))?;
            let mut matches = persisted
                .into_iter()
                .map(|hit| {
                    serde_json::json!({
                        "source": "persisted",
                        "session_id": hit.session_id,
                        "ordinal": hit.ordinal,
                        "role": hit.role,
                        "snippet": hit.snippet,
                        "context": hit
                            .context
                            .into_iter()
                            .map(|message| {
                                serde_json::json!({
                                    "ordinal": message.ordinal,
                                    "role": message.role,
                                    "text": message.text,
                                })
                            })
                            .collect::<Vec<_>>(),
                        "score": hit.score,
                    })
                })
                .collect::<Vec<_>>();
            let query_lower = query.to_lowercase();
            for (ordinal, message) in context.conversation.iter().enumerate() {
                let text = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if query_lower.is_empty() || text.to_lowercase().contains(&query_lower) {
                    let role = match &message.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::Tool => "tool",
                        MessageRole::ProviderOpaque(_) => "provider",
                    };
                    matches.push(serde_json::json!({
                        "source": "current",
                        "session_id": "current",
                        "ordinal": ordinal,
                        "role": role,
                        "text": truncate_text(&text, 4000),
                    }));
                    if matches.len() >= limit {
                        break;
                    }
                }
            }
            matches.truncate(limit);
            vesper_agent::ToolResult::new(
                serde_json::to_string(&matches).map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "semantic_code" => {
            let action = required_string("action")?;
            let requested = required_string("path")?;
            let path = confine(root, &requested)?;
            if action == "workspace_symbols" {
                let query = optional_string("query").unwrap_or_default().to_lowercase();
                let mut symbols = Vec::new();
                collect_source_symbols(&path, &query, &mut symbols, 200)?;
                return vesper_agent::ToolResult::new(
                    serde_json::to_string(&symbols)
                        .map_err(|error| tui_tool_failure(name, error))?,
                );
            }
            let source =
                fs::read_to_string(&path).map_err(|error| tui_tool_failure(name, error))?;
            let query = optional_string("query").unwrap_or_default();
            let symbols = source_symbols(&source, &query);
            match action.as_str() {
                "document_symbols" | "definition" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&symbols)
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "references" => {
                    let needle = if query.is_empty() {
                        required_string("query")?
                    } else {
                        query
                    };
                    let references = source
                        .lines()
                        .enumerate()
                        .filter(|(_, line)| line.contains(&needle))
                        .take(200)
                        .map(|(line, text)| {
                            serde_json::json!({
                                "path": requested,
                                "line": line + 1,
                                "text": truncate_text(text, 400),
                            })
                        })
                        .collect::<Vec<_>>();
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&references)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "hover" => {
                    let line = arguments
                        .get("line")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(1) as usize;
                    let text = source.lines().nth(line.saturating_sub(1)).unwrap_or("");
                    vesper_agent::ToolResult::new(
                        serde_json::json!({
                            "path": requested,
                            "line": line,
                            "text": text,
                            "symbols": symbols,
                        })
                        .to_string(),
                    )
                }
                _ => Err(vesper_agent::ToolError::Failed(format!(
                    "semantic_code action `{action}` requires a language-server host; bounded source inspection supports document_symbols, workspace_symbols, definition, references, and hover"
                ))),
            }
        }
        "apply_patch_set" => {
            let Some(entries) = arguments.get("patches").and_then(|value| value.as_array()) else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "patches must be an array".into(),
                });
            };
            if entries.is_empty() || entries.len() > 20 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "patches must contain between 1 and 20 entries".into(),
                });
            }
            let mut seen = BTreeSet::new();
            let mut candidates = Vec::new();
            for entry in entries {
                let path_text = entry
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "each patch needs path".into(),
                    })?;
                let path = confine(root, path_text)?;
                if !seen.insert(path.clone()) {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: format!("duplicate patch target `{path_text}`"),
                    });
                }
                let old_bytes = fs::read(&path).map_err(|error| tui_tool_failure(name, error))?;
                let expected = entry
                    .get("expected_sha256")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                let actual = Sha256::digest(&old_bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                if expected.len() != 64 || expected != actual {
                    return Err(vesper_agent::ToolError::Failed(format!(
                        "content hash mismatch for `{path_text}`"
                    )));
                }
                let old_text = String::from_utf8(old_bytes.clone()).map_err(|_| {
                    vesper_agent::ToolError::Failed(format!("`{path_text}` is not UTF-8"))
                })?;
                let patch = entry
                    .get("patch")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "each patch needs patch text".into(),
                    })?;
                let new_text = vesper_agent::tools::apply_unified_diff(&old_text, patch)?;
                candidates.push((path, old_bytes, new_text.into_bytes()));
            }
            let mut committed = Vec::new();
            for (path, old_bytes, new_bytes) in &candidates {
                if let Err(error) = fs::write(path, new_bytes) {
                    for (rollback_path, rollback_bytes) in committed.iter().rev() {
                        let _ = fs::write(rollback_path, rollback_bytes);
                    }
                    return Err(tui_tool_failure(name, error));
                }
                committed.push((path, old_bytes));
            }
            vesper_agent::ToolResult::new(format!(
                "transactionally applied {} patch(es)",
                candidates.len()
            ))
        }
        "batch_read" => {
            let Some(operations) = arguments
                .get("operations")
                .and_then(|value| value.as_array())
            else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "operations must be an array".into(),
                });
            };
            if operations.is_empty() || operations.len() > 20 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "operations must contain between 1 and 20 entries".into(),
                });
            }
            let per_result = arguments
                .get("max_chars_per_result")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(4000)
                .clamp(200, 16_000) as usize;
            let mut results = Vec::new();
            let mut ids = BTreeSet::new();
            for (index, operation) in operations.iter().enumerate() {
                let id = operation
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| (index + 1).to_string());
                if !ids.insert(id.clone()) {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: format!("duplicate batch operation id `{id}`"),
                    });
                }
                let tool = operation
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if !matches!(
                    tool,
                    "read_file" | "list_directory" | "search_files" | "grep"
                ) {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: format!("unsupported batch operation `{tool}`"),
                    });
                }
                let call = ToolCall {
                    id: ToolCallId::new(format!("batch-{index}"))
                        .map_err(|error| tui_tool_failure(name, error))?,
                    tool_id: ToolId::new(tool).map_err(|error| tui_tool_failure(name, error))?,
                    arguments: operation
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                    extensions: vesper_domain::ExtensionMap::default(),
                };
                let result = core.execute(&call, context).await;
                match result {
                    Ok(result) => {
                        let text = result.text.as_str().to_owned();
                        results.push(serde_json::json!({
                            "id": id,
                            "tool": tool,
                            "ok": true,
                            "output": truncate_text(&text, per_result),
                            "truncated": text.chars().count() > per_result,
                        }));
                    }
                    Err(error) => results.push(serde_json::json!({
                        "id": id,
                        "tool": tool,
                        "ok": false,
                        "error": truncate_text(&error.to_string(), 1000),
                    })),
                }
            }
            vesper_agent::ToolResult::new(serde_json::json!({"results": results}).to_string())
        }
        "run_workflow" => {
            let Some(steps) = arguments.get("steps").and_then(|value| value.as_array()) else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "steps must be an array".into(),
                });
            };
            if steps.is_empty() || steps.len() > 12 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "steps must contain between 1 and 12 entries".into(),
                });
            }
            let mut ids = BTreeMap::new();
            for (index, step) in steps.iter().enumerate() {
                let id = step
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("step-{index}"));
                if ids.insert(id, index).is_some() {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "workflow step ids must be unique".into(),
                    });
                }
            }
            let mut completed = BTreeSet::new();
            let mut order = Vec::new();
            while order.len() < steps.len() {
                let mut progressed = false;
                for (index, step) in steps.iter().enumerate() {
                    let id = ids
                        .iter()
                        .find_map(|(id, candidate)| (*candidate == index).then_some(id.clone()))
                        .expect("workflow id");
                    if completed.contains(&id) {
                        continue;
                    }
                    let needs = step
                        .get("needs")
                        .and_then(|value| value.as_array())
                        .map(|needs| {
                            needs
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if needs.iter().any(|need| !ids.contains_key(*need)) {
                        return Err(vesper_agent::ToolError::InvalidArguments {
                            tool: name.to_owned(),
                            reason: format!(
                                "workflow step `{id}` references an unknown dependency"
                            ),
                        });
                    }
                    if needs.iter().all(|need| completed.contains(*need)) {
                        completed.insert(id);
                        order.push(index);
                        progressed = true;
                    }
                }
                if !progressed {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "workflow dependencies contain a cycle".into(),
                    });
                }
            }
            let mut results = Vec::new();
            for index in order {
                let step = &steps[index];
                let id = step
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let tool = step
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if core.definition(tool).is_none() {
                    return Err(vesper_agent::ToolError::Failed(format!(
                        "workflow tool `{tool}` is not a registered core tool"
                    )));
                }
                let call = ToolCall {
                    id: ToolCallId::new(format!("workflow-{id}"))
                        .map_err(|error| tui_tool_failure(name, error))?,
                    tool_id: ToolId::new(tool).map_err(|error| tui_tool_failure(name, error))?,
                    arguments: step
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                    extensions: vesper_domain::ExtensionMap::default(),
                };
                match core.execute(&call, context).await {
                    Ok(result) => results.push(serde_json::json!({
                        "id": id,
                        "tool": tool,
                        "ok": true,
                        "output": truncate_text(result.text.as_str(), 8000),
                    })),
                    Err(error) => {
                        results.push(serde_json::json!({"id": id, "tool": tool, "ok": false, "error": error.to_string()}));
                        break;
                    }
                }
            }
            vesper_agent::ToolResult::new(serde_json::json!({"steps": results}).to_string())
        }
        "plugin_package" => {
            let Some(loader) = plugin_loader else {
                return Err(tui_tool_failure(name, "plugin loader unavailable"));
            };
            match required_string("action")?.as_str() {
                "list" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&loader.list())
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "publishers" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&trusted_publishers.list())
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "verify" => {
                    let package = confine(root, &required_string("manifest_path")?)?;
                    let manifest = loader
                        .verify(&package)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&manifest)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "install" => {
                    let package = confine(root, &required_string("manifest_path")?)?;
                    let record = loader
                        .load(&package)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&record)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "trust" => {
                    let publisher = required_string("publisher")?;
                    let key_path = confine(root, &required_string("public_key_path")?)?;
                    let key = fs::read_to_string(&key_path)
                        .map_err(|error| tui_tool_failure(name, error))?
                        .trim()
                        .to_owned();
                    let entry = vesper_mcp::TrustedPublisher {
                        publisher,
                        public_key_hex: key,
                    };
                    trusted_publishers
                        .trust(entry.clone())
                        .map_err(|error| tui_tool_failure(name, error))?;
                    persist_trusted_publishers(plugin_root, trusted_publishers)?;
                    vesper_agent::ToolResult::new("publisher trusted")
                }
                "untrust" => {
                    let publisher = required_string("publisher")?;
                    let removed = trusted_publishers.revoke(&publisher);
                    persist_trusted_publishers(plugin_root, trusted_publishers)?;
                    vesper_agent::ToolResult::new(format!("publisher removed: {removed}"))
                }
                action => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: format!("unsupported plugin action `{action}`"),
                }),
            }
        }
        "delegate_task" => {
            let Some(factory) = worker_factory else {
                return Err(vesper_agent::ToolError::Failed(
                    "delegate_task requires a provider-backed worker factory".into(),
                ));
            };
            if arguments
                .get("background")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                return Err(vesper_agent::ToolError::Failed(
                    "background delegate delivery is unavailable in the foreground TUI".into(),
                ));
            }
            let goal = required_string("goal")?;
            let context_text = optional_string("context").unwrap_or_default();
            let prompt = if context_text.is_empty() {
                goal
            } else {
                format!("{goal}\n\nAdditional read-only context:\n{context_text}")
            };
            let report = run_provider_worker(
                factory,
                Some(worker_service),
                prompt,
                SessionOperatingMode::Plan,
                SessionPermissionMode::ReadOnly,
                None,
            )
            .await?;
            vesper_agent::ToolResult::new(report)
        }
        "worktree_worker" => execute_worktree_worker(name, arguments, root, worker_factory).await,
        _ => Err(vesper_agent::ToolError::UnknownTool(name.to_owned())),
    }
}

async fn run_provider_worker(
    factory: &WorkerFactory,
    service: Option<Arc<LegacyTuiToolService>>,
    prompt: String,
    mode: SessionOperatingMode,
    permission: SessionPermissionMode,
    workspace: Option<std::path::PathBuf>,
) -> Result<String, vesper_agent::ToolError> {
    let mut config = factory.config.clone();
    if let Some(workspace) = workspace {
        config.workspace_roots = vec![WorkspaceRoot {
            name: BoundedString::new("worker").expect("bounded worker root name"),
            path: BoundedString::new(workspace.display().to_string())
                .map_err(|error| tui_tool_failure("worker", error))?,
            primary: true,
        }];
    }
    let tools = if let Some(service) = service {
        ToolRegistry::parity_default().with_service(service)
    } else {
        ToolRegistry::parity_default()
    };
    let worker = AgentLoop::new(Arc::clone(&factory.registry), tools, config);
    let outcome = worker
        .run_prompt(build_user_message(&prompt), mode, permission)
        .await
        .map_err(|error| tui_tool_failure("worker", error))?;
    Ok(outcome_text(&outcome))
}

async fn execute_worktree_worker(
    name: &str,
    arguments: &serde_json::Value,
    root: &std::path::Path,
    worker_factory: Option<&WorkerFactory>,
) -> Result<vesper_agent::ToolResult, vesper_agent::ToolError> {
    let required_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                tool: name.to_owned(),
                reason: format!("missing string argument `{key}`"),
            })
    };
    let optional_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let action = required_string("action")?;
    let confine_path = |value: &str| vesper_agent::confinement::confine(root, value);
    match action.as_str() {
        "run" => {
            let Some(factory) = worker_factory else {
                return Err(vesper_agent::ToolError::Failed(
                    "worktree_worker requires a provider-backed worker factory".into(),
                ));
            };
            let task = required_string("task")?;
            let base_ref = optional_string("base_ref").unwrap_or_else(|| "HEAD".into());
            if base_ref.starts_with('-') {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "base_ref must not begin with '-'".into(),
                });
            }
            let path_text = optional_string("worker_path").unwrap_or_else(|| {
                format!(".agent-vesper/worktrees/worker-{}", std::process::id())
            });
            if let Some(parent) = std::path::Path::new(&path_text).parent() {
                let parent = if parent.is_absolute() {
                    parent.to_path_buf()
                } else {
                    root.join(parent)
                };
                std::fs::create_dir_all(parent).map_err(|error| tui_tool_failure(name, error))?;
            }
            let path = confine_path(&path_text)?;
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(["worktree", "add", "--detach"])
                .arg(&path)
                .arg(&base_ref)
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            if !output.status.success() {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "git worktree add failed: {}",
                    truncate_text(&String::from_utf8_lossy(&output.stderr), 2000)
                )));
            }
            let report = run_provider_worker(
                factory,
                None,
                task,
                SessionOperatingMode::Code,
                SessionPermissionMode::Ask,
                Some(path.clone()),
            )
            .await?;
            vesper_agent::ToolResult::new(format!(
                "worker completed in {}\n{}",
                path.display(),
                report
            ))
        }
        "inspect" | "verify" => {
            let path = confine_path(&required_string("worker_path")?)?;
            let args = if action == "inspect" {
                vec!["diff", "--stat"]
            } else {
                if optional_string("verification_command").as_deref() != Some("git diff --check")
                    && arguments.get("verification_command").is_some()
                {
                    return Err(vesper_agent::ToolError::Failed(
                        "verification_command is restricted to `git diff --check`".into(),
                    ));
                }
                vec!["diff", "--check"]
            };
            let output = std::process::Command::new("git")
                .current_dir(&path)
                .args(args)
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            let body = if output.stdout.is_empty() {
                &output.stderr
            } else {
                &output.stdout
            };
            vesper_agent::ToolResult::new(format!(
                "{} (exit {})\n{}",
                action,
                output.status.code().unwrap_or(1),
                truncate_text(&String::from_utf8_lossy(body), 8000)
            ))
        }
        "discard" => {
            let path = confine_path(&required_string("worker_path")?)?;
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            if !output.status.success() {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "git worktree remove failed: {}",
                    truncate_text(&String::from_utf8_lossy(&output.stderr), 2000)
                )));
            }
            vesper_agent::ToolResult::new(format!("worker discarded: {}", path.display()))
        }
        "promote" => {
            let path = confine_path(&required_string("worker_path")?)?;
            let diff = std::process::Command::new("git")
                .current_dir(&path)
                .args(["diff", "--binary"])
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            let expected = required_string("diff_sha256")?.to_lowercase();
            let actual = sha256_hex(&diff.stdout);
            if expected != actual {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "worker diff hash mismatch: expected {}, found {}",
                    truncate_text(&expected, 16),
                    truncate_text(&actual, 16)
                )));
            }
            let mut apply = std::process::Command::new("git")
                .current_dir(root)
                .args(["apply", "--binary"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|error| tui_tool_failure(name, error))?;
            if let Some(stdin) = apply.stdin.as_mut() {
                use std::io::Write;
                stdin
                    .write_all(&diff.stdout)
                    .map_err(|error| tui_tool_failure(name, error))?;
            }
            let applied = apply
                .wait_with_output()
                .map_err(|error| tui_tool_failure(name, error))?;
            if !applied.status.success() {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "git apply failed: {}",
                    truncate_text(&String::from_utf8_lossy(&applied.stderr), 2000)
                )));
            }
            let _ = std::process::Command::new("git")
                .current_dir(root)
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .output();
            vesper_agent::ToolResult::new(format!("worker diff promoted from {}", path.display()))
        }
        other => Err(vesper_agent::ToolError::InvalidArguments {
            tool: name.to_owned(),
            reason: format!("unsupported worktree action `{other}`"),
        }),
    }
}

fn outcome_text(outcome: &AgentTurnOutcome) -> String {
    match outcome {
        AgentTurnOutcome::Completed {
            assistant_content, ..
        } => assistant_content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        AgentTurnOutcome::MaxIterationsReached { iterations } => {
            format!("worker reached the {iterations}-iteration safety cap")
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn truncate_text(value: &str, limit: usize) -> String {
    let mut output = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        output.push_str("… [truncated]");
    }
    output
}

fn source_symbols(source: &str, query: &str) -> Vec<serde_json::Value> {
    source
        .lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let trimmed = text.trim_start();
            let (kind, name) = if let Some(rest) = trimmed.strip_prefix("fn ") {
                ("function", rest.split(['(', '<', ' ', ':']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                ("function", rest.split(['(', '<', ' ', ':']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("struct ") {
                ("struct", rest.split(['{', '<', ' ']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("pub struct ") {
                ("struct", rest.split(['{', '<', ' ']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("class ") {
                ("class", rest.split(['(', ':', ' ']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("def ") {
                ("function", rest.split(['(', ' ']).next()?)
            } else {
                return None;
            };
            if !query.is_empty() && !name.to_lowercase().contains(&query.to_lowercase()) {
                return None;
            }
            Some(serde_json::json!({"name": name, "kind": kind, "line": line + 1}))
        })
        .collect()
}

fn collect_source_symbols(
    path: &std::path::Path,
    query: &str,
    output: &mut Vec<serde_json::Value>,
    limit: usize,
) -> Result<(), vesper_agent::ToolError> {
    if output.len() >= limit {
        return Ok(());
    }
    if path.is_file() {
        let source = std::fs::read_to_string(path)
            .map_err(|error| tui_tool_failure("semantic_code", error))?;
        for mut symbol in source_symbols(&source, query) {
            symbol["path"] = serde_json::Value::String(path.display().to_string());
            output.push(symbol);
            if output.len() >= limit {
                break;
            }
        }
        return Ok(());
    }
    let entries =
        std::fs::read_dir(path).map_err(|error| tui_tool_failure("semantic_code", error))?;
    for entry in entries.flatten() {
        let child = entry.path();
        if child.file_name().and_then(|name| name.to_str()) == Some(".git") {
            continue;
        }
        if child.is_dir()
            || matches!(
                child.extension().and_then(|ext| ext.to_str()),
                Some("rs" | "py" | "js" | "ts")
            )
        {
            collect_source_symbols(&child, query, output, limit)?;
        }
        if output.len() >= limit {
            break;
        }
    }
    Ok(())
}

fn persist_trusted_publishers(
    root: &std::path::Path,
    publishers: &vesper_mcp::TrustedPublishers,
) -> Result<(), vesper_agent::ToolError> {
    let body = publishers
        .list()
        .into_iter()
        .map(|entry| serde_json::to_string(&entry))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| tui_tool_failure("plugin_package", error))?
        .join("\n");
    std::fs::write(root.join("publishers.jsonl"), format!("{body}\n"))
        .map_err(|error| tui_tool_failure("plugin_package", error))
}

fn execute_tui_tool(
    name: &str,
    arguments: &serde_json::Value,
    stores: &MemoryStores,
) -> Result<vesper_agent::ToolResult, vesper_agent::ToolError> {
    use std::time::UNIX_EPOCH;
    use vesper_memory::{
        Confidence, EpistemicRecord, MemoryEntry, MemoryKind, RecordStatus, SkillBundle, SkillSlug,
    };

    let string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                tool: name.to_owned(),
                reason: format!("missing string argument `{key}`"),
            })
    };
    let optional_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let result = match name {
        "recall_memory" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            let entries = optional_string("query")
                .map_or_else(|| store.list(None), |query| store.query(&query));
            let body =
                serde_json::to_string(&entries).map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(body)
        }
        "store_memory" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            let entry = store
                .append(MemoryEntry {
                    id: String::new(),
                    kind: MemoryKind::Memory,
                    summary: string("entry")?,
                    scopes: Vec::new(),
                    evidence: Vec::new(),
                    created_at: UNIX_EPOCH,
                    updated_at: UNIX_EPOCH,
                })
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("stored memory {}", entry.id))
        }
        "recall_user_profile" => {
            let Some(profile) = stores.profile.as_ref() else {
                return Err(tui_tool_failure(name, "user profile unavailable"));
            };
            vesper_agent::ToolResult::new(profile.read())
        }
        "store_user_profile" => {
            let Some(profile) = stores.profile.as_ref() else {
                return Err(tui_tool_failure(name, "user profile unavailable"));
            };
            let category = string("category")?;
            let entry = string("entry")?;
            let size = profile
                .append(&category, &entry)
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("stored profile entry ({size} bytes)"))
        }
        "forget_memory" => {
            let scope = string("scope")?;
            let entry = string("entry")?;
            match scope.as_str() {
                "project" => {
                    let Some(store) = stores.memory.as_ref() else {
                        return Err(tui_tool_failure(name, "memory store unavailable"));
                    };
                    let ids: Vec<String> = store
                        .query(&entry)
                        .into_iter()
                        .filter(|candidate| candidate.summary == entry)
                        .map(|candidate| candidate.id)
                        .collect();
                    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                    let removed = store
                        .forget(&refs)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("removed {removed} project fact(s)"))
                }
                "user" => {
                    let Some(profile) = stores.profile.as_ref() else {
                        return Err(tui_tool_failure(name, "user profile unavailable"));
                    };
                    let removed = profile
                        .forget(&entry)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("removed {removed} profile line(s)"))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "scope must be project or user".into(),
                }),
            }
        }
        "update_memory" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            let Some(operations) = arguments.get("operations").and_then(|v| v.as_array()) else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "operations must be an array".into(),
                });
            };
            let mut changed = 0usize;
            for operation in operations.iter().take(20) {
                let op = operation.get("op").and_then(|v| v.as_str()).unwrap_or("");
                let entry = operation
                    .get("entry")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "each operation needs an entry".into(),
                    })?;
                match op {
                    "add" => {
                        store
                            .append(MemoryEntry {
                                id: String::new(),
                                kind: MemoryKind::Memory,
                                summary: entry.to_owned(),
                                scopes: Vec::new(),
                                evidence: Vec::new(),
                                created_at: UNIX_EPOCH,
                                updated_at: UNIX_EPOCH,
                            })
                            .map_err(|error| tui_tool_failure(name, error))?;
                        changed += 1;
                    }
                    "remove" => {
                        let ids: Vec<String> = store
                            .query(entry)
                            .into_iter()
                            .filter(|candidate| candidate.summary == entry)
                            .map(|candidate| candidate.id)
                            .collect();
                        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                        changed += store
                            .forget(&refs)
                            .map_err(|error| tui_tool_failure(name, error))?;
                    }
                    _ => {
                        return Err(vesper_agent::ToolError::InvalidArguments {
                            tool: name.to_owned(),
                            reason: "operation must be add or remove".into(),
                        });
                    }
                }
            }
            vesper_agent::ToolResult::new(format!("applied {changed} memory change(s)"))
        }
        "update_awareness" | "update_deliberation" => {
            let Some(ledger) = stores.awareness.as_ref() else {
                return Err(tui_tool_failure(name, "awareness ledger unavailable"));
            };
            let action = string("action")?;
            let id = optional_string("record_id").unwrap_or_default();
            let changed = match action.as_str() {
                "resolve" => ledger
                    .resolve(&id)
                    .map_err(|error| tui_tool_failure(name, error))?,
                "invalidate" => ledger
                    .invalidate(&id)
                    .map_err(|error| tui_tool_failure(name, error))?,
                "upsert" => {
                    let kind = match optional_string("kind").as_deref() {
                        Some("assumption") => MemoryKind::Assumption,
                        Some("hypothesis") => MemoryKind::Hypothesis,
                        Some("contradiction") => MemoryKind::Contradiction,
                        Some("unknown") => MemoryKind::Unknown,
                        Some("capability") => MemoryKind::Capability,
                        _ if name == "update_deliberation" => MemoryKind::Deliberation,
                        _ => MemoryKind::Observation,
                    };
                    let confidence = match optional_string("confidence").as_deref() {
                        Some("low") => Confidence::Low,
                        Some("high") => Confidence::High,
                        _ => Confidence::Medium,
                    };
                    ledger
                        .upsert(EpistemicRecord {
                            id,
                            kind,
                            summary: string("summary")?,
                            scopes: Vec::new(),
                            evidence: Vec::new(),
                            supports: Vec::new(),
                            confidence,
                            status: RecordStatus::Active,
                            created_at: UNIX_EPOCH,
                            updated_at: UNIX_EPOCH,
                        })
                        .map_err(|error| tui_tool_failure(name, error))?;
                    true
                }
                _ => {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "action must be upsert, resolve, or invalidate".into(),
                    });
                }
            };
            ledger
                .save()
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("awareness update accepted: {changed}"))
        }
        "list_skills" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            vesper_agent::ToolResult::new(
                serde_json::to_string(
                    &skills
                        .list()
                        .into_iter()
                        .map(|summary| {
                            serde_json::json!({
                                "name": summary.slug,
                                "headline": summary.headline,
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "read_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(
                skills
                    .read(&slug)
                    .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "learn_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let body = format!(
                "# {}\n\n{}\n\n{}\n",
                slug.as_str(),
                string("description")?,
                string("instructions")?
            );
            skills
                .write(&slug, &body)
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("learned skill {}", slug.as_str()))
        }
        "forget_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let removed = skills
                .forget(&slug)
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("skill removed: {removed}"))
        }
        "manage_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let action = string("action")?;
            let mut body = skills
                .read(&slug)
                .map_err(|error| tui_tool_failure(name, error))?;
            let marker = format!("<!-- vesper:{action} -->");
            match action.as_str() {
                "pin" | "unpin" | "archive" | "restore" => {
                    for previous in ["pin", "unpin", "archive", "restore"] {
                        body = body.replace(&format!("<!-- vesper:{previous} -->\n"), "");
                    }
                    body = format!("{marker}\n{body}");
                    skills
                        .write(&slug, &body)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("skill {}: {}", slug.as_str(), action))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "action must be pin, unpin, archive, or restore".into(),
                }),
            }
        }
        "curate_skills" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let count = skills.list().len();
            vesper_agent::ToolResult::new(format!("skill curation scanned {count} skill(s)"))
        }
        "list_skill_bundles" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let bundles = skills.list_bundles();
            vesper_agent::ToolResult::new(
                serde_json::to_string(&bundles).map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "read_skill_bundle" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(
                serde_json::to_string(
                    &skills
                        .read_bundle(&slug)
                        .map_err(|error| tui_tool_failure(name, error))?,
                )
                .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "manage_skill_bundle" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let action = string("action")?;
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            match action.as_str() {
                "delete" => {
                    let removed = skills
                        .forget_bundle(&slug)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("bundle removed: {removed}"))
                }
                "create" => {
                    let skills_value = arguments
                        .get("skills")
                        .and_then(|value| value.as_array())
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|value| value.as_str().map(str::to_owned))
                                .take(32)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    skills
                        .write_bundle(SkillBundle {
                            name: slug.as_str().to_owned(),
                            description: optional_string("description").unwrap_or_default(),
                            skills: skills_value,
                            instruction: optional_string("instruction").unwrap_or_default(),
                        })
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("bundle created: {}", slug.as_str()))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "action must be create or delete".into(),
                }),
            }
        }
        "evolve_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let action = string("action")?;
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let candidate = SkillSlug::new(&format!("candidate-{}", slug.as_str()))
                .map_err(|error| tui_tool_failure(name, error))?;
            match action.as_str() {
                "draft" | "propose" => {
                    let body = format!(
                        "# {}\n\n{}\n\n{}\n",
                        slug.as_str(),
                        optional_string("description").unwrap_or_default(),
                        optional_string("instructions").unwrap_or_default()
                    );
                    skills
                        .write(&candidate, &body)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!(
                        "candidate staged: {}",
                        candidate.as_str()
                    ))
                }
                "promote" => {
                    let body = skills
                        .read(&candidate)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    skills
                        .write(&slug, &body)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    skills
                        .forget(&candidate)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("candidate promoted: {}", slug.as_str()))
                }
                "discard" => {
                    let removed = skills
                        .forget(&candidate)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("candidate discarded: {removed}"))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "action must be draft, propose, promote, or discard".into(),
                }),
            }
        }
        "failure_corpus" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            if optional_string("action").as_deref() == Some("recall") {
                let entries = store.list(Some(MemoryKind::MetaLearning));
                vesper_agent::ToolResult::new(
                    serde_json::to_string(&entries)
                        .map_err(|error| tui_tool_failure(name, error))?,
                )
            } else {
                let entry = store
                    .append(MemoryEntry {
                        id: String::new(),
                        kind: MemoryKind::MetaLearning,
                        summary: string("summary")?,
                        scopes: Vec::new(),
                        evidence: Vec::new(),
                        created_at: UNIX_EPOCH,
                        updated_at: UNIX_EPOCH,
                    })
                    .map_err(|error| tui_tool_failure(name, error))?;
                vesper_agent::ToolResult::new(format!(
                    "failure observation recorded as {}",
                    entry.id
                ))
            }
        }
        _ => {
            return Err(vesper_agent::ToolError::UnknownTool(name.to_owned()));
        }
    }?;
    Ok(result)
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

fn checkpoint_root_path() -> std::path::PathBuf {
    match std::env::var("AGENT_VESPER_CHECKPOINT_ROOT") {
        Ok(value) => std::path::PathBuf::from(value),
        Err(_) => std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(".agent-vesper")
            .join("checkpoints"),
    }
}

fn mcp_root_path() -> std::path::PathBuf {
    match std::env::var("AGENT_VESPER_MCP_ROOT") {
        Ok(value) => std::path::PathBuf::from(value),
        Err(_) => std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(".agent-vesper")
            .join("mcp"),
    }
}

/// Local transcript root used by the TUI's persisted search bridge. Relative
/// environment values are resolved under the current workspace so the
/// session repository always receives the absolute root it requires.
fn session_root_path() -> std::path::PathBuf {
    let root = match std::env::var("AGENT_VESPER_SESSION_ROOT") {
        Ok(value) => std::path::PathBuf::from(value),
        Err(_) => std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(".agent-vesper")
            .join("sessions"),
    };
    if root.is_absolute() {
        root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(root)
    }
}

fn trajectory_recorder() -> vesper_observability::TrajectoryRecorder {
    let enabled = std::env::var("AGENT_VESPER_TELEMETRY")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false);
    let path = std::env::var("AGENT_VESPER_TRAJECTORY_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".agent-vesper")
                .join("trajectory.jsonl")
        });
    vesper_observability::TrajectoryRecorder::new(Some(path), enabled)
}

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
        let root = checkpoint_root_path();
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
        CheckpointOp::SessionExportLast => {
            let Some(exporter) = stores.exporter.as_ref() else {
                state.transcript.push(format!(
                    "export last: exporter unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match exporter.export_last_response(&state.transcript) {
                Ok(path) => {
                    state
                        .transcript
                        .push(format!("export last: wrote {}", path.display()));
                    state.status = Some(format!("Exported last response to {}.", path.display()));
                }
                Err(vesper_checkpoints::CheckpointError::Unavailable("no response to export")) => {
                    state
                        .transcript
                        .push("export last: no response to export".into());
                    state.status = Some("No response to export.".into());
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("export last: failed — {error}"));
                    state.status = Some(format!("export last failed: {error}"));
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

// ---------------------------------------------------------------------------
// Phase 10 (ADR 0013): the durable MCP + plugins subsystem bridge.
// ---------------------------------------------------------------------------

/// Bundle of the durable MCP + plugins stores.
struct McpStores {
    registry: Option<vesper_mcp::McpRegistry>,
    plugin_loader: Option<vesper_mcp::PluginLoader>,
    /// In-memory trusted-publishers mirror (persisted to publishers.jsonl
    /// by the binary on every trust/revoke).
    trusted: vesper_mcp::TrustedPublishers,
    /// Human-readable root path used in error notices.
    root_display: String,
}

impl McpStores {
    /// Opens the bundle at `AGENT_VESPER_MCP_ROOT` (falling back to
    /// `.agent-vesper/mcp/`).
    fn open_default() -> Self {
        let root = mcp_root_path();
        let _ = std::fs::create_dir_all(&root);
        let root_display = root.display().to_string();
        // Load persisted trusted publishers (best-effort).
        let trusted = load_trusted_publishers(&root);
        let registry = vesper_mcp::McpRegistry::open(&root).ok();
        let plugin_loader = vesper_mcp::PluginLoader::open(&root, trusted.clone()).ok();
        Self {
            registry,
            plugin_loader,
            trusted,
            root_display,
        }
    }
}

/// Loads trusted publishers from `<root>/publishers.jsonl` (best-effort;
/// returns an empty registry when the file is absent).
fn load_trusted_publishers(root: &std::path::Path) -> vesper_mcp::TrustedPublishers {
    let path = root.join("publishers.jsonl");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vesper_mcp::TrustedPublishers::new();
    };
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<vesper_mcp::TrustedPublisher>(line) {
            entries.push(entry);
        }
    }
    vesper_mcp::TrustedPublishers::from_records(entries)
}

/// Drains one [`McpOp`] against the durable stores, pushing the result
/// into the transcript.
fn drain_mcp_op(
    op: agent_vesper_tui::commands::McpOp,
    stores: &mut McpStores,
    state: &mut SessionState,
) {
    use agent_vesper_tui::commands::McpOp;

    match op {
        McpOp::McpList => {
            let Some(registry) = stores.registry.as_ref() else {
                state.transcript.push(format!(
                    "mcp: registry unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let servers = registry.list();
            if servers.is_empty() {
                state.transcript.push("mcp: (no servers configured)".into());
            } else {
                state
                    .transcript
                    .push(format!("mcp: {} server(s)", servers.len()));
                for server in servers.iter().take(50) {
                    let cmd = server.command.as_deref().unwrap_or("(no command)");
                    state.transcript.push(format!(
                        "  {} [{:?}] `{}` {}",
                        server.id,
                        server.transport,
                        cmd,
                        server.args.join(" ")
                    ));
                }
            }
            state.status = None;
        }
        McpOp::McpAdd { id, command, args } => {
            let Some(registry) = stores.registry.as_ref() else {
                state.transcript.push(format!(
                    "mcp add: registry unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let config = vesper_mcp::McpServerConfig {
                id: id.clone(),
                transport: vesper_mcp::McpTransport::Stdio,
                command: Some(command.clone()),
                args,
                url: None,
                auth_env: None,
                label: None,
                created_at: std::time::SystemTime::UNIX_EPOCH,
            };
            match registry.add(config) {
                Ok(added) => {
                    state
                        .transcript
                        .push(format!("mcp add: registered `{}`", added.id));
                    state.status = Some(format!("MCP server `{}` added.", added.id));
                }
                Err(error) => {
                    state.transcript.push(format!("mcp add: failed — {error}"));
                    state.status = Some(format!("mcp add failed: {error}"));
                }
            }
        }
        McpOp::McpRemove { id } => {
            let Some(registry) = stores.registry.as_ref() else {
                state.transcript.push(format!(
                    "mcp remove: registry unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match registry.remove(&id) {
                Ok(true) => {
                    state
                        .transcript
                        .push(format!("mcp remove: unregistered `{}`", id));
                    state.status = Some(format!("MCP server `{}` removed.", id));
                }
                Ok(false) => {
                    state
                        .transcript
                        .push(format!("mcp remove: `{}` was not registered", id));
                    state.status = Some(format!("`{}` was not registered.", id));
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("mcp remove: failed — {error}"));
                    state.status = Some(format!("mcp remove failed: {error}"));
                }
            }
        }
        McpOp::McpTools { id } => {
            let Some(registry) = stores.registry.as_ref() else {
                state.transcript.push(format!(
                    "mcp tools: registry unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let Some(config) = registry.get(&id) else {
                state
                    .transcript
                    .push(format!("mcp tools: `{}` is not registered", id));
                state.status = Some(format!("`{}` is not registered.", id));
                return;
            };
            state
                .transcript
                .push(format!("mcp tools: connecting to `{}`...", id));
            // Spawn + handshake + tools/list. This is a blocking call; in
            // a real interactive session the binary would dispatch it on a
            // background thread to keep the UI responsive.
            match vesper_mcp::McpClient::tools(&config) {
                Ok(tools) => {
                    if tools.is_empty() {
                        state
                            .transcript
                            .push(format!("mcp tools: `{}` advertised no tools", id));
                    } else {
                        state.transcript.push(format!(
                            "mcp tools: `{}` advertised {} tool(s)",
                            id,
                            tools.len()
                        ));
                        for tool in tools.iter().take(50) {
                            let desc = tool.description.as_deref().unwrap_or("");
                            state.transcript.push(format!("  - {} {}", tool.name, desc));
                        }
                    }
                    state.status = Some(format!("`{}` tools listed.", id));
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("mcp tools: `{}` failed — {error}", id));
                    state.status = Some(format!("mcp tools failed: {error}"));
                }
            }
        }
        McpOp::PluginsList => {
            let Some(loader) = stores.plugin_loader.as_ref() else {
                state.transcript.push(format!(
                    "plugins: loader unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            let records = loader.list();
            if records.is_empty() {
                state.transcript.push("plugins: (no plugins loaded)".into());
            } else {
                state
                    .transcript
                    .push(format!("plugins: {} plugin(s) loaded", records.len()));
                for record in records.iter().take(50) {
                    let signed = if record.unsigned_debug {
                        "UNSIGNED(debug)"
                    } else {
                        "signed"
                    };
                    state.transcript.push(format!(
                        "  {} `{}` v{} by `{}` ({})",
                        record.id,
                        record.manifest.name,
                        record.manifest.version,
                        record.publisher,
                        signed
                    ));
                }
            }
            state.status = None;
        }
        McpOp::PluginsPublishers => {
            let publishers = stores.trusted.list();
            if publishers.is_empty() {
                state
                    .transcript
                    .push("plugins publishers: (none trusted)".into());
            } else {
                state
                    .transcript
                    .push(format!("plugins publishers: {} trusted", publishers.len()));
                for publisher in publishers.iter().take(50) {
                    state.transcript.push(format!(
                        "  `{}` key={}…",
                        publisher.publisher,
                        &publisher.public_key_hex[..publisher.public_key_hex.len().min(16)]
                    ));
                }
            }
            state.status = None;
        }
        McpOp::PluginsVerify { path } => {
            let Some(loader) = stores.plugin_loader.as_ref() else {
                state.transcript.push(format!(
                    "plugins verify: loader unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match loader.verify(std::path::Path::new(&path)) {
                Ok(manifest) => {
                    state.transcript.push(format!(
                        "plugins verify: `{}` v{} by `{}` — signature VALID",
                        manifest.name, manifest.version, manifest.publisher
                    ));
                    state.status = Some("Plugin signature verified.".into());
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("plugins verify: {path} — {error}"));
                    state.status = Some(format!("plugins verify failed: {error}"));
                }
            }
        }
        McpOp::PluginsLoad { path } => {
            let Some(loader) = stores.plugin_loader.as_ref() else {
                state.transcript.push(format!(
                    "plugins load: loader unavailable (root {})",
                    stores.root_display
                ));
                return;
            };
            match loader.load(std::path::Path::new(&path)) {
                Ok(record) => {
                    state.transcript.push(format!(
                        "plugins load: `{}` v{} by `{}` loaded ({})",
                        record.manifest.name, record.manifest.version, record.publisher, record.id
                    ));
                    state.status = Some(format!("Plugin {} loaded.", record.id));
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("plugins load: {path} — {error}"));
                    state.status = Some(format!("plugins load failed: {error}"));
                }
            }
        }
        McpOp::PluginsTrust {
            publisher,
            public_key_hex,
        } => {
            let entry = vesper_mcp::TrustedPublisher {
                publisher: publisher.clone(),
                public_key_hex: public_key_hex.clone(),
            };
            match stores.trusted.trust(entry.clone()) {
                Ok(()) => {
                    // Persist to publishers.jsonl (best-effort append).
                    if let Ok(serialized) = serde_json::to_string(&entry) {
                        let path =
                            std::path::Path::new(&stores.root_display).join("publishers.jsonl");
                        use std::io::Write;
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&path)
                        {
                            let _ = writeln!(file, "{serialized}");
                        }
                    }
                    state
                        .transcript
                        .push(format!("plugins trust: `{}` now trusted", publisher));
                    state.status = Some(format!("Publisher `{}` trusted.", publisher));
                }
                Err(error) => {
                    state
                        .transcript
                        .push(format!("plugins trust: failed — {error}"));
                    state.status = Some(format!("plugins trust failed: {error}"));
                }
            }
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

    fn palette_surface() -> ProviderSuperpowerSurface {
        use vesper_provider::{SuperpowerDescriptor, SuperpowerKind, SuperpowerScope};

        let provider_id = ProviderId::new("zai").unwrap();
        let descriptor = |id: &str, alias: &str, values: &[&str]| SuperpowerDescriptor {
            id: BoundedString::new(id).unwrap(),
            provider_id: provider_id.clone(),
            display_name: BoundedString::new(alias).unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Session,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new(values[0]).unwrap(),
            },
            allowed_values: values
                .iter()
                .map(|value| SuperpowerValue::Choice {
                    value: BoundedString::new(*value).unwrap(),
                })
                .collect(),
            command_alias: Some(BoundedString::new(alias).unwrap()),
            help: None,
        };
        ProviderSuperpowerSurface::new(
            provider_id.clone(),
            vec![
                descriptor(
                    "zai:reasoning",
                    "thinking",
                    &["disabled", "enabled", "high", "max"],
                ),
                descriptor(
                    "zai:model",
                    "model",
                    &["glm-5.2", "glm-5.2-air", "glm-5.2-flash"],
                ),
            ],
        )
    }

    #[test]
    fn palette_starts_in_oracle_order_and_exposes_every_command() {
        let registry = CommandRegistry::stage_11b();
        let choices = command_palette_candidates("/", &registry, &palette_surface());
        assert_eq!(choices.len(), registry.names().len());
        assert_eq!(choices[0].0, "/plan");
        assert_eq!(
            choices.last().map(|choice| choice.0.as_str()),
            Some("/quit")
        );
    }

    #[test]
    fn palette_expands_provider_commands_into_live_values() {
        let registry = CommandRegistry::stage_11b();
        let surface = palette_surface();
        let thinking = command_palette_candidates("/thinking ", &registry, &surface);
        assert_eq!(thinking.len(), 4);
        assert_eq!(thinking[0].0, "/thinking disabled");
        assert_eq!(
            command_palette_candidates("/thinking h", &registry, &surface)[0].0,
            "/thinking high"
        );
        assert_eq!(
            command_palette_candidates("/reasoning m", &registry, &surface)[0].0,
            "/reasoning max"
        );
        assert_eq!(
            command_palette_candidates("/model glm-5.2-f", &registry, &surface)[0].0,
            "/model glm-5.2-flash"
        );
    }

    #[test]
    fn palette_only_pauses_submission_for_commands_needing_arguments() {
        let surface = palette_surface();
        assert!(command_expands_to_argument("/thinking", &surface));
        assert!(command_expands_to_argument("/model", &surface));
        assert!(command_expands_to_argument("/goal", &surface));
        assert!(!command_expands_to_argument("/thinking enabled", &surface));
        assert!(!command_expands_to_argument("/help", &surface));
    }

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
            let service = Arc::new(TuiToolService::new(
                Arc::new(MemoryStores::open_default()),
                checkpoint_root_path(),
                mcp_root_path(),
                None,
            ));
            let _agent = build_agent_loop(registry, &provider_id, service)
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

        let event = AgentEvent::Completed {
            outcome: AgentTurnOutcome::Completed {
                assistant_content: vec![ContentPart::Text(
                    ContentText::new("Planning now.").unwrap(),
                )],
                iterations: 1,
                tool_results: Vec::new(),
                plan: Some("# Plan\n1. wire the loop\n2. ship it\n".to_string()),
            },
            history: Vec::new(),
        };
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
        let event = AgentEvent::Completed {
            outcome: AgentTurnOutcome::Completed {
                assistant_content: vec![ContentPart::Text(
                    ContentText::new("Hello, agent.").unwrap(),
                )],
                iterations: 1,
                tool_results: Vec::new(),
                plan: None,
            },
            history: Vec::new(),
        };
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
            AgentEvent::Completed {
                outcome: AgentTurnOutcome::MaxIterationsReached { iterations: 50 },
                history: Vec::new(),
            },
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
            conversation: Vec::new(),
            agent_rx: None,
            agent_running: true,
            approval_rx: mpsc::unbounded_channel().1,
            pending_approval: None,
            command_matches: Vec::new(),
            command_selected: 0,
            session_id: "test-session".into(),
            telemetry: Arc::new(vesper_observability::TrajectoryRecorder::disabled()),
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
            conversation: Vec::new(),
            agent_rx: None,
            agent_running: true,
            approval_rx: mpsc::unbounded_channel().1,
            pending_approval: None,
            command_matches: Vec::new(),
            command_selected: 0,
            session_id: "test-session".into(),
            telemetry: Arc::new(vesper_observability::TrajectoryRecorder::disabled()),
        };
        let (tx, rx): (mpsc::UnboundedSender<AgentEvent>, _) = mpsc::unbounded_channel();
        session.agent_rx = Some(rx);
        drain_agent_event(&mut session);
        assert!(session.agent_running, "still-running turn keeps the banner");
        assert!(session.agent_rx.is_some());
        drop(tx); // quiet unused-tx warning cleanly
    }

    #[test]
    fn tui_tool_service_advertises_the_complete_python_tool_surface() {
        let service = TuiToolService::new(
            Arc::new(MemoryStores::open_default()),
            checkpoint_root_path(),
            mcp_root_path(),
            None,
        );
        let names = vesper_agent::ToolService::definitions(&service)
            .into_iter()
            .map(|definition| definition.harness_name.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), 36);
        for name in [
            "cronjob",
            "session_search",
            "semantic_code",
            "apply_patch_set",
            "batch_read",
            "run_workflow",
            "plugin_package",
            "delegate_task",
            "worktree_worker",
            "mcp_search",
            "mcp_list_tools",
            "mcp_call",
            "search_tools",
            "web_search",
            "web_reader",
            "vision_analyze",
            "browser_ui",
        ] {
            assert!(names.contains(name), "missing hosted tool {name}");
        }
    }

    #[tokio::test]
    async fn hosted_batch_read_and_session_search_are_bounded_and_real() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("note.txt"), "hello hosted tool").unwrap();
        let roots = vec![WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new(workspace.path().display().to_string()).unwrap(),
            primary: true,
        }];
        let mut context = vesper_agent::tools::stub_context(
            roots,
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        );
        context
            .conversation
            .push(build_user_message("hello hosted tool"));
        let service = TuiToolService::new(
            Arc::new(MemoryStores::open_default()),
            workspace.path().join("checkpoints"),
            workspace.path().join("mcp"),
            None,
        );
        let search_call = vesper_domain::ToolCall {
            id: vesper_domain::ToolCallId::new("search-call").unwrap(),
            tool_id: vesper_domain::ToolId::new("session_search").unwrap(),
            arguments: serde_json::json!({"query": "hosted"}),
            extensions: ExtensionMap::default(),
        };
        let search = vesper_agent::ToolService::execute(&service, &search_call, &context)
            .await
            .unwrap();
        assert!(search.text.as_str().contains("hello hosted tool"));

        let batch_call = vesper_domain::ToolCall {
            id: vesper_domain::ToolCallId::new("batch-call").unwrap(),
            tool_id: vesper_domain::ToolId::new("batch_read").unwrap(),
            arguments: serde_json::json!({
                "operations": [{
                    "id": "read-note",
                    "tool": "read_file",
                    "arguments": {"path": "note.txt"}
                }]
            }),
            extensions: ExtensionMap::default(),
        };
        let batch = vesper_agent::ToolService::execute(&service, &batch_call, &context)
            .await
            .unwrap();
        assert!(batch.text.as_str().contains("hello hosted tool"));
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
