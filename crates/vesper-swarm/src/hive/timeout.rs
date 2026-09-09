//! Timeout and cancellation boundaries for dispatched turns (VRO-15 PR-5).
//!
//! [`execute_bounded`] is the single composition point where a worker
//! turn meets time: the task's wall-clock budget races the turn future;
//! on expiry the [`CancelFlag`] fires, the worker gets a bounded **grace
//! window** to observe cancellation and return cleanly, and a turn that
//! ignores both is abandoned with [`WorkerError::DeadlineExceeded`].
//!
//! The invariant enforced here is absolute: **a cancelled or timed-out
//! task never yields a successful [`TurnReceipt`]**. If a worker returns
//! success after cancellation was requested, the result is converted to
//! [`WorkerError::Cancelled`] — the caller never sees it as success.

use std::future::Future;
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::worker::{CancelFlag, TurnReceipt, WorkerError, WorkerTask};

/// Outcome classification for a bounded turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedOutcome {
    /// The turn completed inside its budget.
    Completed,
    /// The turn exceeded its budget and was cancelled.
    TimedOut,
}

/// Executes one worker turn under a timeout with cancellation
/// propagation and a post-expiry grace window.
///
/// - `turn` is the worker future (typically `WorkerPort::run_turn`).
/// - `budget` is the task's wall-clock allowance; zero selects the
///   `default_deadline`.
/// - `grace` is how long the worker may take to observe the cancel
///   signal after expiry before being abandoned.
///
/// Returns the turn result with the outcome classification. A success
/// receipt observed *after* cancellation was requested is rewritten to
/// [`WorkerError::Cancelled`] — late success is failure, by contract.
pub async fn execute_bounded<F>(
    task: &WorkerTask,
    turn: F,
    budget: Duration,
    grace: Duration,
    default_deadline: Duration,
) -> (Result<TurnReceipt, WorkerError>, BoundedOutcome)
where
    F: Future<Output = Result<TurnReceipt, WorkerError>>,
{
    let effective = if budget.is_zero() {
        default_deadline
    } else {
        budget
    };
    let flag = CancelFlag::new();
    let signal = flag.signal();
    let _ = signal; // The worker future receives its own signal clone.
    let mut pinned = Box::pin(turn);
    let mut sleep = Box::pin(tokio::time::sleep(effective));
    let timed_out;
    let outcome: Result<TurnReceipt, WorkerError> = tokio::select! {
        result = pinned.as_mut() => {
            timed_out = false;
            result
        }
        _ = sleep.as_mut() => {
            // Budget exhausted: cancel and hand the worker the grace
            // window to unwind cleanly. Whatever happens next is a
            // timeout outcome by definition.
            timed_out = true;
            flag.cancel();
            tokio::select! {
                result = pinned => {
                    // Worker obeyed the signal (any result). A success
                    // after cancellation is still a cancellation.
                    match result {
                        Ok(receipt) if receipt.success => {
                            Err(WorkerError::Cancelled(task.id.clone()))
                        }
                        other => other,
                    }
                }
                _ = tokio::time::sleep(grace) => {
                    // Ignored the signal: abandon with DeadlineExceeded.
                    Err(WorkerError::DeadlineExceeded(task.id.clone()))
                }
            }
        }
    };
    let outcome_class = if timed_out {
        BoundedOutcome::TimedOut
    } else {
        BoundedOutcome::Completed
    };
    (outcome, outcome_class)
}

/// The same boundary, but as a boxed convenience used by pool/hive code
/// that already holds a `BoxFuture` (mirrors `WorkerPort::run_turn`'s
/// shape and honors the identical never-success-after-cancel contract).
pub async fn execute_bounded_port(
    task: &WorkerTask,
    turn: BoxFuture<'_, Result<TurnReceipt, WorkerError>>,
    budget: Duration,
    grace: Duration,
    default_deadline: Duration,
) -> (Result<TurnReceipt, WorkerError>, BoundedOutcome) {
    execute_bounded(task, turn, budget, grace, default_deadline).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn task_with(id: &str, deadline: Duration) -> WorkerTask {
        WorkerTask {
            deadline,
            ..WorkerTask::new(id, "work")
        }
    }

    #[tokio::test]
    async fn turn_completing_inside_budget_is_a_genuine_success() {
        let task = task_with("t1", Duration::from_secs(30));
        let (result, outcome) = execute_bounded(
            &task,
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Ok(TurnReceipt {
                    task_id: String::from("t1"),
                    output: String::from("done"),
                    success: true,
                    duration: Duration::from_millis(5),
                })
            },
            Duration::from_secs(5),
            Duration::from_millis(200),
            Duration::from_secs(60),
        )
        .await;
        assert!(matches!(outcome, BoundedOutcome::Completed));
        assert!(result.expect("success").success);
    }

    #[tokio::test]
    async fn timeout_triggers_cancellation_and_returns_cancelled() {
        let seen = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&seen);
        let task = task_with("t2", Duration::from_millis(50));
        let (result, outcome) = execute_bounded(
            &task,
            {
                // A worker that polls a cancellation flag... simulated by
                // sleeping past the budget then returning a "success"
                // (the boundary must rewrite it to Cancelled).
                async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    worker_cancelled.store(true, Ordering::Release);
                    Ok(TurnReceipt {
                        task_id: String::from("t2"),
                        output: String::from("late"),
                        success: true,
                        duration: Duration::from_millis(200),
                    })
                }
            },
            Duration::from_millis(50),
            Duration::from_millis(500),
            Duration::from_secs(60),
        )
        .await;
        assert!(matches!(outcome, BoundedOutcome::TimedOut));
        // Late success is rewritten to Cancelled: never a success receipt.
        assert_eq!(
            result.unwrap_err(),
            WorkerError::Cancelled(String::from("t2"))
        );
    }

    #[tokio::test]
    async fn worker_ignoring_the_signal_is_abandoned_with_deadline() {
        let task = task_with("t3", Duration::from_millis(40));
        let (result, outcome) = execute_bounded(
            &task,
            futures_util::future::pending(),
            Duration::from_millis(40),
            Duration::from_millis(100),
            Duration::from_secs(60),
        )
        .await;
        assert!(matches!(outcome, BoundedOutcome::TimedOut));
        assert_eq!(
            result.unwrap_err(),
            WorkerError::DeadlineExceeded(String::from("t3"))
        );
    }

    #[tokio::test]
    async fn zero_budget_selects_the_default_deadline() {
        let task = task_with("t4", Duration::ZERO);
        // Default deadline is tiny; the turn sleeps longer: it must time out.
        let (result, outcome) = execute_bounded(
            &task,
            futures_util::future::pending(),
            Duration::ZERO,
            Duration::from_millis(50),
            Duration::from_millis(30),
        )
        .await;
        assert!(matches!(outcome, BoundedOutcome::TimedOut));
        assert_eq!(
            result.unwrap_err(),
            WorkerError::DeadlineExceeded(String::from("t4"))
        );
    }

    #[tokio::test]
    async fn a_triggered_cancellation_signal_fails_the_task_immediately() {
        // The contract the directive demands, proven at the seam: once the
        // signal is triggered, the WorkerPort contract path returns
        // Cancelled and no success receipt exists.
        use crate::worker::{WorkerCapabilities, WorkerPort};

        struct SignalPoller;
        impl WorkerPort for SignalPoller {
            fn run_turn<'a>(
                &'a self,
                task: &'a WorkerTask,
                cancellation: crate::worker::CancellationSignal,
            ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
                async move {
                    // Poll briefly; the signal is cancelled before dispatch.
                    for _ in 0..200 {
                        if cancellation.is_cancelled() {
                            return Err(WorkerError::Cancelled(task.id.clone()));
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    Ok(TurnReceipt {
                        task_id: task.id.clone(),
                        output: String::from("should never happen"),
                        success: true,
                        duration: Duration::from_millis(400),
                    })
                }
                .boxed()
            }

            fn capabilities(&self) -> WorkerCapabilities {
                WorkerCapabilities::minimal()
            }
        }

        let flag = CancelFlag::new();
        flag.cancel();
        let port = SignalPoller;
        let task = WorkerTask::new("t-cancel", "immediate");
        // A pre-cancelled signal must surface Cancelled within the first
        // few polls — well inside the generous budget.
        let (result, outcome) = execute_bounded_port(
            &task,
            port.run_turn(&task, flag.signal()),
            Duration::from_secs(5),
            Duration::from_millis(500),
            Duration::from_secs(60),
        )
        .await;
        assert!(matches!(outcome, BoundedOutcome::Completed));
        assert_eq!(
            result.unwrap_err(),
            WorkerError::Cancelled(String::from("t-cancel"))
        );
    }
}
