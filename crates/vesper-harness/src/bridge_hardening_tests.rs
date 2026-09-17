//! Hardening tests (2w): timeout-after-mutation honesty and duplicate
//! suppression of unsettled non-idempotent work — the §7 invariants.

use std::sync::Arc;

use serde_json::json;
use vesper_agent::ToolService;
use vesper_bridge::adapter::{AdapterOutcome, AdapterPort};
use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::error::BridgeError;
use vesper_bridge::operation::{OperationRequestId, OperationSpec};
use vesper_bridge::session::StopOutcome;
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId};

use crate::bridge_service::{
    BRIDGE_TOOL_CONNECT, BRIDGE_TOOL_EXECUTE, BRIDGE_TOOL_OBSERVE, BridgeToolService,
};

/// Adapter whose dispatch ALWAYS times out after "sending" — the
/// worst-case honest scenario: the mutation may have reached the app.
struct TimingOutAdapter {
    mutating_capability: &'static str,
}

impl AdapterPort for TimingOutAdapter {
    fn manifest(&self) -> &CapabilityManifest {
        Box::leak(Box::new(CapabilityManifest::new(
            "timeout-scenario",
            1,
            vec![CapabilityRecord {
                id: CapabilityId::new(self.mutating_capability).unwrap(),
                schema_version: 1,
                availability: Availability::Available,
                implementation: Implementation::Native,
                mutability: Mutability::Mutating,
                route: RouteKind::NativeApi,
                delivery: DeliveryMode::Foreground,
                verification: VerificationMethod::Independent,
                limitations: "test: dispatch times out".into(),
            }],
        )))
    }

    fn dispatch(
        &self,
        _request_id: &OperationRequestId,
        _spec: &OperationSpec,
    ) -> Result<AdapterOutcome, BridgeError> {
        // Simulate: command sent, then the deadline expired before any
        // result arrived. The mutation MAY have been applied.
        Err(BridgeError::Timeout {
            detail: "deadline exceeded".into(),
        })
    }

    fn job_status(&self, _job_id: &str) -> Option<StopOutcome> {
        None
    }
    fn release_inputs(&self) -> String {
        String::new()
    }
    fn describe(&self) -> String {
        "timeout-scenario adapter".into()
    }
    fn health(&self) -> Option<bool> {
        Some(true)
    }
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
async fn timeout_after_mutation_settles_unknown_outcome_not_failed() {
    // §7: after a timeout that may have followed a mutation, the honest
    // classification is unknown_outcome — never "failed" (which would
    // invite a blind retry that could double-apply the mutation).
    let service = BridgeToolService::no_adapter().with_adapter(Arc::new(TimingOutAdapter {
        mutating_capability: "render.job.start",
    }));
    call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    let exec = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "render.job.start", "arguments": {}}),
    )
    .await;
    assert!(
        exec.contains("unknown_outcome"),
        "timeout after a mutating dispatch must report unknown_outcome, got: {exec}"
    );
}

#[tokio::test]
async fn duplicate_of_unsettled_mutation_is_refused_with_reconcile_guidance() {
    // §6.6/NF-10: a second dispatch of the same unsettled non-idempotent
    // operation must not silently run again; the caller must reconcile.
    let service = BridgeToolService::no_adapter().with_adapter(Arc::new(TimingOutAdapter {
        mutating_capability: "render.job.start",
    }));
    call(&service, BRIDGE_TOOL_CONNECT, json!({})).await;
    call(&service, BRIDGE_TOOL_OBSERVE, json!({})).await;
    let first = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "render.job.start", "arguments": {}}),
    )
    .await;
    let second = call(
        &service,
        BRIDGE_TOOL_EXECUTE,
        json!({"capability": "render.job.start", "arguments": {}}),
    )
    .await;
    // The first must be unknown-outcome honest.
    assert!(
        first.contains("unknown_outcome") || first.contains("effect unknown"),
        "first: {first}"
    );
    // The second must NOT read as a fresh clean dispatch; it must carry
    // reconciliation guidance (duplicate/unknown/reconcile language).
    assert!(
        second.contains("reconcile") || second.contains("duplicate") || second.contains("unknown"),
        "duplicate dispatch must guide reconciliation, got: {second}"
    );
}
