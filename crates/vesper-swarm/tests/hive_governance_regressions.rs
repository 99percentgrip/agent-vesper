//! VRO-16 PR-1 governance proofs: bounded gates (D1), task-level
//! suspension with sibling parallelism (D2), restart reconstruction, and
//! observable-signals-only SmartPause. Injected clocks throughout; no
//! sleeps, no real-time dependence.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vesper_swarm::hive::orchestrator::{
    GovernanceClock, Hive, HiveConfig, HiveGoal, RoleProfile, WallClockGovernanceClock,
};
use vesper_swarm::hive::{GovernanceConfig, GovernanceProfile, HostCommand, Resolution};
use vesper_swarm::ledger::store::{BoundedText, EmbeddingPort, LedgerError};
use vesper_swarm::worker::{
    CancellationSignal, TaskPriority, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort,
    WorkerTask,
};

/// Deterministic injected clock: tests advance time explicitly.
struct InjectedClock(Arc<AtomicU64>);
impl GovernanceClock for InjectedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Embedding;
impl EmbeddingPort for Embedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> futures_util::future::BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
    }
}

/// Navigator that decomposes into independent sibling tasks.
struct Navigator {
    tasks: usize,
}
impl WorkerPort for Navigator {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let json = serde_json::json!({
            "tasks": (0..self.tasks)
                .map(|index| serde_json::json!({
                    "prompt": format!("task-{index}"),
                    "required_capabilities": ["read"],
                }))
                .collect::<Vec<_>>()
        })
        .to_string();
        Box::pin(async move {
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: if task.id.ends_with("-decompose") {
                    json
                } else {
                    String::from("synthesis")
                },
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}

enum DriverBehavior {
    /// Wait on a shared barrier, then succeed.
    BarrierSucceed,
    /// Return a failed receipt (twice for the SmartPause repeated-failure
    /// threshold), then succeed.
    FailTwiceThenSucceed { calls: Arc<AtomicU64> },
}

struct Driver {
    behavior: DriverBehavior,
    barrier: Option<Arc<tokio::sync::Barrier>>,
    completed: Option<Arc<AtomicU64>>,
}

impl WorkerPort for Driver {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities {
            tools: vec![String::from("read")],
            max_concurrent_tasks: 1,
        }
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let behavior = &self.behavior;
        let barrier = self.barrier.clone();
        let completed = self.completed.clone();
        Box::pin(async move {
            match behavior {
                DriverBehavior::BarrierSucceed => {
                    if let Some(barrier) = barrier {
                        barrier.wait().await;
                    }
                    if let Some(completed) = completed {
                        completed.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(TurnReceipt {
                        task_id: task.id.clone(),
                        output: task.prompt.clone(),
                        success: true,
                        duration: Duration::ZERO,
                    })
                }
                DriverBehavior::FailTwiceThenSucceed { calls } => {
                    let seen = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(TurnReceipt {
                        task_id: task.id.clone(),
                        output: if seen <= 2 {
                            String::new()
                        } else {
                            format!("recovered-{seen}")
                        },
                        success: seen > 2,
                        duration: Duration::ZERO,
                    })
                }
            }
        })
    }
}

fn hive_with_governance(
    drivers: Vec<Arc<dyn WorkerPort>>,
    clock: Arc<dyn GovernanceClock>,
    timeout: Duration,
) -> Hive {
    let mut config = HiveConfig::balanced(&["read"]);
    config.dimensions = 8;
    config.roles = vec![RoleProfile::navigator()];
    let mut ports: Vec<(String, Arc<dyn WorkerPort>)> = vec![(
        String::from("navigator"),
        Arc::new(Navigator {
            tasks: drivers.len(),
        }),
    )];
    for (index, driver) in drivers.into_iter().enumerate() {
        let mut role = RoleProfile::driver(&["read"]);
        role.name = format!("driver-{index}");
        ports.push((role.name.clone(), driver));
        config.roles.push(role);
    }
    let governance = GovernanceConfig {
        profile: GovernanceProfile::Auto,
        gate_timeout: timeout,
        ..GovernanceConfig::default()
    };
    Hive::new(config, ports, Arc::new(Embedding))
        .unwrap()
        .with_governance(governance, clock)
        .unwrap()
}

async fn start_goal(hive: &mut Hive, id: &str) {
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal {
        id: id.into(),
        prompt: "work".into(),
        priority: TaskPriority::Normal,
    })
    .unwrap();
}

/// D1 proof: a gate with a silent host expires at the configured deadline
/// and fires the FailTask fallback — injected clock, no sleeps.
#[tokio::test]
async fn d1_gate_expires_and_fires_failtask_fallback() {
    let clock = Arc::new(AtomicU64::new(1_000));
    let failed_driver_calls = Arc::new(AtomicU64::new(0));
    let failing_driver = Arc::new(Driver {
        behavior: DriverBehavior::FailTwiceThenSucceed {
            calls: failed_driver_calls.clone(),
        },
        barrier: None,
        completed: None,
    });
    let good_driver = Arc::new(Driver {
        behavior: DriverBehavior::BarrierSucceed,
        barrier: None,
        completed: Some(Arc::new(AtomicU64::new(0))),
    });
    let mut hive = hive_with_governance(
        vec![failing_driver, good_driver],
        Arc::new(InjectedClock(clock.clone())),
        Duration::from_secs(60),
    );
    start_goal(&mut hive, "d1").await;

    // First tick: two failures already exceed the SmartPause threshold...
    // but only after two turns. Drive ticks until a gate opens.
    let mut ticks = 0;
    while hive.gate_views(clock.load(Ordering::SeqCst)).is_empty() && ticks < 10 {
        hive.run_tick().await.unwrap();
        ticks += 1;
    }
    let views = hive.gate_views(clock.load(Ordering::SeqCst));
    assert_eq!(views.len(), 1, "exactly one gate for the failing task");
    assert_eq!(views[0].remaining_ms, 60_000);

    // Advance past the 60 s countdown with a silent host.
    clock.store(1_000 + 61_000, Ordering::SeqCst);
    // The expired tick fires the FailTask fallback: the audit trail records
    // GateExpired and the goal resolves through partial synthesis without
    // ever hanging on the silent host.
    let mut expired_seen = false;
    for _ in 0..3 {
        let _completed = hive.run_tick().await.unwrap();
        if hive
            .gate_events()
            .iter()
            .any(|event| matches!(event, vesper_swarm::hive::AuditEvent::GateExpired { .. }))
        {
            expired_seen = true;
            break;
        }
    }
    assert!(
        expired_seen,
        "GateExpired audit event written for silent host"
    );
    // The failed task never re-dispatches after expiry fallback.
    assert!(failed_driver_calls.load(Ordering::SeqCst) <= 4);
    // No gate remains open: the hive is not hung awaiting a human.
    assert!(
        hive.gate_views(clock.load(Ordering::SeqCst)).is_empty(),
        "no gate may remain open after expiry"
    );
}

/// D2 proof: while one task is suspended at a gate, sibling tasks keep
/// executing (barrier proof: siblings demonstrably overlap with the
/// suspended task's absence, and independent work completes).
#[tokio::test]
async fn d2_siblings_execute_during_suspension() {
    let clock = Arc::new(AtomicU64::new(1_000));
    // Three drivers: one fails twice (gates), two siblings synchronize on a
    // shared barrier — proving they both ran while the third was suspended.
    let failing = Arc::new(Driver {
        behavior: DriverBehavior::FailTwiceThenSucceed {
            calls: Arc::new(AtomicU64::new(0)),
        },
        barrier: None,
        completed: None,
    });
    let sibling_barrier = Arc::new(tokio::sync::Barrier::new(2));
    let sibling_completions = Arc::new(AtomicU64::new(0));
    let siblings: Vec<Arc<dyn WorkerPort>> = (0..2)
        .map(|_| {
            Arc::new(Driver {
                behavior: DriverBehavior::BarrierSucceed,
                barrier: Some(sibling_barrier.clone()),
                completed: Some(sibling_completions.clone()),
            }) as Arc<dyn WorkerPort>
        })
        .collect();
    let mut all_drivers: Vec<Arc<dyn WorkerPort>> = vec![failing];
    all_drivers.extend(siblings);
    let mut hive = hive_with_governance(
        all_drivers,
        Arc::new(InjectedClock(clock.clone())),
        Duration::from_secs(300),
    );
    start_goal(&mut hive, "d2").await;

    let mut ticks = 0;
    while hive.gate_views(clock.load(Ordering::SeqCst)).is_empty() && ticks < 10 {
        hive.run_tick().await.unwrap();
        ticks += 1;
    }
    assert_eq!(hive.gate_views(clock.load(Ordering::SeqCst)).len(), 1);
    // Both siblings completed while the gate was open (they met at the
    // barrier — a two-party barrier only releases when both ran).
    assert_eq!(sibling_completions.load(Ordering::SeqCst), 2);

    // Host resolves the gate with a redirect; the goal completes.
    let suspended_task = hive.gate_views(clock.load(Ordering::SeqCst))[0]
        .task_id
        .clone();
    let resolution = hive
        .resolve_host_command(
            &suspended_task,
            HostCommand::Redirect {
                directive: String::from("retry with the second table column"),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        resolution,
        Resolution::Resume {
            directive: Some(String::from("retry with the second table column"))
        }
    );
    let completed = hive.run_tick().await.unwrap();
    assert!(completed, "goal completes after redirect");
    let events = hive.gate_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, vesper_swarm::hive::AuditEvent::GateResolved { .. }))
    );
}

/// Restart proof: gate state reconstructed from the audit trail keeps the
/// countdown derived from the original opening time — a restart neither
/// resets nor extends the deadline.
#[tokio::test]
async fn restart_reconstructs_gate_with_original_countdown() {
    let clock = Arc::new(AtomicU64::new(10_000));
    let failing = Arc::new(Driver {
        behavior: DriverBehavior::FailTwiceThenSucceed {
            calls: Arc::new(AtomicU64::new(0)),
        },
        barrier: None,
        completed: None,
    });
    let good = Arc::new(Driver {
        behavior: DriverBehavior::BarrierSucceed,
        barrier: None,
        completed: Some(Arc::new(AtomicU64::new(0))),
    });
    let mut hive = hive_with_governance(
        vec![failing.clone(), good],
        Arc::new(InjectedClock(clock.clone())),
        Duration::from_secs(300),
    );
    start_goal(&mut hive, "restart").await;
    let mut ticks = 0;
    while hive.gate_views(clock.load(Ordering::SeqCst)).is_empty() && ticks < 10 {
        hive.run_tick().await.unwrap();
        ticks += 1;
    }
    let before = hive.gate_views(clock.load(Ordering::SeqCst))[0].clone();

    // Simulate a restart: rebuild a governor from the audit trail.
    let events = hive.gate_events();
    let rebuilt = vesper_swarm::hive::Governor::replay(
        GovernanceConfig {
            profile: GovernanceProfile::Auto,
            gate_timeout: Duration::from_secs(300),
            ..GovernanceConfig::default()
        },
        &events,
    );
    // 95 s elapsed since opening: remaining must be 205_000 ms, identical
    // to the live governor's derived countdown.
    clock.store(10_000 + 95_000, Ordering::SeqCst);
    let live = hive.gate_views(clock.load(Ordering::SeqCst))[0].remaining_ms;
    let reconstructed = rebuilt.snapshot(clock.load(Ordering::SeqCst))[0].remaining_ms;
    assert_eq!(live, 205_000);
    assert_eq!(live, reconstructed);
    assert_eq!(before.task_id, rebuilt.snapshot(0)[0].task_id);
    let _ = WallClockGovernanceClock; // production clock exists and is injectable-away
}

/// Both hosts share one `HostCommand` surface: the verb set is defined once
/// in the shared crate and asserted stable.
#[tokio::test]
async fn host_command_surface_is_shared_and_stable() {
    assert_eq!(
        HostCommand::verbs(),
        &["resume", "redirect", "fail", "cancel"]
    );
}

/// No-hang proof: every governance configuration is bounded at both ends.
#[tokio::test]
async fn unbounded_gate_timeouts_are_refused() {
    for timeout in [
        Duration::ZERO,
        Duration::from_secs(1),
        Duration::from_secs(59),
        Duration::from_secs(3601),
        Duration::from_secs(u32::MAX as u64),
    ] {
        assert!(
            governor_try(timeout).is_err(),
            "timeout {timeout:?} must be refused"
        );
    }
}

fn governor_try(timeout: Duration) -> Result<(), String> {
    let config = GovernanceConfig {
        profile: GovernanceProfile::Auto,
        gate_timeout: timeout,
        ..GovernanceConfig::default()
    };
    vesper_swarm::hive::Governor::new(config).map(|_| ())
}

/// Sanity: governance-off hive keeps exact VRO-15 semantics (no gates ever).
#[tokio::test]
async fn governance_off_keeps_vro15_semantics() {
    let failing = Arc::new(Driver {
        behavior: DriverBehavior::FailTwiceThenSucceed {
            calls: Arc::new(AtomicU64::new(0)),
        },
        barrier: None,
        completed: None,
    });
    let good = Arc::new(Driver {
        behavior: DriverBehavior::BarrierSucceed,
        barrier: None,
        completed: Some(Arc::new(AtomicU64::new(0))),
    });
    let mut config = HiveConfig::balanced(&["read"]);
    config.dimensions = 8;
    config.roles = vec![RoleProfile::navigator()];
    let mut ports: Vec<(String, Arc<dyn WorkerPort>)> =
        vec![(String::from("navigator"), Arc::new(Navigator { tasks: 2 }))];
    for (index, driver) in [failing, good].into_iter().enumerate() {
        let mut role = RoleProfile::driver(&["read"]);
        role.name = format!("driver-{index}");
        ports.push((role.name.clone(), driver));
        config.roles.push(role);
    }
    let mut hive = Hive::new(config, ports, Arc::new(Embedding)).unwrap();
    start_goal(&mut hive, "off").await;
    // VRO-15: a failed receipt errors the tick; no gate ever opens.
    let result = hive.run_tick().await;
    assert!(result.is_err());
    assert!(hive.gate_views(0).is_empty());
    assert!(hive.gate_events().is_empty());
}

/// Unused-import guard keeps the file warning-free.
#[allow(dead_code)]
struct KeepAlive(Mutex<()>);

// ---- VRO-16 PR-2 proofs ----

/// A Navigator whose decision output always asks to refine: the loop is
/// bounded by the engine cap and the exhaustion verdict is audited.
struct RefiningNavigator {
    tasks: usize,
    decides: usize,
    issued: Arc<AtomicU64>,
}
impl WorkerPort for RefiningNavigator {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let json = serde_json::json!({
            "tasks": (0..self.tasks).map(|index| serde_json::json!({
                "prompt": format!("task-{index}"),
                "required_capabilities": ["read"],
            })).collect::<Vec<_>>()
        })
        .to_string();
        let decide = serde_json::json!({
            "kind": "refine",
            "amended": (0..self.tasks).map(|index| serde_json::json!({
                "index": index,
                "prompt": format!("amended-{index}"),
                "required_capabilities": ["read"],
                "depends_on": []
            })).collect::<Vec<_>>(),
            "iteration": 1
        })
        .to_string();
        let issued = self.issued.clone();
        let max_decisions = self.decides;
        Box::pin(async move {
            let output = if task.id.ends_with("-decompose") {
                json
            } else if task.id.ends_with("-decide") {
                let seen = issued.fetch_add(1, Ordering::SeqCst) + 1;
                if seen > max_decisions as u64 {
                    serde_json::json!({"kind":"proceed"}).to_string()
                } else {
                    decide
                }
            } else {
                String::from("synthesis")
            };
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output,
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}

fn deciding_hive(navigator: Arc<dyn WorkerPort>, drivers: usize, refine_cap: u32) -> Hive {
    let mut config = HiveConfig::balanced(&["read"]);
    config.dimensions = 8;
    config.roles = vec![RoleProfile::navigator()];
    let mut ports: Vec<(String, Arc<dyn WorkerPort>)> =
        vec![(String::from("navigator"), navigator)];
    for index in 0..drivers {
        let mut role = RoleProfile::driver(&["read"]);
        role.name = format!("driver-{index}");
        ports.push((
            role.name.clone(),
            Arc::new(Driver {
                behavior: DriverBehavior::BarrierSucceed,
                barrier: None,
                completed: None,
            }),
        ));
        config.roles.push(role);
    }
    Hive::new(config, ports, Arc::new(Embedding))
        .unwrap()
        .with_decision(vesper_swarm::hive::DecisionConfig {
            refine_cap,
            pivot_cap: 2,
        })
        .unwrap()
}

/// Cap proof through the real orchestrator: refine requests beyond the
/// cap cannot loop; the exhaustion verdict lands in the audit stream and
/// the goal completes.
#[tokio::test]
async fn refine_loop_is_bounded_through_the_orchestrator() {
    let issued = Arc::new(AtomicU64::new(0));
    let navigator = Arc::new(RefiningNavigator {
        tasks: 1,
        decides: 9, // asks to refine forever; engine must cap it
        issued: issued.clone(),
    });
    let mut hive = deciding_hive(navigator, 1, 2);
    start_goal(&mut hive, "cap").await;
    let completed = hive.run_to_completion().await.unwrap();
    assert_eq!(completed, 1, "the capped goal still completes");
    // Refine iterations consumed exactly the cap (2), then the engine
    // forced ProceedWithFailure — audited, never silent.
    let decisions: Vec<_> = hive
        .gate_events()
        .into_iter()
        .filter(|event| matches!(event, vesper_swarm::hive::AuditEvent::DecisionIssued { .. }))
        .collect();
    assert!(
        decisions.len() >= 3,
        "at least cap+1 decisions: {decisions:?}"
    );
    let last = decisions.last().unwrap();
    if let vesper_swarm::hive::AuditEvent::DecisionIssued {
        verdict,
        refine_iterations,
        ..
    } = last
    {
        assert_eq!(*refine_iterations, 2);
        assert!(matches!(
            verdict,
            vesper_swarm::hive::DecisionVerdictPayload::ProceedWithFailure { .. }
        ));
    } else {
        panic!("expected DecisionIssued");
    }
    // And every iteration's ledger version stays retrievable.
    assert!(hive.version_registry().version_count("cap") >= 3);
}

/// Separation proof: judge and author role templates are hash-distinct,
/// and relabeling an author profile as "judge" (the D3 cheat) is refused
/// at assembly because the identity covers the authored surface, not the
/// label.
#[tokio::test]
async fn judge_and_author_templates_are_hash_distinct_and_checked() {
    let navigator = RoleProfile::navigator();
    let judge = RoleProfile::judge();
    let driver = RoleProfile::driver(&["read"]);
    assert_ne!(
        navigator.template_identity(),
        judge.template_identity(),
        "navigator and judge must be structurally distinct"
    );
    assert_ne!(driver.template_identity(), judge.template_identity());
    // The cheat: rename the driver profile to "judge" — the identity must
    // NOT change (labels are not content), so the pair collides.
    let mut impostor = driver.clone();
    impostor.name = String::from("judge");
    assert_eq!(
        impostor.template_identity(),
        driver.template_identity(),
        "relabeling must not change the authored-surface identity"
    );

    let build = |roles: Vec<RoleProfile>| {
        let mut config = HiveConfig::balanced(&["read"]);
        config.dimensions = 8;
        config.roles = roles;
        let ports: Vec<(String, Arc<dyn WorkerPort>)> = vec![
            (String::from("navigator"), Arc::new(Navigator { tasks: 1 })),
            (
                String::from("driver"),
                Arc::new(Driver {
                    behavior: DriverBehavior::BarrierSucceed,
                    barrier: None,
                    completed: None,
                }),
            ),
            (
                String::from("judge"),
                Arc::new(Driver {
                    behavior: DriverBehavior::BarrierSucceed,
                    barrier: None,
                    completed: None,
                }),
            ),
        ];
        Hive::new(config, ports, Arc::new(Embedding))
            .unwrap()
            .with_decision(vesper_swarm::hive::DecisionConfig::default())
    };

    // The relabeled driver as "judge" is refused: identical surface.
    assert!(
        build(vec![
            RoleProfile::navigator(),
            RoleProfile::driver(&["read"]),
            impostor,
        ])
        .is_err(),
        "relabeling an author as judge must be refused (identical surface)"
    );

    // A genuinely distinct judge role passes assembly.
    assert!(
        build(vec![
            RoleProfile::navigator(),
            RoleProfile::driver(&["read"]),
            RoleProfile::judge(),
        ])
        .is_ok(),
        "a real judge with distinct instructions is accepted"
    );
}

/// Retrieval proof through the public API: after a capped refine loop,
/// every version's evidence resolves.
#[tokio::test]
async fn evidence_versions_remain_retrievable_after_decisions() {
    let issued = Arc::new(AtomicU64::new(0));
    let navigator = Arc::new(RefiningNavigator {
        tasks: 1,
        decides: 1,
        issued,
    });
    let mut hive = deciding_hive(navigator, 1, 3);
    start_goal(&mut hive, "versions").await;
    hive.run_to_completion().await.unwrap();
    let registry = hive.version_registry();
    let count = registry.version_count("versions");
    assert!(count >= 1, "at least the pre-decision version: {count}");
    for version in 0..count {
        // Retrieval itself must never fail; each version is immutable.
        let snapshot = registry.version("versions", version).unwrap();
        let _ = snapshot.len();
    }
}

// ---- VRO-16 PR-3 proofs ----

use vesper_swarm::hive::{AuditEvent, BudgetCeiling, BudgetReading, BudgetWatchdog};

/// A Navigator whose synthesis cites a specific ledger id — or hallucinates
/// one. The trace gate must accept the real citation and fail closed on the
/// hallucinated one.
struct CitingNavigator {
    tasks: usize,
    cite: Option<u64>,
}
impl WorkerPort for CitingNavigator {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let json = serde_json::json!({
            "tasks": (0..self.tasks).map(|index| serde_json::json!({
                "prompt": format!("task-{index}"),
                "required_capabilities": ["read"],
            })).collect::<Vec<_>>()
        })
        .to_string();
        let cite = self.cite;
        Box::pin(async move {
            let output = if task.id.ends_with("-decompose") {
                json
            } else if task.id.ends_with("-synthesize") {
                match cite {
                    Some(id) => format!("The answer follows from [evidence:{id}]."),
                    None => String::from("The answer follows from the evidence."),
                }
            } else {
                String::from("proceed-json:{\"kind\":\"proceed\"}")
            };
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output,
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}

fn verifying_hive(
    navigator: Arc<dyn WorkerPort>,
    drivers: usize,
    ceiling: Option<BudgetCeiling>,
) -> Hive {
    let mut config = HiveConfig::balanced(&["read"]);
    config.dimensions = 8;
    config.roles = vec![RoleProfile::navigator()];
    let mut ports: Vec<(String, Arc<dyn WorkerPort>)> =
        vec![(String::from("navigator"), navigator)];
    for index in 0..drivers {
        let mut role = RoleProfile::driver(&["read"]);
        role.name = format!("driver-{index}");
        ports.push((
            role.name.clone(),
            Arc::new(Driver {
                behavior: DriverBehavior::BarrierSucceed,
                barrier: None,
                completed: None,
            }),
        ));
        config.roles.push(role);
    }
    Hive::new(config, ports, Arc::new(Embedding))
        .unwrap()
        .with_verification(ceiling)
        .unwrap()
}

/// Trace proof: a synthesis citing a real ledger entry passes the gates; a
/// hallucinated citation fails closed with an audited VerificationVerdict.
#[tokio::test]
async fn trace_gate_fails_closed_on_hallucinated_citations() {
    // First run: uncited synthesis passes; a fresh ledger's first
    // observation entry is deterministically id 1.
    let mut hive = verifying_hive(
        Arc::new(CitingNavigator {
            tasks: 1,
            cite: None,
        }),
        1,
        None,
    );
    start_goal(&mut hive, "trace-ok").await;
    assert!(hive.run_to_completion().await.is_ok());
    let entry_id = hive
        .ledger()
        .filtered(
            &vesper_swarm::ledger::store::MemoryScope::Swarm,
            vesper_swarm::ledger::store::EntryKind::Observation,
        )
        .first()
        .map(|hit| hit.entry.id)
        .unwrap();

    // Real citation passes end to end. A fresh hive's book contains its
    // own first entry at the same deterministic id.
    let fresh_id = 1;
    let mut good = verifying_hive(
        Arc::new(CitingNavigator {
            tasks: 1,
            cite: Some(fresh_id),
        }),
        1,
        None,
    );
    start_goal(&mut good, "trace-real").await;
    assert!(
        good.run_to_completion().await.is_ok(),
        "real citation id {fresh_id} (first run saw {entry_id})"
    );

    // Hallucinated citation fails closed with an audited verdict.
    let mut bad = verifying_hive(
        Arc::new(CitingNavigator {
            tasks: 1,
            cite: Some(entry_id + 999),
        }),
        1,
        None,
    );
    start_goal(&mut bad, "trace-fake").await;
    let result = bad.run_to_completion().await;
    assert!(result.is_err(), "dangling citation must fail closed");
    let verdicts: Vec<_> = bad
        .gate_events()
        .into_iter()
        .filter(|event| matches!(event, AuditEvent::VerificationVerdict { .. }))
        .collect();
    assert!(
        verdicts.iter().any(|event| matches!(
            event,
            AuditEvent::VerificationVerdict {
                failure: Some(_),
                ..
            }
        )),
        "failure verdict audited: {verdicts:?}"
    );
}

/// Digest proof: a tampered artifact between admission and synthesis is
/// inadmissible. Tampering is simulated by mutating the recorded digest
/// book through the public API surface is not possible — so the proof uses
/// the EvidenceBook directly (the orchestrator path is covered by the
/// trace proof's real pipeline).
#[tokio::test]
async fn digest_gate_rejects_tampered_artifacts() {
    let mut book = vesper_swarm::hive::EvidenceBook::default();
    book.admit("g-task-0", "authentic output", 1).unwrap();
    book.bind_ledger_entry("g-task-0", 41);
    // Untampered verification passes and exposes the trace target.
    assert!(book.verify("g-task-0", "authentic output").is_ok());
    assert_eq!(book.citable_ledger_ids(), vec![41]);
    // Tampered output fails closed with the recorded/recomputed pair.
    let failure = book.verify("g-task-0", "tampered output").unwrap_err();
    assert!(failure.to_string().contains("digest mismatch"));
}

/// Budget proof (injected readings): 50/80 emit without stopping; 100
/// hard-stops with no unbounded consumption — proven at the watchdog level
/// and through the orchestrator's accounting path.
#[tokio::test]
async fn budget_watchdog_emits_and_hard_stops() {
    let mut watchdog = BudgetWatchdog::new(BudgetCeiling {
        tokens: 100,
        elapsed_ms: 1_000_000,
    });
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 50,
            elapsed_ms: 0,
        },
    );
    assert!(matches!(
        state,
        vesper_swarm::hive::BudgetState::Warning { .. }
    ));
    assert_eq!(events.len(), 1);
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 80,
            elapsed_ms: 0,
        },
    );
    assert!(matches!(
        state,
        vesper_swarm::hive::BudgetState::Critical { .. }
    ));
    assert_eq!(events.len(), 1);
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 100,
            elapsed_ms: 0,
        },
    );
    assert!(matches!(
        state,
        vesper_swarm::hive::BudgetState::Exhausted { .. }
    ));
    assert_eq!(events.len(), 1);
    // No level fires twice.
    let (_, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 120,
            elapsed_ms: 0,
        },
    );
    assert!(events.is_empty(), "no duplicate threshold events");
    assert_eq!(watchdog.fired_levels(), vec![50, 80, 100]);
}

/// Budget proof through the orchestrator: a tiny token ceiling stops the
/// goal with truthful partial state and an audited 100% event.
#[tokio::test]
async fn orchestrator_hard_stops_on_budget_ceiling() {
    let mut hive = verifying_hive(
        Arc::new(CitingNavigator {
            tasks: 2,
            cite: None,
        }),
        2,
        Some(BudgetCeiling {
            tokens: 4, // a single small task output exhausts it
            elapsed_ms: 1_000_000,
        }),
    );
    start_goal(&mut hive, "budget").await;
    let result = hive.run_to_completion().await;
    assert!(result.is_err(), "budget ceiling must stop the run");
    let error = result.unwrap_err().to_string();
    assert!(error.contains("budget ceiling"), "{error}");
    // The 100% threshold is audited.
    let thresholds: Vec<u8> = hive
        .gate_events()
        .into_iter()
        .filter_map(|event| match event {
            AuditEvent::BudgetThreshold { level, .. } => Some(level),
            _ => None,
        })
        .collect();
    assert!(thresholds.contains(&100), "audited levels: {thresholds:?}");
    assert!(hive.budget_exhausted());
}

/// Audit-chain proof: a full pipeline run produces an exact-match
/// filterable audit chain covering verification verdicts.
#[tokio::test]
async fn audit_chain_is_complete_and_filterable() {
    let mut hive = verifying_hive(
        Arc::new(CitingNavigator {
            tasks: 1,
            cite: None,
        }),
        1,
        None,
    );
    start_goal(&mut hive, "chain").await;
    assert!(hive.run_to_completion().await.is_ok());
    let events = hive.gate_events();
    let digest_ok = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                AuditEvent::VerificationVerdict {
                    check: vesper_swarm::hive::VerificationCheck::Digest,
                    failure: None,
                    ..
                }
            )
        })
        .count();
    let trace_ok = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                AuditEvent::VerificationVerdict {
                    check: vesper_swarm::hive::VerificationCheck::Trace,
                    failure: None,
                    ..
                }
            )
        })
        .count();
    assert_eq!(digest_ok, 1, "one passing digest verdict: {events:?}");
    assert_eq!(trace_ok, 1, "one passing trace verdict: {events:?}");
}
