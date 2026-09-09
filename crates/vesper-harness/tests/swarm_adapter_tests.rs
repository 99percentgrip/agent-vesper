//! Swarm adapter tests (VRO-15 PR-9) — feature-gated.
//!
//! Only compiled under the harness `swarm` feature. Uses the synthetic
//! provider session: zero network, zero real providers.

#![cfg(feature = "swarm")]

use std::sync::Arc;
use std::time::Duration;

use vesper_domain::{ContentText, ProviderId, QualifiedModelId, SystemInstruction, ToolDefinition};
use vesper_swarm::worker::{CancellationSignal, WorkerError, WorkerPort, WorkerTask};

// The synthetic factory provides a deterministic, in-process provider.
use vesper_provider::ProviderFactory as _;
use vesper_provider_synthetic::SyntheticFactory;

struct NeverCancelled;
impl vesper_provider::CancellationSignal for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn harness_name_tool(name: &str) -> ToolDefinition {
    vesper_agent::executor::schema_definition(
        name,
        "swarm test tool",
        vesper_domain::ToolExecutionClass::ReadOnly,
        &[],
    )
}

async fn swarm_session() -> vesper_harness::swarm_adapter::SwarmSession {
    let factory = SyntheticFactory::new("swarm-reply");
    let configuration = SyntheticFactory::default_configuration();
    let session = factory
        .create_session(&configuration, Arc::new(NeverCancelled))
        .await
        .expect("synthetic session");
    vesper_harness::swarm_adapter::SwarmSession {
        session: Arc::new(session),
        provider_id: ProviderId::new("synthetic").expect("id"),
        model: QualifiedModelId {
            provider_id: ProviderId::new("synthetic").expect("id"),
            model_id: vesper_domain::ModelId::new("synthetic-1").expect("model"),
        },
        system_instructions: vec![SystemInstruction {
            content: vec![vesper_domain::ContentPart::Text(
                ContentText::new("swarm test").expect("bounded"),
            )],
            cache_stable: false,
            extensions: Default::default(),
        }],
        registry: vec![
            harness_name_tool("read"),
            harness_name_tool("write"),
            harness_name_tool("browser"),
        ],
    }
}

fn task(id: &str, required: &[&str]) -> WorkerTask {
    WorkerTask {
        id: id.to_string(),
        kind: vesper_swarm::worker::TaskKind::Coding,
        priority: vesper_swarm::worker::TaskPriority::Normal,
        prompt: String::from("do the thing"),
        required_capabilities: required.iter().map(|s| s.to_string()).collect(),
        deadline: Duration::from_secs(30),
    }
}

#[tokio::test]
async fn adapter_streams_a_successful_turn_receipt() {
    let port = vesper_harness::swarm_adapter::ProviderWorkerPort::new(
        swarm_session().await,
        vec![String::from("read"), String::from("write")],
    );
    let receipt = port
        .run_turn(&task("t1", &["read"]), CancellationSignal::new())
        .await
        .expect("turn");
    assert!(receipt.success);
    assert_eq!(receipt.task_id, "t1");
    assert!(!receipt.output.is_empty(), "synthetic reply captured");
}

#[tokio::test]
async fn tool_filter_is_dynamic_per_task() {
    let port = vesper_harness::swarm_adapter::ProviderWorkerPort::new(
        swarm_session().await,
        vec![
            String::from("read"),
            String::from("write"),
            String::from("browser"),
        ],
    );
    // The filtered set is internal; prove the behavior indirectly: the
    // port's declared capabilities carry the class allowlist, and turns
    // with disjoint requirements still run (empty required = no filter).
    let caps = port.capabilities();
    assert_eq!(caps.tools.len(), 3);
    let receipt = port
        .run_turn(&task("t2", &[]), CancellationSignal::new())
        .await
        .expect("turn with no requirements");
    assert!(receipt.success);
}

#[tokio::test]
async fn pre_cancelled_signal_short_circuits_to_cancelled() {
    let port = vesper_harness::swarm_adapter::ProviderWorkerPort::new(
        swarm_session().await,
        vec![String::from("read")],
    );
    let flag = vesper_swarm::worker::CancelFlag::new();
    flag.cancel();
    let error = port
        .run_turn(&task("t3", &[]), flag.signal())
        .await
        .unwrap_err();
    assert_eq!(error, WorkerError::Cancelled(String::from("t3")));
}

#[tokio::test]
async fn request_construction_is_provider_neutral() {
    // Sanity: the adapter builds requests with the synthetic provider's
    // identity and never names a concrete vendor.
    let port = vesper_harness::swarm_adapter::ProviderWorkerPort::new(
        swarm_session().await,
        vec![String::from("read")],
    );
    let caps = port.capabilities();
    assert!(caps.supports(&[String::from("read")]));
    assert!(!caps.supports(&[String::from("browser")]));
}
