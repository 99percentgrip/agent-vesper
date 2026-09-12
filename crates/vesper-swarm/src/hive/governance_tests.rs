//! Injected-clock and replay proofs for the governance state machine.
use super::*;

#[test]
fn config_bounds_reject_unbounded_timeouts() {
    assert!(GovernanceConfig::default().validated());
    for timeout in [
        Duration::ZERO,
        Duration::from_secs(10),
        Duration::from_secs(7200),
    ] {
        let config = GovernanceConfig {
            gate_timeout: timeout,
            ..GovernanceConfig::default()
        };
        assert!(!config.validated(), "{timeout:?}");
        assert!(Governor::new(config).is_err());
    }
}

#[test]
fn gate_lifecycle_open_resolve_resume() {
    let mut governor = Governor::new(GovernanceConfig::default()).unwrap();
    let opened = governor
        .open_gate("goal-1", "task-1", "ambiguous table", 1_000)
        .unwrap();
    assert_eq!(opened.gate_id(), "task-1-gate");
    assert!(governor.has_open_gates());
    assert_eq!(
        governor.snapshot(1_000 + 60_000)[0].remaining_ms,
        DEFAULT_GATE_TIMEOUT.as_millis() as u64 - 60_000
    );
    let (resolution, resolved) = governor
        .resolve(
            "task-1",
            HostCommand::Redirect {
                directive: String::from("prefer the second column"),
            },
            1_000 + 90_000,
        )
        .unwrap();
    assert_eq!(
        resolution,
        Resolution::Resume {
            directive: Some(String::from("prefer the second column"))
        }
    );
    assert!(matches!(resolved, AuditEvent::GateResolved { .. }));
    assert!(!governor.has_open_gates());
}

#[test]
fn expiry_fires_preconfigured_fallback_automatically() {
    let mut governor = Governor::new(GovernanceConfig::default()).unwrap();
    governor
        .open_gate("goal-1", "task-1", "conflicting peer findings", 1_000)
        .unwrap();
    // 300s timeout: not yet expired at +299s, expired at +301s.
    assert!(governor.expire_due(1_000 + 299_000).is_empty());
    let expired = governor.expire_due(1_000 + 301_000);
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].0, "task-1");
    assert_eq!(
        expired[0].1,
        Resolution::FailTask {
            reason: String::from("gate_timeout")
        }
    );
    assert!(matches!(expired[0].2, AuditEvent::GateExpired { .. }));
    // Resolution after expiry is refused: the fallback already applied.
    assert!(
        governor
            .resolve("task-1", HostCommand::Resume, 1_000 + 302_000)
            .is_err()
    );
}

#[test]
fn replay_reconstructs_identical_remaining_time() {
    let mut governor = Governor::new(GovernanceConfig::default()).unwrap();
    governor
        .open_gate("goal-1", "task-1", "ambiguous artifact", 10_000)
        .unwrap();
    governor
        .open_gate("goal-1", "task-2", "budget pressure", 20_000)
        .unwrap();
    governor
        .resolve("task-2", HostCommand::Resume, 25_000)
        .unwrap();
    let events = governor.events();

    // A restart at t=100_000 reconstructs open gates from the audit trail.
    let mut rebuilt = Governor::replay(GovernanceConfig::default(), &events);
    assert_eq!(rebuilt.open_gate_task_ids(), vec![String::from("task-1")]);
    assert_eq!(
        rebuilt.snapshot(100_000)[0].remaining_ms,
        governor.snapshot(100_000)[0].remaining_ms
    );
    // And the reconstructed gate expires on the same original schedule —
    // reconstruction never resets the countdown. Opened at t=10_000 with a
    // 300 s timeout, so it expires at t=310_000 regardless of the restart.
    assert!(rebuilt.expire_due(310_000).len() == 1);
    assert!(rebuilt.expire_due(309_000).is_empty());
}

#[test]
fn smartpause_uses_observable_signals_only() {
    // Clean success never gates.
    assert!(!evaluate_receipt(&ReceiptSignals::ok()));
    // Single failure does not gate (ordinary retry semantics).
    assert!(!evaluate_receipt(&ReceiptSignals {
        success: false,
        consecutive_failures: 1,
        anomalies: 0,
        budget_remaining_percent: 100,
    }));
    // Repeated failure gates.
    assert!(evaluate_receipt(&ReceiptSignals {
        success: false,
        consecutive_failures: 2,
        anomalies: 0,
        budget_remaining_percent: 100,
    }));
    // Machine-flagged anomalies gate.
    assert!(evaluate_receipt(&ReceiptSignals {
        success: true,
        consecutive_failures: 0,
        anomalies: 1,
        budget_remaining_percent: 100,
    }));
    // Budget pressure gates.
    assert!(evaluate_receipt(&ReceiptSignals {
        success: true,
        consecutive_failures: 0,
        anomalies: 0,
        budget_remaining_percent: 20,
    }));
}

#[test]
fn profiles_gate_only_their_boundaries() {
    let auto = GovernanceConfig {
        profile: GovernanceProfile::Auto,
        ..GovernanceConfig::default()
    };
    let gated = GovernanceConfig {
        profile: GovernanceProfile::Gated,
        ..GovernanceConfig::default()
    };
    assert!(!auto.gates_decomposition() && !auto.gates_synthesis());
    assert!(gated.gates_decomposition() && gated.gates_synthesis());
    // Failed receipts gate only via SmartPause observable signals, never
    // unconditionally.
    let clean = ReceiptSignals::ok();
    assert!(!auto.gates_failed_receipt(&clean));
    let degraded = ReceiptSignals {
        success: false,
        consecutive_failures: 2,
        anomalies: 0,
        budget_remaining_percent: 100,
    };
    assert!(auto.gates_failed_receipt(&degraded));
}

#[test]
fn host_command_verbs_are_the_shared_surface() {
    assert_eq!(
        HostCommand::verbs(),
        &["resume", "redirect", "fail", "cancel"]
    );
}

#[test]
fn commands_and_reasons_are_bounded() {
    let mut governor = Governor::new(GovernanceConfig::default()).unwrap();
    governor.open_gate("g", "t", "reason", 0).unwrap();
    let big = "x".repeat(MAX_GATE_TEXT_BYTES + 1);
    assert!(
        governor
            .resolve(
                "t",
                HostCommand::Redirect {
                    directive: big.clone()
                },
                1
            )
            .is_err()
    );
    assert!(
        governor
            .resolve(
                "t",
                HostCommand::Fail {
                    reason: String::new()
                },
                1
            )
            .is_err()
    );
    // Duplicate gates are refused.
    assert!(governor.open_gate("g", "t", "again", 2).is_err());
}

#[test]
fn cancel_resolution_is_goal_scoped() {
    let mut governor = Governor::new(GovernanceConfig::default()).unwrap();
    governor
        .open_gate("goal-9", "task-9", "dangerous", 0)
        .unwrap();
    let (resolution, _) = governor.resolve("task-9", HostCommand::Cancel, 1).unwrap();
    assert_eq!(resolution, Resolution::CancelGoal);
    // Goal-scoped: another task can still open its own gate afterward.
    governor.open_gate("goal-9", "task-10", "next", 2).unwrap();
}
