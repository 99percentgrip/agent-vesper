//! Sandbox lease coordination for parallel workers (VRO-15 PR-8).
//!
//! This module owns the *policy* layer of sandbox concurrency: which
//! workers may hold which isolation boundary, how many boundaries exist
//! at once, who may share one, and what happens when they go away. The
//! actual OS isolation stays in the sandbox backend at the composition
//! boundary — nothing here performs syscalls, spawns processes, or
//! touches the filesystem.
//!
//! Fail-closed rules enforced here:
//!
//! - **Capability gate**: a hive whose isolation requirement exceeds the
//!   backend's verified strength fails at construction with a diagnostic
//!   naming the unmet axis; `Unknown` capability status counts as denial.
//! - **Bounded bookkeeping**: [`LeaseBook`] enforces
//!   `max_concurrent_leases` counting **boundaries**, not members — a
//!   shared group is one boundary however many workers join it.
//!   Exhaustion queues the worker (FIFO) and never over-provisions.
//! - **Strict sharing**: a `Shared` group only admits a spec whose
//!   requirement, network-grant provenance, and write paths match the
//!   boundary's origin — anything else is refused, never upgraded
//!   silently.
//! - **Owned teardown**: dropping a lease dispatches cleanup outside the book
//!   lock. Shutdown reports held, pending and quarantined boundaries explicitly;
//!   only verified cleanup frees capacity. A timeout does not stop a backend call.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use vesper_security::{CapabilityStatus, IsolationRequirement, SandboxCapabilities};

/// Errors surfaced by the lease layer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LeaseError {
    /// The backend cannot satisfy the configured requirement; the axis
    /// names exactly what is missing.
    #[error("sandbox backend cannot satisfy {requirement:?}: unmet axis {axis}")]
    CapabilityDenied {
        /// The requirement that was rejected.
        requirement: IsolationRequirement,
        /// The human-naming of the unmet capability axis.
        axis: &'static str,
    },
    /// A `Shared` request is incompatible with the existing boundary.
    #[error("shared lease refused: {reason}")]
    SharedRefused {
        /// Which compatibility rule failed.
        reason: &'static str,
    },
    /// The port itself refused (backend-level failure).
    #[error("sandbox backend refused lease for worker {worker}: {reason}")]
    PortRefused {
        /// The worker whose lease was refused.
        worker: String,
        /// Backend-supplied reason.
        reason: String,
    },
    /// Provisioning may have created resources; identity and capacity must remain
    /// quarantined until the backend's explicitly safe cleanup retry succeeds.
    #[error("sandbox provisioning uncertain for worker {worker}: {reason}")]
    ProvisioningUncertain {
        /// Reserved worker identity.
        worker: String,
        /// Backend diagnostic, without credentials or private payloads.
        reason: String,
    },
    /// Backend unwound; admission is closed and uncertain resources stay reserved.
    #[error("sandbox backend panicked during {0}; lease book closed")]
    BackendPanicked(&'static str),
    /// The wait for a queued lease exceeded the caller's patience.
    #[error("lease wait timed out after {waited:?}")]
    WaitTimedOut {
        /// How long the caller waited.
        waited: Duration,
    },
    /// Worker identity is already active, queued, or reserved by a boundary.
    #[error("sandbox worker identity already reserved: {0}")]
    WorkerReserved(String),
    /// Admission exceeded a hard resource limit; no backend call was made.
    #[error("lease resource limit exceeded: {0}")]
    ResourceLimit(&'static str),
    /// The book was closed; queued waiters observe this immediately.
    #[error("lease book is closed")]
    Closed,
}

/// Provenance of a network grant: identical strings mean identical
/// provenance. Sharing requires an exact match.
pub type NetworkGrant = String;

/// How a lease relates to other workers' leases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LeaseMode {
    /// A dedicated boundary just for this worker.
    Isolated,
    /// Join (or start) a group sharing one boundary. Compatibility is
    /// verified strictly: same requirement, same network grant, and
    /// disjoint per-worker write paths.
    Shared(String),
}

/// One lease request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseSpec {
    /// Minimum isolation the worker demands.
    pub requirement: IsolationRequirement,
    /// Network grant provenance (empty = none). Sharing requires an
    /// exact match.
    pub network_grant: NetworkGrant,
    /// Isolated or a named shared group.
    pub mode: LeaseMode,
    /// The worker this lease is for.
    pub worker_id: String,
    /// Per-worker write path inside the boundary. Workers in one shared
    /// boundary must be pairwise disjoint.
    pub write_path: String,
}

impl LeaseSpec {
    /// Convenience constructor for an isolated lease.
    #[must_use]
    pub fn isolated(worker_id: impl Into<String>, requirement: IsolationRequirement) -> Self {
        let worker_id = worker_id.into();
        Self {
            requirement,
            network_grant: String::new(),
            mode: LeaseMode::Isolated,
            write_path: format!("/w/{worker_id}/"),
            worker_id,
        }
    }

    /// Convenience constructor for a shared-group lease.
    #[must_use]
    pub fn shared(
        worker_id: impl Into<String>,
        requirement: IsolationRequirement,
        network_grant: impl Into<String>,
        group: impl Into<String>,
    ) -> Self {
        let worker_id = worker_id.into();
        Self {
            requirement,
            network_grant: network_grant.into(),
            mode: LeaseMode::Shared(group.into()),
            write_path: format!("/w/{worker_id}/"),
            worker_id,
        }
    }

    fn validate_bounds(&self) -> Result<(), LeaseError> {
        if self.worker_id.is_empty()
            || self.worker_id.len() > 256
            || self.network_grant.len() > 1024
            || self.write_path.len() > 4096
            || matches!(&self.mode, LeaseMode::Shared(group) if group.is_empty() || group.len() > 256)
        {
            return Err(LeaseError::ResourceLimit("lease specification bytes"));
        }
        Ok(())
    }

    /// Whether two specs may share one boundary: same group name, same
    /// requirement, same network-grant provenance, disjoint write paths.
    #[must_use]
    pub fn can_share(&self, other: &LeaseSpec) -> bool {
        let (LeaseMode::Shared(mine), LeaseMode::Shared(theirs)) = (&self.mode, &other.mode) else {
            return false;
        };
        mine == theirs
            && self.requirement == other.requirement
            && self.network_grant == other.network_grant
            && disjoint_write_paths(&self.write_path, &other.write_path)
    }
}

// Paths are logical absolute paths inside a boundary, not host filesystem
// paths. Reject traversal and platform-ambiguous forms rather than resolving
// aliases without filesystem authority. The backend must also prevent symlink
// aliases when enforcing the granted roots.
fn disjoint_write_paths(left: &str, right: &str) -> bool {
    fn components(path: &str) -> Option<Vec<String>> {
        if !path.starts_with('/') || path.contains(['\\', ':', '\0']) {
            return None;
        }
        let parts: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
        if parts.is_empty() || parts.iter().any(|part| matches!(*part, "." | "..")) {
            return None;
        }
        Some(parts.into_iter().map(str::to_lowercase).collect())
    }
    match (components(left), components(right)) {
        (Some(left), Some(right)) => !left.starts_with(&right) && !right.starts_with(&left),
        _ => false,
    }
}

/// The composition-boundary seam the harness implements against the real
/// sandbox backend. The swarm crate never links the backend itself.
pub trait SandboxLeasePort: Send + Sync {
    /// Provisions (or joins) one boundary. `Err` means the backend
    /// refused; nothing was provisioned. Partial provisioning MUST return
    /// `ProvisioningUncertain` and retain backend cleanup ownership. Calls run
    /// on the runtime blocking pool, outside the book lock. Calls must eventually
    /// complete; an unbounded backend can retain one slot indefinitely.
    fn acquire(&self, spec: &LeaseSpec) -> Result<(), LeaseError>;

    /// Releases one previously acquired boundary.
    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError>;

    /// Retries an incomplete teardown only when the backend supports safe retry.
    /// Implementations must be bounded and idempotent for this
    /// boundary, including when the initial release partially completed. Success
    /// certifies that its resources are gone; an error preserves quarantine.
    /// The default refuses rather than blindly repeating a potentially unsafe call.
    fn retry_release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        Err(LeaseError::PortRefused {
            worker: spec.worker_id.clone(),
            reason: String::from("backend does not support safe teardown retry"),
        })
    }
}

/// The unmet-axis diagnostic for the fail-closed capability gate.
/// `Unknown` and `Unavailable` both count as denial.
fn unmet_axis(
    capabilities: &SandboxCapabilities,
    requirement: IsolationRequirement,
) -> Option<&'static str> {
    let denied = |status: CapabilityStatus| !matches!(status, CapabilityStatus::Available);
    match requirement {
        IsolationRequirement::None => None,
        IsolationRequirement::ProcessTree => {
            denied(capabilities.process_tree).then_some("process-tree")
        }
        IsolationRequirement::Filesystem => {
            if denied(capabilities.process_tree) {
                Some("process-tree")
            } else {
                denied(capabilities.filesystem).then_some("filesystem")
            }
        }
        IsolationRequirement::Network => {
            if denied(capabilities.process_tree) {
                Some("process-tree")
            } else {
                denied(capabilities.network).then_some("network")
            }
        }
        IsolationRequirement::Full => {
            if denied(capabilities.process_tree) {
                Some("process-tree")
            } else if denied(capabilities.filesystem) {
                Some("filesystem")
            } else {
                denied(capabilities.network).then_some("network")
            }
        }
    }
}

fn shared_refusal_reason(origin: &LeaseSpec, incoming: &LeaseSpec) -> &'static str {
    if origin.requirement != incoming.requirement {
        "isolation requirements differ"
    } else if origin.network_grant != incoming.network_grant {
        "network grant provenance differs"
    } else {
        "write paths collide"
    }
}

/// A member of a boundary. Drop schedules owned cleanup; use
/// [`LeaseBook::settle`] or [`LeaseBook::shutdown`] to observe its outcome.
pub struct Lease {
    id: u64,
    book: Arc<LeaseBookInner>,
}
impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease").field("id", &self.id).finish()
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let operation = {
            let mut state = self.book.state.lock().expect("book lock");
            state.boundaries.iter_mut().find_map(|(id, boundary)| {
                if boundary.members.remove(&self.id).is_some() && boundary.members.is_empty() {
                    boundary.phase = Phase::Releasing;
                    Some((*id, Operation::Release))
                } else {
                    None
                }
            })
        };
        if let Some((id, operation)) = operation {
            dispatch(&self.book, id, operation);
        }
        self.book.changed.notify_waiters();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Provisioning,
    Live,
    Releasing,
    Quarantined,
}
#[derive(Debug, Clone, Copy)]
enum Operation {
    Acquire,
    Release,
    Retry,
}
type Reply = tokio::sync::oneshot::Sender<Result<Lease, LeaseError>>;
#[derive(Debug)]
struct Boundary {
    origin: LeaseSpec,
    members: BTreeMap<u64, LeaseSpec>,
    phase: Phase,
    reply: Option<Reply>,
}
#[derive(Debug)]
struct QueuedWaiter {
    id: u64,
    spec: LeaseSpec,
    reply: Reply,
}
#[derive(Debug, Default)]
struct BookState {
    boundaries: BTreeMap<u64, Boundary>,
    shared_groups: BTreeMap<String, u64>,
    next_id: u64,
    closed: bool,
    queue: VecDeque<QueuedWaiter>,
    release_errors: VecDeque<String>,
    total_acquired: u64,
    total_released: u64,
}
impl BookState {
    fn push_release_error(&mut self, mut message: String) {
        if message.len() > 4096 {
            let mut end = 4096;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        if self.release_errors.len() >= 64 {
            self.release_errors.pop_front();
        }
        self.release_errors.push_back(message);
    }
    fn close(&mut self) {
        self.closed = true;
        for waiter in self.queue.drain(..) {
            let _ = waiter.reply.send(Err(LeaseError::Closed));
        }
        for boundary in self.boundaries.values_mut() {
            if let Some(reply) = boundary.reply.take() {
                let _ = reply.send(Err(LeaseError::Closed));
            }
        }
    }
    fn remove_boundary(&mut self, id: u64) {
        if let Some(boundary) = self.boundaries.remove(&id)
            && let LeaseMode::Shared(group) = boundary.origin.mode
        {
            self.shared_groups.remove(&group);
        }
    }
    fn members_reserved(&self) -> usize {
        self.boundaries
            .values()
            .map(|b| b.members.len() + usize::from(b.phase == Phase::Provisioning))
            .sum()
    }
}
struct LeaseBookInner {
    capacity: usize,
    capabilities: SandboxCapabilities,
    port: Arc<dyn SandboxLeasePort>,
    state: Mutex<BookState>,
    runtime: std::sync::OnceLock<tokio::runtime::Handle>,
    changed: tokio::sync::Notify,
}
/// Shared, bounded reservation/dispatch/commit coordinator. Blocking port calls
/// run on the captured runtime's blocking pool, never under the bookkeeping lock.
/// Each dispatched operation retains its book and capacity until completion.
#[derive(Clone)]
pub struct LeaseBook {
    inner: Arc<LeaseBookInner>,
}
impl std::fmt::Debug for LeaseBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.inner.state.lock().expect("book lock");
        f.debug_struct("LeaseBook")
            .field("capacity", &self.inner.capacity)
            .field("boundaries", &state.boundaries.len())
            .field("closed", &state.closed)
            .finish()
    }
}
/// Observed cleanup state. A deadline never proves physical resource reclamation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    /// Boundaries still held by callers.
    pub held: usize,
    /// Owned backend calls still pending/running, including cancelled acquisitions.
    pub pending: usize,
    /// Boundaries whose teardown has not been verified.
    pub quarantined: usize,
    /// Waiting callers (zero after shutdown closes admission).
    pub queued: usize,
    /// The observation deadline elapsed while backend work remained pending.
    pub timed_out: bool,
}
impl CleanupReport {
    /// All boundaries and queued callers have been resolved.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.held == 0 && self.pending == 0 && self.quarantined == 0 && self.queued == 0
    }
}

// Reserve every dispatch under the lock before making any external call. Queue
// order determines reservation order, not backend completion order.
fn pump(inner: &Arc<LeaseBookInner>) {
    let (ids, deliveries) = {
        let mut state = inner.state.lock().expect("book lock");
        let mut ids = Vec::new();
        let mut deliveries = Vec::new();
        while !state.closed {
            let Some(front) = state.queue.front() else {
                break;
            };
            if front.reply.is_closed() {
                state.queue.pop_front();
                continue;
            }
            let shared = match &front.spec.mode {
                LeaseMode::Shared(group) => state.shared_groups.get(group).copied(),
                LeaseMode::Isolated => None,
            };
            if let Some(id) = shared {
                if state.boundaries[&id].phase == Phase::Provisioning {
                    break;
                }
            } else if state.boundaries.len() >= inner.capacity {
                break;
            }
            let waiter = state.queue.pop_front().expect("front waiter");
            if state.members_reserved() >= 4096 {
                deliveries.push((
                    waiter.reply,
                    Err(LeaseError::ResourceLimit("4096 active members")),
                ));
                continue;
            }
            if let Some(id) = shared {
                let boundary = state.boundaries.get_mut(&id).expect("shared reservation");
                let result = if boundary.phase != Phase::Live {
                    Err(LeaseError::SharedRefused {
                        reason: "boundary teardown is unverified",
                    })
                } else if let Some(conflict) = boundary
                    .members
                    .values()
                    .find(|s| !waiter.spec.can_share(s))
                {
                    Err(LeaseError::SharedRefused {
                        reason: shared_refusal_reason(conflict, &waiter.spec),
                    })
                } else {
                    boundary.members.insert(waiter.id, waiter.spec);
                    Ok(Lease {
                        id: waiter.id,
                        book: Arc::clone(inner),
                    })
                };
                deliveries.push((waiter.reply, result));
                continue;
            }
            if let LeaseMode::Shared(group) = &waiter.spec.mode {
                state.shared_groups.insert(group.clone(), waiter.id);
            }
            state.boundaries.insert(
                waiter.id,
                Boundary {
                    origin: waiter.spec,
                    members: BTreeMap::new(),
                    phase: Phase::Provisioning,
                    reply: Some(waiter.reply),
                },
            );
            ids.push(waiter.id);
        }
        (ids, deliveries)
    };
    for (reply, result) in deliveries {
        let _ = reply.send(result);
    }
    for id in ids {
        dispatch(inner, id, Operation::Acquire);
    }
    inner.changed.notify_waiters();
}

// The guard is moved into the blocking task, not held by the awaiting caller.
// Runtime shutdown may cancel a task before it starts: its guard quarantines the
// reserved identity instead of silently losing cleanup ownership.
struct PendingOperation {
    inner: Arc<LeaseBookInner>,
    id: u64,
    operation: Operation,
    committed: bool,
    completion: Option<tokio::sync::oneshot::Sender<bool>>,
}
impl Drop for PendingOperation {
    fn drop(&mut self) {
        if !self.committed {
            let mut state = self.inner.state.lock().expect("book lock");
            if let Some(boundary) = state.boundaries.get_mut(&self.id) {
                boundary.phase = Phase::Quarantined;
            }
            state.push_release_error("backend operation abandoned before commit".into());
            state.close();
            drop(state);
            self.inner.changed.notify_waiters();
        }
    }
}
fn dispatch(
    inner: &Arc<LeaseBookInner>,
    id: u64,
    operation: Operation,
) -> tokio::sync::oneshot::Receiver<bool> {
    let (completion, receive) = tokio::sync::oneshot::channel();
    let pending = PendingOperation {
        inner: Arc::clone(inner),
        id,
        operation,
        committed: false,
        completion: Some(completion),
    };
    let Some(runtime) = inner.runtime.get() else {
        drop(pending);
        return receive;
    };
    // Dropping this join handle detaches only the observer. The closure owns the
    // reservation, completion publication and any required follow-up cleanup.
    runtime.spawn_blocking(move || pending.run());
    receive
}
impl PendingOperation {
    fn run(mut self) {
        let spec = {
            let state = self.inner.state.lock().expect("book lock");
            state
                .boundaries
                .get(&self.id)
                .expect("reserved operation")
                .origin
                .clone()
        };
        let phase = match self.operation {
            Operation::Acquire => "acquire",
            Operation::Release => "release",
            Operation::Retry => "retry_release",
        };
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match self.operation {
                Operation::Acquire => self.inner.port.acquire(&spec),
                Operation::Release => self.inner.port.release(&spec),
                Operation::Retry => self.inner.port.retry_release(&spec),
            }))
            .unwrap_or(Err(LeaseError::BackendPanicked(phase)));
        self.commit(result);
    }
    fn commit(&mut self, result: Result<(), LeaseError>) {
        let success = result.is_ok();
        let mut delivery = None;
        let mut cleanup = false;
        {
            let mut state = self.inner.state.lock().expect("book lock");
            let panicked = matches!(result, Err(LeaseError::BackendPanicked(_)));
            match self.operation {
                Operation::Acquire => {
                    let closed = state.closed;
                    let boundary = state.boundaries.get_mut(&self.id).expect("reservation");
                    let reply = boundary.reply.take();
                    match result {
                        Ok(()) => {
                            if !closed && reply.as_ref().is_some_and(|r| !r.is_closed()) {
                                boundary.phase = Phase::Live;
                                boundary.members.insert(self.id, boundary.origin.clone());
                                delivery = reply.map(|r| {
                                    (
                                        r,
                                        Ok(Lease {
                                            id: self.id,
                                            book: Arc::clone(&self.inner),
                                        }),
                                    )
                                });
                            } else {
                                boundary.phase = Phase::Releasing;
                                cleanup = true;
                                delivery = reply.map(|r| (r, Err(LeaseError::Closed)));
                            }
                            state.total_acquired = state.total_acquired.saturating_add(1);
                        }
                        Err(error) => {
                            if panicked || matches!(error, LeaseError::ProvisioningUncertain { .. })
                            {
                                boundary.phase = Phase::Quarantined;
                                state.push_release_error(error.to_string());
                            } else {
                                state.remove_boundary(self.id);
                            }
                            delivery = reply.map(|r| (r, Err(error)));
                        }
                    }
                }
                Operation::Release | Operation::Retry => match result {
                    Ok(()) => {
                        state.remove_boundary(self.id);
                        state.total_released = state.total_released.saturating_add(1);
                    }
                    Err(error) => {
                        state
                            .boundaries
                            .get_mut(&self.id)
                            .expect("reservation")
                            .phase = Phase::Quarantined;
                        state.push_release_error(error.to_string());
                    }
                },
            }
            if panicked {
                state.close();
            }
            self.committed = true;
        }
        // Sending to an abandoned receiver drops the returned Lease. That must
        // happen outside the book lock because its Drop schedules cleanup.
        if let Some((reply, result)) = delivery {
            let _ = reply.send(result);
        }
        if cleanup {
            dispatch(&self.inner, self.id, Operation::Release);
        }
        pump(&self.inner);
        if let Some(completion) = self.completion.take() {
            let _ = completion.send(success);
        }
    }
}

struct WaiterGuard {
    inner: Arc<LeaseBookInner>,
    id: u64,
}
impl Drop for WaiterGuard {
    fn drop(&mut self) {
        {
            let mut state = self.inner.state.lock().expect("book lock");
            state.queue.retain(|w| w.id != self.id);
            if let Some(boundary) = state.boundaries.get_mut(&self.id) {
                // A late successful acquisition sees no recipient and owns its
                // release; cancellation cannot erase the provisioning reservation.
                boundary.reply.take();
            }
        }
        pump(&self.inner);
    }
}
impl LeaseBook {
    /// Construct without performing backend work. Unknown capabilities deny.
    pub fn new(
        capacity: usize,
        floor: IsolationRequirement,
        capabilities: SandboxCapabilities,
        port: Arc<dyn SandboxLeasePort>,
    ) -> Result<Self, LeaseError> {
        if capacity > 4096 {
            return Err(LeaseError::ResourceLimit("4096 boundaries"));
        }
        if capacity == 0 {
            return Err(LeaseError::CapabilityDenied {
                requirement: floor,
                axis: "zero-capacity book",
            });
        }
        if let Some(axis) = unmet_axis(&capabilities, floor) {
            return Err(LeaseError::CapabilityDenied {
                requirement: floor,
                axis,
            });
        }
        Ok(Self {
            inner: Arc::new(LeaseBookInner {
                capacity,
                capabilities,
                port,
                state: Mutex::new(BookState::default()),
                runtime: std::sync::OnceLock::new(),
                changed: tokio::sync::Notify::new(),
            }),
        })
    }
    /// Boundaries including in-flight operations and quarantine.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.state.lock().expect("book lock").boundaries.len()
    }
    /// Whether an origin still owns a live, in-flight or quarantined boundary.
    /// Composition uses this after dropping an acquire future to distinguish
    /// unused preparation from backend work that still owns its scope.
    #[must_use]
    pub fn boundary_reserved(&self, worker: &str) -> bool {
        self.inner
            .state
            .lock()
            .expect("book lock")
            .boundaries
            .values()
            .any(|boundary| boundary.origin.worker_id == worker)
    }
    /// Admission has been permanently closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.state.lock().expect("book lock").closed
    }
    /// FIFO callers awaiting capacity.
    #[must_use]
    pub fn queued_count(&self) -> usize {
        self.inner.state.lock().expect("book lock").queue.len()
    }
    /// Successful acquisitions; partial/panicked acquisitions do not increment it.
    #[must_use]
    pub fn acquisitions(&self) -> u64 {
        self.inner.state.lock().expect("book lock").total_acquired
    }
    /// Verified cleanups; may exceed acquisitions after uncertain provisioning.
    #[must_use]
    pub fn releases(&self) -> u64 {
        self.inner.state.lock().expect("book lock").total_released
    }
    /// Bounded backend error diagnostics.
    #[must_use]
    pub fn release_errors(&self) -> Vec<String> {
        self.inner
            .state
            .lock()
            .expect("book lock")
            .release_errors
            .iter()
            .cloned()
            .collect()
    }
    /// Close admission and wake queued/in-flight callers. Held leases retain
    /// their ownership; late provisioning success schedules cleanup, never a lease.
    pub fn close(&self) {
        self.inner.state.lock().expect("book lock").close();
        self.inner.changed.notify_waiters();
    }
    /// Observe current ownership without waiting or invoking a backend.
    #[must_use]
    pub fn cleanup_report(&self) -> CleanupReport {
        let state = self.inner.state.lock().expect("book lock");
        let mut report = CleanupReport {
            held: 0,
            pending: 0,
            quarantined: 0,
            queued: state.queue.len(),
            timed_out: false,
        };
        for boundary in state.boundaries.values() {
            match boundary.phase {
                Phase::Live => report.held += 1,
                Phase::Provisioning | Phase::Releasing => report.pending += 1,
                Phase::Quarantined => report.quarantined += 1,
            }
        }
        report
    }
    /// Wait for currently owned operations to settle, bounded by caller patience.
    /// Live leases and quarantined boundaries are reported without waiting for
    /// their owners. No automatic retry or forced capacity reclamation occurs.
    pub async fn settle(&self, timeout: Duration) -> Result<CleanupReport, LeaseError> {
        if timeout > Duration::from_secs(86_400) {
            return Err(LeaseError::ResourceLimit("24-hour wait timeout"));
        }
        let wait = async {
            loop {
                let notified = self.inner.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let report = self.cleanup_report();
                if report.pending == 0 {
                    return report;
                }
                notified.await;
            }
        };
        Ok(match tokio::time::timeout(timeout, wait).await {
            Ok(report) => report,
            Err(_) => {
                let mut report = self.cleanup_report();
                report.timed_out = report.pending != 0;
                report
            }
        })
    }
    /// Close admission, then observe owned cleanup. A non-clean report explicitly
    /// requires owner action or safe retry; a timeout cannot stop a blocking call.
    pub async fn shutdown(&self, timeout: Duration) -> Result<CleanupReport, LeaseError> {
        self.close();
        self.settle(timeout).await
    }
    /// Reserve at most `max_attempts` quarantined boundaries once each, in
    /// admission order. Concurrent retries cannot dispatch the same boundary.
    /// Dropping this observer does not cancel owned cleanup. Unsupported retry
    /// stays quarantined. This has no implicit deadline; callers may bound their
    /// wait and use `settle` to inspect operations that remain running.
    pub async fn retry_quarantined(&self, max_attempts: usize) -> Result<usize, LeaseError> {
        if max_attempts > 4096 {
            return Err(LeaseError::ResourceLimit("4096 cleanup attempts"));
        }
        let ids = {
            let mut state = self.inner.state.lock().expect("book lock");
            state
                .boundaries
                .iter_mut()
                .filter(|(_, b)| b.phase == Phase::Quarantined)
                .take(max_attempts)
                .map(|(id, b)| {
                    b.phase = Phase::Releasing;
                    *id
                })
                .collect::<Vec<_>>()
        };
        let completions = ids
            .into_iter()
            .map(|id| dispatch(&self.inner, id, Operation::Retry))
            .collect::<Vec<_>>();
        Ok(futures_util::future::join_all(completions)
            .await
            .into_iter()
            .filter(|r| matches!(r, Ok(true)))
            .count())
    }
    /// Acquire with bounded FIFO admission. Backend dispatch reserves capacity,
    /// origin identity and group before leaving the lock; cancellation retains
    /// the reservation until late completion and verified cleanup.
    pub async fn acquire(&self, spec: LeaseSpec) -> Result<Lease, LeaseError> {
        spec.validate_bounds()?;
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| LeaseError::ResourceLimit("Tokio runtime required"))?;
        self.inner.runtime.get_or_init(|| runtime);
        let (reply, receive) = tokio::sync::oneshot::channel();
        let id = {
            let mut state = self.inner.state.lock().expect("book lock");
            if state.closed {
                return Err(LeaseError::Closed);
            }
            if state.boundaries.values().any(|b| {
                b.origin.worker_id == spec.worker_id
                    || b.members.values().any(|s| s.worker_id == spec.worker_id)
            }) || state
                .queue
                .iter()
                .any(|w| w.spec.worker_id == spec.worker_id)
            {
                return Err(LeaseError::WorkerReserved(spec.worker_id));
            }
            if let Some(axis) = unmet_axis(&self.inner.capabilities, spec.requirement) {
                return Err(LeaseError::CapabilityDenied {
                    requirement: spec.requirement,
                    axis,
                });
            }
            if state.members_reserved() >= 4096 {
                return Err(LeaseError::ResourceLimit("4096 active members"));
            }
            let id = state
                .next_id
                .checked_add(1)
                .ok_or(LeaseError::ResourceLimit("lease identity exhaustion"))?;
            if let LeaseMode::Shared(group) = &spec.mode
                && let Some(boundary_id) = state.shared_groups.get(group).copied()
            {
                let boundary = state
                    .boundaries
                    .get_mut(&boundary_id)
                    .expect("shared reservation");
                if boundary.phase != Phase::Live && boundary.phase != Phase::Provisioning {
                    return Err(LeaseError::SharedRefused {
                        reason: "boundary teardown is unverified",
                    });
                }
                if let Some(conflict) = boundary
                    .members
                    .values()
                    .find(|member| !spec.can_share(member))
                {
                    return Err(LeaseError::SharedRefused {
                        reason: shared_refusal_reason(conflict, &spec),
                    });
                }
                if boundary.phase == Phase::Live {
                    boundary.members.insert(id, spec);
                    state.next_id = id;
                    return Ok(Lease {
                        id,
                        book: Arc::clone(&self.inner),
                    });
                }
            }
            if state.queue.len() >= 4096 {
                return Err(LeaseError::ResourceLimit("4096 queued waiters"));
            }
            state.next_id = id;
            state.queue.push_back(QueuedWaiter { id, spec, reply });
            id
        };
        let _guard = WaiterGuard {
            inner: Arc::clone(&self.inner),
            id,
        };
        pump(&self.inner);
        receive.await.unwrap_or(Err(LeaseError::Closed))
    }
    /// Caller wait bound only. A timed-out dispatch retains capacity and cleanup
    /// ownership; no claim is made that the backend operation was interrupted.
    pub async fn acquire_with_timeout(
        &self,
        spec: LeaseSpec,
        timeout: Duration,
    ) -> Result<Lease, LeaseError> {
        if timeout > Duration::from_secs(86_400) {
            return Err(LeaseError::ResourceLimit("24-hour wait timeout"));
        }
        tokio::time::timeout(timeout, self.acquire(spec))
            .await
            .unwrap_or(Err(LeaseError::WaitTimedOut { waited: timeout }))
    }
}
