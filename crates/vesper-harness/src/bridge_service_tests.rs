//! VB-PRD-001 Phase 2 evidence: disabled-path and gated-composition tests
//! for the hosted Bridge tool service (BR-30, NF-01, AT-01, AT-05/AT-06
//! core-route coverage).
//!
//! These are integration-class tests over the real harness service paths.
//! They prove advertisement gating, honest refusal text and permission
//! classification routing. They do NOT certify any application-control
//! capability — no adapter exists in this phase and none is simulated as
//! production behavior.

#![cfg(feature = "bridge")]

use std::sync::Arc;

use vesper_agent::ToolService;
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId};

use crate::MemoryStores;
use crate::bridge_service::{
    BRIDGE_TOOL_EXECUTE, BRIDGE_TOOL_NAMES, BridgeToolService, bridge_definitions,
};

fn gated_bridge() -> BridgeToolService {
    use vesper_bridge::capability::{
        CapabilityId, CapabilityRecord, DeliveryMode, Implementation, Mutability, RouteKind,
        VerificationMethod,
    };
    let mut record = CapabilityRecord {
        id: CapabilityId::new("media.timeline.create").unwrap(),
        schema_version: 1,
        availability: vesper_bridge::capability::Availability::Available,
        implementation: Implementation::Native,
        mutability: Mutability::Mutating,
        route: RouteKind::NativeApi,
        delivery: DeliveryMode::Background,
        verification: VerificationMethod::Independent,
        limitations: "test manifest; no adapter is attached".into(),
    };
    record.id = CapabilityId::new("media.timeline.create").unwrap();
    BridgeToolService::with_manifest(vesper_bridge::capability::CapabilityManifest::new(
        "test-gate",
        1,
        vec![record],
    ))
}

fn service() -> Arc<crate::HarnessToolService> {
    let stores = Arc::new(MemoryStores {
        memory: None,
        skills: None,
        profile: None,
        awareness: None,
    });
    let mut harness = crate::HarnessToolService::new(
        stores,
        std::path::PathBuf::new(),
        std::path::PathBuf::new(),
        None,
    );
    let _ = &mut harness;
    Arc::new(harness)
}

fn gated_service(enabled: bool) -> Arc<crate::HarnessToolService> {
    let stores = Arc::new(MemoryStores {
        memory: None,
        skills: None,
        profile: None,
        awareness: None,
    });
    let harness = crate::HarnessToolService::new(
        stores,
        std::path::PathBuf::new(),
        std::path::PathBuf::new(),
        None,
    )
    .with_bridge(enabled);
    Arc::new(harness)
}

fn call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("t1").unwrap(),
        tool_id: ToolId::new(name).unwrap(),
        arguments: args,
        extensions: Default::default(),
    }
}

fn context(
    mode: SessionOperatingMode,
    permission: SessionPermissionMode,
) -> vesper_agent::ToolContext {
    vesper_agent::executor::uncancellable_context(vec![], mode, permission)
}

#[test]
fn disabled_bridge_advertises_zero_bridge_tools() {
    // AT-01/BR-30: without with_bridge(true), no bridge tool appears in the
    // hosted definitions — the advertisement surface is unchanged.
    let harness = service();
    let names: Vec<String> = ToolService::definitions(harness.as_ref())
        .into_iter()
        .map(|d| d.harness_name.as_str().to_owned())
        .collect();
    for tool in BRIDGE_TOOL_NAMES {
        assert!(
            !names.contains(&tool.to_string()),
            "{tool} must not be advertised when Bridge is disabled"
        );
    }
}

#[test]
fn enabled_bridge_advertises_exactly_eight_tools() {
    let harness = gated_service(true);
    let names: Vec<String> = ToolService::definitions(harness.as_ref())
        .into_iter()
        .map(|d| d.harness_name.as_str().to_owned())
        .collect();
    for tool in BRIDGE_TOOL_NAMES {
        assert!(
            names.contains(&tool.to_string()),
            "{tool} must be advertised when Bridge is enabled"
        );
    }
    assert_eq!(bridge_definitions().len(), 8);
}

#[tokio::test]
async fn disabled_bridge_tool_call_is_refused_not_executed() {
    // BR-30 inverse direction: even a direct call to a bridge tool with the
    // service disabled must fail closed, never execute.
    let harness = gated_service(false);
    let result = ToolService::execute(
        harness.as_ref(),
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "media.timeline.create", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await;
    assert!(result.is_err(), "disabled bridge must refuse execution");
    let message = result.err().unwrap().to_string();
    assert!(
        message.contains("disabled"),
        "refusal must name the disabled state: {message}"
    );
}

#[tokio::test]
async fn no_adapter_composition_refuses_execute_truthfully() {
    // The no-adapter service never fabricates an application effect.
    let bridge = BridgeToolService::no_adapter();
    let connect = ToolService::execute(
        &bridge,
        &call(
            "bridge_connect",
            serde_json::json!({"application": "resolve"}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("connect on the no-adapter composition succeeds as a session bind");
    assert!(connect.text.as_str().contains("no-adapter"));

    let execute = ToolService::execute(
        &bridge,
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "bridge.placeholder.none", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("the no-adapter denial is a model-visible outcome, not an error path");
    let text = execute.text.as_str();
    assert!(
        text.contains("capability_unavailable")
            || text.contains("DependencyMissing")
            || text.contains("capability"),
        "refusal must state capability unavailability honestly: {text}"
    );
    assert!(
        !text.to_lowercase().contains("success"),
        "no success language in a refusal: {text}"
    );
}

#[tokio::test]
async fn plan_mode_mutation_is_denied_through_the_service_route() {
    // AT-06 (core route): plan mode + bridge_execute ⇒ denial text, no dispatch.
    let bridge = gated_bridge();
    // Connect first so the gate (not the session check) produces the denial.
    ToolService::execute(
        &bridge,
        &call(
            "bridge_connect",
            serde_json::json!({"application": "resolve"}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("connect for the gate test");
    let result = ToolService::execute(
        &bridge,
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "media.timeline.create", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Plan, SessionPermissionMode::Bypass),
    )
    .await
    .expect("denial is a model-visible result");
    let text = result.text.as_str();
    assert!(
        text.contains("plan mode"),
        "plan-mode denial must name plan mode: {text}"
    );
}

#[tokio::test]
async fn read_only_permission_mutation_is_denied_through_the_service_route() {
    let bridge = gated_bridge();
    ToolService::execute(
        &bridge,
        &call(
            "bridge_connect",
            serde_json::json!({"application": "resolve"}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("connect for the gate test");
    let result = ToolService::execute(
        &bridge,
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "media.timeline.create", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::ReadOnly),
    )
    .await
    .expect("denial is a model-visible result");
    let text = result.text.as_str();
    assert!(
        text.contains("read-only"),
        "read-only denial must name read-only: {text}"
    );
}

#[tokio::test]
async fn discover_is_passive_and_names_no_adapter_truthfully() {
    let bridge = BridgeToolService::no_adapter();
    let result = ToolService::execute(
        &bridge,
        &call("bridge_discover", serde_json::json!({})),
        &context(SessionOperatingMode::Plan, SessionPermissionMode::ReadOnly),
    )
    .await
    .expect("discover is read-only and always available");
    // The no-adapter SERVICE never fabricates an adapter; environment
    // discovery (live MPRIS players) may legitimately appear, so the
    // invariant under test is the truthful "no adapter configured" clause.
    assert!(result.text.as_str().contains("no adapter configured"));
}

#[tokio::test]
async fn execute_requires_session_first() {
    let bridge = BridgeToolService::no_adapter();
    let error = match ToolService::execute(
        &bridge,
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "media.timeline.create", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    {
        Ok(_) => panic!("unknown capability on an unconnected session must fail"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("session"),
        "error must name the missing session: {error}"
    );
}
