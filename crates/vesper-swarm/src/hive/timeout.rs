//! Deadline/cancellation boundary for worker turns.
//!
//! The boundary creates the cancellation signal before constructing the turn.
//! Expiry and dropped callers cancel that same signal; a bounded grace period
//! permits cleanup, but no late successful receipt escapes the boundary.

use std::future::Future;
use std::time::Duration;

use crate::worker::{
    CancelFlag, CancellationSignal, TurnReceipt, WorkerError, WorkerPort, WorkerTask,
};

/// Outcome classification for a bounded turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedOutcome {
    /// The turn completed inside its budget.
    Completed,
    /// The turn exceeded its budget and was cancelled.
    TimedOut,
}

struct CancelOnDrop(CancelFlag);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Constructs and executes a worker turn with the boundary-owned signal.
///
/// `start` must pass its signal to the worker, not create a private flag.
/// Zero `budget` selects `default_deadline`. Deadline wins simultaneous
/// readiness; expiry signals cancellation and allows at most `grace` to unwind.
/// Dropping this future also signals cancellation. Detached work is not joined
/// by this helper and remains the execution port's cleanup responsibility.
pub async fn execute_bounded<S, F>(
    task: &WorkerTask,
    start: S,
    budget: Duration,
    grace: Duration,
    default_deadline: Duration,
) -> (Result<TurnReceipt, WorkerError>, BoundedOutcome)
where
    S: FnOnce(CancellationSignal) -> F,
    F: Future<Output = Result<TurnReceipt, WorkerError>>,
{
    let effective = if budget.is_zero() {
        default_deadline
    } else {
        budget
    };
    let guard = CancelOnDrop(CancelFlag::new());
    let deadline = tokio::time::sleep(effective);
    tokio::pin!(deadline);
    let turn = start(guard.0.signal());
    tokio::pin!(turn);
    tokio::select! {
        biased;
        _ = &mut deadline => {}
        result = &mut turn => return (result, BoundedOutcome::Completed),
    }
    guard.0.cancel();
    let outcome = tokio::select! {
        biased;
        _ = tokio::time::sleep(grace) => Err(WorkerError::DeadlineExceeded(task.id.clone())),
        result = &mut turn => match result {
            Ok(receipt) if receipt.success => Err(WorkerError::Cancelled(task.id.clone())),
            other => other,
        },
    };
    (outcome, BoundedOutcome::TimedOut)
}

/// Runs a real execution port using the exact signal cancelled by the boundary.
pub async fn execute_bounded_port(
    task: &WorkerTask,
    port: &dyn WorkerPort,
    budget: Duration,
    grace: Duration,
    default_deadline: Duration,
) -> (Result<TurnReceipt, WorkerError>, BoundedOutcome) {
    execute_bounded(
        task,
        |signal| port.run_turn(task, signal),
        budget,
        grace,
        default_deadline,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::WorkerCapabilities;
    use futures_util::{FutureExt, future::BoxFuture};

    #[tokio::test(start_paused = true)]
    async fn injected_runtime_clock_controls_deadline_and_grace_exactly() {
        let task = WorkerTask::new("clock", "work");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut turn = Box::pin(execute_bounded(
            &task,
            |signal| {
                tx.send(signal).unwrap();
                std::future::pending()
            },
            Duration::from_secs(10),
            Duration::from_secs(3),
            Duration::from_secs(1),
        ));
        std::future::poll_fn(|cx| {
            assert!(turn.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let signal = rx.try_recv().unwrap();
        tokio::time::advance(Duration::from_secs(9)).await;
        std::future::poll_fn(|cx| {
            assert!(turn.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(!signal.is_cancelled());
        tokio::time::advance(Duration::from_secs(1)).await;
        std::future::poll_fn(|cx| {
            assert!(turn.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(signal.is_cancelled());
        tokio::time::advance(Duration::from_secs(2)).await;
        std::future::poll_fn(|cx| {
            assert!(turn.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        tokio::time::advance(Duration::from_secs(1)).await;
        let (result, outcome) = turn.await;
        assert_eq!(result, Err(WorkerError::DeadlineExceeded(task.id.clone())));
        assert_eq!(outcome, BoundedOutcome::TimedOut);
    }

    fn receipt(task: &WorkerTask) -> TurnReceipt {
        TurnReceipt {
            task_id: task.id.clone(),
            output: "done".into(),
            success: true,
            duration: Duration::ZERO,
        }
    }

    #[tokio::test]
    async fn inside_budget_is_success() {
        let task = WorkerTask::new("fast", "work");
        let (result, outcome) = execute_bounded(
            &task,
            |_| async { Ok(receipt(&task)) },
            Duration::from_secs(1),
            Duration::ZERO,
            Duration::from_secs(1),
        )
        .await;
        assert!(result.unwrap().success);
        assert_eq!(outcome, BoundedOutcome::Completed);
    }

    struct Poller;
    impl WorkerPort for Poller {
        fn capabilities(&self) -> WorkerCapabilities {
            WorkerCapabilities::minimal()
        }
        fn run_turn<'a>(
            &'a self,
            task: &'a WorkerTask,
            signal: CancellationSignal,
        ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
            async move {
                while !signal.is_cancelled() {
                    tokio::task::yield_now().await;
                }
                Err(WorkerError::Cancelled(task.id.clone()))
            }
            .boxed()
        }
    }

    #[tokio::test]
    async fn expiry_reaches_actual_worker_signal() {
        let task = WorkerTask::new("poller", "work");
        let (result, outcome) = execute_bounded_port(
            &task,
            &Poller,
            Duration::from_millis(5),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(result, Err(WorkerError::Cancelled(task.id.clone())));
        assert_eq!(outcome, BoundedOutcome::TimedOut);
    }

    #[tokio::test]
    async fn late_success_is_rejected() {
        let task = WorkerTask::new("late", "work");
        let (result, outcome) = execute_bounded(
            &task,
            |signal| {
                let task = &task;
                async move {
                    while !signal.is_cancelled() {
                        tokio::task::yield_now().await;
                    }
                    Ok(receipt(task))
                }
            },
            Duration::from_millis(5),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(result, Err(WorkerError::Cancelled(task.id.clone())));
        assert_eq!(outcome, BoundedOutcome::TimedOut);
    }

    #[tokio::test]
    async fn ignoring_signal_is_abandoned_and_zero_budget_uses_default() {
        let task = WorkerTask::new("hang", "work");
        let (result, outcome) = execute_bounded(
            &task,
            |_| std::future::pending(),
            Duration::ZERO,
            Duration::from_millis(5),
            Duration::from_millis(5),
        )
        .await;
        assert_eq!(result, Err(WorkerError::DeadlineExceeded(task.id.clone())));
        assert_eq!(outcome, BoundedOutcome::TimedOut);
    }

    #[tokio::test]
    async fn dropping_caller_cancels_worker_signal() {
        let task = WorkerTask::new("drop", "work");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut turn = Box::pin(execute_bounded(
            &task,
            |signal| {
                tx.send(signal).unwrap();
                std::future::pending()
            },
            Duration::from_secs(1),
            Duration::ZERO,
            Duration::from_secs(1),
        ));
        std::future::poll_fn(|cx| {
            assert!(turn.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let signal = rx.try_recv().unwrap();
        assert!(!signal.is_cancelled());
        drop(turn);
        assert!(signal.is_cancelled());
    }
}
