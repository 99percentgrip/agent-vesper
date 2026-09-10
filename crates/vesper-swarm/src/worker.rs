//! Worker execution interface (VRO-15 PR-3).
//!
//! [`WorkerPort`] is the swarm crate's single seam to execution: a
//! provider-neutral, async, cancellable unit of work. The swarm crate
//! defines the trait and consumes it; concrete implementations (real
//! provider turns over `ProviderSession`, sandboxed drivers) live at the
//! composition boundary in later PRs and are forbidden here.
//!
//! Everything in this module is pure shape plus small helpers: no I/O, no
//! clock, no provider names.

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

/// Cooperative cancellation source, cheap to clone and poll.
///
/// The pool owns one signal per in-flight task. Workers must poll
/// [`CancellationSignal::is_cancelled`] inside their turn loops; a
/// cancelled task must never report success.
#[derive(Debug, Clone)]
pub struct CancellationSignal {
    flag: Arc<std::sync::atomic::AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl CancellationSignal {
    /// Creates an uncancelled signal bound to a fresh flag.
    #[must_use]
    pub fn new() -> Self {
        Self {
            flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Parks without polling a clock until cancellation. Clones share wakeups.
    pub async fn cancelled(&self) {
        let notified = self.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.is_cancelled() {
            notified.await;
        }
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Default for CancellationSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// The writable half of a [`CancellationSignal`], held by the pool.
#[derive(Debug, Clone)]
pub struct CancelFlag {
    signal: CancellationSignal,
}

impl CancelFlag {
    /// Creates a signal/flag pair.
    #[must_use]
    pub fn new() -> Self {
        Self {
            signal: CancellationSignal::new(),
        }
    }

    /// The read-only signal workers receive.
    #[must_use]
    pub fn signal(&self) -> CancellationSignal {
        self.signal.clone()
    }

    /// Requests cancellation (idempotent).
    pub fn cancel(&self) {
        self.signal
            .flag
            .store(true, std::sync::atomic::Ordering::Release);
        self.signal.notify.notify_waiters();
    }
}

impl Default for CancelFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// What a worker can do, declared up front for task routing.
///
/// Capability names are free-form strings owned by the composition layer
/// (e.g. "read", "write", "browser"); the pool only matches sets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCapabilities {
    /// Capability names this worker accepts.
    pub tools: Vec<String>,
    /// Concurrent tasks this worker may run at once.
    pub max_concurrent_tasks: u32,
}

impl WorkerCapabilities {
    /// Capabilities accepting any single task at a time, with no tools.
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            tools: Vec::new(),
            max_concurrent_tasks: 1,
        }
    }

    /// Whether every required capability is declared.
    #[must_use]
    pub fn supports(&self, required: &[String]) -> bool {
        required
            .iter()
            .all(|need| self.tools.iter().any(|have| have == need))
    }
}

/// Coarse task classes understood by assignment (PR-5 refines routing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    /// Read-only research or retrieval.
    Research,
    /// Analysis, planning, or synthesis.
    Analysis,
    /// Code or artifact production.
    Coding,
    /// Verification of produced work.
    Testing,
    /// Critique of produced work.
    Review,
    /// Documentation authoring.
    Documentation,
    /// Free-form work declared only by capabilities.
    Custom,
}

/// Task urgency, ordered low to high.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum TaskPriority {
    /// Background work; first to be shed under pressure.
    Background,
    /// Routine work.
    Low,
    /// Default urgency.
    #[default]
    Normal,
    /// Time-sensitive work.
    High,
    /// Drop-everything work.
    Critical,
}

/// One bounded unit of work handed to a worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerTask {
    /// Stable task identity.
    pub id: String,
    /// Coarse class of work.
    pub kind: TaskKind,
    /// Urgency.
    pub priority: TaskPriority,
    /// The instruction the worker executes.
    pub prompt: String,
    /// Capabilities required to run this task.
    pub required_capabilities: Vec<String>,
    /// Hard wall-clock budget for the whole turn.
    pub deadline: Duration,
}

impl WorkerTask {
    /// A minimal task with the given id, prompt, and default shape.
    #[must_use]
    pub fn new(id: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: TaskKind::Custom,
            priority: TaskPriority::Normal,
            prompt: prompt.into(),
            required_capabilities: Vec::new(),
            deadline: Duration::from_secs(120),
        }
    }
}

/// Measured outcome of one completed worker turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnReceipt {
    /// The task this receipt belongs to.
    pub task_id: String,
    /// Worker-visible result text (bounded by the executor).
    pub output: String,
    /// Whether the worker considers the work successful.
    pub success: bool,
    /// Wall-clock duration the worker reports for the turn.
    pub duration: Duration,
}

/// Why a turn ended without a successful receipt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkerError {
    /// The worker rejected the task (e.g. missing capabilities).
    #[error("worker rejected task {0}")]
    Rejected(String),
    /// The deadline elapsed before the turn completed.
    #[error("task {0} exceeded its deadline")]
    DeadlineExceeded(String),
    /// Cancellation was requested while the turn ran.
    #[error("task {0} was cancelled")]
    Cancelled(String),
    /// The worker failed executing the turn.
    #[error("worker failed on task {0}: {1}")]
    Failed(String, String),
}

/// Provider-neutral execution seam.
///
/// Implementations run one bounded turn for one task. They receive the
/// task's cancellation signal and must honor it: a cancelled turn returns
/// [`WorkerError::Cancelled`], never a success receipt.
pub trait WorkerPort: Send + Sync {
    /// Nonblocking lifecycle observation. Implementations that own work beyond
    /// a dropped turn future must report it until that work and its native
    /// state have settled. Pure caller-owned futures use the default.
    fn pending_work(&self) -> bool {
        false
    }

    /// Runs one turn to completion.
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>>;

    /// Declares what this worker can do.
    fn capabilities(&self) -> WorkerCapabilities;
}

/// Blanket helper: whether a port can run a task by capability match.
#[must_use]
pub fn port_supports(port: &dyn WorkerPort, task: &WorkerTask) -> bool {
    port.capabilities().supports(&task.required_capabilities)
}
