//! LIVE latency measurement: the real product path (discovery → connect →
//! observe → execute) against a running MPRIS player. Not a unit test —
//! an honest stopwatch on the shipped code path.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use vesper_agent::ToolService;
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId};

use crate::bridge_service::{
    BRIDGE_TOOL_CONNECT, BRIDGE_TOOL_EXECUTE, BRIDGE_TOOL_OBSERVE, BridgeToolService,
};

fn context() -> vesper_agent::ToolContext {
    vesper_agent::executor::uncancellable_context(
        vec![],
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
}

async fn timed(service: &BridgeToolService, tool: &str, args: serde_json::Value) -> (f64, String) {
    let started = Instant::now();
    let text = ToolService::execute(
        service,
        &ToolCall {
            id: ToolCallId::new("t").unwrap(),
            tool_id: ToolId::new(tool).unwrap(),
            arguments: args,
            extensions: Default::default(),
        },
        &context(),
    )
    .await
    .map(|r| r.text.to_string())
    .unwrap_or_else(|e| format!("ERROR: {e}"));
    (started.elapsed().as_secs_f64() * 1000.0, text)
}

#[tokio::test]
#[ignore = "live: requires a running MPRIS player"]
async fn live_product_path_latency_budget() {
    // The number Alex asked about: discovery → connect → observe → execute.
    let t0 = Instant::now();
    let players = crate::bridge_adapters::discover_mpris_players();
    let discovery_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(!players.is_empty(), "no MPRIS player running");

    let adapter = Arc::new(crate::bridge_adapters::MprisAdapter::new(
        players.into_iter().next().unwrap(),
    ));
    let service = BridgeToolService::no_adapter().with_adapter(adapter);

    let (connect_ms, connect) = timed(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    assert!(connect.contains("MPRIS"), "{connect}");

    let (observe_ms, observe) = timed(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    assert!(observe.contains("observation"), "{observe}");

    let (execute_ms, execute) = timed(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "mpris.player.status", "arguments": {}}),
    )
    .await;
    assert!(
        execute.contains("applied") || execute.contains("verified"),
        "{execute}"
    );

    println!(
        "LATENCY discovery={discovery_ms:.0}ms connect={connect_ms:.0}ms observe={observe_ms:.0}ms execute={execute_ms:.0}ms total={:.0}ms",
        discovery_ms + connect_ms + observe_ms + execute_ms
    );
    // The budget this test pins: the interactive path must feel instant.
    assert!(
        discovery_ms + connect_ms + observe_ms + execute_ms < 1000.0,
        "interactive Bridge path exceeded the 1s budget"
    );
}
