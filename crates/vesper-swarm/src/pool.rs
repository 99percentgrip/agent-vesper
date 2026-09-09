//! Pooled worker lifecycle (VRO-15 PR-3).
//!
//! [`WorkerPool`] owns a bounded set of logical workers behind a
//! [`WorkerPort`]: parallel initialization of the minimum count, lease
//! guarded acquire/release, dynamic scaling within bounds, and a health
//! monitor that fails silent workers, cancels their in-flight task, and
//! replaces them.
//!
//! The pool owns no clock and no randomness. Time enters only through
//! deadlines the caller supplies and the tokio interval the *caller* owns
//! for monitoring (see [`WorkerPool::heartbeat_interval`]); the pool never
//! spawns threads or timers on its own.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::worker::{
    CancelFlag, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

/// Pool bounds and timings, validated by [`PoolConfig::validate`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PoolConfig {
    /// Minimum live workers the pool maintains.
    pub min_workers: u32,
    /// Maximum workers the pool may ever hold.
    pub max_workers: u32,
    /// Heartbeat cadence for the caller-owned monitor interval.
    pub heartbeat_interval: Duration,
    /// Silence tolerated before a worker is considered failed.
    pub heartbeat_timeout: Duration,
    /// Hard per-task turn budget when a task declares none.
    pub default_turn_deadline: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_workers: 2,
            max_workers: 8,
            heartbeat_interval: Duration::from_millis(250),
            heartbeat_timeout: Duration::from_millis(1000),
            default_turn_deadline: Duration::from_secs(120),
        }
    }
}

/// Rejections of a [`PoolConfig`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PoolConfigError {
    /// `min_workers` must be at least one.
    #[error("min_workers must be at least one")]
    ZeroMinWorkers,
    /// `max_workers` must not be below `min_workers`.
    #[error("max_workers ({max_workers}) must be >= min_workers ({min_workers})")]
    MaxBelowMin {
        /// The rejected maximum.
        max_workers: u32,
        /// The enforced minimum.
        min_workers: u32,
    },
    /// The heartbeat interval must be positive.
    #[error("heartbeat_interval must be positive")]
    ZeroHeartbeatInterval,
    /// The timeout must tolerate at least one interval.
    #[error(
        "heartbeat_timeout ({heartbeat_timeout:?}) must be >= heartbeat_interval ({heartbeat_interval:?})"
    )]
    TimeoutBelowInterval {
        /// The configured silence tolerance.
        heartbeat_timeout: Duration,
        /// The configured monitor cadence.
        heartbeat_interval: Duration,
    },
    /// The default turn deadline must be positive.
    #[error("default_turn_deadline must be positive")]
    ZeroTurnDeadline,
}

impl PoolConfig {
    /// Fails closed on impossible shapes.
    pub fn validate(&self) -> Result<(), PoolConfigError> {
        if self.min_workers == 0 {
            return Err(PoolConfigError::ZeroMinWorkers);
        }
        if self.max_workers < self.min_workers {
            return Err(PoolConfigError::MaxBelowMin {
                max_workers: self.max_workers,
                min_workers: self.min_workers,
            });
        }
        if self.heartbeat_interval.is_zero() {
            return Err(PoolConfigError::ZeroHeartbeatInterval);
        }
        if self.heartbeat_timeout < self.heartbeat_interval {
            return Err(PoolConfigError::TimeoutBelowInterval {
                heartbeat_timeout: self.heartbeat_timeout,
                heartbeat_interval: self.heartbeat_interval,
            });
        }
        if self.default_turn_deadline.is_zero() {
            return Err(PoolConfigError::ZeroTurnDeadline);
        }
        Ok(())
    }
}

/// Lifecycle state of one pooled worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerStatus {
    /// Booted and waiting for work.
    Idle,
    /// Leased to a caller running a turn.
    Busy,
    /// Failed; awaiting replacement.
    Failed,
}

/// One pooled worker record.
#[derive(Debug)]
struct PooledWorker {
    id: u64,
    status: WorkerStatus,
    capabilities: WorkerCapabilities,
    /// Liveness instant, refreshed by activity and heartbeats.
    last_seen: tokio::time::Instant,
    in_flight: Option<InFlight>,
}

impl PooledWorker {
    fn fresh(
        id: u64,
        capabilities: WorkerCapabilities,
        status: WorkerStatus,
        now: tokio::time::Instant,
    ) -> Self {
        Self {
            id,
            status,
            capabilities,
            last_seen: now,
            in_flight: None,
        }
    }
}

#[derive(Debug)]
struct InFlight {
    task_id: String,
    deadline: tokio::time::Instant,
    cancel: CancelFlag,
}

/// Pool-originated lifecycle events (bounded ring, newest last).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolEvent {
    /// A worker was created.
    WorkerCreated(u64),
    /// A worker was acquired for a task.
    WorkerAcquired(u64, String),
    /// A worker was released.
    WorkerReleased(u64),
    /// A worker missed its heartbeat window.
    WorkerHeartbeatMissed(u64),
    /// A failed worker's in-flight task was cancelled.
    InFlightCancelled(u64, String),
    /// A failed worker was replaced by a fresh one.
    WorkerReplaced(u64, u64),
    /// The pool refused to exceed its bounds.
    CapacityRefused(u32),
}

/// Errors from pool operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PoolError {
    /// The config is invalid.
    #[error("invalid pool config: {0}")]
    InvalidConfig(PoolConfigError),
    /// The pool is closed.
    #[error("pool is closed")]
    Closed,
    /// At `max_workers` with no idle worker available.
    #[error("pool exhausted: {idle} idle of {total} workers, max {max}")]
    Exhausted {
        /// Idle workers at refusal time.
        idle: u32,
        /// Total live workers at refusal time.
        total: u32,
        /// The configured maximum.
        max: u32,
    },
}

/// Interior state behind one lock.
#[derive(Debug, Default)]
struct Inner {
    workers: Vec<PooledWorker>,
    events: VecDeque<PoolEvent>,
    next_id: u64,
    concurrent_boot_high_water: usize,
}

impl Inner {
    fn push_event(&mut self, event: PoolEvent) {
        if self.events.len() >= 512 {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn total(&self) -> usize {
        self.workers.len()
    }

    fn idle(&self) -> usize {
        self.workers
            .iter()
            .filter(|w| w.status == WorkerStatus::Idle)
            .count()
    }
}

/// Shared pool state.
#[derive(Debug, Default)]
struct Shared {
    inner: Mutex<Inner>,
    closed: AtomicBool,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("pool lock poisoned")
    }
}

/// A lease on one acquired worker; dropping it releases the worker.
pub struct WorkerLease {
    worker_id: u64,
    shared: Arc<Shared>,
    released: AtomicBool,
}

impl WorkerLease {
    /// The leased worker's identity.
    #[must_use]
    pub fn worker_id(&self) -> u64 {
        self.worker_id
    }

    fn release_inner(&self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        if let Some(worker) = inner.workers.iter_mut().find(|w| w.id == self.worker_id) {
            // Never resurrect a failed worker: the health monitor's verdict
            // outranks a late release.
            if worker.status == WorkerStatus::Busy {
                worker.status = WorkerStatus::Idle;
            }
            worker.in_flight = None;
            worker.last_seen = now;
        }
        inner.push_event(PoolEvent::WorkerReleased(self.worker_id));
    }
}

impl Drop for WorkerLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// Bounded pool of workers over one [`WorkerPort`].
///
/// A plain value with interior synchronization: share as
/// `Arc<WorkerPool>`. It performs no I/O and spawns nothing.
pub struct WorkerPool {
    config: PoolConfig,
    port: Arc<dyn WorkerPort>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.shared.lock();
        f.debug_struct("WorkerPool")
            .field("config", &self.config)
            .field("closed", &self.shared.closed.load(Ordering::Acquire))
            .field("workers", &(inner.total(), inner.idle()))
            .finish()
    }
}

impl WorkerPool {
    /// Creates an empty pool; boot it with [`initialize`](Self::initialize).
    pub fn new(config: PoolConfig, port: Arc<dyn WorkerPort>) -> Result<Self, PoolError> {
        config.validate().map_err(PoolError::InvalidConfig)?;
        Ok(Self {
            config,
            port,
            shared: Arc::new(Shared::default()),
        })
    }

    /// The enforced configuration.
    #[must_use]
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    fn check_open(&self) -> Result<(), PoolError> {
        if self.shared.closed.load(Ordering::Acquire) {
            Err(PoolError::Closed)
        } else {
            Ok(())
        }
    }

    /// Marks the pool closed; all subsequent operations refuse.
    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }

    /// Live worker count.
    #[must_use]
    pub fn live_workers(&self) -> usize {
        self.shared.lock().total()
    }

    /// Idle worker count.
    #[must_use]
    pub fn idle_workers(&self) -> usize {
        self.shared.lock().idle()
    }

    /// Snapshot of the bounded event log (oldest first).
    #[must_use]
    pub fn events(&self) -> Vec<PoolEvent> {
        self.shared.lock().events.iter().cloned().collect()
    }

    /// Largest simultaneous boot wave seen (diagnostic for the
    /// parallel-initialization guarantee).
    #[must_use]
    pub fn concurrent_boot_high_water(&self) -> usize {
        self.shared.lock().concurrent_boot_high_water
    }

    /// Boots the pool to `min_workers`, composing all boot probes
    /// concurrently with `join_all` so minimum capacity comes up as one
    /// parallel wave rather than serially.
    pub async fn initialize(&self) -> Result<(), PoolError> {
        self.check_open()?;
        let target = self.config.min_workers as usize;
        let boots = std::iter::repeat_with(|| {
            let port = Arc::clone(&self.port);
            async move { port.capabilities() }
        })
        .take(target);
        let resolved = futures_util::future::join_all(boots).await;
        debug_assert_eq!(resolved.len(), target);
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        inner.concurrent_boot_high_water = inner.concurrent_boot_high_water.max(target);
        for capability in resolved {
            inner.next_id += 1;
            let id = inner.next_id;
            inner
                .workers
                .push(PooledWorker::fresh(id, capability, WorkerStatus::Idle, now));
            inner.push_event(PoolEvent::WorkerCreated(id));
        }
        Ok(())
    }

    /// Acquires an idle capable worker, creating one when below
    /// `max_workers`; refuses with [`PoolError::Exhausted`] at capacity.
    pub async fn acquire(
        &self,
        required: &[String],
    ) -> Result<(WorkerLease, WorkerCapabilities), PoolError> {
        self.check_open()?;
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        if let Some(idle_worker) = inner
            .workers
            .iter_mut()
            .find(|w| w.status == WorkerStatus::Idle && w.capabilities.supports(required))
            .map(|worker| (worker.id, worker.capabilities.clone()))
        {
            let worker_id = idle_worker.0;
            let capabilities = idle_worker.1;
            if let Some(worker) = inner.workers.iter_mut().find(|w| w.id == worker_id) {
                worker.status = WorkerStatus::Busy;
                worker.last_seen = now;
            }
            inner.push_event(PoolEvent::WorkerAcquired(worker_id, String::new()));
            let lease = WorkerLease {
                worker_id,
                shared: Arc::clone(&self.shared),
                released: AtomicBool::new(false),
            };
            return Ok((lease, capabilities));
        }
        if inner.total() < self.config.max_workers as usize {
            inner.next_id += 1;
            let id = inner.next_id;
            let capabilities = self.port.capabilities();
            inner.workers.push(PooledWorker::fresh(
                id,
                capabilities.clone(),
                WorkerStatus::Busy,
                now,
            ));
            inner.push_event(PoolEvent::WorkerCreated(id));
            inner.push_event(PoolEvent::WorkerAcquired(id, String::new()));
            let lease = WorkerLease {
                worker_id: id,
                shared: Arc::clone(&self.shared),
                released: AtomicBool::new(false),
            };
            return Ok((lease, capabilities));
        }
        let refused = PoolError::Exhausted {
            idle: inner.idle() as u32,
            total: inner.total() as u32,
            max: self.config.max_workers,
        };
        inner.push_event(PoolEvent::CapacityRefused(self.config.max_workers));
        Err(refused)
    }

    /// Releases a leased worker back to the idle set. Dropping the lease
    /// does the same, so explicit release is a courtesy, not a duty.
    pub fn release(&self, lease: WorkerLease) {
        lease.release_inner();
    }

    /// Refreshes one worker's liveness (heartbeat from the orchestrator).
    pub fn note_heartbeat(&self, worker_id: u64) {
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        if let Some(worker) = inner.workers.iter_mut().find(|w| w.id == worker_id) {
            worker.last_seen = now;
        }
    }

    /// Runs one task end to end: acquire, mark in-flight, dispatch to the
    /// port under a deadline race, record the outcome, always release.
    ///
    /// A turn that exceeds its deadline is cancelled and reports
    /// [`WorkerError::DeadlineExceeded`]; a cancelled or timed-out task
    /// never produces a success receipt.
    pub async fn run_task(&self, task: WorkerTask) -> Result<TurnReceipt, WorkerError> {
        self.check_open()
            .map_err(|error| WorkerError::Failed(task.id.clone(), error.to_string()))?;
        let budget = if task.deadline.is_zero() {
            self.config.default_turn_deadline
        } else {
            task.deadline
        };
        let (lease, _capabilities) = self
            .acquire(&task.required_capabilities)
            .await
            .map_err(|error| WorkerError::Failed(task.id.clone(), error.to_string()))?;
        let cancel = CancelFlag::new();
        {
            let now = tokio::time::Instant::now();
            let mut inner = self.shared.lock();
            if let Some(worker) = inner.workers.iter_mut().find(|w| w.id == lease.worker_id()) {
                worker.in_flight = Some(InFlight {
                    task_id: task.id.clone(),
                    deadline: now + budget,
                    cancel: cancel.clone(),
                });
            }
        }
        let signal = cancel.signal();
        let mut turn = std::pin::pin!(self.port.run_turn(&task, signal));
        let mut sleep = std::pin::pin!(tokio::time::sleep(budget));
        let result = tokio::select! {
            outcome = &mut turn => outcome,
            _ = &mut sleep => {
                cancel.cancel();
                Err(WorkerError::DeadlineExceeded(task.id.clone()))
            }
        };
        {
            let now = tokio::time::Instant::now();
            let mut inner = self.shared.lock();
            if let Some(worker) = inner.workers.iter_mut().find(|w| w.id == lease.worker_id()) {
                worker.in_flight = None;
                worker.last_seen = now;
            }
        }
        self.release(lease);
        result
    }

    /// One health-monitor tick: fails workers whose liveness is older than
    /// `heartbeat_timeout` (cancelling any in-flight task first) and fails
    /// busy workers whose in-flight task passed its own deadline.
    ///
    /// Returns `(failed_worker, cancelled_task_id)` pairs. The caller owns
    /// the interval that drives this — see [`heartbeat_interval`].
    ///
    /// [`heartbeat_interval`]: Self::heartbeat_interval
    pub fn health_tick(&self, now: tokio::time::Instant) -> Vec<(u64, Option<String>)> {
        let mut outcomes = Vec::new();
        let mut inner = self.shared.lock();
        // First pass: decide which workers fail, collecting their ids.
        let failing: Vec<u64> = inner
            .workers
            .iter()
            .filter(|worker| {
                if worker.status == WorkerStatus::Failed {
                    return false;
                }
                let silent = now.duration_since(worker.last_seen) >= self.config.heartbeat_timeout;
                let overdue = worker
                    .in_flight
                    .as_ref()
                    .is_some_and(|flight| now >= flight.deadline);
                silent || overdue
            })
            .map(|worker| worker.id)
            .collect();
        // Second pass: mutate — cancel in-flight tasks, mark Failed, log.
        for worker_id in failing {
            let cancelled_task = inner
                .workers
                .iter_mut()
                .find(|worker| worker.id == worker_id)
                .and_then(|worker| {
                    let flight = worker.in_flight.take();
                    worker.status = WorkerStatus::Failed;
                    flight
                });
            if let Some(flight) = cancelled_task {
                flight.cancel.cancel();
                inner.push_event(PoolEvent::InFlightCancelled(
                    worker_id,
                    flight.task_id.clone(),
                ));
                outcomes.push((worker_id, Some(flight.task_id)));
            } else {
                outcomes.push((worker_id, None));
            }
            inner.push_event(PoolEvent::WorkerHeartbeatMissed(worker_id));
        }
        outcomes
    }

    /// Replaces every failed worker with a fresh idle one, within bounds.
    pub async fn replace_failed(&self) -> Result<usize, PoolError> {
        self.check_open()?;
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        let failed: Vec<u64> = inner
            .workers
            .iter()
            .filter(|w| w.status == WorkerStatus::Failed)
            .map(|w| w.id)
            .collect();
        let mut replaced = 0;
        for old in failed {
            if inner.total() >= self.config.max_workers as usize {
                inner.push_event(PoolEvent::CapacityRefused(self.config.max_workers));
                break;
            }
            inner.workers.retain(|w| w.id != old);
            inner.next_id += 1;
            let fresh = inner.next_id;
            inner.workers.push(PooledWorker::fresh(
                fresh,
                self.port.capabilities(),
                WorkerStatus::Idle,
                now,
            ));
            inner.push_event(PoolEvent::WorkerReplaced(old, fresh));
            replaced += 1;
        }
        Ok(replaced)
    }

    /// Grows or shrinks the live worker count by `delta` within bounds.
    ///
    /// Scaling up boots new idle workers immediately; scaling down sheds
    /// idle workers only — a busy worker is never killed mid-turn — and
    /// never below `min_workers`.
    pub fn scale(&self, delta: i32) -> Result<usize, PoolError> {
        self.check_open()?;
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        let before = inner.total();
        if delta > 0 {
            let room = (self.config.max_workers as usize).saturating_sub(before);
            let grow = (delta as usize).min(room);
            for _ in 0..grow {
                inner.next_id += 1;
                let id = inner.next_id;
                inner.workers.push(PooledWorker::fresh(
                    id,
                    self.port.capabilities(),
                    WorkerStatus::Idle,
                    now,
                ));
                inner.push_event(PoolEvent::WorkerCreated(id));
            }
            Ok(before + grow)
        } else if delta < 0 {
            let floor = self.config.min_workers as usize;
            let shed = (delta.unsigned_abs() as usize).min(before.saturating_sub(floor));
            let mut removed = 0;
            inner.workers.retain(|worker| {
                if removed < shed && worker.status == WorkerStatus::Idle {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
            Ok(before - removed)
        } else {
            Ok(before)
        }
    }

    /// A fresh monitor interval the orchestrator owns and drives:
    ///
    /// ```ignore
    /// let mut interval = pool.heartbeat_interval();
    /// loop {
    ///     interval.tick().await;
    ///     pool.health_tick(interval.deadline());
    /// }
    /// ```
    ///
    /// Never a global timer; the interval dies with the caller's future.
    #[must_use]
    pub fn heartbeat_interval(&self) -> tokio::time::Interval {
        tokio::time::interval(self.config.heartbeat_interval)
    }

    /// Test-only: freezes every worker's liveness `silence` in the past so
    /// `health_tick` sees them as silent without real waiting. Exposed for
    /// the crate's integration tests; not part of the orchestration
    /// surface.
    pub fn test_freeze_last_seen(&self, silence: Duration) {
        let now = tokio::time::Instant::now();
        let mut inner = self.shared.lock();
        for worker in inner.workers.iter_mut() {
            worker.last_seen = now - silence;
        }
    }

    /// Test-only: whether any worker currently holds the given task
    /// in-flight. Lets integration tests synchronize on turn start
    /// without fixed sleeps.
    pub fn has_in_flight_task(&self, task_id: &str) -> bool {
        self.shared.lock().workers.iter().any(|worker| {
            worker
                .in_flight
                .as_ref()
                .is_some_and(|flight| flight.task_id == task_id)
        })
    }
}
