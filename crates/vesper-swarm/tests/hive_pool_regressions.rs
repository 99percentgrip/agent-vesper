//! Actual one-navigator/three-driver pool routing, not nominal topology slots.
use futures_util::future::BoxFuture;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vesper_swarm::hive::orchestrator::{Hive, HiveConfig, HiveEvent, HiveGoal};
use vesper_swarm::ledger::store::{BoundedText, EmbeddingPort, LedgerError};
use vesper_swarm::pool::WorkerInstanceFactory;
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
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
struct Factory {
    navigator: bool,
    barrier: Arc<tokio::sync::Barrier>,
    seen: Arc<Mutex<Vec<(String, u64)>>>,
}
struct Worker {
    id: u64,
    navigator: bool,
    barrier: Arc<tokio::sync::Barrier>,
    seen: Arc<Mutex<Vec<(String, u64)>>>,
}
impl WorkerInstanceFactory for Factory {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        id: u64,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move {
            Ok(Arc::new(Worker {
                id,
                navigator: self.navigator,
                barrier: self.barrier.clone(),
                seen: self.seen.clone(),
            }) as Arc<dyn WorkerPort>)
        })
    }
}
impl WorkerPort for Worker {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push((task.id.clone(), self.id));
            let output = if self.navigator && task.id.ends_with("-decompose") {
                r#"{"tasks":[{"prompt":"first"},{"prompt":"second"},{"prompt":"third"}]}"#.into()
            } else if self.navigator {
                for id in 1..=3 {
                    assert!(task.prompt.contains(&format!("instance-{id}")));
                }
                "grounded synthesis".into()
            } else {
                self.barrier.wait().await;
                format!("instance-{}", self.id)
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
async fn three_pool_workers_overlap_and_assignments_name_the_executing_instance() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let factories: Vec<(String, Arc<dyn WorkerInstanceFactory>)> = [true, false]
        .into_iter()
        .map(|navigator| {
            (
                if navigator { "navigator" } else { "driver" }.into(),
                Arc::new(Factory {
                    navigator,
                    barrier: barrier.clone(),
                    seen: seen.clone(),
                }) as Arc<dyn WorkerInstanceFactory>,
            )
        })
        .collect();
    let mut config = HiveConfig::balanced(&[]);
    config.roles[1].min_workers = 3;
    let mut hive = Hive::with_factories(config, factories, Arc::new(Embedding))
        .await
        .unwrap();
    hive.admit_topology().unwrap();
    assert_eq!(hive.topology().node_count(), 4);
    for node in ["navigator-1", "driver-1", "driver-2", "driver-3"] {
        assert!(hive.topology().nodes.keys().any(|id| id.as_str() == node));
    }
    hive.submit(HiveGoal::new("goal", "work")).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), hive.run_to_completion())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    {
        let seen = seen.lock().unwrap();
        for event in hive.events() {
            if let HiveEvent::TaskAssigned(task, node, _) = event {
                let id = seen.iter().find(|(actual, _)| actual == &task).unwrap().1;
                assert_eq!(node, format!("driver-{id}"));
                let hits = hive
                    .ledger()
                    .snapshot()
                    .exact(&vesper_swarm::ledger::store::MemoryScope::Swarm, &task);
                assert_eq!(hits.len(), 1);
                assert_eq!(hits[0].entry.provenance.worker_id, node);
                assert_eq!(hits[0].entry.provenance.role, "driver");
            }
        }
    }
    assert_eq!(hive.ledger().len(), 4);
    hive.close();
    assert!(hive.submit(HiveGoal::new("closed", "work")).is_err());
    assert!(hive.run_tick().await.is_err());
}
#[tokio::test]
async fn a_single_port_cannot_advertise_multiple_worker_slots() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let port: Arc<dyn WorkerPort> = Arc::new(Worker {
        id: 1,
        navigator: true,
        barrier: Arc::new(tokio::sync::Barrier::new(3)),
        seen,
    });
    let mut config = HiveConfig::balanced(&[]);
    config.roles[1].min_workers = 3;
    assert!(
        Hive::new(
            config,
            vec![("navigator".into(), port.clone()), ("driver".into(), port)],
            Arc::new(Embedding)
        )
        .is_err()
    );
}

struct SharedFactory(Arc<dyn WorkerPort>);
impl WorkerInstanceFactory for SharedFactory {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        _: u64,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}
#[tokio::test]
async fn different_role_pools_cannot_alias_one_execution_instance() {
    let port: Arc<dyn WorkerPort> = Arc::new(Worker {
        id: 1,
        navigator: true,
        barrier: Arc::new(tokio::sync::Barrier::new(3)),
        seen: Arc::new(Mutex::new(Vec::new())),
    });
    let factory: Arc<dyn WorkerInstanceFactory> = Arc::new(SharedFactory(port));
    assert!(
        Hive::with_factories(
            HiveConfig::balanced(&[]),
            vec![
                ("navigator".into(), factory.clone()),
                ("driver".into(), factory)
            ],
            Arc::new(Embedding)
        )
        .await
        .is_err()
    );
}

struct HangingFactory {
    navigator: bool,
    signals: Arc<Mutex<Vec<CancellationSignal>>>,
}
struct HangingWorker {
    navigator: bool,
    signals: Arc<Mutex<Vec<CancellationSignal>>>,
}
impl WorkerInstanceFactory for HangingFactory {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        _: u64,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move {
            Ok(Arc::new(HangingWorker {
                navigator: self.navigator,
                signals: self.signals.clone(),
            }) as Arc<dyn WorkerPort>)
        })
    }
}
impl WorkerPort for HangingWorker {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        signal: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            if !self.navigator {
                self.signals.lock().unwrap().push(signal);
                std::future::pending::<()>().await;
            }
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: r#"{"tasks":[{"prompt":"first"},{"prompt":"second"},{"prompt":"third"}]}"#
                    .into(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
#[tokio::test]
async fn dropping_hive_tick_cancels_actual_pooled_turns_and_prevents_replay() {
    use std::future::Future;
    use std::task::{Context, Waker};
    let signals = Arc::new(Mutex::new(Vec::new()));
    let factories: Vec<(String, Arc<dyn WorkerInstanceFactory>)> = [true, false]
        .into_iter()
        .map(|navigator| {
            (
                if navigator { "navigator" } else { "driver" }.into(),
                Arc::new(HangingFactory {
                    navigator,
                    signals: signals.clone(),
                }) as Arc<dyn WorkerInstanceFactory>,
            )
        })
        .collect();
    let mut config = HiveConfig::balanced(&[]);
    config.roles[1].min_workers = 3;
    let mut hive = Hive::with_factories(config, factories, Arc::new(Embedding))
        .await
        .unwrap();
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal::new("dropped", "work")).unwrap();
    let mut tick = Box::pin(hive.run_tick());
    assert!(
        tick.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(tick);
    assert_eq!(signals.lock().unwrap().len(), 3);
    assert!(
        signals
            .lock()
            .unwrap()
            .iter()
            .all(CancellationSignal::is_cancelled)
    );
    assert!(hive.interrupted_goal().is_some());
    assert!(hive.run_tick().await.is_err());
}

#[tokio::test]
async fn cancellation_waiters_wake_before_or_after_registration() {
    use std::future::Future;
    use std::task::{Context, Waker};
    use vesper_swarm::worker::CancelFlag;
    for cancel_first in [false, true] {
        let flag = CancelFlag::new();
        let signal = flag.signal();
        let clone = signal.clone();
        if cancel_first {
            flag.cancel();
        }
        let mut first = Box::pin(signal.cancelled());
        let mut second = Box::pin(clone.cancelled());
        if !cancel_first {
            assert!(
                first
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            assert!(
                second
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            flag.cancel();
        }
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready()
        );
    }
}

#[tokio::test]
async fn topology_admission_is_transactional_and_idempotent() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let factories: Vec<(String, Arc<dyn WorkerInstanceFactory>)> = [true, false]
        .into_iter()
        .map(|navigator| {
            (
                if navigator { "navigator" } else { "driver" }.into(),
                Arc::new(Factory {
                    navigator,
                    barrier: barrier.clone(),
                    seen: seen.clone(),
                }) as Arc<dyn WorkerInstanceFactory>,
            )
        })
        .collect();
    let mut hive = Hive::with_factories(HiveConfig::balanced(&[]), factories, Arc::new(Embedding))
        .await
        .unwrap();
    hive.bus().subscribe("driver-1", &[]).unwrap();
    assert!(hive.admit_topology().is_err());
    assert_eq!(hive.topology().node_count(), 0);
    // The failed batch must not leave its earlier navigator subscription behind.
    hive.bus().subscribe("navigator-1", &[]).unwrap();
    hive.bus().unsubscribe("navigator-1").unwrap();
    hive.bus().unsubscribe("driver-1").unwrap();
    hive.admit_topology().unwrap();
    hive.admit_topology().unwrap();
    assert_eq!(hive.topology().node_count(), 2);
    hive.close();
    assert!(hive.admit_topology().is_err());
}
