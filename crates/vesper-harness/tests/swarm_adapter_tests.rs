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
        ToolRegistry::parity_default(),
        allowed.iter().map(|name| (*name).into()).collect(),
        SessionOperatingMode::Code,
        permission,
        Arc::new(DenyPermissionPort),
    )
}
fn transcript(port: &ProviderWorkerPort) -> String {
    serde_json::to_string(&port.history()).unwrap()
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
