//! Actual worker signals survive caller-drop, close and health cancellation.
use futures_util::{FutureExt, future::BoxFuture};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;
use vesper_swarm::pool::{PoolConfig, WorkerPool};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

#[derive(Default)]
struct HeldWorker {
    signal: Mutex<Option<CancellationSignal>>,
    finish: tokio::sync::Notify,
}
impl WorkerPort for HeldWorker {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        signal: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        async move {
            *self.signal.lock().unwrap() = Some(signal);
            self.finish.notified().await;
            // Deliberately ignore cancellation: the boundary must refuse success.
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: "late".into(),
                success: true,
                duration: Duration::ZERO,
            })
        }
        .boxed()
    }
}

#[tokio::test]
async fn close_and_health_cancel_actual_signal_and_refuse_late_success() {
    for close in [false, true] {
        let port = Arc::new(HeldWorker::default());
        let pool = WorkerPool::new(PoolConfig::default(), port.clone()).unwrap();
        let mut task = Box::pin(pool.run_task(WorkerTask::new("task", "work")));
        assert!(
            task.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        if close {
            pool.close();
        } else {
            pool.health_tick(tokio::time::Instant::now() + pool.config().heartbeat_timeout);
        }
        assert!(port.signal.lock().unwrap().as_ref().unwrap().is_cancelled());
        port.finish.notify_one();
        assert!(matches!(task.await, Err(WorkerError::Cancelled(_))));
    }
}

#[tokio::test]
async fn dropping_caller_signals_worker_before_releasing_lease() {
    let port = Arc::new(HeldWorker::default());
    let pool = WorkerPool::new(PoolConfig::default(), port.clone()).unwrap();
    let mut task = Box::pin(pool.run_task(WorkerTask::new("task", "work")));
    assert!(
        task.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(task);
    assert!(port.signal.lock().unwrap().as_ref().unwrap().is_cancelled());
    assert_eq!(pool.idle_workers(), 1);
}

#[tokio::test]
async fn extreme_capacity_and_turn_budget_refuse_before_allocation_or_dispatch() {
    let port = Arc::new(HeldWorker::default());
    assert!(
        WorkerPool::new(
            PoolConfig {
                max_workers: u32::MAX,
                ..PoolConfig::default()
            },
            port.clone()
        )
        .is_err()
    );
    assert!(
        WorkerPool::new(
            PoolConfig {
                heartbeat_interval: Duration::MAX,
                heartbeat_timeout: Duration::MAX,
                ..PoolConfig::default()
            },
            port.clone()
        )
        .is_err()
    );
    let pool = WorkerPool::new(PoolConfig::default(), port.clone()).unwrap();
    let mut task = WorkerTask::new("overflow", "work");
    task.deadline = Duration::MAX;
    assert!(matches!(
        pool.run_task(task).await,
        Err(WorkerError::Rejected(_))
    ));
    assert_eq!(pool.live_workers(), 0);
    assert!(port.signal.lock().unwrap().is_none());
}
