//! Adapter tests: deterministic fakes (no application required) plus
//! env-gated live tests that only run when the real target exists.

use std::sync::Arc;

use serde_json::json;
use vesper_agent::ToolService;
use vesper_bridge::adapter::{AdapterOutcome, AdapterPort};
use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::error::BridgeError;
use vesper_bridge::operation::{OperationOutcome, OperationRequestId, OperationSpec};
use vesper_bridge::session::StopOutcome;
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId};

use crate::bridge_service::{
    BRIDGE_TOOL_CONNECT, BRIDGE_TOOL_EXECUTE, BRIDGE_TOOL_OBSERVE, BridgeToolService,
};

/// Deterministic fake: records dispatches, returns a fixed outcome.
struct FakeAdapter {
    fail: bool,
    dispatched: std::sync::Mutex<Vec<String>>,
}

impl AdapterPort for FakeAdapter {
    fn manifest(&self) -> &CapabilityManifest {
        // static-ish: rebuild each call is wasteful but fine for tests
        Box::leak(Box::new(manifest()))
    }

    fn dispatch(
        &self,
        _request_id: &OperationRequestId,
        spec: &OperationSpec,
    ) -> Result<AdapterOutcome, BridgeError> {
        self.dispatched
            .lock()
            .unwrap()
            .push(spec.capability.0.clone());
        if self.fail {
            return Err(BridgeError::Transport {
                detail: "fake transport failure".into(),
            });
        }
        Ok(AdapterOutcome {
            outcome: OperationOutcome::Applied,
            summary: format!("fake applied {}", spec.capability.0),
            evidence: String::new(),
            job_created: false,
        })
    }

    fn job_status(&self, _job_id: &str) -> Option<StopOutcome> {
        None
    }
    fn release_inputs(&self) -> String {
        "fake owns no inputs".into()
    }
    fn describe(&self) -> String {
        "fake adapter".into()
    }
    fn health(&self) -> Option<bool> {
        Some(true)
    }
}

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "fake",
        1,
        vec![CapabilityRecord {
            id: CapabilityId::new("fake.op.hello").unwrap(),
            schema_version: 1,
            availability: Availability::Available,
            implementation: Implementation::Native,
            mutability: Mutability::Mutating,
            route: RouteKind::NativeApi,
            delivery: DeliveryMode::Foreground,
            verification: VerificationMethod::Independent,
            limitations: "test fake".into(),
        }],
    )
}

fn context() -> vesper_agent::ToolContext {
    vesper_agent::executor::uncancellable_context(
        vec![],
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
}

async fn call(service: &BridgeToolService, tool: &str, args: serde_json::Value) -> String {
    ToolService::execute(
        service,
        &ToolCall {
            id: ToolCallId::new("t1").unwrap(),
            tool_id: ToolId::new(tool).unwrap(),
            arguments: args,
            extensions: Default::default(),
        },
        &context(),
    )
    .await
    .map(|r| r.text.to_string())
    .unwrap_or_else(|e| format!("ERROR: {e}"))
}

#[tokio::test]
async fn adapter_dispatch_reaches_the_fake_and_settles_applied() {
    let adapter = Arc::new(FakeAdapter {
        fail: false,
        dispatched: std::sync::Mutex::new(Vec::new()),
    });
    let service = BridgeToolService::no_adapter().with_adapter(adapter.clone());

    let connect = call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    assert!(connect.contains("fake adapter"), "connect: {connect}");

    let observe = call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    assert!(observe.contains("observation"), "observe: {observe}");

    let exec = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "fake.op.hello", "arguments": {}}),
    )
    .await;
    assert!(exec.contains("applied"), "exec: {exec}");
    assert!(exec.contains("fake applied fake.op.hello"), "exec: {exec}");

    let dispatched = adapter.dispatched.lock().unwrap().clone();
    assert_eq!(dispatched, vec!["fake.op.hello".to_string()]);
}

#[tokio::test]
async fn adapter_failure_settles_failed_and_reports_transport() {
    let adapter = Arc::new(FakeAdapter {
        fail: true,
        dispatched: std::sync::Mutex::new(Vec::new()),
    });
    let service = BridgeToolService::no_adapter().with_adapter(adapter);

    call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    let exec = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "fake.op.hello", "arguments": {}}),
    )
    .await;
    assert!(exec.contains("ERROR"), "exec must surface failure: {exec}");
    assert!(exec.contains("fake transport failure"), "exec: {exec}");
}

#[tokio::test]
async fn no_adapter_composition_still_refuses_truthfully() {
    let service = BridgeToolService::no_adapter();
    call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    let exec = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "bridge.placeholder.none", "arguments": {}}),
    )
    .await;
    // The no-adapter manifest marks its only capability dependency_missing,
    // so the CORE denies at the capability gate (step 3) — before any
    // adapter handoff. That denial IS the truthful no-adapter behavior.
    assert!(
        exec.contains("capability_unavailable"),
        "exec must be a truthful capability denial, got: {exec}"
    );
}

/// LIVE: requires VESPER_BRIDGE_RESOLVE_IPC + a running worker.
#[tokio::test]
#[ignore = "live: set VESPER_BRIDGE_RESOLVE_IPC with the worker pasted in Resolve"]
async fn live_resolve_worker_status_roundtrip() {
    let dir = std::env::var("VESPER_BRIDGE_RESOLVE_IPC").unwrap();
    let adapter = Arc::new(crate::bridge_adapters::FileIpcAdapter::new(dir));
    let service = BridgeToolService::no_adapter().with_adapter(adapter);
    let connect = call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    // Describe() lowercases? No — describe() returns "Resolve free-edition
    // worker (file IPC at …)". Case-sensitive but exact.
    assert!(
        connect.to_lowercase().contains("resolve"),
        "connect: {connect}"
    );
    call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    let exec = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "resolve.worker.status", "arguments": {}}),
    )
    .await;
    assert!(
        exec.to_lowercase().contains("davinci resolve"),
        "exec must carry live app state, got: {exec}"
    );
}

/// LIVE: requires a running MPRIS player (e.g. Elisa).
#[tokio::test]
#[ignore = "live: run any MPRIS player first"]
async fn live_mpris_player_control() {
    let players = crate::bridge_adapters::discover_mpris_players();
    assert!(!players.is_empty(), "no MPRIS player running");
    let adapter = Arc::new(crate::bridge_adapters::MprisAdapter::new(
        players.into_iter().next().unwrap(),
    ));
    let service = BridgeToolService::no_adapter().with_adapter(adapter);
    let connect = call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    assert!(connect.contains("MPRIS"), "{connect}");
    call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    // Read-only status through the full authorization path.
    let exec = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "mpris.player.status", "arguments": {}}),
    )
    .await;
    assert!(
        exec.contains("applied") || exec.contains("verified"),
        "{exec}"
    );
}
