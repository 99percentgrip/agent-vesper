//! Shrink cannot make externally retained instances invisible to capacity checks.
use futures_util::future::BoxFuture;
use std::sync::{Arc, Mutex};
use vesper_swarm::pool::{PoolConfig, WorkerInstanceFactory, WorkerPool};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};
struct Worker;
impl WorkerPort for Worker {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move { Err(WorkerError::Rejected(task.id.clone())) })
    }
}
#[derive(Default)]
struct Factory(Mutex<Vec<Arc<dyn WorkerPort>>>);
impl WorkerInstanceFactory for Factory {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        _: u64,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move {
            let port: Arc<dyn WorkerPort> = Arc::new(Worker);
            self.0.lock().unwrap().push(port.clone());
            Ok(port)
        })
    }
}
#[tokio::test]
async fn shrink_preflights_all_selected_owners_without_partial_removal() {
    for weak_only in [false, true] {
        let factory = Arc::new(Factory::default());
        let pool = WorkerPool::with_factory(
            PoolConfig {
                min_workers: 1,
                max_workers: 3,
                ..Default::default()
            },
            factory.clone(),
        )
        .unwrap();
        pool.initialize().await.unwrap();
        pool.scale_async(2).await.unwrap();
        let mut external = factory.0.lock().unwrap().drain(..).collect::<Vec<_>>();
        // First selected worker is exclusive; second is externally retained.
        // A sequential remove-then-check implementation must not remove the first.
        let second = external.remove(1);
        drop(external);
        let weak = Arc::downgrade(&second);
        let strong = if weak_only {
            drop(second);
            None
        } else {
            Some(second)
        };
        let before = pool.events();
        assert!(pool.scale_async(-2).await.is_err());
        assert_eq!(pool.live_workers(), 3);
        assert_eq!(pool.idle_workers(), 3);
        assert_eq!(pool.events(), before);
        drop((strong, weak));
        assert_eq!(pool.scale_async(-2).await.unwrap(), 1);
        assert_eq!(pool.scale_async(2).await.unwrap(), 3);
    }
}
