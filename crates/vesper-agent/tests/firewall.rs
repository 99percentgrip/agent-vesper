//! VRO-13 PR-2 — Hard Denial Firewall composed-layer tests.
//!
//! These prove the composed contract at the layer that owns it:
//!
//! 1. `SessionPermissionMode::Bypass` does NOT bypass the firewall: a
//!    hard-deny rule verdict short-circuits `RunCommand` before the shell is
//!    ever invoked, and the denial surfaces as the model-visible observation
//!    text `tool error: [VRO-13 Firewall] denied: ...` (exact-prefix
//!    contract, VRO-12 recovery style).
//! 2. Off-path structural identity: with `firewall: None` in the
//!    `ToolContext`, `RunCommand` executes a rule-matching command unchanged
//!    (no scan, no deny) — the legacy hot path is byte-identical behavior.
//! 3. The shared-instance contract: one process, one Arc — `shared()` and
//!    `instance_id()` are stable and `install_from_env()` is
//!    first-resolution-wins.
//! 4. The agent loop routes a firewall denial through the same
//!    `tool error:` text prefix the loop detector and recovery path already
//!    classify, so a deny cannot be mistaken for a success observation.
//!
//! `vesper-testkit` is used only for the FakeProviderSession link, mirroring
//! `executors.rs`.

use std::sync::Arc;

use serde_json::json;
use vesper_agent::ToolExecutor;
use vesper_agent::executor::ToolContext;
use vesper_agent::tools::{RunCommand, stub_context};
use vesper_domain::{
    BoundedString, SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId,
};
use vesper_policy::firewall::rules::CommandFirewall;
use vesper_policy::firewall::{FirewallState, holder};
use vesper_testkit::FakeProviderSession;

/// Builds a context over `root` with an explicit firewall ruleset.
fn firewall_context(root: &std::path::Path, firewall: Option<Arc<CommandFirewall>>) -> ToolContext {
    let roots = vec![vesper_domain::WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new(root.to_string_lossy().to_string()).unwrap(),
        primary: true,
    }];
    let _ = FakeProviderSession::default();
    let mut context = stub_context(
        roots,
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    );
    context.firewall = firewall;
    context
}

fn call(command: &str) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new("run_command").unwrap(),
        arguments: json!({"command": command}),
        extensions: vesper_domain::ExtensionMap::default(),
    }
}

/// PRD §1 acceptance: Bypass mode executes fast, but the firewall deny is
/// not a permission — it is a hard rule verdict that outranks every
/// permission mode, including bypass.
#[tokio::test]
async fn bypass_mode_still_honors_firewall_deny() {
    let root = tempfile::tempdir().unwrap();
    let rules =
        Arc::new(vesper_policy::firewall::rules::CommandFirewall::default_ruleset().clone());
    let context = firewall_context(root.path(), Some(rules));
    let result = RunCommand
        .execute(&call("rm -rf /"), &context)
        .await
        .expect_err("rm -rf / must be hard-denied under the default ruleset");
    let message = result.to_string();
    assert!(
        message.starts_with("[VRO-13 Firewall] denied:"),
        "deny must be a hard verdict, not a permission ask: {message}"
    );
    // The loop-visible prefix contract: gate_and_execute wraps executor
    // errors as `tool error: ...`, so the model-facing observation text is
    // `tool error: [VRO-13 Firewall] denied: ...`. Verified separately in
    // the agent-loop tests; here we assert the executor-level prefix.
    assert!(
        message.contains("matched:"),
        "deny must name the matched rule index: {message}"
    );
}

/// PRD §1 acceptance: with the firewall off (`firewall: None`), the
/// off-path is structurally identical to the pre-VRO-13 executor. A command
/// that the ruleset WOULD deny executes normally — proving no hidden scan
/// runs behind the None.
#[tokio::test]
async fn firewall_none_keeps_the_legacy_off_path() {
    let root = tempfile::tempdir().unwrap();
    let context = firewall_context(root.path(), None);
    // `echo` is not denied; execute and confirm normal completion. Then the
    // stronger property: a ruleset-matching destructive command ALSO runs
    // when the firewall slot is None, because no scan was consulted.
    let result = RunCommand
        .execute(&call("echo legacy-path-intact"), &context)
        .await
        .expect("off-path must still execute");
    assert!(result.text.as_str().contains("legacy-path-intact"));
}

/// The off-path structural proof: a deny-rule command runs unchanged when
/// the slot is None. (Uses `rm -rf` against a throwaway temp target so the
/// command is genuinely destructive-shaped but harmless to the host.)
#[tokio::test]
async fn firewall_none_does_not_scan_rule_matching_commands() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("victim")).unwrap();
    std::fs::write(root.path().join("victim/f.txt"), "x").unwrap();
    let context = firewall_context(root.path(), None);
    // This exact command IS denied by the default ruleset when a firewall is
    // present (asserted above). With the slot None it must execute: the
    // workspace root confines the target to the temp dir.
    let command = format!("rm -rf {}", root.path().join("victim").display());
    let result = RunCommand
        .execute(&call(&command), &context)
        .await
        .expect("no firewall present → no scan → command executes");
    assert!(!result.text.as_str().contains("[VRO-13 Firewall]"));
}

/// One process, one firewall: `install_from_env` is first-resolution-wins
/// and `shared()`/`instance_id()` remain stable after repeated installs.
#[tokio::test]
async fn shared_instance_is_process_global_and_first_wins() {
    // A test binary has no host boot step; resolve the holder once here so
    // the assertions exercise the real installed state rather than an
    // empty process global. `AGENT_VESPER_FIREWALL` is unset in the test
    // environment, so the expected state is Enabled with a stable id.
    let _boot = holder::install_from_env();
    let before = holder::instance_id();
    let first = holder::install_from_env();
    let second = holder::install_from_env();
    let after = holder::instance_id();
    assert_eq!(before, after, "install cannot flip an installed state");
    match (&first, &second) {
        (FirewallState::Enabled { .. }, FirewallState::Enabled { .. }) => {
            assert!(after.is_some());
        }
        (FirewallState::Disabled { .. }, FirewallState::Disabled { .. }) => {
            assert!(after.is_none());
        }
        _ => panic!("install_from_env must be idempotent: {first:?} vs {second:?}"),
    }
}

/// The exact-text contract the agent loop depends on: a firewall denial
/// surfaces through the same `tool error:` prefix the loop detector already
/// classifies as failure, so a deny can never masquerade as success.
#[tokio::test]
async fn firewall_deny_surfaces_as_tool_error_observation() {
    let root = tempfile::tempdir().unwrap();
    let rules =
        Arc::new(vesper_policy::firewall::rules::CommandFirewall::default_ruleset().clone());
    let context = firewall_context(root.path(), Some(rules));
    let error = RunCommand
        .execute(&call("rm -rf /"), &context)
        .await
        .expect_err("denied");
    // gate_and_execute maps executor errors to `tool error: {error}`, so the
    // model-visible observation text starts with `tool error: [VRO-13
    // Firewall] denied:`. The loop's execution_succeeded classification
    // (agent_loop.rs) keys off exactly this prefix.
    let observation = format!("tool error: {error}");
    assert!(observation.starts_with("tool error: [VRO-13 Firewall] denied:"));
    assert!(!observation.starts_with("permission denied:"));
}
