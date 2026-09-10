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
    next: AtomicUsize,
    requests: Mutex<Vec<(usize, String)>>,
}
struct Factory {
    sandbox: bool,
    id: ProviderId,
    barrier: Arc<tokio::sync::Barrier>,
    trace: Arc<Trace>,
}
struct Session {
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
        _: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        Box::pin(async move {
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
            let turn = self.turn.fetch_add(1, Ordering::SeqCst);
            let events = if messages.contains("Return only JSON") {
                assert!(request.tools.is_empty());
                vec![
                    text(
                        &r#"{"tasks":[{"prompt":"driver-A","required_capabilities":["read_file"]},{"prompt":"driver-B","required_capabilities":["read_file"]},{"prompt":"driver-C","required_capabilities":["read_file"]}]}"#.replace("read_file", tool),
                    ),
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
                if turn == 0 {
                    // Serial or aliased native workers cannot pass this barrier.
                    self.barrier.wait().await;
                    vec![
                        ProviderStreamEvent::ToolCallCompleted(ToolCall {
                            id: ToolCallId::new("read").unwrap(),
                            tool_id: ToolId::new(tool).unwrap(),
                            arguments: if self.sandbox {
                                serde_json::json!({"command":format!("printf verified-{label}-native-file > result.txt; cat result.txt"), "timeout_seconds":5})
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
                    assert_eq!(turn, if self.sandbox { 2 } else { 1 }, "unexpected replay");
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
    run_hive(false, false).await;
}

#[tokio::test]
#[ignore = "requires the real namespace supervisor; fails rather than skips when unavailable"]
async fn native_scoped_hive_runs_two_permissioned_commands_per_worker_and_verifies_cleanup() {
    run_hive(true, false).await;
}

#[cfg(feature = "docker")]
#[tokio::test]
#[ignore = "requires a real container runtime and explicitly selected local image"]
async fn native_container_hive_runs_two_permissioned_commands_per_worker_and_verifies_cleanup() {
    run_hive(true, true).await;
}

async fn run_hive(sandbox: bool, container: bool) {
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
            context_window_tokens: 131_072,
            system_instructions: Vec::new(),
            workspace_roots: vec![WorkspaceRoot {
                name: BoundedString::new("worker").unwrap(),
                path: BoundedString::new(root.path().display().to_string()).unwrap(),
                primary: true,
            }],
            max_tool_iterations: 4,
            firewall: None,
            sandbox: None,
        };
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
            if sandbox { 11 } else { 8 },
            "exact native tool continuation count"
        );
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
        hive.close();
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
