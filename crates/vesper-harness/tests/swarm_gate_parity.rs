//! VRO-16 PR-1 cross-host governance parity proofs.
//!
//! The parity contract requires identical `HostCommand` semantics in both
//! hosts. The mechanism: ONE shared implementation (`swarm_gate_surface`
//! parsing/rendering + `SwarmControls::command` routing + the service
//! gate channel), with both hosts delegating. These tests prove the shared
//! surface and the settings round-trip without file edits.
#![cfg(feature = "swarm")]

use vesper_harness::swarm_gate_surface::{parse_gate_command, render_gate};
use vesper_harness::swarm_settings::{
    GovernanceSetting, SwarmCommandOutcome, SwarmControls, SwarmSettings,
};
use vesper_swarm::hive::governance::{FallbackAction, GateView, HostCommand};

/// Both hosts parse identical gate commands through the one shared parser;
/// every ratified verb round-trips and every malformed form is refused with
/// the same shared usage text.
#[test]
fn both_hosts_share_one_gate_command_surface() {
    // The canonical verb set is defined exactly once, in the swarm crate.
    assert_eq!(
        HostCommand::verbs(),
        &["resume", "redirect", "fail", "cancel"]
    );

    // The shared parser accepts every verb with identical results.
    let cases = [
        ("t0 resume", HostCommand::Resume),
        (
            "t0 redirect tighten the scope",
            HostCommand::Redirect {
                directive: String::from("tighten the scope"),
            },
        ),
        (
            "t0 fail corrupt output",
            HostCommand::Fail {
                reason: String::from("corrupt output"),
            },
        ),
        ("t0 cancel", HostCommand::Cancel),
    ];
    for (argument, expected) in cases {
        let (task, command) = parse_gate_command(argument).unwrap();
        assert_eq!(task, "t0");
        assert_eq!(command, expected);
    }

    // Malformed forms are refused identically for both hosts.
    for bad in ["", "t0", "t0 explode", "t0 redirect ", "t0 fail ", "resume"] {
        assert!(parse_gate_command(bad).is_err(), "{bad}");
    }
}

/// Both hosts route `/swarm gate ...` through the one shared control
/// surface and produce the same outcome shape.
#[test]
fn swarm_controls_route_gate_verbs_identically() {
    let root = tempfile::tempdir().unwrap();
    let mut controls = SwarmControls::default();
    for (argument, expected_task) in [
        ("gate goal-task-0 resume", "goal-task-0"),
        ("gate goal-task-1 cancel", "goal-task-1"),
    ] {
        match controls.command(root.path(), argument).unwrap() {
            SwarmCommandOutcome::Gate(task, command) => {
                assert_eq!(task, expected_task);
                assert!(matches!(command, HostCommand::Resume | HostCommand::Cancel));
            }
            other => panic!("expected Gate outcome, got {other:?}"),
        }
    }
}

/// Gate rendering with derived countdown is one implementation; both hosts
/// display the same bounded format.
#[test]
fn gate_countdown_rendering_is_shared() {
    let view = GateView {
        gate_id: String::from("g-task-2-gate"),
        task_id: String::from("g-task-2"),
        reason: String::from("repeated failed turns"),
        remaining_ms: 3 * 60_000 + 25_000,
        fallback: FallbackAction::FailTask,
    };
    let line = render_gate(&view);
    assert!(line.contains("3m 25s remaining"));
    assert!(line.contains("fallback fail-task"));
    // Expired gates render zero without going negative.
    let mut expired = view;
    expired.remaining_ms = 0;
    assert!(render_gate(&expired).contains("0m 00s remaining"));
}

/// The governance profile is a persisted native setting: draft edits apply
/// without any file side effect, Save persists once, and a reload reads it
/// back — no manual configuration file editing is ever required.
#[test]
fn governance_settings_round_trip_requires_no_file_edits() {
    let root = tempfile::tempdir().unwrap();
    let mut controls = SwarmControls::default();

    // Draft edits never touch the filesystem.
    controls
        .command(root.path(), "settings governance gated")
        .unwrap();
    assert!(
        !root
            .path()
            .join(".agent-vesper/swarm-settings.json")
            .exists()
    );

    // Save persists once; a fresh load reads the same profile.
    controls.command(root.path(), "settings save").unwrap();
    assert!(
        root.path()
            .join(".agent-vesper/swarm-settings.json")
            .exists()
    );
    let saved = vesper_harness::swarm_settings::load(root.path()).unwrap();
    assert_eq!(saved.governance, GovernanceSetting::Gated);

    // Cancel discards an unsaved draft change.
    let mut controls = SwarmControls::default();
    controls
        .command(root.path(), "settings governance auto")
        .unwrap();
    controls.command(root.path(), "settings cancel").unwrap();
    let unchanged = vesper_harness::swarm_settings::load(root.path()).unwrap();
    assert_eq!(unchanged.governance, GovernanceSetting::Gated);

    // Legacy settings files without the governance field deserialize to
    // the default profile (serde default), so old workspaces keep working.
    let legacy = serde_json::json!({
        "enabled": true,
        "topology": "mesh",
        "drivers": 3,
        "failover": false,
        "shared_scope": false
    });
    let parsed: SwarmSettings = serde_json::from_value(legacy).unwrap();
    assert_eq!(parsed.governance, GovernanceSetting::Auto);
    assert!(parsed.validate().is_ok());
}
