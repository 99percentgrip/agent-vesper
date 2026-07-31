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

use agent_vesper_tui::{
    CommandIntent, CommandOutcome, CommandRegistry, PlanGesture, PlanPhase, PlanState,
    ProviderSuperpowerSurface, ViewModel, query_startup_view, render_to_frame,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::error;
use vesper_domain::ProviderId;

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

    let registry = vesper_runtime::ProviderRegistry::new();
    register_default_providers(&registry)
        .await
        .map_err(|error| format!("provider registration failed: {error:?}"))?;

    let startup = query_startup_view(&registry, &provider_id).await;
    let surface = ProviderSuperpowerSurface::new(startup.provider_id.clone(), startup.superpowers);

    let registry_commands = CommandRegistry::stage_11b();
    let mut session = TuiSession {
        plan_state: PlanState::default(),
        transcript: Vec::new(),
        input: String::new(),
        status: None,
    };

    enter_raw_mode().map_err(|error| format!("failed to enter raw mode: {error}"))?;
    let result = drive_loop(&provider_id, &registry_commands, &surface, &mut session);
    let _ = leave_raw_mode();
    result
}

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
struct TuiSession {
    plan_state: PlanState,
    transcript: Vec<String>,
    input: String,
    status: Option<String>,
}

fn drive_loop(
    provider_id: &ProviderId,
    registry_commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    session: &mut TuiSession,
) -> Result<(), String> {
    let mut terminal = Terminal::new(Backend::new(stdout()))
        .map_err(|error| format!("terminal init failed: {error}"))?;

    loop {
        let model = ViewModel {
            plan: session.plan_state.clone(),
            superpowers: Some(surface.clone()),
            transcript: session.transcript.clone(),
            input: session.input.clone(),
            status: session.status.clone(),
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
                session.transcript.push("Interrupted.".into());
                break;
            }
            KeyCode::Char('d') if ctrl => {
                session.transcript.push("EOF.".into());
                break;
            }
            KeyCode::Enter => {
                let intent = CommandIntent::parse(&session.input);
                let outcome = registry_commands.resolve(
                    &intent,
                    &session.plan_state,
                    provider_id,
                    surface.descriptors(),
                );
                if matches!(outcome, CommandOutcome::Quit) {
                    session.transcript.push("bye.".into());
                    break;
                }
                apply_outcome(outcome, session);
                session.input.clear();
            }
            KeyCode::Backspace => {
                session.input.pop();
            }
            KeyCode::Char(ch) => {
                session.input.push(ch);
            }
            KeyCode::Esc if session.plan_state.phase() != PlanPhase::Normal => {
                session.plan_state.cancel();
                session.status = Some("Plan cancelled.".into());
            }
            _ => {}
        }
    }
    Ok(())
}

fn apply_outcome(outcome: CommandOutcome, session: &mut TuiSession) {
    let TuiSession {
        plan_state,
        transcript,
        status,
        input: _,
    } = session;
    match outcome {
        CommandOutcome::Error(message) => {
            *status = Some(message);
        }
        CommandOutcome::Prompt(text) => {
            transcript.push(format!("user: {text}"));
            *status = None;
        }
        CommandOutcome::Plan { prd } => match plan_state.start(&prd) {
            Ok(_) => {
                transcript.push(format!("plan: entered PLANNING ({} bytes)", prd.len()));
                *status = Some("Plan Mode active.".into());
            }
            Err(error) => *status = Some(error.to_string()),
        },
        CommandOutcome::PlanGesture(gesture) => match gesture {
            PlanGesture::Approve => match plan_state.approve() {
                Ok(_) => *status = Some("Plan approved; EXECUTING.".into()),
                Err(error) => *status = Some(error.to_string()),
            },
            PlanGesture::Cancel => {
                plan_state.cancel();
                *status = Some("Plan cancelled.".into());
            }
        },
        CommandOutcome::Superpower {
            descriptor, value, ..
        } => {
            transcript.push(format!(
                "superpower: {} set to {}",
                descriptor.display_name.as_str(),
                format_superpower_value(&value)
            ));
            *status = None;
        }
        CommandOutcome::Help(text) => transcript.push(text),
        CommandOutcome::Quit => {}
    }
}

fn format_superpower_value(value: &vesper_provider::SuperpowerValue) -> String {
    use vesper_provider::SuperpowerValue;
    match value {
        SuperpowerValue::Choice { value } => value.as_str().to_string(),
        SuperpowerValue::Flag { value } => value.to_string(),
        SuperpowerValue::Number { value } => value.to_string(),
    }
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
