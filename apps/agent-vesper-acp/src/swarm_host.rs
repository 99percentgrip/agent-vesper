//! ACP protocol composition of shared swarm controls, execution and cancellation.
use super::*;
use vesper_harness::swarm_service::{CancelFlag, HiveGoal, NativeSwarmService, SwarmRunContext};
use vesper_harness::swarm_settings::{SwarmCommandOutcome, SwarmControls};

#[derive(Default)]
pub(super) struct SwarmHost {
    controls: std::sync::Mutex<BTreeMap<vesper_domain::SessionId, SwarmControls>>,
    active: Arc<std::sync::Mutex<BTreeMap<vesper_domain::SessionId, CancelFlag>>>,
    service: Arc<NativeSwarmService>,
    report: std::sync::Mutex<
        Option<(
            vesper_domain::SessionId,
            std::path::PathBuf,
            vesper_harness::swarm_service::SwarmRunReport,
        )>,
    >,
}
impl SwarmHost {
    pub(super) async fn shutdown(&self) -> bool {
        for flag in self.active.lock().expect("swarm active").values() {
            flag.cancel();
        }
        self.service
            .shutdown(std::time::Duration::from_secs(35))
            .await
    }
    pub(super) fn cancel(&self, session: &vesper_domain::SessionId) -> bool {
        if let Some(flag) = self.active.lock().expect("swarm active").get(session) {
            flag.cancel();
            true
        } else {
            false
        }
    }
}
struct ActiveRun {
    active: Arc<std::sync::Mutex<BTreeMap<vesper_domain::SessionId, CancelFlag>>>,
    session: vesper_domain::SessionId,
    flag: CancelFlag,
}
impl Drop for ActiveRun {
    fn drop(&mut self) {
        self.flag.cancel();
        self.active
            .lock()
            .expect("swarm active")
            .remove(&self.session);
    }
}

impl AcpHarnessEngine {
    pub(super) async fn swarm_command(
        &self,
        request: &AcpPromptRequest,
        argument: &str,
    ) -> Result<AcpPromptResult, String> {
        let root = workspace_root_path(&request.workspace_roots);
        let outcome = {
            let mut controls = self
                .swarm
                .controls
                .lock()
                .map_err(|_| "Swarm controls unavailable")?;
            if controls.len() >= 128 && !controls.contains_key(&request.session_id) {
                return Err("Swarm settings session capacity reached.".into());
            }
            controls
                .entry(request.session_id.clone())
                .or_default()
                .command(&root, argument)?
        };
        let goal = match outcome {
            SwarmCommandOutcome::Text(mut text) => {
                if self.swarm.service.is_running() {
                    text.push_str("\nA swarm goal is running.");
                }
                if !self.swarm.service.gate_snapshot().is_empty() {
                    for gate in self.swarm.service.gate_snapshot() {
                        text.push_str(&format!(
                            "\n{}",
                            vesper_harness::swarm_gate_surface::render_gate(&gate)
                        ));
                    }
                }
                let owned_report = self
                    .swarm
                    .report
                    .lock()
                    .map_err(|_| "Swarm report unavailable")?;
                if let Some((_, _, report)) =
                    owned_report.as_ref().filter(|(session, workspace, _)| {
                        session == &request.session_id && workspace == &root
                    })
                {
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
                    if !report.gate_events.is_empty() {
                        text.push_str("\nGovernance audit:\n");
                        text.push_str(&vesper_harness::swarm_gate_surface::render_audit(
                            &report.gate_events,
                        ));
                    }
                }
                return Ok(AcpPromptResult {
                    text,
                    cancelled: false,
                    persist_turn: false,
                    history_replacement: None,
                });
            }
            SwarmCommandOutcome::Gate(task_id, command) => {
                self.swarm.service.resolve_gate(&task_id, command)?;
                return Ok(AcpPromptResult {
                    text: String::from(
                        "Gate command queued; the running swarm applies it on its next tick.",
                    ),
                    cancelled: false,
                    persist_turn: false,
                    history_replacement: None,
                });
            }
            SwarmCommandOutcome::Run(goal) => goal,
        };
        let flag = CancelFlag::new();
        let _active = {
            let mut active = self
                .swarm
                .active
                .lock()
                .map_err(|_| "Swarm admission unavailable")?;
            if !active.is_empty() || self.swarm.service.is_running() {
                return Err("A swarm goal is already active.".into());
            }
            active.insert(request.session_id.clone(), flag.clone());
            ActiveRun {
                active: self.swarm.active.clone(),
                session: request.session_id.clone(),
                flag: flag.clone(),
            }
        };
        let result = async {
            let settings = vesper_harness::swarm_settings::load(&root)?;
            let config = self.turn_configuration(request).await;
            let recall_bundle = self.cognition.clone();
            let recall_goal = goal.clone();
            let recalled_context = vesper_harness::swarm_embedding::cancellable_setup(
                move || cognition::cognitive_context_for_prompt(&recall_bundle, &recall_goal),
                flag.signal(),
            )
            .await
            .map_err(|_| "Memory recall task failed.")?;
            let cognition = self.cognition.clone();
            let embedding = vesper_harness::swarm_embedding::cancellable_setup(
                move || cognition.swarm_embedding(),
                flag.signal(),
            )
            .await
            .map_err(|_| "Embedding setup task failed.")??;
            if flag.signal().is_cancelled() {
                return Err("Swarm cancelled before provisioning.".into());
            }
            let (backend, backend_choice) =
                vesper_harness::swarm_service::configured_backend(&root).await?;
            let permission_port: Arc<dyn vesper_agent::PermissionPort> = request
                .permission_requester
                .as_ref()
                .map(|requester| {
                    Arc::new(AcpHarnessPermissionPort {
                        requester: requester.clone(),
                        session_id: request.session_id.clone(),
                    }) as Arc<dyn vesper_agent::PermissionPort>
                })
                .unwrap_or_else(|| Arc::new(vesper_agent::DenyPermissionPort));
            let progress: Arc<dyn vesper_agent::AgentProgressPort> =
                Arc::new(AcpEngineProgressPort {
                    sink: request.event_sink.clone(),
                    tool_seq: std::sync::atomic::AtomicU64::new(0),
                    outstanding: std::sync::Mutex::new(BTreeMap::new()),
                    session_id: request.session_id.clone(),
                    plans: self.plans_shared(),
                });
            let mut active_acceptance = self.acceptance_session(&request.session_id);
            vesper_harness::acceptance::activate_for_prompt(
                &mut active_acceptance,
                &root,
                WorkerFactory::new(self.registry.clone(), config.clone()),
                &goal,
            )?;
            if let Some(active) = &active_acceptance {
                self.acceptance
                    .lock()
                    .expect("acceptance sessions")
                    .insert(request.session_id.clone(), active.clone());
            }
            let parent = active_acceptance.map(|acceptance| {
                acceptance.attach(
                    vesper_agent::AgentLoop::new(
                        self.registry.clone(),
                        self.tool_registry(request),
                        config.clone(),
                    )
                    .with_permission_port(permission_port.clone()),
                )
            });
            if let Some(parent) = &parent {
                parent
                    .prepare_acceptance(
                        request.operating_mode,
                        request.permission_mode,
                        vesper_harness::swarm_service::parent_cancellation(flag.signal()),
                    )
                    .await?;
            }
            let progress = if parent.is_some() {
                Arc::new(vesper_agent::acceptance::HoldCompletionProgress(progress))
                    as Arc<dyn vesper_agent::AgentProgressPort>
            } else {
                progress
            };
            let tools = self.tool_registry(request);
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
            .filter(|name| tools.contains(name))
            .map(str::to_owned)
            .collect();
            let report = self
                .swarm
                .service
                .run(
                    settings,
                    SwarmRunContext {
                        root: root.clone(),
                        factory: WorkerFactory::new(self.registry.clone(), config),
                        tools,
                        allowed_tools,
                        mode: request.operating_mode,
                        permission: request.permission_mode,
                        permission_port,
                        progress: Some(progress),
                        recalled_context,
                        embedding: Some((embedding.0, embedding.1)),
                        backend,
                        backend_choice,
                    },
                    HiveGoal::new(format!("acp-{}", next_id()), goal.clone()),
                    flag.signal(),
                )
                .await?;
            *self
                .swarm
                .report
                .lock()
                .map_err(|_| "Swarm report unavailable")? =
                Some((request.session_id.clone(), root, report.clone()));
            let text = format!(
                "{}\n\nWorker artifacts: {}",
                report.output,
                report.artifacts.display()
            );
            if let Some(parent) = parent {
                let mut history = self
                    .histories
                    .lock()
                    .await
                    .get(&request.session_id)
                    .cloned()
                    .unwrap_or_else(|| request.history.clone());
                history.push(ConversationMessage {
                    id: MessageId::new(format!("acceptance-swarm-{}", next_id()))
                        .map_err(|error| error.to_string())?,
                    role: MessageRole::User,
                    content: vec![ContentPart::Text(
                        vesper_domain::ContentText::new(goal.clone())
                            .map_err(|error| error.to_string())?,
                    )],
                    extensions: Default::default(),
                });
                let (outcome, history) = parent
                    .finish_delegated_acceptance(
                        history,
                        &text,
                        request.operating_mode,
                        request.permission_mode,
                        vesper_harness::swarm_service::parent_cancellation(flag.signal()),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                let text = outcome_text(&outcome);
                self.histories
                    .lock()
                    .await
                    .insert(request.session_id.clone(), history.clone());
                return Ok(AcpPromptResult {
                    text,
                    cancelled: flag.signal().is_cancelled(),
                    persist_turn: true,
                    history_replacement: Some(history),
                });
            }
            // Worker progress has already streamed content, so the protocol
            // adapter will not synthesize another final chunk from result.text.
            // Deliver the complete report explicitly before ending the turn.
            if let Some(sink) = &request.event_sink {
                sink.event(vesper_acp::AcpEngineEvent::ContentDelta {
                    text: format!("\n\nSwarm result:\n{text}"),
                });
            }
            let mut histories = self.histories.lock().await;
            let history = histories
                .entry(request.session_id.clone())
                .or_insert_with(|| request.history.clone());
            for (role, text) in [
                (MessageRole::User, goal),
                (MessageRole::Assistant, text.clone()),
            ] {
                history.push(ConversationMessage {
                    id: MessageId::new(format!("swarm-{}", next_id()))
                        .map_err(|error| error.to_string())?,
                    role,
                    content: vec![ContentPart::Text(
                        vesper_domain::ContentText::new(text).map_err(|error| error.to_string())?,
                    )],
                    extensions: Default::default(),
                });
            }
            Ok(AcpPromptResult {
                text,
                cancelled: report.cancelled,
                persist_turn: true,
                history_replacement: Some(history.clone()),
            })
        }
        .await;
        if result.is_err() && flag.signal().is_cancelled() {
            Ok(AcpPromptResult {
                text: "Swarm cancelled; setup or execution stopped.".into(),
                cancelled: true,
                persist_turn: false,
                history_replacement: None,
            })
        } else {
            result
        }
    }
}

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
