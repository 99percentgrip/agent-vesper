//! Independent async factories, transactional publication and real turn routing.
use futures_util::future::BoxFuture;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;
use vesper_swarm::pool::{PoolConfig, PoolError, WorkerInstanceFactory, WorkerPool};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

struct Instance {
    id: u64,
    barrier: Option<Arc<tokio::sync::Barrier>>,
}
impl WorkerPort for Instance {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            if let Some(barrier) = &self.barrier {
                barrier.wait().await;
            }
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: self.id.to_string(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
#[derive(Default)]
struct Factory {
    boot_barrier: Option<Arc<tokio::sync::Barrier>>,
    turn_barrier: Option<Arc<tokio::sync::Barrier>>,
    calls: AtomicUsize,
    hang: AtomicBool,
    fail: bool,
    alias: Option<Arc<dyn WorkerPort>>,
    signals: Mutex<Vec<CancellationSignal>>,
}
impl WorkerInstanceFactory for Factory {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        id: u64,
        signal: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.signals.lock().unwrap().push(signal);
            if let Some(barrier) = &self.boot_barrier {
                barrier.wait().await;
            }
            if self.hang.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if self.fail && id == 2 {
                return Err(WorkerError::Failed(id.to_string(), "boot failed".into()));
            }
            Ok(self.alias.clone().unwrap_or_else(|| {
                Arc::new(Instance {
                    id,
                    barrier: self.turn_barrier.clone(),
                })
            }))
        })
    }
}
fn config(min: u32, max: u32) -> PoolConfig {
    PoolConfig {
        min_workers: min,
        max_workers: max,
        default_turn_deadline: Duration::from_secs(2),
        ..Default::default()
    }
}
#[tokio::test]
async fn boot_and_turn_barriers_require_three_independent_instances() {
    let factory = Arc::new(Factory {
        boot_barrier: Some(Arc::new(tokio::sync::Barrier::new(3))),
        turn_barrier: Some(Arc::new(tokio::sync::Barrier::new(3))),
        ..Default::default()
    });
    let pool = WorkerPool::with_factory(config(3, 3), factory.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), pool.initialize())
        .await
        .unwrap()
        .unwrap();
    pool.initialize().await.unwrap();
    assert_eq!(factory.calls.load(Ordering::SeqCst), 3);
    let receipts = tokio::time::timeout(
        Duration::from_secs(1),
        futures_util::future::try_join_all(
            (0..3).map(|index| pool.run_task(WorkerTask::new(format!("task-{index}"), "work"))),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let identities: std::collections::BTreeSet<_> = receipts
        .iter()
        .map(|receipt| receipt.output.clone())
        .collect();
    assert_eq!(identities.len(), 3);
    assert_eq!(pool.idle_workers(), 3);
}
#[tokio::test]
async fn growth_replacement_and_shrink_use_real_instances() {
    let factory = Arc::new(Factory::default());
    let pool = WorkerPool::with_factory(config(1, 2), factory.clone()).unwrap();
    pool.initialize().await.unwrap();
    let (first, _) = pool.acquire(&[]).await.unwrap();
    let (second, _) = pool.acquire(&[]).await.unwrap();
    assert_ne!(first.worker_id(), second.worker_id());
    assert_eq!(factory.calls.load(Ordering::SeqCst), 2);
    drop((first, second));
    pool.test_freeze_last_seen(Duration::from_secs(3));
    assert_eq!(pool.health_tick(tokio::time::Instant::now()).len(), 2);
    assert_eq!(pool.replace_failed().await.unwrap(), 2);
    assert_eq!(factory.calls.load(Ordering::SeqCst), 4);
    let receipt = pool
        .run_task(WorkerTask::new("replacement", "work"))
        .await
        .unwrap();
    assert!(receipt.output.parse::<u64>().unwrap() >= 3);
    assert_eq!(pool.scale_async(-2).await.unwrap(), 1);
    assert!(matches!(pool.scale(1), Err(PoolError::AsyncScaleRequired)));
    assert_eq!(pool.scale_async(9).await.unwrap(), 2);
    assert_eq!(factory.calls.load(Ordering::SeqCst), 5);
}
#[tokio::test]
async fn failed_boot_wave_publishes_nothing_and_cancels_siblings() {
    let factory = Arc::new(Factory {
        boot_barrier: Some(Arc::new(tokio::sync::Barrier::new(3))),
        fail: true,
        ..Default::default()
    });
    let pool = WorkerPool::with_factory(config(3, 3), factory.clone()).unwrap();
    assert!(pool.initialize().await.is_err());
    assert_eq!(pool.live_workers(), 0);
    assert_eq!(factory.calls.load(Ordering::SeqCst), 3);
    assert!(
        factory
            .signals
            .lock()
            .unwrap()
            .iter()
            .all(CancellationSignal::is_cancelled)
    );
}
#[tokio::test]
async fn close_and_caller_drop_cancel_actual_boot_signals() {
    for close in [false, true] {
        let factory = Arc::new(Factory {
            hang: AtomicBool::new(true),
            ..Default::default()
        });
        let pool = WorkerPool::with_factory(config(3, 3), factory.clone()).unwrap();
        let mut boot = Box::pin(pool.initialize());
        assert!(
            boot.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        if close {
            pool.close();
            assert!(matches!(boot.await, Err(PoolError::Closed)));
        } else {
            drop(boot);
        }
        assert_eq!(pool.live_workers(), 0);
        assert_eq!(factory.calls.load(Ordering::SeqCst), 3);
        assert!(
            factory
                .signals
                .lock()
                .unwrap()
                .iter()
                .all(CancellationSignal::is_cancelled)
        );
        if !close {
            factory.hang.store(false, Ordering::SeqCst);
            pool.initialize().await.unwrap();
            assert_eq!(pool.live_workers(), 3);
        }
    }
}
#[tokio::test]
async fn aliased_factory_output_is_not_independent_capacity() {
    let factory = Arc::new(Factory {
        alias: Some(Arc::new(Instance {
            id: 0,
            barrier: None,
        })),
        ..Default::default()
    });
    let pool = WorkerPool::with_factory(config(2, 2), factory).unwrap();
    assert!(matches!(
        pool.initialize().await,
        Err(PoolError::Initialization(_))
    ));
    assert_eq!(pool.live_workers(), 0);
}

#[tokio::test]
async fn failed_but_still_leased_instances_cannot_be_replaced() {
    let factory = Arc::new(Factory::default());
    let pool = WorkerPool::with_factory(config(1, 1), factory.clone()).unwrap();
    pool.initialize().await.unwrap();
    let (lease, _) = pool.acquire(&[]).await.unwrap();
    pool.test_freeze_last_seen(Duration::from_secs(3));
    assert_eq!(pool.health_tick(tokio::time::Instant::now()).len(), 1);
    assert_eq!(pool.replace_failed().await.unwrap(), 0);
    assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
    drop(lease);
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn hanging_boot_has_its_own_deadline_and_leaves_no_slots() {
    let factory = Arc::new(Factory {
        hang: AtomicBool::new(true),
        ..Default::default()
    });
    let mut config = config(3, 3);
    config.default_turn_deadline = Duration::from_millis(5);
    let pool = WorkerPool::with_factory(config, factory.clone()).unwrap();
    assert!(
        matches!(pool.initialize().await, Err(PoolError::Initialization(message)) if message == "worker boot deadline exceeded")
    );
    assert_eq!(pool.live_workers(), 0);
    assert!(
        factory
            .signals
            .lock()
            .unwrap()
            .iter()
            .all(CancellationSignal::is_cancelled)
    );
}
