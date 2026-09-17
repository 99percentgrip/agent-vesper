//! VB-PRD-001 NF-02 / AT-21 (production route): `/bridge stop` must close
//! admission against the LIVE hosted service without a model turn, in BOTH
//! hosts' composition (`HarnessToolService::with_bridge(true)`), and the
//! report must be truthful when no session exists. Before this increment
//! no production path could close admission at all — `stop_for_tests` was
//! the only stop trigger, and `/bridge stop`'s text pointed at a tool that
//! did not exist.

#![cfg(feature = "bridge")]

use vesper_agent::ToolService;
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId};

use crate::bridge_service::{BRIDGE_TOOL_EXECUTE, BridgeToolService, bridge_definitions};

fn service() -> std::sync::Arc<crate::HarnessToolService> {
    let stores = std::sync::Arc::new(crate::MemoryStores {
        memory: None,
        skills: None,
        profile: None,
        awareness: None,
    });
    std::sync::Arc::new(
        crate::HarnessToolService::new(
            stores,
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
            None,
        )
        .with_bridge(true),
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
async fn production_stop_closes_admission_through_the_service_route() {
    // The production composition (with_bridge(true)), not a test fixture:
    // connect through the real tool route, stop through the production
    // `HarnessToolService::bridge_stop`, then a further execute must be
    // refused with closed admission — no model inference involved.
    let harness = service();
    let connect = ToolService::execute(
        harness.as_ref(),
        &call(
            "bridge_connect",
            serde_json::json!({"application": "resolve"}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("connect binds a session");
    // Truthful binding either way: the production composition attaches a
    // live adapter when the environment offers one (MPRIS player running)
    // and names the no-adapter state otherwise. Both are honest; the
    // invariant under test below is the STOP behavior.
    assert!(
        connect.text.as_str().contains("Bridge session bound to"),
        "unexpected connect text: {}",
        connect.text
    );

    let report = harness.bridge_stop();
    assert!(
        report.contains("stopped") && report.contains("admission closed"),
        "stop report must be truthful: {report}"
    );
    assert!(
        !report.to_lowercase().contains("killed"),
        "stop must not claim to kill anything: {report}"
    );

    // Admission is closed for the model route too: execute is refused
    // with the closed-admission denial, not dispatched.
    let result = ToolService::execute(
        harness.as_ref(),
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "bridge.placeholder.none", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("denial is a model-visible outcome");
    assert!(
        result.text.as_str().to_lowercase().contains("admission"),
        "post-stop execute must name closed admission: {}",
        result.text.as_str()
    );
}

#[tokio::test]
async fn production_resume_reopens_admission_and_keeps_denial_precedence() {
    let harness = service();
    ToolService::execute(
        harness.as_ref(),
        &call(
            "bridge_connect",
            serde_json::json!({"application": "resolve"}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("connect");
    let _stopped = harness.bridge_stop();
    let resumed = harness.bridge_resume();
    assert!(
        resumed.contains("fresh observation"),
        "resume must require a fresh observation: {resumed}"
    );
    // After resume, admission is OPEN again, so the no-adapter denial is
    // the capability one (documented step-3 precedence) — NOT closed
    // admission. The freshness requirement itself is enforced at the
    // core level (`at21_resume_requires_fresh_observation`); this layer
    // proves stop/resume reopened admission honestly.
    let result = ToolService::execute(
        harness.as_ref(),
        &call(
            BRIDGE_TOOL_EXECUTE,
            serde_json::json!({"capability": "bridge.placeholder.none", "arguments": {}}),
        ),
        &context(SessionOperatingMode::Code, SessionPermissionMode::Bypass),
    )
    .await
    .expect("outcome is model-visible");
    assert!(
        !result.text.as_str().to_lowercase().contains("admission"),
        "resume must reopen admission: {}",
        result.text.as_str()
    );
}

#[tokio::test]
async fn stop_without_a_session_is_truthful() {
    let harness = service();
    let report = harness.bridge_stop();
    assert!(
        report.contains("no session"),
        "stop without a session must say so: {report}"
    );
    let resumed = harness.bridge_resume();
    assert!(
        resumed.contains("no session"),
        "resume without a session must say so: {resumed}"
    );
}

#[test]
fn bridge_stop_advertises_no_new_tool_and_keeps_the_eight_tool_surface() {
    // NF-02 stop is a HOST command, not a ninth model tool: the tool
    // surface stays exactly the eight tools (AT-01/BR-30 unchanged).
    let harness = service();
    let names: Vec<String> = ToolService::definitions(harness.as_ref())
        .into_iter()
        .map(|d| d.harness_name.as_str().to_owned())
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("bridge_"))
            .count(),
        8,
        "exactly eight bridge tools"
    );
    assert!(
        !names.iter().any(|name| name.contains("stop")),
        "stop must not be a model tool"
    );
    let _ = BridgeToolService::no_adapter(); // composition unchanged
    let _ = bridge_definitions().len();
}
