//! TUI composition of the shared native swarm service and settings controls.
use super::*;
pub async fn shutdown() -> bool {
    service().shutdown(std::time::Duration::from_secs(35)).await
}
use vesper_harness::swarm_service::{CancelFlag, HiveGoal, NativeSwarmService, SwarmRunContext};
use vesper_harness::swarm_settings::{SwarmCommandOutcome, SwarmControls, SwarmSettingsDraft};

fn service() -> &'static Arc<NativeSwarmService> {
    static SERVICE: std::sync::OnceLock<Arc<NativeSwarmService>> = std::sync::OnceLock::new();
    SERVICE.get_or_init(|| Arc::new(NativeSwarmService::default()))
}

pub async fn settings(terminal: &mut Terminal<Backend>, theme: &str) -> Result<String, String> {
    use agent_vesper_tui::swarm_hub::{SwarmHub, render};
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut hub = SwarmHub::new(SwarmSettingsDraft::open(&root)?);
    loop {
        terminal
            .draw(|frame| render(frame, &hub, theme))
            .map_err(|error| error.to_string())?;
        let Event::Key(key) = event::read().map_err(|error| error.to_string())? else {
            continue;
        };
        if key.kind == event::KeyEventKind::Release {
            continue;
        }
        match key.code {
            KeyCode::Esc => return Ok("Swarm settings cancelled; nothing saved.".into()),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok("Swarm settings cancelled; nothing saved.".into());
            }
            KeyCode::Up => hub.selected = (hub.selected + 8) % 8,
            KeyCode::Down | KeyCode::Tab => hub.selected = (hub.selected + 1) % 8,
            KeyCode::Char('s' | 'S') => {
                hub.draft.save(&root)?;
                return Ok(
                    "Swarm preferences saved; capability and permission checks apply to every run."
                        .into(),
                );
            }
            KeyCode::Enter | KeyCode::Char(' ') => match hub.selected {
                6 => {
                    hub.draft.save(&root)?;
                    return Ok("Swarm preferences saved; capability and permission checks apply to every run.".into());
                }
                7 => return Ok("Swarm settings cancelled; nothing saved.".into()),
                _ => hub.change(),
            },
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn command(
    argument: &str,
    registry: &Arc<vesper_runtime::ProviderRegistry>,
    agent: &Arc<AgentLoop>,
    tools: &Arc<dyn vesper_agent::ToolService>,
    permission: &Arc<dyn vesper_agent::PermissionPort>,
    session: &mut TuiSession,
    surface: &ProviderSuperpowerSurface,
    cognition: &CognitionBundle,
) -> Result<String, String> {
    static CONTROLS: std::sync::OnceLock<std::sync::Mutex<SwarmControls>> =
        std::sync::OnceLock::new();
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let outcome = CONTROLS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Swarm settings lock failed")?
        .command(&root, argument)?;
    let SwarmCommandOutcome::Run(goal) = outcome else {
        let SwarmCommandOutcome::Text(mut text) = outcome else {
            // VRO-16 gate resolution: forward to the running service.
            let SwarmCommandOutcome::Gate(task_id, command) = outcome else {
                unreachable!()
            };
            service().resolve_gate(&task_id, command)?;
            return Ok(
                "Gate command queued; the running swarm applies it on its next tick.".into(),
            );
        };
        if service().is_running() {
            text.push_str("\nA swarm goal is running.");
        }
        if !service().gate_snapshot().is_empty() {
            for gate in service().gate_snapshot() {
                text.push_str(&format!(
                    "\n{}",
                    vesper_harness::swarm_gate_surface::render_gate(&gate)
                ));
            }
        }
        if let Some(report) = service().last_report() {
            text.push_str(&format!(
                "\nLast run: success={}, cleanup={:?}{}. Artifacts: {}",
                report.success,
                report.cleanup,
                if report.budget_exhausted {
                    " — budget ceiling hard-stop"
                } else {
                    ""
                },
                report.artifacts.display()
            ));
        }
        return Ok(text);
    };
    if session.agent_running || service().is_running() {
        return Err("Finish or cancel the active turn before starting another swarm goal.".into());
    }
    let saved = vesper_harness::swarm_settings::load(&root)?;
    let config = turn_configuration(agent, &session.state, surface)?;
    vesper_harness::acceptance::activate_for_prompt(
        &mut session.acceptance,
        &root,
        vesper_harness::WorkerFactory::new(registry.clone(), config.clone()),
        &goal,
    )?;
    let parent = session.acceptance.as_ref().map(|acceptance| {
        acceptance.attach(
            agent
                .as_ref()
                .clone()
                .with_turn_configuration(config.clone()),
        )
    });
    let embedding_config = EmbeddingConfig::load(&cognition.root);
    if !matches!(
        embedding_config.source.as_deref(),
        Some("lmstudio" | "bigmodel")
    ) {
        return Err("Swarm requires an explicitly configured real embedding source. Use the native /embedding controls to select lmstudio or bigmodel.".into());
    }
    let credential = cognition.credential_source.clone();
    let cognition = cognition.clone();
    let recall_goal = goal.clone();
    let registry = registry.clone();
    let permission_port = permission.clone();
    let tool_registry = ToolRegistry::parity_default().with_service(tools.clone());
    let allowed_tools = [
        "read_file",
        "write_file",
        "edit_file",
        "grep",
        "update_plan",
        "apply_patch",
        "run_command",
        "list_directory",
        "search_files",
        "request_human_review",
        "request_human_input",
    ]
    .into_iter()
    .filter(|name| tool_registry.contains(name))
    .map(str::to_owned)
    .collect();
    let mode = session.state.controls.operating_mode;
    let permission_mode = session.state.controls.permission_mode;
    let (tx, rx) = mpsc::unbounded_channel::<AgentEvent>();
    let progress = Arc::new(ChannelProgressPort { tx: tx.clone() });
    let owner = service().clone();
    let cancel = CancelFlag::new();
    session.state.swarm_cancel = Some(cancel.clone());
    let mut history = session.conversation.clone();
    history.push(build_user_message(&goal));
    let task = tokio::spawn(async move {
        if let Some(parent) = &parent
            && let Err(error) = parent
                .prepare_acceptance(
                    mode,
                    permission_mode,
                    vesper_harness::swarm_service::parent_cancellation(cancel.signal()),
                )
                .await
        {
            let _ = tx.send(AgentEvent::Failed(AgentLoopError::LoopDetected(format!(
                "Acceptance incomplete: {error}"
            ))));
            return;
        }
        let progress: Arc<dyn vesper_agent::AgentProgressPort> = if parent.is_some() {
            Arc::new(vesper_agent::acceptance::HoldCompletionProgress(progress))
        } else {
            progress
        };
        let result = async {
            let recalled_context = vesper_harness::swarm_embedding::cancellable_setup(
                move || cognitive_context_for_prompt(&cognition, &recall_goal),
                cancel.signal(),
            )
            .await
            .map_err(|_| "Memory recall task failed.")?;
            let embedding = vesper_harness::swarm_embedding::cancellable_setup(
                move || {
                    let (embedder, _, _) = CognitionBundle::build_independent_embedder(
                        &embedding_config,
                        768,
                        &credential,
                    );
                    let dimensions = embedder
                        .embed(
                            "Swarm embedding capability probe",
                            vesper_cognition::EmbedAction::Search,
                        )
                        .map_err(|_| "Configured embedding probe failed.")?
                        .len();
                    let bridge = vesper_harness::swarm_embedding::ConfiguredEmbedding::new(
                        dimensions,
                        Arc::new(move |texts| {
                            embedder
                                .embed_batch(
                                    &texts.iter().map(String::as_str).collect::<Vec<_>>(),
                                    vesper_cognition::EmbedAction::Add,
                                )
                                .map_err(|_| "Configured embedding failed.".into())
                        }),
                    )?;
                    Ok::<_, String>((Arc::new(bridge), dimensions))
                },
                cancel.signal(),
            )
            .await
            .map_err(|_| "Embedding setup task failed.")??;
            let (backend, backend_choice) =
                vesper_harness::swarm_service::configured_backend(&root).await?;
            owner
                .run(
                    saved,
                    SwarmRunContext {
                        root,
                        factory: vesper_harness::WorkerFactory::new(registry, config),
                        tools: tool_registry,
                        allowed_tools,
                        mode,
                        permission: permission_mode,
                        permission_port,
                        progress: Some(progress),
                        recalled_context,
                        embedding: Some((embedding.0, embedding.1)),
                        backend,
                        backend_choice,
                    },
                    HiveGoal::new(format!("tui-{}", uuid::Uuid::new_v4()), goal),
                    cancel.signal(),
                )
                .await
        }
        .await;
        let (text, success, cancelled) = match result {
            Ok(report) => (
                format!(
                    "{}\n\nWorker artifacts: {}",
                    report.output,
                    report.artifacts.display()
                ),
                report.success,
                report.cancelled,
            ),
            Err(error) => (
                format!("Swarm refused: {error}"),
                false,
                cancel.signal().is_cancelled(),
            ),
        };
        if let Some(parent) = parent {
            match parent
                .finish_delegated_acceptance(
                    history,
                    &text,
                    mode,
                    permission_mode,
                    vesper_harness::swarm_service::parent_cancellation(cancel.signal()),
                )
                .await
            {
                Ok((outcome, history)) => {
                    let _ = tx.send(AgentEvent::Completed { outcome, history });
                }
                Err(error) => {
                    let _ = tx.send(AgentEvent::Failed(error));
                }
            }
            return;
        }
        if let Ok(content) = ContentText::new(text.clone()) {
            history.push(ConversationMessage {
                id: MessageId::new(format!("swarm-result-{}", uuid::Uuid::new_v4()))
                    .expect("bounded id"),
                role: MessageRole::Assistant,
                content: vec![ContentPart::Text(content)],
                extensions: Default::default(),
            });
        }
        let event = AgentEvent::Swarm {
            text,
            success,
            cancelled,
            history,
        };
        let _ = tx.send(event);
    });
    session.agent_task = Some(task);
    session.agent_rx = Some(rx);
    session.agent_running = true;
    session.live_response.clear();
    session.reasoning.clear();
    Ok("Swarm goal started with independent scoped workers.".into())
}
