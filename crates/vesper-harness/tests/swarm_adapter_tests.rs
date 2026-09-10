//! Default-off native swarm adapter acceptance through the real AgentLoop.
#![cfg(feature = "swarm")]
use std::sync::Arc;
use vesper_agent::{AgentLoopConfig, DenyPermissionPort, ToolRegistry};
use vesper_domain::*;
use vesper_harness::{WorkerFactory, swarm_adapter::ProviderWorkerPort};
use vesper_provider::{
    CancellationSignal as ProviderCancellation, ProviderConfiguration, ProviderError,
    ProviderFactory, ProviderFuture, ProviderStreamEvent,
};
use vesper_runtime::ProviderRegistry;
use vesper_swarm::worker::{CancellationSignal, WorkerError, WorkerPort, WorkerTask};
use vesper_testkit::FakeProviderSession;

struct Factory {
    id: ProviderId,
    session: FakeProviderSession,
}
impl ProviderFactory for Factory {
    type Session = FakeProviderSession;
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn create_session<'a>(
        &'a self,
        _: &'a ProviderConfiguration,
        _: Arc<dyn ProviderCancellation>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
}
fn completed(finish: FinishOutcome) -> ProviderStreamEvent {
    ProviderStreamEvent::Completed {
        finish,
        metadata: Default::default(),
    }
}
fn script(tool: &str, arguments: serde_json::Value) -> FakeProviderSession {
    FakeProviderSession::with_scripts([
        Ok(vec![
            Ok(ProviderStreamEvent::ToolCallCompleted(ToolCall {
                id: ToolCallId::new("call-1").unwrap(),
                tool_id: ToolId::new(tool).unwrap(),
                arguments,
                extensions: Default::default(),
            })),
            Ok(completed(FinishOutcome::ToolCalls)),
        ]),
        Ok(vec![
            Ok(ProviderStreamEvent::ContentDelta {
                stream_id: BoundedString::new("text").unwrap(),
                part: ContentPart::Text(ContentText::new("done").unwrap()),
            }),
            Ok(completed(FinishOutcome::Stop)),
        ]),
    ])
}
async fn port(
    root: &std::path::Path,
    session: FakeProviderSession,
    allowed: &[&str],
    permission: SessionPermissionMode,
) -> ProviderWorkerPort {
    port_with_tools(
        root,
        session,
        allowed,
        permission,
        ToolRegistry::parity_default(),
    )
    .await
}
async fn port_with_tools(
    root: &std::path::Path,
    session: FakeProviderSession,
    allowed: &[&str],
    permission: SessionPermissionMode,
    tools: ToolRegistry,
) -> ProviderWorkerPort {
    let id = ProviderId::new("test.swarm").unwrap();
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(Factory {
            id: id.clone(),
            session,
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
            path: BoundedString::new(root.display().to_string()).unwrap(),
            primary: true,
        }],
        max_tool_iterations: 4,
        firewall: None,
        sandbox: None,
    };
    ProviderWorkerPort::new(
        WorkerFactory::new(registry, config),
        tools,
        allowed.iter().map(|name| (*name).into()).collect(),
        SessionOperatingMode::Code,
        permission,
        Arc::new(DenyPermissionPort),
    )
}
fn transcript(port: &ProviderWorkerPort) -> String {
    serde_json::to_string(&port.history()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit real browser -> native worker -> captured provider continuation"]
async fn real_browser_feedback_reaches_native_provider_continuation() {
    struct Empty;
    impl vesper_agent::ToolService for Empty {
        fn definitions(&self) -> Vec<ToolDefinition> {
            Vec::new()
        }
        fn execute<'a>(
            &'a self,
            _: &'a ToolCall,
            _: &'a vesper_agent::ToolContext,
        ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
        {
            Box::pin(async { Err(vesper_agent::ToolError::Failed("unexpected tool".into())) })
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let service = vesper_harness::lens_tools::LensToolService::new(
        Arc::new(Empty),
        Arc::new(vesper_harness::lens_tools::NativeLensPort::new()),
        Arc::new(move |url| {
            tx.send(url.to_owned()).unwrap();
        }),
        12,
        "Auto".into(),
    );
    let session = script(
        "request_human_input",
        serde_json::json!({"title":"Native worker interview", "questions":[{"id":"scope", "prompt":"Choose scope", "options":["Patch", "Minor"], "required":true}]}),
    );
    let worker = port_with_tools(
        root.path(),
        session.clone(),
        &["request_human_input"],
        SessionPermissionMode::ReadOnly,
        ToolRegistry::parity_default().with_service(Arc::new(service)),
    )
    .await;
    let browser = async {
        let url = rx.recv().await.unwrap();
        let status = tokio::process::Command::new(
            std::env::var("VESPER_TEST_NODE").unwrap_or_else(|_| "node".into()),
        )
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/swarm_lens_browser.mjs"
        ))
        .arg(url)
        .kill_on_drop(true)
        .status()
        .await
        .unwrap();
        assert!(status.success());
    };
    let task = WorkerTask::new("browser", "Ask for a scope and use the human response");
    let (receipt, ()) = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        tokio::join!(worker.run_turn(&task, CancellationSignal::new()), browser)
    })
    .await
    .unwrap();
    assert!(receipt.unwrap().success);
    let requests = session.requests();
    assert_eq!(
        requests.len(),
        2,
        "one tool call and one native continuation, no replay"
    );
    let captured = serde_json::to_string(&requests[1].messages).unwrap();
    assert!(captured.contains("native-browser-feedback-472"));
    assert!(captured.contains("scope") && captured.contains("Patch"));
    assert!(requests[1].messages.iter().flat_map(|message| &message.content).any(|part| matches!(part, ContentPart::ToolResult(result) if result.status == ToolResultStatus::Succeeded)));
    assert!(transcript(&worker).contains("native-browser-feedback-472"));
    assert!(!root.path().join(".agent-vesper").exists());
}

#[tokio::test]
async fn native_tool_turn_executes_and_returns_result_to_provider() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("input.txt"), "fixture-evidence-472").unwrap();
    let session = script("read_file", serde_json::json!({"path":"input.txt"}));
    let port = port(
        root.path(),
        session.clone(),
        &["read_file"],
        SessionPermissionMode::ReadOnly,
    )
    .await;
    let receipt = port
        .run_turn(
            &WorkerTask::new("read", "inspect input"),
            CancellationSignal::new(),
        )
        .await
        .unwrap();
    assert!(receipt.success);
    assert_eq!(receipt.output, "done");
    let requests = session.requests();
    assert_eq!(requests.len(), 2, "real tool continuation, not one stream");
    assert!(
        serde_json::to_string(&requests[1].messages)
            .unwrap()
            .contains("fixture-evidence-472")
    );
    assert!(transcript(&port).contains("fixture-evidence-472"));
    assert!(
        requests.iter().all(|request| request.tools.len() == 1
            && request.tools[0].harness_name.as_str() == "read_file")
    );
}

#[tokio::test]
async fn native_permission_denial_and_role_restriction_cannot_write() {
    for (allowed, permission) in [
        (vec!["write_file"], SessionPermissionMode::ReadOnly),
        (vec!["read_file"], SessionPermissionMode::Bypass),
    ] {
        let root = tempfile::tempdir().unwrap();
        let session = script(
            "write_file",
            serde_json::json!({"path":"forbidden.txt","content":"must not write"}),
        );
        let port = port(root.path(), session.clone(), &allowed, permission).await;
        port.run_turn(
            &WorkerTask::new("denied", "work"),
            CancellationSignal::new(),
        )
        .await
        .unwrap();
        assert!(!root.path().join("forbidden.txt").exists());
        assert_eq!(session.requests().len(), 2);
        assert!(transcript(&port).contains(r#""status":"failed""#));
    }
}

#[tokio::test]
async fn cancellation_and_invalid_prompt_refuse_before_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let session = FakeProviderSession::with_scripts([]);
    let port = port(
        root.path(),
        session.clone(),
        &[],
        SessionPermissionMode::ReadOnly,
    )
    .await;
    let flag = vesper_swarm::worker::CancelFlag::new();
    flag.cancel();
    assert!(matches!(
        port.run_turn(&WorkerTask::new("cancel", "work"), flag.signal())
            .await,
        Err(WorkerError::Cancelled(_))
    ));
    assert!(
        port.run_turn(
            &WorkerTask::new("large", "x".repeat(1_048_577)),
            CancellationSignal::new()
        )
        .await
        .is_err()
    );
    assert!(session.requests().is_empty());
}

#[tokio::test]
async fn native_incomplete_terminal_is_not_success() {
    let root = tempfile::tempdir().unwrap();
    let session =
        FakeProviderSession::with_scripts([Ok(vec![Ok(completed(FinishOutcome::OutputLimit))])]);
    let port = port(root.path(), session, &[], SessionPermissionMode::ReadOnly).await;
    assert!(
        port.run_turn(
            &WorkerTask::new("length", "work"),
            CancellationSignal::new()
        )
        .await
        .is_err()
    );
}

#[test]
fn restricted_registry_drops_prefix_gateways_and_unnamed_tools() {
    let registry = ToolRegistry::parity_default()
        .with_gateway("mcp__", Arc::new(vesper_agent::tools::ReadFile));
    let restricted = registry.restricted_to(&["read_file".into()]);
    assert!(restricted.contains("read_file"));
    assert!(!restricted.contains("write_file"));
    assert!(!restricted.contains("mcp__anything"));
    assert!(
        registry.contains("write_file"),
        "original registry unchanged"
    );
}

#[tokio::test]
async fn native_interruption_preserves_visible_output_and_history_without_success() {
    let root = tempfile::tempdir().unwrap();
    let session = FakeProviderSession::with_scripts([Ok(vec![
        Ok(ProviderStreamEvent::ContentDelta {
            stream_id: BoundedString::new("text").unwrap(),
            part: ContentPart::Text(ContentText::new("visible partial answer").unwrap()),
        }),
        Ok(completed(FinishOutcome::StreamInterrupted {
            cause: Default::default(),
            tool_call_started: true,
        })),
    ])]);
    let port = port(
        root.path(),
        session.clone(),
        &[],
        SessionPermissionMode::ReadOnly,
    )
    .await;
    let receipt = port
        .run_turn(
            &WorkerTask::new("partial", "work"),
            CancellationSignal::new(),
        )
        .await
        .unwrap();
    assert!(!receipt.success);
    assert_eq!(receipt.output, "visible partial answer");
    assert!(transcript(&port).contains("visible partial answer"));
    assert_eq!(
        session.requests().len(),
        1,
        "ambiguous tool activity cannot replay"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_native_observer_retains_busy_ownership_and_partial_history() {
    struct Gate {
        entered: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl vesper_agent::AgentProgressPort for Gate {
        fn emit(&self, event: vesper_agent::AgentProgressEvent) {
            if matches!(event, vesper_agent::AgentProgressEvent::ContentDelta { .. })
                && let Some(entered) = self.entered.lock().unwrap().take()
            {
                let _ = entered.send(());
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let provider = FakeProviderSession::with_scripts([Ok(vec![
        Ok(ProviderStreamEvent::ContentDelta {
            stream_id: BoundedString::new("text").unwrap(),
            part: ContentPart::Text(ContentText::new("already visible native evidence").unwrap()),
        }),
        Ok(completed(FinishOutcome::Stop)),
    ])]);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let worker = Arc::new(
        port(
            root.path(),
            provider.clone(),
            &[],
            SessionPermissionMode::ReadOnly,
        )
        .await
        .with_progress_port(Arc::new(Gate {
            entered: std::sync::Mutex::new(Some(entered_tx)),
            resume: std::sync::Mutex::new(resume_rx),
        })),
    );
    let observed = worker.clone();
    let task = tokio::spawn(async move {
        observed
            .run_turn(
                &WorkerTask::new("interrupted", "inspect"),
                CancellationSignal::new(),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        worker.pending_work(),
        "caller drop cannot retire native state"
    );
    assert!(matches!(
        worker
            .run_turn(
                &WorkerTask::new("overlap", "work"),
                CancellationSignal::new()
            )
            .await,
        Err(WorkerError::Rejected(_))
    ));
    resume_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while worker.pending_work() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        transcript(&worker).contains("already visible native evidence"),
        "{}",
        transcript(&worker)
    );
    assert_eq!(
        provider.requests().len(),
        1,
        "cancelled work must not replay"
    );
}

#[tokio::test]
async fn independent_pool_factory_executes_native_tools_without_a_second_loop() {
    use vesper_swarm::pool::{PoolConfig, WorkerPool};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("input.txt"), "pool-native-evidence").unwrap();
    let session = script("read_file", serde_json::json!({"path":"input.txt"}));
    let recipe = port(
        root.path(),
        session.clone(),
        &["read_file"],
        SessionPermissionMode::ReadOnly,
    )
    .await;
    let pool = WorkerPool::with_factory(
        PoolConfig {
            min_workers: 3,
            max_workers: 3,
            ..Default::default()
        },
        Arc::new(recipe.into_instance_factory()),
    )
    .unwrap();
    pool.initialize().await.unwrap();
    assert_eq!(pool.idle_workers(), 3);
    let receipt = pool
        .run_task(WorkerTask::new("pooled-read", "inspect input"))
        .await
        .unwrap();
    assert!(receipt.success);
    assert_eq!(receipt.output, "done");
    let requests = session.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        serde_json::to_string(&requests[1].messages)
            .unwrap()
            .contains("pool-native-evidence")
    );
    pool.close();
}
