#![forbid(unsafe_code)]
//! `agent-vesper-tui` binary entry point.
//!
//! Stage 11b ships a minimal but functional interactive loop:
//! 1. Select a provider via `AGENT_VESPER_PROVIDER` (default `zai`).
//! 2. Query the runtime registry for that provider's advertised superpowers.
//! 3. Enter a crossterm/ratatui event loop, parse slash commands, and apply
//!    Plan Mode transitions in memory.
//!
//! The binary deliberately keeps the rendering minimal; the architectural
//! surface (Plan Mode, commands, superpowers, TerminalRenderer) lives in the
//! library and is unit-tested there. The binary's stdout stays free of any
//! ACP/JSON-RPC contract — it writes only terminal escapes via crossterm.

use std::io::{self, stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_vesper_tui::{
    CommandIntent, CommandRegistry, DispatchOutcome, PlanPhase, ProviderSuperpowerSurface,
    SessionState, ViewModel, dispatch, query_startup_view, render_to_frame,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::{error, warn};
use vesper_domain::{
    BoundedString, CommandId, CommandInitiator, CommandSchemaVersion, CorrelationId, EndpointId,
    HarnessCommand, HarnessCommandPayload, ModelId, ProviderId, QualifiedModelId, Revision,
    SessionId, WorkspaceRoot,
};

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
    let mut session = TuiSession {
        // Pure dispatch state lives in the library so the full Plan Mode
        // lifecycle is unit-testable; the binary only owns the input buffer.
        state: SessionState::new(),
        input: String::new(),
    };

    enter_raw_mode().map_err(|error| format!("failed to enter raw mode: {error}"))?;
    let result = drive_loop(
        &provider_id,
        &registry_commands,
        &surface,
        &mut session,
        &supervisor,
        &runtime_session_id,
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
}

async fn drive_loop(
    provider_id: &ProviderId,
    registry_commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    session: &mut TuiSession,
    supervisor: &vesper_runtime::RuntimeSupervisor,
    runtime_session_id: &SessionId,
) -> Result<(), String> {
    let mut terminal = Terminal::new(Backend::new(stdout()))
        .map_err(|error| format!("terminal init failed: {error}"))?;

    loop {
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
