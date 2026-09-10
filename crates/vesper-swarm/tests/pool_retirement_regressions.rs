//! Physical instance retirement must precede replacement boot.
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vesper_swarm::pool::{PoolConfig, WorkerInstanceFactory, WorkerPool};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

#[derive(Default)]
struct Resources {
    live: AtomicUsize,
    peak: AtomicUsize,
    boots: AtomicUsize,
    fail: AtomicBool,
    pending: AtomicBool,
    drop_gate: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            std::sync::mpsc::Receiver<()>,
        )>,
    >,
}
struct Instance(Arc<Resources>);
impl Drop for Instance {
    fn drop(&mut self) {
        if let Some((entered, release)) = self.0.drop_gate.lock().unwrap().take() {
            let _ = entered.send(());
            release
                .recv_timeout(Duration::from_secs(5))
                .expect("retirement must not block the async executor");
        }
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}
impl WorkerPort for Instance {
    fn pending_work(&self) -> bool {
        self.0.pending.load(Ordering::Acquire)
    }
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: "ok".into(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
struct Factory {
    resources: Arc<Resources>,
    retain: bool,
    external: Mutex<Option<Arc<dyn WorkerPort>>>,
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
            self.resources.boots.fetch_add(1, Ordering::SeqCst);
            if self.resources.fail.load(Ordering::SeqCst) {
                return Err(WorkerError::Failed(id.to_string(), "boot refusal".into()));
            }
            let live = self.resources.live.fetch_add(1, Ordering::SeqCst) + 1;
            self.resources.peak.fetch_max(live, Ordering::SeqCst);
            let port: Arc<dyn WorkerPort> = Arc::new(Instance(self.resources.clone()));
            if self.retain {
                *self.external.lock().unwrap() = Some(port.clone());
            }
            Ok(port)
        })
    }
}
fn pool(retain: bool) -> (WorkerPool, Arc<Factory>) {
    let factory = Arc::new(Factory {
        resources: Arc::default(),
        retain,
        external: Mutex::new(None),
    });
    let pool = WorkerPool::with_factory(
        PoolConfig {
            min_workers: 1,
            max_workers: 1,
            ..Default::default()
        },
        factory.clone(),
    )
    .unwrap();
    (pool, factory)
}
fn fail(pool: &WorkerPool) {
    pool.test_freeze_last_seen(Duration::from_secs(10));
    assert_eq!(pool.health_tick(tokio::time::Instant::now()).len(), 1);
}

#[tokio::test]
async fn abandoned_blocking_retirement_keeps_admission_reserved_until_completion() {
    let (pool, factory) = pool(false);
    let pool = Arc::new(pool);
    pool.initialize().await.unwrap();
    fail(&pool);
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    *factory.resources.drop_gate.lock().unwrap() = Some((entered, gate));
    let owned = pool.clone();
    let replacement = tokio::spawn(async move { owned.replace_failed().await });
    tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .unwrap()
        .unwrap();
    replacement.abort();
    assert!(replacement.await.unwrap_err().is_cancelled());
    assert!(pool.replace_failed().await.is_err());
    assert!(pool.scale_async(1).await.is_err());
    assert_eq!(factory.resources.boots.load(Ordering::Acquire), 1);
    assert!(!pool.settle_retirements(Duration::ZERO).await);
    release.send(()).unwrap();
    assert!(pool.settle_retirements(Duration::from_secs(2)).await);
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.resources.peak.load(Ordering::Acquire), 1);
    pool.close();
    assert!(pool.settle_retirements(Duration::from_secs(2)).await);
}
#[tokio::test]
async fn replacement_never_overlaps_failed_physical_instance() {
    let (pool, factory) = pool(false);
    pool.initialize().await.unwrap();
    fail(&pool);
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.resources.live.load(Ordering::SeqCst), 1);
    assert_eq!(factory.resources.peak.load(Ordering::SeqCst), 1);
    pool.close();
    assert!(pool.settle_retirements(Duration::from_secs(1)).await);
    assert_eq!(factory.resources.live.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn failed_replacement_keeps_slot_quarantined_but_not_retired_resource() {
    let (pool, factory) = pool(false);
    pool.initialize().await.unwrap();
    fail(&pool);
    factory.resources.fail.store(true, Ordering::SeqCst);
    assert!(pool.replace_failed().await.is_err());
    assert_eq!(factory.resources.live.load(Ordering::SeqCst), 0);
    assert_eq!(pool.idle_workers(), 0);
    assert_eq!(pool.live_workers(), 1);
    factory.resources.fail.store(false, Ordering::SeqCst);
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.resources.peak.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn externally_retained_instance_refuses_replacement_before_boot() {
    let (pool, factory) = pool(true);
    pool.initialize().await.unwrap();
    fail(&pool);
    assert!(pool.replace_failed().await.is_err());
    assert_eq!(factory.resources.boots.load(Ordering::SeqCst), 1);
    assert_eq!(factory.resources.live.load(Ordering::SeqCst), 1);
    factory.external.lock().unwrap().take();
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.resources.peak.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn upgradeable_weak_owner_also_blocks_retirement() {
    let (pool, factory) = pool(true);
    pool.initialize().await.unwrap();
    let external = factory.external.lock().unwrap().take().unwrap();
    let weak = Arc::downgrade(&external);
    drop(external);
    fail(&pool);
    assert!(pool.replace_failed().await.is_err());
    assert_eq!(factory.resources.boots.load(Ordering::SeqCst), 1);
    drop(weak);
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.resources.peak.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_native_work_must_settle_before_physical_replacement() {
    let (pool, factory) = pool(false);
    pool.initialize().await.unwrap();
    fail(&pool);
    factory.resources.pending.store(true, Ordering::Release);
    assert!(pool.replace_failed().await.is_err());
    assert_eq!(factory.resources.boots.load(Ordering::Acquire), 1);
    assert_eq!(factory.resources.live.load(Ordering::Acquire), 1);
    factory.resources.pending.store(false, Ordering::Release);
    assert_eq!(pool.replace_failed().await.unwrap(), 1);
    assert_eq!(factory.resources.peak.load(Ordering::Acquire), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_last_pool_owner_retires_blocking_instances_off_executor() {
    let (pool, factory) = pool(false);
    pool.initialize().await.unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    *factory.resources.drop_gate.lock().unwrap() = Some((entered_tx, release_rx));
    drop(pool);
    entered_rx.await.unwrap();
    assert_eq!(factory.resources.live.load(Ordering::SeqCst), 1);
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while factory.resources.live.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
