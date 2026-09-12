//! Same Hive -> native factory -> AgentLoop -> real tool path used by compositions.
//! Offline provider/embedding fixtures are confined to tests; no provider I/O.
#![cfg(feature = "swarm")]
use futures_util::future::BoxFuture;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use vesper_agent::{AgentLoopConfig, DenyPermissionPort, ToolRegistry};
use vesper_domain::*;
use vesper_harness::{WorkerFactory, swarm_adapter::ProviderWorkerPort};
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderError, ProviderEventStream, ProviderFactory,
    ProviderFuture, ProviderRequest, ProviderSession, ProviderStreamEvent,
};
use vesper_runtime::ProviderRegistry;
use vesper_swarm::{
    hive::orchestrator::{Hive, HiveConfig, HiveEvent, HiveGoal},
    ledger::store::{BoundedText, EmbeddingPort, LedgerError},
    pool::WorkerInstanceFactory,
    topology::TopologyKind,
};

#[derive(Default)]
struct Trace {
    compactions: AtomicUsize,
    next: AtomicUsize,
    requests: Mutex<Vec<(usize, String)>>,
}
struct Factory {
    compaction: bool,
    sandbox: bool,
    id: ProviderId,
    barrier: Arc<tokio::sync::Barrier>,
    trace: Arc<Trace>,
}
struct Session {
    compaction: bool,
    sandbox: bool,
    id: usize,
    barrier: Arc<tokio::sync::Barrier>,
    trace: Arc<Trace>,
    turn: AtomicUsize,
}
impl ProviderFactory for Factory {
    type Session = Session;
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn create_session<'a>(
        &'a self,
        _: &'a ProviderConfiguration,
        _: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Session, ProviderError>> {
        Box::pin(async move {
            Ok(Session {
                compaction: self.compaction,
                sandbox: self.sandbox,
                id: self.trace.next.fetch_add(1, Ordering::SeqCst),
                barrier: self.barrier.clone(),
                trace: self.trace.clone(),
                turn: AtomicUsize::new(0),
            })
        })
    }
}
fn text(value: &str) -> ProviderStreamEvent {
    ProviderStreamEvent::ContentDelta {
        stream_id: BoundedString::new("text").unwrap(),
        part: ContentPart::Text(ContentText::new(value).unwrap()),
    }
}
fn completed(finish: FinishOutcome) -> ProviderStreamEvent {
    ProviderStreamEvent::Completed {
        finish,
        metadata: Default::default(),
    }
}
impl ProviderSession for Session {
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Ok(Box::pin(futures_util::stream::iter(vec![Ok(completed(
                    FinishOutcome::Cancelled,
                ))])) as ProviderEventStream);
            }
            let messages = serde_json::to_string(&request.messages).unwrap();
            self.trace
                .requests
                .lock()
                .unwrap()
                .push((self.id, messages.clone()));
            if self.sandbox {
                for part in request.messages.iter().flat_map(|message| &message.content) {
                    if let ContentPart::ToolResult(result) = part {
                        assert_eq!(
                            result.status,
                            ToolResultStatus::Succeeded,
                            "native tool failed: {:?}",
                            result.output
                        );
                    }
                }
            }
            let tool = if self.sandbox {
                "run_command"
            } else {
                "read_file"
            };
            if self.compaction && self.turn.load(Ordering::SeqCst) > 0 && request.tools.is_empty() {
                self.trace.compactions.fetch_add(1, Ordering::SeqCst);
                let label = ["A", "B", "C"]
                    .into_iter()
                    .find(|label| messages.contains(&format!("driver-{label}")))
                    .unwrap();
                return Ok(Box::pin(futures_util::stream::iter(vec![
                    Ok(text(&format!(
                        "## Goal\ndriver-{label} verified-{label}-native-file\n## Evidence\nPrior file reads completed; continue remaining reads without replay."
                    ))),
                    Ok(completed(FinishOutcome::Stop)),
                ])) as ProviderEventStream);
            }
            let turn = self.turn.fetch_add(1, Ordering::SeqCst);
            let events = if messages.contains("Return only JSON") {
                assert!(request.tools.is_empty());
                vec![
                    text(
                        &r#"{"tasks":[{"prompt":"driver-A","required_capabilities":["read_file"]},{"prompt":"driver-B","required_capabilities":["read_file"]},{"prompt":"driver-C","required_capabilities":["read_file"]}]}"#.replace("read_file", tool),
                    ),
                    completed(FinishOutcome::Stop),
                ]
            } else if messages.contains("Evaluate the evidence below for rigor") {
                // VRO-16 review-panel turns: tool-free evaluation of the
                // evidence with grounded review positions.
                assert!(request.tools.is_empty(), "review turns are tool-free");
                vec![
                    text("review-grounded: evidence verified against reads"),
                    completed(FinishOutcome::Stop),
                ]
            } else if messages.contains("return only JSON deciding") {
                // VRO-16 PR-2 decision turns: accept the synthesis.
                assert!(request.tools.is_empty());
                vec![
                    text(r#"{"kind":"proceed"}"#),
                    completed(FinishOutcome::Stop),
                ]
            } else if request.tools.is_empty() {
                for label in ["A", "B", "C"] {
                    assert!(
                        messages.contains(&format!("verified-{label}-native-file")),
                        "synthesis missing actual driver evidence"
                    );
                }
                vec![
                    text("grounded-native-synthesis"),
                    completed(FinishOutcome::Stop),
                ]
            } else {
                assert_eq!(request.tools.len(), 1);
                assert_eq!(request.tools[0].harness_name.as_str(), tool);
                let label = ["A", "B", "C"]
                    .into_iter()
                    .find(|label| messages.contains(&format!("driver-{label}")))
                    .unwrap();
                if turn == 0 || (self.compaction && turn < 8) {
                    // Serial or aliased native workers cannot pass this barrier.
                    if turn == 0 {
                        self.barrier.wait().await;
                    }
                    vec![
                        ProviderStreamEvent::ToolCallCompleted(ToolCall {
                            id: ToolCallId::new(format!("read-{turn}")).unwrap(),
                            tool_id: ToolId::new(tool).unwrap(),
                            arguments: if self.sandbox {
                                serde_json::json!({"command":format!("printf verified-{label}-native-file > result.txt; cat result.txt"), "timeout_seconds":5})
                            } else if self.compaction {
                                serde_json::json!({"path":format!("{label}-{turn}.txt")})
                            } else {
                                serde_json::json!({"path":format!("{label}.txt")})
                            },
                            extensions: Default::default(),
                        }),
                        completed(FinishOutcome::ToolCalls),
                    ]
                } else if self.sandbox && turn == 1 {
                    assert!(messages.contains(&format!("verified-{label}-native-file")));
                    vec![
                        ProviderStreamEvent::ToolCallCompleted(ToolCall {
                            id: ToolCallId::new("second-command").unwrap(),
                            tool_id: ToolId::new(tool).unwrap(),
                            arguments: serde_json::json!({"command":"cat result.txt", "timeout_seconds":5}),
                            extensions: Default::default(),
                        }),
                        completed(FinishOutcome::ToolCalls),
                    ]
                } else {
                    assert_eq!(
                        turn,
                        if self.compaction {
                            8
                        } else if self.sandbox {
                            2
                        } else {
                            1
                        },
                        "unexpected replay"
                    );
                    assert!(
                        messages.contains(&format!("verified-{label}-native-file")),
                        "real read result not delivered"
                    );
                    vec![
                        text(&format!("verified-{label}-native-file")),
                        completed(FinishOutcome::Stop),
                    ]
                }
            };
            Ok(
                Box::pin(futures_util::stream::iter(events.into_iter().map(Ok)))
                    as ProviderEventStream,
            )
        })
    }
}
struct Embedding;
impl EmbeddingPort for Embedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
    }
}
#[tokio::test]
async fn native_hive_overlaps_three_isolated_sessions_executes_tools_and_grounds_synthesis() {
    run_hive(false, false, false, false, false).await;
}

#[tokio::test]
#[ignore = "requires the real namespace supervisor; fails rather than skips when unavailable"]
async fn native_scoped_hive_runs_two_permissioned_commands_per_worker_and_verifies_cleanup() {
    run_hive(true, false, false, false, false).await;
}

#[cfg(feature = "docker")]
#[tokio::test]
#[ignore = "requires a real container runtime and explicitly selected local image"]
async fn native_container_hive_runs_two_permissioned_commands_per_worker_and_verifies_cleanup() {
    run_hive(true, true, false, false, false).await;
}

async fn run_hive(
    sandbox: bool,
    container: bool,
    shared_service: bool,
    compaction: bool,
    shared_scope: bool,
) {
    #[cfg(not(feature = "docker"))]
    let _ = shared_scope;
    for kind in [
        TopologyKind::Mesh,
        TopologyKind::Hierarchical,
        TopologyKind::Centralized,
        TopologyKind::Hybrid,
    ] {
        let root = tempfile::tempdir().unwrap();
        for label in ["A", "B", "C"] {
            std::fs::write(
                root.path().join(format!("{label}.txt")),
                format!("verified-{label}-native-file"),
            )
            .unwrap();
        }
        if compaction {
            for label in ["A", "B", "C"] {
                for turn in 0..8 {
                    std::fs::write(
                        root.path().join(format!("{label}-{turn}.txt")),
                        format!(
                            "verified-{label}-native-file read {turn} {}",
                            "x".repeat(3000)
                        ),
                    )
                    .unwrap();
                }
            }
        }
        let tool = if sandbox { "run_command" } else { "read_file" };
        let sandbox_leases = if sandbox {
            let scopes = root.path().join("workers");
            std::fs::create_dir(&scopes).unwrap();
            let backend: Arc<dyn vesper_sandbox::SandboxBackend> = if container {
                #[cfg(feature = "docker")]
                {
                    assert!(
                        std::env::var("VESPER_DOCKER_IMAGE").is_ok(),
                        "explicit local image required"
                    );
                    Arc::new(vesper_sandbox::DockerBackend::new(Default::default()))
                }
                #[cfg(not(feature = "docker"))]
                panic!("docker feature required");
            } else {
                Arc::new(vesper_sandbox::LinuxNamespacesBackend::new())
            };
            Some(Arc::new(
                vesper_harness::swarm_sandbox::NativeSandboxLeases::new(
                    backend,
                    vesper_agent::sandbox_route::SandboxDemand {
                        requirement: vesper_agent::sandbox_route::IsolationRequirement::Filesystem,
                        ..Default::default()
                    },
                    if container {
                        vesper_agent::sandbox_route::SandboxBackendChoice::Docker
                    } else {
                        vesper_agent::sandbox_route::SandboxBackendChoice::Default
                    },
                    5,
                    scopes.canonicalize().unwrap(),
                    String::new(),
                )
                .expect(
                    "real sandbox capability is required; unavailable is an acceptance failure",
                ),
            ))
        } else {
            None
        };
        let approvals = Arc::new(Permissions::default());
        let id = ProviderId::new("test.native-hive").unwrap();
        let trace = Arc::new(Trace::default());
        let registry = Arc::new(ProviderRegistry::new());
        registry
            .register(Factory {
                compaction,
                sandbox,
                id: id.clone(),
                barrier: Arc::new(tokio::sync::Barrier::new(3)),
                trace: trace.clone(),
            })
            .await
            .unwrap();
        let config = AgentLoopConfig {
            provider_id: id.clone(),
            provider_configuration: ProviderConfiguration {
                provider_id: id.clone(),
                values: VersionedExtensionEnvelope {
                    namespace: ExtensionNamespace::new("provider.test").unwrap(),
                    version: SchemaVersion::new(1).unwrap(),
                    values: Default::default(),
                },
            },
            model: QualifiedModelId {
                provider_id: id,
                model_id: ModelId::new("fixture").unwrap(),
            },
            context_window_tokens: if compaction { 5_000 } else { 131_072 },
            system_instructions: Vec::new(),
            workspace_roots: vec![WorkspaceRoot {
                name: BoundedString::new("worker").unwrap(),
                path: BoundedString::new(root.path().display().to_string()).unwrap(),
                primary: true,
            }],
            max_tool_iterations: if compaction { 16 } else { 4 },
            firewall: None,
            sandbox: None,
        };

        #[cfg(feature = "docker")]
        if shared_service {
            use vesper_harness::swarm_service::{
                CancellationSignal, NativeSwarmService, SwarmRunContext,
            };
            let owner = Arc::new(NativeSwarmService::default());
            let settings = vesper_harness::swarm_settings::SwarmSettings {
                enabled: true,
                topology: kind,
                shared_scope,
                ..Default::default()
            };
            let context = SwarmRunContext {
                root: root.path().to_path_buf(),
                factory: WorkerFactory::new(registry.clone(), config),
                tools: ToolRegistry::parity_default(),
                allowed_tools: vec![tool.into()],
                mode: SessionOperatingMode::Code,
                permission: SessionPermissionMode::Ask,
                permission_port: approvals.clone(),
                progress: None,
                recalled_context: Some(
                    "local cognition fixture: verify the provided project files".into(),
                ),
                embedding: Some((Arc::new(Embedding), 8)),
                backend: Arc::new(vesper_sandbox::DockerBackend::new(Default::default())),
                backend_choice: vesper_agent::sandbox_route::SandboxBackendChoice::Docker,
            };
            let goal = HiveGoal::new("refusal", "inspect project");
            let disabled = vesper_harness::swarm_settings::SwarmSettings::default();
            assert!(
                owner
                    .run(
                        disabled,
                        context.clone(),
                        goal.clone(),
                        CancellationSignal::new()
                    )
                    .await
                    .is_err()
            );
            let mut missing = context.clone();
            missing.embedding = None;
            assert!(
                owner
                    .run(
                        settings.clone(),
                        missing,
                        goal.clone(),
                        CancellationSignal::new()
                    )
                    .await
                    .is_err()
            );
            let cancelled = vesper_harness::swarm_service::CancelFlag::new();
            cancelled.cancel();
            assert!(
                owner
                    .run(settings.clone(), context.clone(), goal, cancelled.signal())
                    .await
                    .is_err()
            );
            assert!(!root.path().join(".agent-vesper").exists());
            assert_eq!(trace.next.load(Ordering::SeqCst), 0);
            let report = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                owner.run(
                    settings.clone(),
                    context.clone(),
                    HiveGoal::new("native-service", "inspect three independent files"),
                    CancellationSignal::new(),
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(report.success, "{report:?}");
            assert!(report.cleanup.is_clean());
            assert!(report.output.contains("grounded-native-synthesis"));
            assert!(!owner.is_running());
            assert!(owner.last_report().unwrap().success);
            // VRO-16 composed governance: the shared-service run now
            // records the review-panel turns (round 0 + round 1 × three
            // reviewers) and the Navigator decision turn on top of the
            // original decompose + 3 tool-turn drivers + synthesis shape.
            // Worker sessions: 3 drivers + 1 navigator + 6 review turns +
            // 1 decision turn + 1 synthesis = 12; provider turns: 1
            // decompose + 3×3 driver + 6 review + 1 decide + 1 synthesis.
            assert_eq!(trace.next.load(Ordering::SeqCst), 12);
            assert_eq!(report.workers.len(), 12);
            assert_eq!(
                report
                    .workers
                    .iter()
                    .map(|worker| worker.provider_turns)
                    .sum::<usize>(),
                18
            );
            assert!(report.gate_events.iter().any(|event| matches!(
                event,
                vesper_swarm::hive::governance::AuditEvent::VerificationVerdict { .. }
            )));
            assert!(report.gate_events.iter().any(|event| matches!(
                event,
                vesper_swarm::hive::governance::AuditEvent::DecisionIssued { .. }
            )));
            assert!(
                report
                    .workers
                    .iter()
                    .all(|worker| !worker.history.is_empty())
            );
            assert!(trace.requests.lock().unwrap().iter().all(|(_, request)| {
                request.contains("local cognition fixture: verify the provided project files")
                    && request.contains("untrusted-recalled-context")
            }));
            let approval_records = approvals.0.lock().unwrap().clone();
            assert_eq!(approval_records.len(), 6);
            let scopes: std::collections::BTreeSet<_> = approval_records.iter().collect();
            assert_eq!(scopes.len(), 3);
            for scope in scopes {
                assert!(std::path::Path::new(scope).starts_with(&report.artifacts));
                assert_eq!(
                    std::fs::read_to_string(std::path::Path::new(scope).join("A.txt")).unwrap(),
                    "verified-A-native-file"
                );
                assert!(!std::path::Path::new(scope).join(".agent-vesper").exists());
            }
            assert!(
                !root.path().join("result.txt").exists(),
                "worker writes leaked into source project"
            );
            struct CancelProgress(vesper_harness::swarm_service::CancelFlag);
            impl vesper_agent::AgentProgressPort for CancelProgress {
                fn emit(&self, event: vesper_agent::AgentProgressEvent) {
                    if matches!(event, vesper_agent::AgentProgressEvent::ContentDelta { text } if text.as_str().starts_with("verified-"))
                    {
                        self.0.cancel();
                    }
                }
            }
            let cancel = vesper_harness::swarm_service::CancelFlag::new();
            let mut cancelled_context = context.clone();
            cancelled_context.progress = Some(Arc::new(CancelProgress(cancel.clone())));
            let cancelled_report = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                owner.run(
                    settings.clone(),
                    cancelled_context,
                    HiveGoal::new("cancelled-native", "inspect three independent files"),
                    cancel.signal(),
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(cancelled_report.cancelled);
            assert!(!cancelled_report.success);
            assert!(cancelled_report.cleanup.is_clean());
            assert!(cancelled_report.workers_settled);
            let cancelled_dispatches = trace.next.load(Ordering::SeqCst);
            struct ShutdownProgress(Arc<tokio::sync::Notify>);
            impl vesper_agent::AgentProgressPort for ShutdownProgress {
                fn emit(&self, event: vesper_agent::AgentProgressEvent) {
                    if matches!(event, vesper_agent::AgentProgressEvent::ContentDelta { text } if text.as_str().starts_with("verified-"))
                    {
                        self.0.notify_one();
                    }
                }
            }
            let started = Arc::new(tokio::sync::Notify::new());
            let mut shutdown_context = context.clone();
            shutdown_context.progress = Some(Arc::new(ShutdownProgress(started.clone())));
            let running_owner = owner.clone();
            let running_settings = settings.clone();
            let running = tokio::spawn(async move {
                running_owner
                    .run(
                        running_settings,
                        shutdown_context,
                        HiveGoal::new("shutdown-native", "inspect three independent files"),
                        CancellationSignal::new(),
                    )
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(30), started.notified())
                .await
                .unwrap();
            assert!(owner.shutdown(std::time::Duration::from_secs(35)).await);
            let shutdown_report = running.await.unwrap().unwrap();
            assert!(shutdown_report.cancelled && !shutdown_report.success);
            assert!(shutdown_report.workers_settled && shutdown_report.cleanup.is_clean());
            assert!(
                owner
                    .run(
                        settings,
                        context,
                        HiveGoal::new("after-shutdown", "must refuse"),
                        CancellationSignal::new()
                    )
                    .await
                    .is_err()
            );
            assert!(
                cancelled_report.output.contains("verified-"),
                "partial native output lost: {}",
                cancelled_report.output
            );
            assert!(cancelled_report.workers.iter().any(|worker| {
                serde_json::to_string(&worker.history)
                    .unwrap()
                    .contains("verified-")
            }));
            // Cancellation after the three driver tasks: with governance
            // composed, the cancelled goal has dispatched decompose (1) +
            // 3 drivers (9 sessions? no — 3 driver sessions) + the round-0
            // review wave was in flight; the exact number is the sessions
            // created before the cancel signal landed, which now includes
            // the three round-0 reviewers. Synthesis, decision and the
            // second review round must NOT have run.
            assert!(
                (12..=16).contains(&cancelled_dispatches),
                "cancelled goal must not run post-evidence phases: {cancelled_dispatches}"
            );
            // The shutdown-race goal legitimately created sessions before
            // its cancel landed; what must never happen is any NEW session
            // after the service is closed (the refused "after-shutdown"
            // goal dispatches nothing).
            let after_refusal = trace.next.load(Ordering::SeqCst);
            let shutdown_report_sessions = after_refusal - cancelled_dispatches;
            assert!(
                shutdown_report_sessions > 0,
                "shutdown-race goal should have dispatched before its cancel"
            );
            continue;
        }
        #[cfg(not(feature = "docker"))]
        assert!(
            !shared_service,
            "shared contained service gate requires docker"
        );
        let factories: Vec<(String, Arc<dyn WorkerInstanceFactory>)> = ["navigator", "driver"]
            .into_iter()
            .map(|role| {
                let allowed = if role == "driver" {
                    vec![tool.into()]
                } else {
                    Vec::new()
                };
                let port = ProviderWorkerPort::new(
                    WorkerFactory::new(registry.clone(), config.clone()),
                    ToolRegistry::parity_default(),
                    allowed,
                    SessionOperatingMode::Code,
                    if sandbox {
                        SessionPermissionMode::Ask
                    } else {
                        SessionPermissionMode::ReadOnly
                    },
                    if sandbox {
                        approvals.clone()
                    } else {
                        Arc::new(DenyPermissionPort)
                    },
                );
                let mut factory = port.into_instance_factory();
                if let Some(leases) = &sandbox_leases {
                    factory = factory.with_sandbox_leases(leases.clone(), role.into());
                }
                (
                    role.into(),
                    Arc::new(factory) as Arc<dyn WorkerInstanceFactory>,
                )
            })
            .collect();
        let mut hive_config = HiveConfig::balanced(&[tool]);
        hive_config.topology_kind = kind;
        hive_config.topology_config.failover_enabled = sandbox;
        hive_config.roles[0].allowed_tools.clear();
        hive_config.roles[1].min_workers = 3;
        let mut hive = Hive::with_factories(hive_config, factories, Arc::new(Embedding))
            .await
            .unwrap();
        hive.admit_topology().unwrap();
        hive.submit(HiveGoal::new("native", "inspect three independent files"))
            .unwrap();
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(if sandbox { 90 } else { 5 }),
                hive.run_to_completion()
            )
            .await
            .unwrap()
            .unwrap(),
            1
        );
        assert_eq!(
            trace.next.load(Ordering::SeqCst),
            5,
            "decompose + 3 isolated driver sessions + synthesis"
        );
        assert_eq!(
            trace.requests.lock().unwrap().len(),
            if compaction {
                29 + trace.compactions.load(Ordering::SeqCst)
            } else if sandbox {
                11
            } else {
                8
            },
            "exact native tool continuation count"
        );
        if compaction {
            assert!(trace.compactions.load(Ordering::SeqCst) >= 3);
            assert!(
                trace
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, messages)| messages.contains("compaction-"))
            );
        }
        let assignments: std::collections::BTreeSet<_> = hive
            .events()
            .into_iter()
            .filter_map(|event| {
                if let HiveEvent::TaskAssigned(_, node, _) = event {
                    Some(node)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(assignments.len(), 3);
        assert_eq!(hive.ledger().len(), 4);
        assert!(
            !root.path().join(".agent-vesper").exists(),
            "no implicit durable worker state"
        );
        if let Some(leases) = &sandbox_leases {
            assert_eq!(hive.scale_role("driver", 1).await.unwrap(), 4);
            assert_eq!(leases.cleanup_report().held, 5);
            assert_eq!(hive.scale_role("driver", -1).await.unwrap(), 3);
            let settled = leases
                .settle(std::time::Duration::from_secs(10))
                .await
                .unwrap();
            assert_eq!(settled.held, 4);
            assert_eq!(settled.pending + settled.quarantined, 0);
            assert_eq!(
                hive.maintain_workers(
                    tokio::time::Instant::now() + std::time::Duration::from_secs(600)
                )
                .await
                .unwrap(),
                4
            );
            hive.submit(HiveGoal::new(
                "after-replacement",
                "inspect three independent files",
            ))
            .unwrap();
            assert_eq!(hive.run_to_completion().await.unwrap(), 1);
        }
        if !sandbox {
            let requests_before = trace.requests.lock().unwrap().len();
            assert!(
                hive.maintain_workers(
                    tokio::time::Instant::now() + std::time::Duration::from_secs(60)
                )
                .await
                .is_err(),
                "disabled navigator failover must close native dispatch"
            );
            assert!(
                hive.submit(HiveGoal::new(
                    "after-failed-navigator",
                    "must not fall back"
                ))
                .is_err()
            );
            assert_eq!(
                trace.requests.lock().unwrap().len(),
                requests_before,
                "disabled failover must not dispatch a replacement or replay"
            );
        }
        hive.close();
        assert!(
            hive.settle_workers(std::time::Duration::from_secs(10))
                .await
        );
        drop(hive);
        if let Some(leases) = sandbox_leases {
            let report = leases
                .shutdown(std::time::Duration::from_secs(10))
                .await
                .unwrap();
            assert!(
                report.is_clean(),
                "native cleanup unresolved: {report:?}; {:?}",
                leases.cleanup_errors()
            );
            let approvals = approvals.0.lock().unwrap();
            assert_eq!(approvals.len(), 12);
            let roots: std::collections::BTreeSet<_> = approvals.iter().cloned().collect();
            assert_eq!(
                roots.len(),
                6,
                "selected workers must have distinct permission scopes"
            );
            for worker_root in roots {
                assert!(
                    std::path::Path::new(&worker_root).starts_with(root.path().join("workers"))
                );
                let result =
                    std::fs::read_to_string(std::path::Path::new(&worker_root).join("result.txt"))
                        .unwrap();
                assert!(result.starts_with("verified-"));
            }
        }
    }
}

#[derive(Default)]
struct Permissions(Mutex<Vec<String>>);
impl vesper_agent::PermissionPort for Permissions {
    fn authorize<'a>(
        &'a self,
        call: &'a ToolCall,
        _: &'a ToolDefinition,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, vesper_agent::PermissionDecision> {
        Box::pin(async move {
            assert_eq!(call.tool_id.as_str(), "run_command");
            assert!(context.sandbox.is_some());
            assert_eq!(context.workspace_roots.len(), 1);
            self.0
                .lock()
                .unwrap()
                .push(context.workspace_roots[0].path.as_str().into());
            vesper_agent::PermissionDecision::Allow
        })
    }
}

#[cfg(feature = "docker")]
#[tokio::test]
#[ignore = "requires a real container runtime and explicitly selected local image"]
async fn native_shared_service_runs_permissioned_workers_with_project_inputs_and_cleanup() {
    run_hive(true, true, true, false, false).await;
}

#[tokio::test]
async fn native_hive_compacts_real_tool_transactions_and_finishes_without_replay() {
    run_hive(false, false, false, true, false).await;
}

#[cfg(feature = "docker")]
#[tokio::test]
#[ignore = "requires bundled Landlock image and real container isolation"]
async fn native_shared_scope_hive_runs_all_topologies_and_cancellation() {
    run_hive(true, true, true, false, true).await;
}
