//! End-to-end synthetic swarm tests (VRO-15 PR-9).
//!
//! One navigator port and one driver port, driven across mesh and
//! hierarchical topologies with fake turns: the current pipeline — goal →
//! decomposition → bus assignment (priority-mapped) → scored driver turns
//! → ledger trajectory writes → synthesis — runs in-process with zero
//! network, zero providers.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use vesper_swarm::hive::orchestrator::{
    Hive, HiveConfig, HiveError, HiveEvent, HiveGoal, RoleProfile,
};
use vesper_swarm::ledger::store::{
    BoundedText, EmbeddingPort, EntryKind, LedgerError, MemoryScope,
};
use vesper_swarm::topology::{TopologyKind, TopologyRole};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

// ---------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------

/// Deterministic embedding port (hash vectors, like PR-7's).
struct FakeEmbeddingPort;

impl EmbeddingPort for FakeEmbeddingPort {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> futures_util::future::BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move {
            Ok(texts
                .iter()
                .map(|text| {
                    let mut state = 0xcbf2_9ce4_8422_2325u64;
                    for byte in text.as_str().as_bytes() {
                        state ^= u64::from(*byte);
                        state = state.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                    (0..8)
                        .map(|index| {
                            state ^= state >> 12;
                            state ^= state << 25;
                            state ^= state >> 27;
                            ((state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> (48 + index % 8)) as f32
                                / 256.0)
                                - 0.5
                        })
                        .collect()
                })
                .collect())
        })
    }
}

/// Scripted navigator: decomposes into the configured task count.
struct FakeNavigator {
    tasks: usize,
    turns: AtomicUsize,
}

impl WorkerPort for FakeNavigator {
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _cancellation: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let tasks = self.tasks;
        let turns = &self.turns;
        Box::pin(async move {
            turns.fetch_add(1, Ordering::AcqRel);
            let output = if task.id.ends_with("-decompose") {
                serde_json::json!({"tasks": (0..tasks.min(65)).map(|index| serde_json::json!({"prompt":format!("concrete task {index}"), "required_capabilities":["read"]})).collect::<Vec<_>>()}).to_string()
            } else {
                assert!(task.prompt.contains("Completed task evidence"));
                assert!(
                    task.prompt.contains("did:"),
                    "synthesis needs real completed task output"
                );
                format!("synthesis of {}", task.id)
            };
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output,
                success: true,
                duration: Duration::from_millis(1),
            })
        })
    }

    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
}

/// Fake driver: succeeds after a tiny delay; counts turns.
struct FakeDriver {
    turns: AtomicUsize,
    fail_on: Option<String>,
}

impl WorkerPort for FakeDriver {
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let turns = &self.turns;
        let fail_on = self.fail_on.clone();
        Box::pin(async move {
            turns.fetch_add(1, Ordering::AcqRel);
            // Observe cancellation promptly (cooperative worker).
            for _ in 0..5 {
                if cancellation.is_cancelled() {
                    return Err(WorkerError::Cancelled(task.id.clone()));
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            if fail_on.as_deref() == Some(task.id.as_str()) {
                return Err(WorkerError::Failed(task.id.clone(), "scripted".into()));
            }
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: format!("did: {}", task.prompt),
                success: true,
                duration: Duration::from_millis(5),
            })
        })
    }

    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities {
            tools: vec![String::from("read"), String::from("write")],
            max_concurrent_tasks: 1,
        }
    }
}

fn ports(navigator_tasks: usize) -> (Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let nav_turns = Arc::new(AtomicUsize::new(0));
    let driver_turns = Arc::new(AtomicUsize::new(0));
    let _ = navigator_tasks;
    (nav_turns, driver_turns)
}

// ---------------------------------------------------------------------
// Topology parameterization
// ---------------------------------------------------------------------

fn config_for(kind: TopologyKind) -> HiveConfig {
    let mut config = HiveConfig::balanced(&["read", "write"]);
    config.topology_kind = kind;
    config
}

async fn assembled(kind: TopologyKind, tasks: usize) -> (Hive, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let config = config_for(kind);
    let navigator = Arc::new(FakeNavigator {
        tasks,
        turns: AtomicUsize::new(0),
    });
    let driver = Arc::new(FakeDriver {
        turns: AtomicUsize::new(0),
        fail_on: None,
    });
    let nav_turns = Arc::new(AtomicUsize::new(0));
    let driver_turns = Arc::new(AtomicUsize::new(0));
    // Wrap the counters we can observe: reuse the ports' internal ones by
    // sharing AtomicUsize instances.
    let navigator_port: Arc<dyn WorkerPort> = Arc::new(CountingPort {
        inner: navigator,
        counter: Arc::clone(&nav_turns),
    });
    let driver_port: Arc<dyn WorkerPort> = Arc::new(CountingPort {
        inner: driver,
        counter: Arc::clone(&driver_turns),
    });
    let mut hive = Hive::new(
        config,
        vec![
            (String::from("navigator"), navigator_port),
            (String::from("driver"), driver_port),
        ],
        Arc::new(FakeEmbeddingPort),
    )
    .expect("hive assembles");
    hive.admit_topology().expect("topology admits");
    (hive, nav_turns, driver_turns)
}

/// Wrapper that counts turns across any inner port.
struct CountingPort {
    inner: Arc<dyn WorkerPort>,
    counter: Arc<AtomicUsize>,
}

impl WorkerPort for CountingPort {
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        self.counter.fetch_add(1, Ordering::AcqRel);
        self.inner.run_turn(task, cancellation)
    }

    fn capabilities(&self) -> WorkerCapabilities {
        self.inner.capabilities()
    }
}

// ---------------------------------------------------------------------
// The directive's e2e scenario
// ---------------------------------------------------------------------

#[tokio::test]
async fn one_navigator_three_driver_turns_mesh() {
    let (mut hive, nav_turns, driver_turns) = assembled(TopologyKind::Mesh, 3).await;

    // Topology: navigator (queen) + drivers admitted, wired, led.
    let topology = hive.topology().clone();
    assert!(
        topology.node_count() >= 2,
        "navigator + driver classes admitted"
    );
    assert_eq!(
        topology.leader.as_ref().map(|id| id.as_str()),
        Some("navigator-0")
    );
    assert!(!topology.edges.is_empty(), "mesh wiring present");

    hive.submit(HiveGoal::new("g1", "ship the feature"))
        .unwrap();
    let completed = hive.run_to_completion().await.expect("hive completes");
    assert_eq!(completed, 1);

    // Navigator: 1 decomposition + 1 synthesis.
    assert_eq!(nav_turns.load(Ordering::Acquire), 2);
    // Drivers: 3 tasks.
    assert_eq!(driver_turns.load(Ordering::Acquire), 3);

    // Event pipeline in order.
    let events = hive.events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HiveEvent::GoalAccepted(id) if id == "g1"))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HiveEvent::Decomposed(id, 3) if id == "g1"))
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, HiveEvent::TaskAssigned(_, _, _)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, HiveEvent::TrajectoryWritten(_)))
            .count(),
        3
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HiveEvent::GoalSynthesized(id) if id == "g1"))
    );

    // Ledger: 3 trajectories + 1 synthesis, all in the shared swarm scope.
    let ledger = hive.ledger().clone();
    assert_eq!(ledger.len(), 4);
    let hits = ledger
        .query(vesper_swarm::ledger::store::LedgerQuery::Filtered {
            scope: MemoryScope::Swarm,
            kind: EntryKind::Observation,
        })
        .await
        .expect("query");
    assert_eq!(hits.len(), 4);
    // The task trajectories are keyed by task id.
    for index in 0..3 {
        assert_eq!(
            ledger
                .exact(&MemoryScope::Swarm, &format!("g1-task-{index}"))
                .len(),
            1,
            "trajectory for task {index} is exact-queryable"
        );
    }
}

#[tokio::test]
async fn one_navigator_three_driver_turns_hierarchical() {
    let (mut hive, nav_turns, driver_turns) = assembled(TopologyKind::Hierarchical, 3).await;
    hive.submit(HiveGoal::new("h1", "migrate the module"))
        .unwrap();
    hive.run_to_completion().await.expect("completes");
    assert_eq!(nav_turns.load(Ordering::Acquire), 2);
    assert_eq!(driver_turns.load(Ordering::Acquire), 3);
    // Hierarchical wiring: parent→child edges only.
    let topology = hive.topology();
    assert!(
        topology.edges.iter().all(|edge| edge.from != edge.to),
        "no self edges"
    );
    assert_eq!(hive.ledger().len(), 4);
}

#[tokio::test]
async fn multiple_goals_run_sequentially_through_one_pipeline() {
    let (mut hive, _, driver_turns) = assembled(TopologyKind::Mesh, 2).await;
    for index in 0..3 {
        hive.submit(HiveGoal::new(format!("multi-{index}"), "work"))
            .unwrap();
    }
    let completed = hive.run_to_completion().await.expect("completes");
    assert_eq!(completed, 3);
    assert_eq!(driver_turns.load(Ordering::Acquire), 6);
    assert_eq!(hive.ledger().len(), 9, "3 goals × (2 tasks + 1 synthesis)");
}

#[tokio::test]
async fn failing_driver_turn_retains_goal_without_automatic_replay() {
    let config = config_for(TopologyKind::Mesh);
    let navigator = Arc::new(FakeNavigator {
        tasks: 1,
        turns: AtomicUsize::new(0),
    });
    let driver = Arc::new(FakeDriver {
        turns: AtomicUsize::new(0),
        fail_on: Some(String::from("fail-1-task-0")),
    });
    let nav_port: Arc<dyn WorkerPort> = Arc::new(CountingPort {
        inner: navigator,
        counter: Arc::new(AtomicUsize::new(0)),
    });
    let driver_port: Arc<dyn WorkerPort> = Arc::new(CountingPort {
        inner: driver,
        counter: Arc::new(AtomicUsize::new(0)),
    });
    let mut hive = Hive::new(
        config,
        vec![
            (String::from("navigator"), nav_port),
            (String::from("driver"), driver_port),
        ],
        Arc::new(FakeEmbeddingPort),
    )
    .expect("assembles");
    hive.admit_topology().expect("admits");
    hive.submit(HiveGoal::new("fail-1", "will fail a task"))
        .unwrap();
    // A failing driver turn surfaces as a Worker error through the hive.
    let outcome = hive.run_tick().await;
    match outcome {
        Err(HiveError::Worker(error)) => {
            assert!(matches!(error, WorkerError::Failed(_, _)));
        }
        other => panic!("expected worker failure, got {other:?}"),
    }
    assert_eq!(hive.interrupted_goal().unwrap().id, "fail-1");
    assert!(matches!(
        hive.run_tick().await,
        Err(HiveError::Interrupted(_))
    ));
    assert!(
        hive.events()
            .iter()
            .any(|e| matches!(e, HiveEvent::GoalAccepted(_)))
    );
}

#[tokio::test]
async fn empty_queue_tick_is_a_clean_no_op() {
    let (mut hive, _, _) = assembled(TopologyKind::Mesh, 1).await;
    assert!(!hive.run_tick().await.expect("tick"));
    assert!(hive.events().is_empty());
}

#[tokio::test]
async fn missing_ports_are_refused_at_assembly() {
    let config = config_for(TopologyKind::Mesh);
    let error = Hive::new(config, Vec::new(), Arc::new(FakeEmbeddingPort)).unwrap_err();
    assert!(
        matches!(error, HiveError::MissingPort(_) | HiveError::UnknownRole(_)),
        "empty ports must be refused loudly, got {error:?}"
    );
}

#[tokio::test]
async fn navigator_must_be_the_first_role() {
    let mut config = config_for(TopologyKind::Mesh);
    config.roles = vec![RoleProfile::driver(&["read"]), RoleProfile::navigator()];
    let navigator = Arc::new(FakeNavigator {
        tasks: 1,
        turns: AtomicUsize::new(0),
    });
    let driver = Arc::new(FakeDriver {
        turns: AtomicUsize::new(0),
        fail_on: None,
    });
    let error = Hive::new(
        config,
        vec![
            (String::from("navigator"), navigator as Arc<dyn WorkerPort>),
            (String::from("driver"), driver as Arc<dyn WorkerPort>),
        ],
        Arc::new(FakeEmbeddingPort),
    )
    .unwrap_err();
    assert!(matches!(error, HiveError::UnknownRole(_)));
}

#[test]
fn topology_roles_reflect_the_hive_structure() {
    // Driver classes join as Workers; the navigator is the Queen.
    let mut config = config_for(TopologyKind::Centralized);
    config.roles[1].min_workers = 1;
    let navigator = Arc::new(FakeNavigator {
        tasks: 1,
        turns: AtomicUsize::new(0),
    });
    let driver = Arc::new(FakeDriver {
        turns: AtomicUsize::new(0),
        fail_on: None,
    });
    let mut hive = Hive::new(
        config,
        vec![
            (String::from("navigator"), navigator as Arc<dyn WorkerPort>),
            (String::from("driver"), driver as Arc<dyn WorkerPort>),
        ],
        Arc::new(FakeEmbeddingPort),
    )
    .expect("assembles");
    hive.admit_topology().expect("admits");
    let topology = hive.topology();
    let queen = topology
        .nodes
        .values()
        .filter(|node| node.role == TopologyRole::Queen)
        .count();
    assert_eq!(queen, 1, "exactly one queen (the navigator)");
    let workers = topology
        .nodes
        .values()
        .filter(|node| node.role == TopologyRole::Worker)
        .count();
    assert_eq!(workers, 1, "only the supplied driver instance joins");
}

#[allow(dead_code)]
fn _silence_helper() {
    let _ = ports;
}

#[tokio::test]
async fn excessive_task_count_is_refused_before_driver_dispatch() {
    let (mut hive, _, drivers) = assembled(TopologyKind::Mesh, usize::MAX).await;
    hive.submit(HiveGoal::new("too-many", "work")).unwrap();
    assert!(hive.run_tick().await.is_err());
    assert_eq!(drivers.load(Ordering::Acquire), 0);
    assert_eq!(hive.interrupted_goal().unwrap().id, "too-many");
    assert!(
        hive.run_tick().await.is_err(),
        "no ambiguous automatic replay"
    );
}

#[tokio::test]
async fn duplicate_and_oversized_goals_are_refused_without_enqueueing() {
    let (mut hive, _, _) = assembled(TopologyKind::Mesh, 1).await;
    hive.submit(HiveGoal::new("same", "work")).unwrap();
    assert!(hive.submit(HiveGoal::new("same", "repeat")).is_err());
    assert!(
        hive.submit(HiveGoal::new("large", "x".repeat(65_537)))
            .is_err()
    );
    assert_eq!(hive.run_to_completion().await.unwrap(), 1);
    assert!(hive.submit(HiveGoal::new("same", "replay")).is_err());
}

struct StructuredNavigator;
impl WorkerPort for StructuredNavigator {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            let output = if task.id.ends_with("decompose") {
                r#"{"tasks":[{"prompt":"apply the discovered change","required_capabilities":["write"],"depends_on":[1]},{"prompt":"inspect the source","required_capabilities":["read"]}]}"#.to_owned()
            } else {
                assert!(task.prompt.contains("inspect the source"));
                assert!(task.prompt.contains("Prerequisite 1 output"));
                "grounded synthesis".into()
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

#[tokio::test]
async fn structured_tasks_select_capable_classes_and_deliver_prerequisite_output() {
    let mut config = HiveConfig::balanced(&["read"]);
    let mut writer = RoleProfile::driver(&["write"]);
    writer.name = "writer".into();
    config.roles.push(writer);
    let reader_count = Arc::new(AtomicUsize::new(0));
    let writer_count = Arc::new(AtomicUsize::new(0));
    let driver = || {
        Arc::new(FakeDriver {
            turns: AtomicUsize::new(0),
            fail_on: None,
        })
    };
    let mut hive = Hive::new(
        config,
        vec![
            ("navigator".into(), Arc::new(StructuredNavigator)),
            (
                "driver".into(),
                Arc::new(CountingPort {
                    inner: driver(),
                    counter: reader_count.clone(),
                }),
            ),
            (
                "writer".into(),
                Arc::new(CountingPort {
                    inner: driver(),
                    counter: writer_count.clone(),
                }),
            ),
        ],
        Arc::new(FakeEmbeddingPort),
    )
    .unwrap();
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal::new(
        "structured",
        "original goal must not replace decomposition",
    ))
    .unwrap();
    assert_eq!(hive.run_to_completion().await.unwrap(), 1);
    assert_eq!(reader_count.load(Ordering::Acquire), 1);
    assert_eq!(writer_count.load(Ordering::Acquire), 1);
    let assignments: Vec<_> = hive
        .events()
        .into_iter()
        .filter_map(|event| match event {
            HiveEvent::TaskAssigned(task, worker, _) => Some((task, worker)),
            _ => None,
        })
        .collect();
    assert_eq!(
        assignments,
        vec![
            ("structured-task-1".into(), "driver-0".into()),
            ("structured-task-0".into(), "writer-0".into())
        ]
    );
    let trajectory = hive
        .ledger()
        .exact(&MemoryScope::Swarm, "structured-task-0");
    assert_eq!(trajectory[0].entry.provenance.role, "writer");
    assert!(
        trajectory[0]
            .entry
            .text
            .as_str()
            .contains("inspect the source")
    );
}

#[tokio::test]
async fn unrelated_bus_message_with_identical_prompt_cannot_substitute_for_assignment() {
    use vesper_swarm::bus::{MessageKind, MessagePriority, OutgoingMessage};
    let (mut hive, _, driver_turns) = assembled(TopologyKind::Mesh, 1).await;
    hive.bus()
        .send(
            OutgoingMessage::new("navigator", "driver-0")
                .kind(MessageKind::TaskAssign)
                .priority(MessagePriority::Urgent)
                .payload("concrete task 0"),
        )
        .unwrap();
    hive.submit(HiveGoal::new("correlation", "work")).unwrap();
    assert!(matches!(hive.run_tick().await, Err(HiveError::Bus(_))));
    assert_eq!(driver_turns.load(Ordering::SeqCst), 0);
    assert!(hive.interrupted_goal().is_some());
    assert!(matches!(
        hive.run_tick().await,
        Err(HiveError::Interrupted(_))
    ));
}
