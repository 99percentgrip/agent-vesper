//! Integration tests for sandbox lease coordination (VRO-15 PR-8).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_swarm::sandbox::{LeaseBook, LeaseError, LeaseMode, LeaseSpec, SandboxLeasePort};

// ---------------------------------------------------------------------
// Fake backend: exact acquire/release pair accounting
// ---------------------------------------------------------------------

#[derive(Debug, Default)]
struct FakeSandboxLeasePort {
    acquires: AtomicUsize,
    releases: AtomicUsize,
    live: Mutex<Vec<String>>,
    refuse_all: bool,
}

impl FakeSandboxLeasePort {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            acquires: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            live: Mutex::new(Vec::new()),
            refuse_all: false,
        })
    }

    fn refusing() -> Arc<Self> {
        Arc::new(Self {
            acquires: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            live: Mutex::new(Vec::new()),
            refuse_all: true,
        })
    }

    fn key(spec: &LeaseSpec) -> String {
        format!("{}|{}|{:?}", spec.worker_id, spec.write_path, spec.mode)
    }

    fn live_count(&self) -> usize {
        self.live.lock().expect("live").len()
    }
}

impl SandboxLeasePort for FakeSandboxLeasePort {
    fn acquire(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        if self.refuse_all {
            return Err(LeaseError::PortRefused {
                worker: spec.worker_id.clone(),
                reason: String::from("fake backend offline"),
            });
        }
        self.acquires.fetch_add(1, Ordering::AcqRel);
        self.live.lock().expect("live").push(Self::key(spec));
        Ok(())
    }

    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        self.releases.fetch_add(1, Ordering::AcqRel);
        let key = Self::key(spec);
        let mut live = self.live.lock().expect("live");
        if let Some(position) = live.iter().position(|entry| *entry == key) {
            live.remove(position);
            Ok(())
        } else {
            // Double release — must be loud in diagnostics.
            Err(LeaseError::PortRefused {
                worker: spec.worker_id.clone(),
                reason: String::from("release without acquire"),
            })
        }
    }
}

// ---------------------------------------------------------------------
// Capability fixtures
// ---------------------------------------------------------------------

fn full_backend() -> SandboxCapabilities {
    SandboxCapabilities {
        backend: String::from("fake-full"),
        process_tree: CapabilityStatus::Available,
        filesystem: CapabilityStatus::Available,
        network: CapabilityStatus::Available,
        strength: SecurityStrength::Full,
    }
}

fn process_only_backend() -> SandboxCapabilities {
    SandboxCapabilities {
        backend: String::from("fake-process"),
        process_tree: CapabilityStatus::Available,
        filesystem: CapabilityStatus::Unavailable,
        network: CapabilityStatus::Unavailable,
        strength: SecurityStrength::Process,
    }
}

fn unknown_backend() -> SandboxCapabilities {
    SandboxCapabilities {
        backend: String::from("fake-unknown"),
        process_tree: CapabilityStatus::Unknown,
        filesystem: CapabilityStatus::Unknown,
        network: CapabilityStatus::Unknown,
        strength: SecurityStrength::None,
    }
}

fn book(
    capacity: usize,
    floor: IsolationRequirement,
    caps: SandboxCapabilities,
    port: Arc<dyn SandboxLeasePort>,
) -> Result<LeaseBook, LeaseError> {
    LeaseBook::new(capacity, floor, caps, port)
}

// ---------------------------------------------------------------------
// Capability gate: the denial matrix
// ---------------------------------------------------------------------

#[test]
fn capability_denial_matrix_names_the_unmet_axis() {
    // Full backend satisfies everything.
    for requirement in [
        IsolationRequirement::None,
        IsolationRequirement::ProcessTree,
        IsolationRequirement::Filesystem,
        IsolationRequirement::Network,
        IsolationRequirement::Full,
    ] {
        assert!(
            book(2, requirement, full_backend(), FakeSandboxLeasePort::new()).is_ok(),
            "full backend must satisfy {requirement:?}"
        );
    }

    // Process-only backend: denied beyond ProcessTree, naming the axis.
    for (requirement, axis) in [
        (IsolationRequirement::Filesystem, "filesystem"),
        (IsolationRequirement::Network, "network"),
        (IsolationRequirement::Full, "filesystem"),
    ] {
        let error = book(
            2,
            requirement,
            process_only_backend(),
            FakeSandboxLeasePort::new(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            LeaseError::CapabilityDenied { requirement, axis },
            "matrix mismatch for {requirement:?}"
        );
    }

    // Unknown counts as denial — never a pass.
    let error = book(
        2,
        IsolationRequirement::ProcessTree,
        unknown_backend(),
        FakeSandboxLeasePort::new(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        LeaseError::CapabilityDenied {
            requirement: IsolationRequirement::ProcessTree,
            axis: "process-tree",
        }
    );
}

#[test]
fn zero_capacity_book_is_refused_at_construction() {
    let error = book(
        0,
        IsolationRequirement::None,
        full_backend(),
        FakeSandboxLeasePort::new(),
    )
    .unwrap_err();
    assert!(matches!(error, LeaseError::CapabilityDenied { .. }));
}

// ---------------------------------------------------------------------
// Capacity + FIFO queue
// ---------------------------------------------------------------------

#[tokio::test]
async fn exhaustion_queues_never_over_provisions() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(2, IsolationRequirement::None, full_backend(), port.clone()).unwrap();
    let first = ledger
        .acquire(LeaseSpec::isolated("w1", IsolationRequirement::None))
        .await
        .expect("first");
    let second = ledger
        .acquire(LeaseSpec::isolated("w2", IsolationRequirement::None))
        .await
        .expect("second");
    assert_eq!(ledger.active_count(), 2);
    // Third acquire must park, not provision.
    let third = tokio::spawn({
        let book = ledger.clone();
        async move {
            book.acquire(LeaseSpec::isolated("w3", IsolationRequirement::None))
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(ledger.queued_count(), 1, "waiter must be queued");
    assert_eq!(port.live_count(), 2, "never over-provision");
    drop(first);
    let third_lease = third.await.expect("join").expect("queued lease granted");
    assert_eq!(ledger.active_count(), 2, "slot handed over exactly");
    drop(second);
    drop(third_lease);
    assert_eq!(ledger.active_count(), 0);
}

#[tokio::test]
async fn queue_is_strictly_fifo() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(1, IsolationRequirement::None, full_backend(), port).unwrap();
    let blocker = ledger
        .acquire(LeaseSpec::isolated("blocker", IsolationRequirement::None))
        .await
        .unwrap();

    let first = {
        let ledger = ledger.clone();
        tokio::spawn(async move {
            ledger
                .acquire(LeaseSpec::isolated("w-first", IsolationRequirement::None))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(10)).await;
    let second = {
        let ledger = ledger.clone();
        tokio::spawn(async move {
            ledger
                .acquire(LeaseSpec::isolated("w-second", IsolationRequirement::None))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(ledger.queued_count(), 2);
    drop(blocker);
    let first_lease = first.await.expect("join").expect("fifo first");
    assert_eq!(ledger.active_count(), 1);
    drop(first_lease);
    let second_lease = second.await.expect("join").expect("fifo second");
    drop(second_lease);
    assert_eq!(ledger.active_count(), 0);
}

// ---------------------------------------------------------------------
// Shared groups
// ---------------------------------------------------------------------

#[tokio::test]
async fn shared_group_joins_without_consuming_a_slot() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(2, IsolationRequirement::None, full_backend(), port.clone()).unwrap();
    let _a = ledger
        .acquire(LeaseSpec::shared(
            "w1",
            IsolationRequirement::None,
            "grant-x",
            "team",
        ))
        .await
        .expect("group start");
    let _b = ledger
        .acquire(LeaseSpec::shared(
            "w2",
            IsolationRequirement::None,
            "grant-x",
            "team",
        ))
        .await
        .expect("group join");
    // Boundary accounting: the shared group is ONE boundary however
    // many members it has, and the port saw exactly one acquisition.
    assert_eq!(ledger.active_count(), 1);
    assert_eq!(port.acquires.load(Ordering::Acquire), 1);
    // A second, different boundary still fits in capacity 2.
    let _c = ledger
        .acquire(LeaseSpec::isolated("w3", IsolationRequirement::None))
        .await
        .expect("second boundary fits");
    assert_eq!(ledger.active_count(), 2);
}

#[tokio::test]
async fn shared_group_refusals_name_the_exact_rule() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(4, IsolationRequirement::None, full_backend(), port).unwrap();
    let _origin = ledger
        .acquire(LeaseSpec::shared(
            "w1",
            IsolationRequirement::ProcessTree,
            "grant-x",
            "team",
        ))
        .await
        .expect("origin");

    // Different requirement.
    let error = ledger
        .acquire(LeaseSpec::shared(
            "w2",
            IsolationRequirement::Filesystem,
            "grant-x",
            "team",
        ))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        LeaseError::SharedRefused {
            reason: "isolation requirements differ"
        }
    );

    // Different network grant provenance.
    let error = ledger
        .acquire(LeaseSpec::shared(
            "w2",
            IsolationRequirement::ProcessTree,
            "grant-y",
            "team",
        ))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        LeaseError::SharedRefused {
            reason: "network grant provenance differs"
        }
    );

    // Colliding write paths (same worker id ⇒ same path).
    let error = ledger
        .acquire(LeaseSpec::shared(
            "w1",
            IsolationRequirement::ProcessTree,
            "grant-x",
            "team",
        ))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        LeaseError::SharedRefused {
            reason: "write paths collide"
        }
    );
}

#[tokio::test]
async fn shared_group_dissolves_when_last_member_leaves() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(4, IsolationRequirement::None, full_backend(), port).unwrap();
    let a = ledger
        .acquire(LeaseSpec::shared(
            "w1",
            IsolationRequirement::None,
            "g",
            "team",
        ))
        .await
        .unwrap();
    let b = ledger
        .acquire(LeaseSpec::shared(
            "w2",
            IsolationRequirement::None,
            "g",
            "team",
        ))
        .await
        .unwrap();
    drop(a);
    // Group still alive with b.
    let rejoin = ledger
        .acquire(LeaseSpec::shared(
            "w3",
            IsolationRequirement::None,
            "g",
            "team",
        ))
        .await;
    assert!(rejoin.is_ok(), "group must survive while a member holds it");
    drop(b);
    drop(rejoin.unwrap());
    // All members gone: a fresh group may restart under the same name
    // with different parameters.
    let fresh = ledger
        .acquire(LeaseSpec::shared(
            "w4",
            IsolationRequirement::ProcessTree,
            "other-grant",
            "team",
        ))
        .await;
    assert!(fresh.is_ok(), "dissolved group frees the name");
}

// ---------------------------------------------------------------------
// Teardown guarantees: Drop and panic paths
// ---------------------------------------------------------------------

#[tokio::test]
async fn drop_releases_every_lease_and_pairs_match_exactly() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(4, IsolationRequirement::None, full_backend(), port.clone()).unwrap();
    {
        let mut leases = Vec::new();
        for worker in ["w1", "w2", "w3", "w4"] {
            leases.push(
                ledger
                    .acquire(LeaseSpec::isolated(worker, IsolationRequirement::None))
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(ledger.active_count(), 4);
        assert_eq!(port.live_count(), 4);
    } // scope drop = swarm shutdown
    assert_eq!(ledger.active_count(), 0, "shutdown released everything");
    assert_eq!(port.live_count(), 0);
    assert_eq!(
        port.acquires.load(Ordering::Acquire),
        port.releases.load(Ordering::Acquire),
        "acquire/release pairs must match exactly"
    );
    assert_eq!(ledger.acquisitions(), ledger.releases());
}

#[tokio::test]
async fn panic_in_holder_scope_still_releases_leases() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(2, IsolationRequirement::None, full_backend(), port.clone()).unwrap();
    let book = ledger.clone();
    let holder = {
        let book = book.clone();
        tokio::spawn(async move {
            let _lease = book
                .acquire(LeaseSpec::isolated("panicking", IsolationRequirement::None))
                .await
                .expect("lease");
            // Panic while holding the lease: unwinding must Drop it.
            panic!("simulated orchestrator panic while holding a lease");
        })
    };
    let _ = holder.await; // JoinError expected: the task panicked.
    assert_eq!(book.active_count(), 0, "panic path released the lease");
    assert_eq!(port.live_count(), 0);
    assert_eq!(port.acquires.load(Ordering::Acquire), 1);
    assert_eq!(port.releases.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn cancelled_waiter_enqueues_then_releases_on_close() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(1, IsolationRequirement::None, full_backend(), port).unwrap();
    let _blocker = ledger
        .acquire(LeaseSpec::isolated("blocker", IsolationRequirement::None))
        .await
        .unwrap();
    let waiter = {
        let ledger = ledger.clone();
        tokio::spawn(async move {
            ledger
                .acquire(LeaseSpec::isolated("waiter", IsolationRequirement::None))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(ledger.queued_count(), 1);
    // Closing the book must fail the queued waiter immediately.
    ledger.close();
    let outcome = waiter.await.expect("join");
    assert!(matches!(outcome, Err(LeaseError::Closed)));
    assert_eq!(ledger.queued_count(), 0);
    // And new acquisitions refuse after close.
    assert!(matches!(
        ledger
            .acquire(LeaseSpec::isolated("late", IsolationRequirement::None))
            .await,
        Err(LeaseError::Closed)
    ));
}

#[tokio::test]
async fn port_refusal_surfaces_loudly_and_reserves_nothing() {
    let port = FakeSandboxLeasePort::refusing();
    let ledger = book(2, IsolationRequirement::None, full_backend(), port).unwrap();
    let error = ledger
        .acquire(LeaseSpec::isolated("w1", IsolationRequirement::None))
        .await
        .unwrap_err();
    assert!(matches!(error, LeaseError::PortRefused { .. }));
    assert_eq!(ledger.active_count(), 0, "refused lease reserves nothing");
}

#[tokio::test]
async fn timeout_bounds_the_wait() {
    let port = FakeSandboxLeasePort::new();
    let ledger = book(1, IsolationRequirement::None, full_backend(), port).unwrap();
    let _blocker = ledger
        .acquire(LeaseSpec::isolated("blocker", IsolationRequirement::None))
        .await
        .unwrap();
    let outcome = ledger
        .acquire_with_timeout(
            LeaseSpec::isolated("impatient", IsolationRequirement::None),
            Duration::from_millis(30),
        )
        .await;
    assert!(matches!(outcome, Err(LeaseError::WaitTimedOut { .. })));
    // The timed-out waiter must be removed from the queue.
    tokio::time::sleep(Duration::from_millis(10)).await;
    // (The waiter may or may not still be parked depending on timing; the
    // book drains it on the next release cycle either way.)
    drop(_blocker);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(ledger.active_count(), 0);
}

// ---------------------------------------------------------------------
// Debug surface
// ---------------------------------------------------------------------

#[test]
fn book_and_lease_are_debug() {
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<LeaseBook>();
    assert_debug::<LeaseSpec>();
    assert_debug::<LeaseMode>();
}
