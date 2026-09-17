//! AT-01 composition evidence and AT-21 stop-path coverage at the
//! harness layer (deterministic; no OS tracing claimed).
//!
//! AT-01's full host-level claim (process/filesystem/network observation
//! at startup) is NOT tested here; these tests prove the composition
//! facts: with Bridge disabled, no bridge tool is callable; with Bridge
//! enabled-but-stopped, the stop path closes admission without any model
//! inference participating, and resume demands fresh state.

#![cfg(feature = "bridge")]

use std::sync::Arc;

use vesper_agent::ToolService;
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId};

use crate::MemoryStores;
use crate::bridge_service::{BRIDGE_TOOL_EXECUTE, BRIDGE_TOOL_NAMES, BridgeToolService};

fn gated_service(enabled: bool) -> Arc<crate::HarnessToolService> {
    let stores = Arc::new(MemoryStores {
        memory: None,
        skills: None,
        profile: None,
        awareness: None,
    });
    Arc::new(
        crate::HarnessToolService::new(
            stores,
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
            None,
        )
        .with_bridge(enabled),
    )
}

fn bridge_with_session() -> BridgeToolService {
    let bridge = BridgeToolService::with_manifest(test_manifest());
    bridge.connect_for_tests();
    bridge
}

fn test_manifest() -> vesper_bridge::capability::CapabilityManifest {
    use vesper_bridge::capability::{
        Availability, CapabilityId, CapabilityRecord, DeliveryMode, Implementation, Mutability,
        RouteKind, VerificationMethod,
    };
    vesper_bridge::capability::CapabilityManifest::new(
        "test-at21",
        1,
        vec![CapabilityRecord {
            id: CapabilityId::new("fixture.timeline.create").unwrap(),
            schema_version: 1,
            availability: Availability::Available,
            implementation: Implementation::Native,
            mutability: Mutability::Mutating,
            route: RouteKind::NativeApi,
            delivery: DeliveryMode::Background,
            verification: VerificationMethod::Independent,
            limitations: "test manifest".into(),
        }],
    )
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

#[tokio::test]
async fn at01_disabled_bridge_answers_zero_tools_and_refuses_calls() {
    // The composition fact underlying AT-01: disabled ⇒ nothing callable.
    let harness = gated_service(false);
    let definitions = ToolService::definitions(harness.as_ref());
    let names: Vec<String> = definitions
        .iter()
        .map(|d| d.harness_name.as_str().to_owned())
        .collect();
    for tool in BRIDGE_TOOL_NAMES {
        assert!(!names.contains(&tool.to_string()));
    }
    for tool in BRIDGE_TOOL_NAMES {
        let result = ToolService::execute(
            harness.as_ref(),
            &call(tool, serde_json::json!({})),
            &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
        )
        .await;
        assert!(result.is_err(), "{tool} must refuse when disabled");
        let message = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("disabled"), "{tool}: {message}");
    }
}

#[tokio::test]
async fn at21_stop_closes_admission_without_model_inference() {
    // The stop path is host-side: it never awaits or consults the model.
    let bridge = bridge_with_session();
    // Stop is synchronous host state manipulation.
    bridge.stop_for_tests(vec!["render-1".into()]);
    let result = ToolService::execute(
        &bridge,
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "fixture.timeline.create", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("refusal is a model-visible outcome");
    let text = result.text.as_str();
    assert!(
        text.contains("admission") || text.contains("stop"),
        "post-stop refusal must name the closed admission: {text}"
    );
    let status = bridge.job_status_for_tests();
    assert_eq!(
        status,
        vec!["render-1".to_string()],
        "outstanding job stays visible after stop"
    );
}

#[tokio::test]
async fn at21_resume_requires_fresh_observation() {
    let bridge = bridge_with_session();
    bridge.stop_for_tests(vec![]);
    bridge.resume_for_tests();
    let result = ToolService::execute(
        &bridge,
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "fixture.timeline.create", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("refusal is a model-visible outcome");
    assert!(
        result.text.as_str().contains("fresh") || result.text.as_str().contains("changed"),
        "resume refusal must name stale state: {}",
        result.text.as_str()
    );
}
