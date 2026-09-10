//! Selection and execution must use the same live, pool-owned lease.
use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use vesper_swarm::pool::{PoolConfig, WorkerInstanceFactory, WorkerPool};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};
struct Factory(Arc<AtomicUsize>);
struct Instance {
    id: u64,
    calls: Arc<AtomicUsize>,
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: self.id.to_string(),
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
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
            Ok(Arc::new(Instance {
                id,
                calls: self.0.clone(),
            }) as Arc<dyn WorkerPort>)
        })
    }
}
async fn pool(calls: Arc<AtomicUsize>) -> WorkerPool {
    let pool = WorkerPool::with_factory(
        PoolConfig {
            min_workers: 2,
            max_workers: 2,
            ..Default::default()
        },
        Arc::new(Factory(calls)),
    )
    .unwrap();
    pool.initialize().await.unwrap();
    pool
}
#[tokio::test]
async fn selected_second_worker_is_not_reacquired_as_first() {
    let calls = Arc::new(AtomicUsize::new(0));
    let pool = pool(calls.clone()).await;
    let (first, _) = pool.acquire(&[]).await.unwrap();
    let (second, _) = pool.acquire(&[]).await.unwrap();
    let selected = second.worker_id();
    drop(first);
    let receipt = pool
        .run_leased_task(second, WorkerTask::new("task", "work"))
        .await
        .unwrap();
    assert_eq!(receipt.output, selected.to_string());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(pool.idle_workers(), 2);
}
#[tokio::test]
async fn foreign_and_failed_leases_never_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let first = pool(calls.clone()).await;
    let second = pool(calls.clone()).await;
    let (foreign, _) = first.acquire(&[]).await.unwrap();
    assert!(matches!(
        second
            .run_leased_task(foreign, WorkerTask::new("foreign", "work"))
            .await,
        Err(WorkerError::Rejected(_))
    ));
    assert_eq!(first.idle_workers(), 2);
    let (failed, _) = second.acquire(&[]).await.unwrap();
    second.test_freeze_last_seen(Duration::from_secs(3));
    second.health_tick(tokio::time::Instant::now());
    assert!(matches!(
        second
            .run_leased_task(failed, WorkerTask::new("failed", "work"))
            .await,
        Err(WorkerError::Rejected(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(second.replace_failed().await.unwrap(), 2);
}
#[tokio::test]
async fn incapable_or_oversized_task_releases_selected_lease_without_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let pool = pool(calls.clone()).await;
    for oversized in [false, true] {
        let (lease, _) = pool.acquire(&[]).await.unwrap();
        let mut task = WorkerTask::new("rejected", "work");
        if oversized {
            task.deadline = Duration::MAX;
        } else {
            task.required_capabilities = vec!["not-supported".into()];
        }
        assert!(matches!(
            pool.run_leased_task(lease, task).await,
            Err(WorkerError::Rejected(_))
        ));
        assert_eq!(pool.idle_workers(), 2);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
