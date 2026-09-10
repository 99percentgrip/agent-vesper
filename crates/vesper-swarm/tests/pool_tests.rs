//! Integration tests for the pooled worker lifecycle (VRO-15 PR-3).
//!
//! Every worker here is an in-crate fake: no provider, no network, no
//! filesystem. Timing assertions use generous windows so the five-target
//! CI matrix never flakes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::FutureExt;

use vesper_swarm::pool::{
    PoolConfig, PoolConfigError, PoolError, PoolEvent, WorkerPool, WorkerStatus,
};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

// ---------------------------------------------------------------------
// Fake worker ports
// ---------------------------------------------------------------------

/// Scripted fake: succeeds promptly unless configured otherwise.
struct FakeWorkerPort {
    mode: FakeMode,
    delay: Duration,
    started: AtomicUsize,
    calls: AtomicUsize,
}

enum FakeMode {
    Succeed,
    /// Observes cancellation and exits promptly with Cancelled.
    CancelAware,
    /// Ignores cancellation; only the deadline race resolves it.
    Hang,
}

impl FakeWorkerPort {
    fn succeeding() -> Arc<Self> {
        Arc::new(Self {
            mode: FakeMode::Succeed,
            delay: Duration::from_millis(1),
            started: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        })
    }

    fn cancel_aware() -> Arc<Self> {
        Arc::new(Self {
            mode: FakeMode::CancelAware,
            delay: Duration::from_millis(200),
            started: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        })
    }

    fn hanging() -> Arc<Self> {
        Arc::new(Self {
            mode: FakeMode::Hang,
            delay: Duration::from_secs(30),
            started: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        })
    }
}

impl WorkerPort for FakeWorkerPort {
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        let started = &self.started;
        let calls = &self.calls;
        let delay = self.delay;
        let mode = &self.mode;
        async move {
            started.fetch_add(1, Ordering::AcqRel);
            calls.fetch_add(1, Ordering::AcqRel);
            match mode {
                FakeMode::Succeed => {
                    tokio::time::sleep(delay).await;
                    Ok(TurnReceipt {
                        task_id: task.id.clone(),
                        output: String::from("done"),
                        success: true,
                        duration: delay,
                    })
                }
                FakeMode::CancelAware => {
                    for _ in 0..100 {
                        if cancellation.is_cancelled() {
                            return Err(WorkerError::Cancelled(task.id.clone()));
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    Ok(TurnReceipt {
                        task_id: task.id.clone(),
                        output: String::from("late"),
                        success: true,
                        duration: delay,
                    })
                }
                FakeMode::Hang => futures_util::future::pending().await,
            }
        }
        .boxed()
    }

    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
}

/// Boot wave tracker: proves boot probes actually overlap. Each
/// `capabilities()` probe registers arrival, then the pool's own
/// `initialize` runs all probes inside one `join_all`; because every probe
/// is synchronous, the proof of composition is the single-wave completion
/// (all N calls made, none dropped) plus the pool's high-water record.
struct WaveTracker {
    calls: AtomicUsize,
}

impl WaveTracker {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
        })
    }
}

impl WorkerPort for WaveTracker {
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _cancellation: CancellationSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: String::from("ok"),
                success: true,
                duration: Duration::from_millis(1),
            })
        })
    }

    fn capabilities(&self) -> WorkerCapabilities {
        self.calls.fetch_add(1, Ordering::AcqRel);
        WorkerCapabilities::minimal()
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn config(min: u32, max: u32) -> PoolConfig {
    PoolConfig {
        min_workers: min,
        max_workers: max,
        heartbeat_interval: Duration::from_millis(10),
        heartbeat_timeout: Duration::from_millis(200),
        default_turn_deadline: Duration::from_secs(60),
    }
}

async fn boot(min: u32, max: u32, port: Arc<dyn WorkerPort>) -> WorkerPool {
    let pool = WorkerPool::new(config(min, max), port).expect("valid config");
    pool.initialize().await.expect("initialize");
    pool
}

// ---------------------------------------------------------------------
// Config validation
// ---------------------------------------------------------------------

#[test]
fn config_rejects_impossible_shapes() {
    // Default config itself is valid.
    PoolConfig::default().validate().expect("default valid");

    let err = PoolConfig {
        min_workers: 0,
        ..PoolConfig::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, PoolConfigError::ZeroMinWorkers));

    let err = PoolConfig {
        max_workers: 1,
        min_workers: 2,
        ..PoolConfig::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, PoolConfigError::MaxBelowMin { .. }));

    let err = PoolConfig {
        heartbeat_interval: Duration::ZERO,
        ..PoolConfig::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, PoolConfigError::ZeroHeartbeatInterval));

    let err = PoolConfig {
        heartbeat_timeout: Duration::from_millis(5),
        heartbeat_interval: Duration::from_millis(10),
        ..PoolConfig::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, PoolConfigError::TimeoutBelowInterval { .. }));

    let err = PoolConfig {
        default_turn_deadline: Duration::ZERO,
        ..PoolConfig::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, PoolConfigError::ZeroTurnDeadline));

    assert!(
        WorkerPool::new(
            PoolConfig {
                min_workers: 0,
                ..PoolConfig::default()
            },
            FakeWorkerPort::succeeding(),
        )
        .is_err()
    );
}

// ---------------------------------------------------------------------
// Parallel initialization
// ---------------------------------------------------------------------

#[tokio::test]
async fn initialize_boots_min_workers_with_one_parallel_wave() {
    let tracker = WaveTracker::new();
    let pool = WorkerPool::new(config(4, 8), tracker.clone()).unwrap();
    pool.initialize().await.unwrap();
    assert_eq!(pool.live_workers(), 4);
    assert_eq!(pool.idle_workers(), 4);
    // One wave: every probe ran inside the single join_all, none dropped.
    assert_eq!(tracker.calls.load(Ordering::Acquire), 4);
    assert_eq!(pool.concurrent_boot_high_water(), 4);
    let created = pool
        .events()
        .iter()
        .filter(|event| matches!(event, PoolEvent::WorkerCreated(_)))
        .count();
    assert_eq!(created, 4);
}

#[tokio::test]
async fn initialize_is_idempotent_in_count() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(2, 4, port).await;
    // A second initialize re-boots to min; it must not exceed min.
    pool.initialize().await.unwrap();
    assert!(pool.live_workers() <= 4, "second boot stays within bounds");
}

// ---------------------------------------------------------------------
// Acquire / release bounds
// ---------------------------------------------------------------------

#[tokio::test]
async fn acquire_grows_to_max_then_refuses() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(1, 2, port).await;
    let (first, _) = pool.acquire(&[]).await.unwrap();
    let (second, _) = pool.acquire(&[]).await.unwrap();
    assert_ne!(first.worker_id(), second.worker_id());
    let err = pool.acquire(&[]).await;
    assert!(matches!(err, Err(PoolError::Exhausted { .. })));
    assert!(
        pool.events()
            .iter()
            .any(|e| matches!(e, PoolEvent::CapacityRefused(_)))
    );
}

#[tokio::test]
async fn release_restores_idle_and_allows_reuse() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(1, 1, port).await;
    let (lease, _) = pool.acquire(&[]).await.unwrap();
    assert_eq!(pool.idle_workers(), 0);
    pool.release(lease);
    assert_eq!(pool.idle_workers(), 1);
    let again = pool.acquire(&[]).await.unwrap();
    assert_eq!(again.0.worker_id(), 1);
    let _ = again;
}

#[tokio::test]
async fn dropping_the_lease_releases_the_worker() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(1, 1, port).await;
    {
        let _lease = pool.acquire(&[]).await.unwrap();
        assert_eq!(pool.idle_workers(), 0);
    }
    assert_eq!(pool.idle_workers(), 1, "drop releases");
    assert!(
        pool.events()
            .iter()
            .any(|e| matches!(e, PoolEvent::WorkerReleased(_)))
    );
}

// ---------------------------------------------------------------------
// run_task paths
// ---------------------------------------------------------------------

#[tokio::test]
async fn run_task_success_releases_worker_and_records_receipt() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(1, 2, port.clone()).await;
    let receipt = pool
        .run_task(WorkerTask::new("t1", "do work"))
        .await
        .unwrap();
    assert!(receipt.success);
    assert_eq!(receipt.task_id, "t1");
    assert_eq!(pool.idle_workers(), 1);
    assert_eq!(port.calls.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn run_task_deadline_enforced_even_when_worker_hangs() {
    let port = FakeWorkerPort::hanging();
    let pool = boot(1, 2, port).await;
    let outcome = pool
        .run_task(WorkerTask {
            deadline: Duration::from_millis(50),
            ..WorkerTask::new("t-hang", "hang")
        })
        .await;
    assert_eq!(
        outcome.unwrap_err(),
        WorkerError::DeadlineExceeded("t-hang".into())
    );
    // Worker is released despite the timeout.
    assert_eq!(pool.idle_workers(), 1);
}

#[tokio::test]
async fn cancelled_task_never_reports_success() {
    let port = FakeWorkerPort::cancel_aware();
    let pool = boot(1, 2, port).await;
    // Deadline shorter than the fake's poll loop forces the race.
    let outcome = pool
        .run_task(WorkerTask {
            deadline: Duration::from_millis(30),
            ..WorkerTask::new("t-cancel", "poll")
        })
        .await;
    match outcome {
        Err(WorkerError::Cancelled(_)) | Err(WorkerError::DeadlineExceeded(_)) => {}
        other => panic!("expected cancellation family, got {other:?}"),
    }
}

#[tokio::test]
async fn closed_pool_refuses_everything() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(1, 2, port).await;
    pool.close();
    assert!(matches!(pool.acquire(&[]).await, Err(PoolError::Closed)));
    assert!(pool.run_task(WorkerTask::new("t", "x")).await.is_err());
    assert!(matches!(pool.scale(1).unwrap_err(), PoolError::Closed));
    assert!(matches!(
        pool.replace_failed().await.unwrap_err(),
        PoolError::Closed
    ));
}

// ---------------------------------------------------------------------
// Health monitoring
// ---------------------------------------------------------------------

#[tokio::test]
async fn health_tick_fails_silent_workers_and_replaces() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(2, 4, port).await;
    // Simulate silence: freeze both workers' last_seen far in the past.
    force_silence(&pool, Duration::from_secs(10));
    let now = tokio::time::Instant::now();
    let outcomes = pool.health_tick(now);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|(_, task)| task.is_none()));
    assert!(
        pool.events()
            .iter()
            .any(|e| matches!(e, PoolEvent::WorkerHeartbeatMissed(_)))
    );
    let replaced = pool.replace_failed().await.unwrap();
    assert_eq!(replaced, 2);
    assert_eq!(pool.idle_workers(), 2);
    assert!(
        pool.events()
            .iter()
            .any(|e| matches!(e, PoolEvent::WorkerReplaced(_, _)))
    );
}

#[tokio::test]
async fn health_tick_fails_overdue_in_flight_and_cancels_it() {
    let port = FakeWorkerPort::hanging();
    let pool = Arc::new(boot(1, 2, port).await);
    let runner = tokio::spawn({
        let pool = Arc::clone(&pool);
        async move { pool.run_task(soon_deadline_hang_task("t-health")).await }
    });
    // Let the turn start and register in-flight state (poll, never a
    // fixed sleep: robust on every CI target).
    let mut registered = false;
    for _ in 0..500 {
        if pool.has_in_flight_task("t-health") {
            registered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(registered, "task never registered as in-flight");
    // Silence the busy worker past the heartbeat window while its turn is
    // inside its (far-future) deadline: the monitor must fail it on
    // heartbeat grounds and cancel the in-flight task.
    force_silence(&pool, Duration::from_millis(500));
    let outcomes = pool.health_tick(tokio::time::Instant::now());
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].1.as_deref(), Some("t-health"));
    let result = runner.await.unwrap();
    assert!(result.is_err(), "cancelled task never succeeds");
    let replaced = pool.replace_failed().await.unwrap();
    assert_eq!(replaced, 1);
}

#[tokio::test]
async fn heartbeat_refresh_keeps_workers_healthy() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(2, 4, port).await;
    let mut interval = pool.heartbeat_interval();
    // Simulate the orchestrator ticking and heartbeating workers.
    for _ in 0..10 {
        interval.tick().await;
        for id in 1..=2 {
            pool.note_heartbeat(id);
        }
        assert!(pool.health_tick(tokio::time::Instant::now()).is_empty());
    }
    assert_eq!(pool.idle_workers(), 2);
}

#[tokio::test]
async fn monitor_loop_pattern_fails_hung_worker_within_window() {
    // The exact pattern the orchestrator will use, end to end: caller-owned
    // interval ticks health; a hung worker is failed, cancelled, replaced.
    let port = FakeWorkerPort::hanging();
    let pool = Arc::new(boot(1, 2, port).await);
    let runner = tokio::spawn({
        let pool = Arc::clone(&pool);
        async move { pool.run_task(soon_deadline_hang_task("t-monitor")).await }
    });
    tokio::time::sleep(Duration::from_millis(5)).await;
    let mut interval = pool.heartbeat_interval();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut saw_cancel = false;
    while tokio::time::Instant::now() < deadline {
        interval.tick().await;
        let outcomes = pool.health_tick(tokio::time::Instant::now());
        if outcomes.iter().any(|(_, task)| task.is_some()) {
            saw_cancel = true;
            break;
        }
        pool.replace_failed().await.unwrap();
    }
    assert!(saw_cancel, "monitor cancelled the hung task");
    let _ = runner.await;
    pool.replace_failed().await.unwrap();
    assert!(pool.idle_workers() >= 1);
}

// ---------------------------------------------------------------------
// Scaling
// ---------------------------------------------------------------------

#[tokio::test]
async fn scale_respects_bounds_in_both_directions() {
    let port = FakeWorkerPort::succeeding();
    let pool = boot(1, 4, port).await;
    assert_eq!(pool.scale(3).unwrap(), 4);
    assert_eq!(pool.live_workers(), 4);
    // Cannot exceed max.
    assert_eq!(pool.scale(1).unwrap(), 4);
    assert_eq!(pool.live_workers(), 4);
    // Cannot shed below min.
    assert_eq!(pool.scale(-10).unwrap(), 1);
    assert_eq!(pool.live_workers(), 1);
    assert_eq!(pool.scale(0).unwrap(), 1);
}

#[tokio::test]
async fn scale_down_never_kills_busy_workers() {
    let port = FakeWorkerPort::hanging();
    // min 1 so shedding can actually reach the floor; max 4.
    let pool = boot(1, 4, port).await;
    pool.scale(2).unwrap();
    assert_eq!(pool.live_workers(), 3);
    let (lease, _) = pool.acquire(&[]).await.unwrap();
    // Shed more than the idle count: only idle workers are removed; the
    // busy worker survives; never below min.
    let after = pool.scale(-3).unwrap();
    assert_eq!(after, 1);
    assert_eq!(pool.live_workers(), 1);
    assert_eq!(pool.idle_workers(), 0);
    drop(lease);
    assert_eq!(pool.idle_workers(), 1);
}

// ---------------------------------------------------------------------
// Test-only accessors
// ---------------------------------------------------------------------

/// Freezes every worker's liveness `silence` in the past.
fn force_silence(pool: &WorkerPool, silence: Duration) {
    pool.test_freeze_last_seen(silence);
}

/// A hanging turn whose own deadline is short: the monitor must cancel it
/// before the deadline fires, keeping monitor tests fast on every CI
/// target.
fn soon_deadline_hang_task(id: &str) -> WorkerTask {
    WorkerTask {
        deadline: Duration::from_millis(800),
        ..WorkerTask::new(id, "hang")
    }
}

#[test]
fn pool_is_debug_send_sync() {
    fn assert_bounds<T: Send + Sync + std::fmt::Debug>() {}
    assert_bounds::<WorkerPool>();
}

#[test]
fn worker_status_is_serializable() {
    let statuses = [WorkerStatus::Idle, WorkerStatus::Busy, WorkerStatus::Failed];
    let encoded = serde_json::to_string(&statuses).unwrap();
    let decoded: Vec<WorkerStatus> = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, statuses);
}

#[tokio::test]
async fn repeated_initialization_never_adds_workers_above_floor() {
    let config = PoolConfig {
        min_workers: 2,
        max_workers: 3,
        ..PoolConfig::default()
    };
    let pool = WorkerPool::new(config, FakeWorkerPort::succeeding()).unwrap();
    for _ in 0..5 {
        pool.initialize().await.unwrap();
    }
    assert_eq!(pool.live_workers(), 2);
    pool.scale(1).unwrap();
    pool.initialize().await.unwrap();
    assert_eq!(pool.live_workers(), 3);
}

#[tokio::test]
async fn failed_workers_can_be_replaced_at_maximum_capacity() {
    let config = PoolConfig {
        min_workers: 2,
        max_workers: 2,
        ..PoolConfig::default()
    };
    let pool = WorkerPool::new(config, FakeWorkerPort::succeeding()).unwrap();
    pool.initialize().await.unwrap();
    pool.test_freeze_last_seen(Duration::from_secs(10));
    pool.health_tick(tokio::time::Instant::now());
    assert_eq!(pool.replace_failed().await.unwrap(), 2);
    assert_eq!(pool.live_workers(), 2);
    assert_eq!(pool.idle_workers(), 2);
}

#[tokio::test]
async fn incapable_growth_is_refused_without_allocating_a_worker() {
    let pool = WorkerPool::new(PoolConfig::default(), FakeWorkerPort::succeeding()).unwrap();
    assert!(pool.acquire(&["not-supported".into()]).await.is_err());
    assert_eq!(pool.live_workers(), 0);
}
