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
//! - **Guaranteed teardown**: [`Lease`] is an RAII guard; `Drop` removes
//!   the member from its boundary and, when the last member leaves,
//!   releases the boundary through the port and fulfils the first
//!   queued waiter. Panic paths unwind through the same `Drop`.

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
    /// The wait for a queued lease exceeded the caller's patience.
    #[error("lease wait timed out after {waited:?}")]
    WaitTimedOut {
        /// How long the caller waited.
        waited: Duration,
    },
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
            && self.write_path != other.write_path
    }
}

/// The composition-boundary seam the harness implements against the real
/// sandbox backend. The swarm crate never links the backend itself.
pub trait SandboxLeasePort: Send + Sync {
    /// Provisions (or joins) one boundary. `Err` means the backend
    /// refused; nothing was provisioned.
    fn acquire(&self, spec: &LeaseSpec) -> Result<(), LeaseError>;

    /// Releases one previously acquired boundary.
    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError>;
}

/// RAII lease guard — one member of one boundary. Dropping it removes
/// the member; when the last member drops, the boundary is released
/// through the port and queued waiters are fulfilled. Panic unwinds run
/// this same `Drop`.
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
        release_member(&self.book, self.id);
    }
}

/// One provisioned boundary (an isolated worker or a shared group).
#[derive(Debug)]
struct Boundary {
    /// The spec that created the boundary (released verbatim).
    origin: LeaseSpec,
    /// Member leases by member id.
    members: BTreeMap<u64, LeaseSpec>,
}

#[derive(Debug)]
struct QueuedWaiter {
    spec: LeaseSpec,
    /// Hand-off slot filled by the releaser.
    slot: Arc<Mutex<Option<Result<Lease, LeaseError>>>>,
    notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Default)]
struct BookState {
    boundaries: BTreeMap<u64, Boundary>,
    /// Boundary id per shared-group name.
    shared_groups: BTreeMap<String, u64>,
    next_id: u64,
    closed: bool,
    queue: VecDeque<QueuedWaiter>,
    release_errors: VecDeque<String>,
    total_acquired: u64,
    total_released: u64,
}

impl BookState {
    fn push_release_error(&mut self, message: String) {
        if self.release_errors.len() >= 64 {
            self.release_errors.pop_front();
        }
        self.release_errors.push_back(message);
    }
}

/// Interior state shared by the book, its leases, and parked waiters.
struct LeaseBookInner {
    capacity: usize,
    capabilities: SandboxCapabilities,
    port: Arc<dyn SandboxLeasePort>,
    state: Mutex<BookState>,
}

impl std::fmt::Debug for LeaseBookInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().expect("book lock");
        f.debug_struct("LeaseBookInner")
            .field("capacity", &self.capacity)
            .field("boundaries", &state.boundaries.len())
            .field("queued", &state.queue.len())
            .field("closed", &state.closed)
            .finish_non_exhaustive()
    }
}

/// The lease book: capability gate, capacity bounds, FIFO queue, shared
/// groups, and guaranteed release wiring — the single policy object the
/// orchestrator (PR-9) funnels every acquisition through.
pub struct LeaseBook {
    inner: Arc<LeaseBookInner>,
}

/// Cloning shares the same book state (one policy object, many handles).
impl Clone for LeaseBook {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl std::fmt::Debug for LeaseBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaseBook")
            .field("inner", &self.inner)
            .finish()
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

/// Opens a fresh boundary through the port and inserts it. Policy checks
/// are the caller's responsibility; this only performs the acquisition.
fn open_boundary(
    inner: &Arc<LeaseBookInner>,
    state: &mut std::sync::MutexGuard<'_, BookState>,
    spec: LeaseSpec,
) -> Result<Lease, LeaseError> {
    inner.port.acquire(&spec).map_err(|error| match error {
        LeaseError::PortRefused { reason, .. } => LeaseError::PortRefused {
            worker: spec.worker_id.clone(),
            reason,
        },
        other => other,
    })?;
    state.next_id += 1;
    let boundary_id = state.next_id;
    state.total_acquired += 1;
    state.next_id += 1;
    let member_id = state.next_id;
    let group = match &spec.mode {
        LeaseMode::Shared(group) => Some(group.clone()),
        LeaseMode::Isolated => None,
    };
    state.boundaries.insert(
        boundary_id,
        Boundary {
            origin: spec.clone(),
            members: BTreeMap::from([(member_id, spec)]),
        },
    );
    if let Some(group) = group {
        state.shared_groups.insert(group, boundary_id);
    }
    Ok(Lease {
        id: member_id,
        book: Arc::clone(inner),
    })
}

/// Joins an existing shared boundary: compatibility already verified.
fn join_boundary(
    inner: &Arc<LeaseBookInner>,
    state: &mut std::sync::MutexGuard<'_, BookState>,
    boundary_id: u64,
    spec: LeaseSpec,
) -> Lease {
    state.next_id += 1;
    let member_id = state.next_id;
    state
        .boundaries
        .get_mut(&boundary_id)
        .expect("group mapping always has a boundary")
        .members
        .insert(member_id, spec);
    Lease {
        id: member_id,
        book: Arc::clone(inner),
    }
}

/// Releases one member; releases the boundary when the last member
/// leaves; fulfils queued waiters with freed slots.
fn release_member(inner: &Arc<LeaseBookInner>, member_id: u64) {
    let mut state = inner.state.lock().expect("book lock");
    // Find and remove the member.
    let mut boundary_id_of_member = None;
    for (boundary_id, boundary) in state.boundaries.iter_mut() {
        if boundary.members.remove(&member_id).is_some() {
            boundary_id_of_member = Some(*boundary_id);
            break;
        }
    }
    let Some(boundary_id) = boundary_id_of_member else {
        return;
    };
    // Still has members: nothing to release.
    if state
        .boundaries
        .get(&boundary_id)
        .is_some_and(|boundary| !boundary.members.is_empty())
    {
        return;
    }
    // Last member left: release the boundary.
    let Some(boundary) = state.boundaries.remove(&boundary_id) else {
        return;
    };
    if let LeaseMode::Shared(group) = &boundary.origin.mode {
        state.shared_groups.remove(group);
    }
    if let Err(error) = inner.port.release(&boundary.origin) {
        state.push_release_error(error.to_string());
    }
    state.total_released += 1;
    // Fulfil waiters while a boundary slot is free.
    while state.boundaries.len() < inner.capacity
        && let Some(waiter) = state.queue.pop_front()
    {
        let outcome = admit(inner, &mut state, waiter.spec.clone());
        let mut slot = waiter.slot.lock().expect("waiter slot");
        *slot = Some(outcome);
        drop(slot);
        waiter.notify.notify_one();
    }
}

/// Full admission policy for one spec, boundary-aware. Called with the
/// state lock held.
fn admit(
    inner: &Arc<LeaseBookInner>,
    state: &mut std::sync::MutexGuard<'_, BookState>,
    spec: LeaseSpec,
) -> Result<Lease, LeaseError> {
    if state.closed {
        return Err(LeaseError::Closed);
    }
    // Shared group with an existing boundary: strict compatibility, then
    // join without a new boundary slot.
    if let LeaseMode::Shared(group) = &spec.mode
        && let Some(&boundary_id) = state.shared_groups.get(group)
        && let Some(boundary) = state.boundaries.get(&boundary_id)
    {
        if !spec.can_share(&boundary.origin) {
            return Err(LeaseError::SharedRefused {
                reason: shared_refusal_reason(&boundary.origin, &spec),
            });
        }
        return Ok(join_boundary(inner, state, boundary_id, spec));
    }
    // Capability gate for every fresh boundary.
    if let Some(axis) = unmet_axis(&inner.capabilities, spec.requirement) {
        return Err(LeaseError::CapabilityDenied {
            requirement: spec.requirement,
            axis,
        });
    }
    open_boundary(inner, state, spec)
}

impl LeaseBook {
    /// Constructs a book with the pre-spawn fail-closed gate: the floor
    /// requirement must be satisfied by verified capabilities, else the
    /// constructor fails naming the unmet axis. Zero capacity is refused
    /// (such a book could only queue forever).
    pub fn new(
        capacity: usize,
        floor: IsolationRequirement,
        capabilities: SandboxCapabilities,
        port: Arc<dyn SandboxLeasePort>,
    ) -> Result<Self, LeaseError> {
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
            }),
        })
    }

    /// Active boundary count (a shared group counts once).
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.state.lock().expect("book lock").boundaries.len()
    }

    /// Queued waiter count.
    #[must_use]
    pub fn queued_count(&self) -> usize {
        self.inner.state.lock().expect("book lock").queue.len()
    }

    /// Total port acquisitions performed (diagnostics; pairs with
    /// [`releases`](Self::releases) in leak-detector tests).
    #[must_use]
    pub fn acquisitions(&self) -> u64 {
        self.inner.state.lock().expect("book lock").total_acquired
    }

    /// Total port releases performed (diagnostics).
    #[must_use]
    pub fn releases(&self) -> u64 {
        self.inner.state.lock().expect("book lock").total_released
    }

    /// Bounded log of backend release failures (newest last).
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

    /// Closes the book: queued waiters fail immediately with
    /// [`LeaseError::Closed`]; new acquisitions fail too; outstanding
    /// leases still release normally through `Drop`.
    pub fn close(&self) {
        let queue = {
            let mut state = self.inner.state.lock().expect("book lock");
            state.closed = true;
            std::mem::take(&mut state.queue)
        };
        for waiter in queue {
            let mut slot = waiter.slot.lock().expect("waiter slot");
            if slot.is_none() {
                *slot = Some(Err(LeaseError::Closed));
            }
            drop(slot);
            waiter.notify.notify_one();
        }
    }

    /// Acquires a lease, queueing FIFO when the book is at capacity.
    ///
    /// Policy order: closed check → shared-group join (slot-free) →
    /// capacity/queue → capability gate → port. Shared joins never touch
    /// the port: the boundary already exists. Exhaustion parks the
    /// caller until a release fulfils it — never over-provisioning.
    pub async fn acquire(&self, spec: LeaseSpec) -> Result<Lease, LeaseError> {
        enum Step {
            Done(Result<Lease, LeaseError>),
            Parked(
                Arc<Mutex<Option<Result<Lease, LeaseError>>>>,
                Arc<tokio::sync::Notify>,
            ),
        }
        let step = {
            let mut state = self.inner.state.lock().expect("book lock");
            if state.closed {
                return Err(LeaseError::Closed);
            }
            // Shared-group joins are exempt from the capacity count: they
            // consume no new boundary.
            let is_join = matches!(&spec.mode, LeaseMode::Shared(group)
                if state.shared_groups.contains_key(group));
            if !is_join && state.boundaries.len() >= self.inner.capacity {
                let slot: Arc<Mutex<Option<Result<Lease, LeaseError>>>> =
                    Arc::new(Mutex::new(None));
                let notify = Arc::new(tokio::sync::Notify::new());
                state.queue.push_back(QueuedWaiter {
                    spec,
                    slot: Arc::clone(&slot),
                    notify: Arc::clone(&notify),
                });
                Step::Parked(slot, notify)
            } else {
                Step::Done(admit(&self.inner, &mut state, spec))
            }
        };
        let Step::Parked(slot, notify) = step else {
            return match step {
                Step::Done(outcome) => outcome,
                Step::Parked(..) => unreachable!("excluded above"),
            };
        };
        // Park until a release (or close) fills the slot, with a guard
        // that removes this waiter from the queue if the future is
        // dropped before fulfilment (cancellation safety).
        struct WaiterGuard {
            armed: std::sync::atomic::AtomicBool,
            notify: Arc<tokio::sync::Notify>,
            inner: Arc<LeaseBookInner>,
        }
        impl Drop for WaiterGuard {
            fn drop(&mut self) {
                if self.armed.swap(false, std::sync::atomic::Ordering::AcqRel) {
                    let mut state = self.inner.state.lock().expect("book lock");
                    remove_waiter(&mut state, &self.notify);
                }
            }
        }
        let guard = WaiterGuard {
            armed: std::sync::atomic::AtomicBool::new(true),
            notify: Arc::clone(&notify),
            inner: Arc::clone(&self.inner),
        };
        let notified = notify.notified();
        tokio::pin!(notified);
        let outcome = loop {
            notified.as_mut().enable();
            {
                let mut slot_guard = slot.lock().expect("waiter slot");
                if let Some(outcome) = slot_guard.take() {
                    break outcome;
                }
            }
            notified.as_mut().await;
        };
        // Fulfilled: disarm so Drop does not also remove us from the
        // queue (we were already popped by the releaser).
        guard
            .armed
            .store(false, std::sync::atomic::Ordering::Release);
        outcome
    }

    /// Bounded variant of [`acquire`](Self::acquire) for callers that
    /// cannot wait forever. On timeout the parked waiter is removed from
    /// the queue (cancellation-safe: an abandoned acquire never leaks a
    /// queue entry).
    pub async fn acquire_with_timeout(
        &self,
        spec: LeaseSpec,
        timeout: Duration,
    ) -> Result<Lease, LeaseError> {
        let acquire = std::pin::pin!(self.acquire(spec));
        match tokio::time::timeout(timeout, acquire).await {
            Ok(outcome) => outcome,
            Err(_) => Err(LeaseError::WaitTimedOut { waited: timeout }),
        }
    }
}

/// Removes an abandoned waiter from the queue. Returns true when a
/// matching waiter was still parked (and was dropped without a lease).
fn remove_waiter(state: &mut BookState, notify: &Arc<tokio::sync::Notify>) -> bool {
    let before = state.queue.len();
    state
        .queue
        .retain(|waiter| !Arc::ptr_eq(&waiter.notify, notify));
    state.queue.len() != before
}
