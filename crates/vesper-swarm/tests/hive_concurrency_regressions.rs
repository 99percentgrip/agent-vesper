//! Real pending futures prove overlap, dependency barriers and sibling cancellation.
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vesper_swarm::hive::orchestrator::{Hive, HiveConfig, HiveGoal, RoleProfile};
use vesper_swarm::ledger::store::{BoundedText, EmbeddingPort, LedgerError};
use vesper_swarm::worker::{
    CancellationSignal, TaskPriority, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort,
    WorkerTask,
};

struct Embedding;
impl EmbeddingPort for Embedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
    }
}
struct Navigator;
impl WorkerPort for Navigator {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            let output = if task.id.ends_with("-decompose") {
                serde_json::json!({"tasks":[
                    {"prompt":"first", "required_capabilities":["read"]},
                    {"prompt":"second", "required_capabilities":["read"]},
                    {"prompt":"third", "required_capabilities":["read"]},
                    {"prompt":"dependent", "required_capabilities":["read"], "depends_on":[0,1,2]}
                ]})
                .to_string()
            } else {
                "final".into()
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
struct Driver {
    barrier: Arc<tokio::sync::Barrier>,
    finished: Arc<AtomicUsize>,
    signals: Arc<Mutex<Vec<CancellationSignal>>>,
    fail: bool,
}
impl WorkerPort for Driver {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities {
            tools: vec!["read".into()],
            max_concurrent_tasks: 1,
        }
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        signal: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            if task.prompt.starts_with("dependent") {
                assert_eq!(self.finished.load(Ordering::SeqCst), 3);
                for index in 0..3 {
                    assert!(
                        task.prompt
                            .contains(&format!("Prerequisite {index} output"))
                    );
                }
            } else {
                self.signals.lock().unwrap().push(signal);
                self.barrier.wait().await;
                if self.fail {
                    if task.id.ends_with("-0") {
                        return Err(WorkerError::Failed(
                            task.id.clone(),
                            "scripted failure".into(),
                        ));
                    }
                    std::future::pending::<()>().await;
                }
                self.finished.fetch_add(1, Ordering::SeqCst);
            }
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: task.prompt.clone(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
fn setup(fail: bool) -> (Hive, Arc<Mutex<Vec<CancellationSignal>>>) {
    let mut config = HiveConfig::balanced(&["read"]);
    config.dimensions = 8;
    config.roles = vec![RoleProfile::navigator()];
    let mut ports: Vec<(String, Arc<dyn WorkerPort>)> =
        vec![("navigator".into(), Arc::new(Navigator))];
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let finished = Arc::new(AtomicUsize::new(0));
    let signals = Arc::new(Mutex::new(Vec::new()));
    for index in 0..3 {
        let mut role = RoleProfile::driver(&["read"]);
        role.name = format!("driver-{index}");
        ports.push((
            role.name.clone(),
            Arc::new(Driver {
                barrier: barrier.clone(),
                finished: finished.clone(),
                signals: signals.clone(),
                fail,
            }),
        ));
        config.roles.push(role);
    }
    let hive = Hive::new(config, ports, Arc::new(Embedding)).unwrap();
    (hive, signals)
}
async fn start(hive: &mut Hive) {
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal {
        id: "goal".into(),
        prompt: "work".into(),
        priority: TaskPriority::Normal,
    })
    .unwrap();
}
#[tokio::test]
async fn three_distinct_ports_overlap_before_dependent_task() {
    let (mut hive, signals) = setup(false);
    start(&mut hive).await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), hive.run_to_completion())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(signals.lock().unwrap().len(), 3);
}
#[tokio::test]
async fn failed_sibling_cancels_other_dispatched_turns_and_blocks_replay() {
    let (mut hive, signals) = setup(true);
    start(&mut hive).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), hive.run_tick())
            .await
            .unwrap()
            .is_err()
    );
    {
        let signals = signals.lock().unwrap();
        assert_eq!(signals.len(), 3);
        assert!(signals.iter().all(CancellationSignal::is_cancelled));
    }
    assert!(hive.run_tick().await.is_err());
}

struct AliasedDriver {
    active: AtomicUsize,
    high_water: AtomicUsize,
    calls: AtomicUsize,
}
impl WorkerPort for AliasedDriver {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities {
            tools: vec!["read".into()],
            max_concurrent_tasks: 1,
        }
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.high_water.fetch_max(active, Ordering::SeqCst);
            tokio::task::yield_now().await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: "evidence".into(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
#[tokio::test]
async fn aliasing_one_port_under_multiple_classes_does_not_multiply_capacity() {
    let driver = Arc::new(AliasedDriver {
        active: AtomicUsize::new(0),
        high_water: AtomicUsize::new(0),
        calls: AtomicUsize::new(0),
    });
    let mut config = HiveConfig::balanced(&["read"]);
    config.roles = vec![RoleProfile::navigator()];
    let mut ports: Vec<(String, Arc<dyn WorkerPort>)> =
        vec![("navigator".into(), Arc::new(Navigator))];
    for index in 0..3 {
        let mut role = RoleProfile::driver(&["read"]);
        role.name = format!("alias-{index}");
        ports.push((role.name.clone(), driver.clone()));
        config.roles.push(role);
    }
    let mut hive = Hive::new(config, ports, Arc::new(Embedding)).unwrap();
    start(&mut hive).await;
    assert_eq!(hive.run_to_completion().await.unwrap(), 1);
    assert_eq!(driver.high_water.load(Ordering::SeqCst), 1);
    assert_eq!(driver.calls.load(Ordering::SeqCst), 4);
}
