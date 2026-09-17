//! VB-PRD-001 AT-01 (OS-observation half): with the `bridge` feature
//! compiled in **and enabled in settings**, an ACP host session that
//! never invokes a Bridge tool must not hold child processes, sockets,
//! or descriptors beyond the pre-existing fixture/model connections the
//! same binary uses with Bridge entirely absent.
//!
//! Evidence class: real-process observation against kernel accounting
//! (`/proc/<pid>/task/*/children`), `ss -tnp`, and `lsof`. This is the
//! host-startup observation lane promised by phase2b; it proves the
//! enabled-but-idle Bridge path adds zero OS footprint, complementing
//! the registry/advertisement proof (`bridge_service_tests`) and the
//! compiled-out proof (feature-off builds, phase2).
//!
//! Not proven here (stays NOT TESTED): capture, compositor, or input
//! lanes — no adapter exists in this build and none is simulated.

#![cfg(all(feature = "bridge", unix))]
#![allow(dead_code)]

mod support;

use std::collections::BTreeSet;

use serde_json::json;
use support::ProcessHarness;

/// Kernel-reported direct children of a pid (Linux procfs).
fn child_pids(pid: u32) -> BTreeSet<u32> {
    let mut children = BTreeSet::new();
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return children;
    };
    for entry in tasks.flatten() {
        let children_file = entry.path().join("children");
        if let Ok(text) = std::fs::read_to_string(&children_file) {
            for token in text.split_whitespace() {
                if let Ok(child) = token.parse::<u32>() {
                    children.insert(child);
                }
            }
        }
    }
    children
}

/// Kernel-reported TCP sockets of a pid via `ss -tnp` (no strace needed).
fn tcp_peers(pid: u32) -> BTreeSet<String> {
    let output = std::process::Command::new("ss")
        .args(["-tnp", "-H"])
        .output();
    let Ok(output) = output else {
        return BTreeSet::new();
    };
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.lines()
        .filter(|line| line.contains(&format!("pid={pid},")))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _state = fields.next()?;
            let _recv = fields.next()?;
            let _send = fields.next()?;
            let local = fields.next()?.to_owned();
            let peer = fields.next()?.to_owned();
            Some(format!("{local}->{peer}"))
        })
        .collect()
}

#[test]
fn at01_enabled_bridge_startup_observes_no_extra_process_or_socket() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cognition = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();

    // --- Pass 1: bridge ENABLED via the user-owned settings file.
    // The host resolves workspace settings from its OWN cwd, which the
    // process harness pins to the isolated temp root — so the settings
    // file must live at the root itself, not in a stray tempdir that the
    // host never reads (the previous shape passed only vacuously).
    let mut enabled = ProcessHarness::spawn_with_environment(
        listener.local_addr().unwrap(),
        [
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            (
                "AGENT_VESPER_COGNITION_ROOT",
                cognition.path().display().to_string(),
            ),
            (
                "AGENT_VESPER_GLOBAL_COGNITION_ROOT",
                global.path().display().to_string(),
            ),
        ],
    );
    // The host's cwd is the isolated ROOT (spawn pins current_dir there),
    // so the enabled settings file must sit at the root's .agent-vesper/
    // for this pass to be non-vacuous.
    std::fs::create_dir_all(enabled.isolated_root().join(".agent-vesper")).unwrap();
    std::fs::write(
        enabled
            .isolated_root()
            .join(".agent-vesper/bridge-settings.json"),
        serde_json::json!({"enabled": true}).to_string(),
    )
    .unwrap();
    let enabled_workspace = enabled.isolated_root().join("workspace");
    std::fs::create_dir_all(&enabled_workspace).unwrap();

    enabled
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert!(enabled.response(1).get("error").is_none());
    enabled.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":enabled_workspace,"mcpServers":[]}}));
    let session = enabled.response(2)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    // `/bridge status` proves the host resolved the ENABLED settings
    // without ever dispatching an application action.
    enabled.prompt(3, &session, "/bridge status", "bridge-status");
    assert!(enabled.response(3).get("error").is_none());
    let status = support::update_texts(enabled.transcript(), "agent_message_chunk").join("\n");
    assert!(
        status.contains("enabled"),
        "settings resolution must report enabled: {status}"
    );

    // --- OS observation of the enabled host.
    let enabled_pid = enabled.pid();
    let enabled_children = child_pids(enabled_pid);
    let enabled_sockets = tcp_peers(enabled_pid);
    let enabled_root = enabled.isolated_root().to_path_buf();
    enabled.finish();

    // --- Pass 2: identical process WITHOUT the settings file (disabled).
    let mut disabled = ProcessHarness::spawn_with_environment(
        listener.local_addr().unwrap(),
        [
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            (
                "AGENT_VESPER_COGNITION_ROOT",
                cognition.path().display().to_string(),
            ),
            (
                "AGENT_VESPER_GLOBAL_COGNITION_ROOT",
                global.path().display().to_string(),
            ),
        ],
    );
    let disabled_workspace = disabled.isolated_root().join("workspace");
    std::fs::create_dir_all(&disabled_workspace).unwrap();
    disabled
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert!(disabled.response(1).get("error").is_none());
    disabled.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":disabled_workspace,"mcpServers":[]}}));
    let disabled_session = disabled.response(2)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    disabled.prompt(3, &disabled_session, "/bridge status", "disabled-status");
    assert!(disabled.response(3).get("error").is_none());
    let disabled_status =
        support::update_texts(disabled.transcript(), "agent_message_chunk").join("\n");
    assert!(
        disabled_status.contains("disabled"),
        "absent settings must disable: {disabled_status}"
    );

    let disabled_pid = disabled.pid();
    let disabled_children = child_pids(disabled_pid);
    let disabled_sockets = tcp_peers(disabled_pid);
    disabled.finish();

    // --- Verdicts.
    assert!(
        enabled_children.is_empty(),
        "enabled-but-idle Bridge must hold no child processes (kernel children: {enabled_children:?})"
    );
    assert!(
        disabled_children.is_empty(),
        "baseline must hold no child processes: {disabled_children:?}"
    );
    assert!(
        enabled_sockets.is_empty() || enabled_sockets == disabled_sockets,
        "enabled-but-idle Bridge must add no TCP socket beyond the disabled baseline \
         (enabled: {enabled_sockets:?}, disabled: {disabled_sockets:?})"
    );
    // Durable-state honesty: no durable bridge state outside the workspace.
    assert!(
        !enabled_root
            .join(".agent-vesper")
            .join("bridge-state")
            .exists(),
        "enabled idle Bridge must not create durable bridge state"
    );
}
