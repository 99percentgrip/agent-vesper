//! Reservation, caller cancellation and cleanup ownership at the backend boundary.
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_swarm::sandbox::{LeaseBook, LeaseError, LeaseSpec, SandboxLeasePort};

fn book(port: Arc<dyn SandboxLeasePort>) -> LeaseBook {
    LeaseBook::new(
        1,
        IsolationRequirement::ProcessTree,
        SandboxCapabilities {
            backend: "fixture".into(),
            process_tree: CapabilityStatus::Available,
            filesystem: CapabilityStatus::Available,
            network: CapabilityStatus::Available,
            strength: SecurityStrength::Full,
        },
        port,
    )
    .unwrap()
}
fn spec(id: &str) -> LeaseSpec {
    LeaseSpec::isolated(id, IsolationRequirement::ProcessTree)
}

#[derive(Default)]
struct Reentrant {
    book: Mutex<Option<LeaseBook>>,
}
impl Reentrant {
    fn inspect(&self) -> Result<(), LeaseError> {
        let book = self.book.lock().unwrap().as_ref().unwrap().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(book.active_count());
        });
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(1) => Ok(()),
            _ => Err(LeaseError::PortRefused {
                worker: "fixture".into(),
                reason: "book locked or reservation lost during backend operation".into(),
            }),
        }
    }
}
impl SandboxLeasePort for Reentrant {
    fn acquire(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        self.inspect()
    }
    fn release(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        self.inspect()
    }
}
#[tokio::test]
async fn backend_can_inspect_reserved_capacity_without_book_lock() {
    let port = Arc::new(Reentrant::default());
    let book = book(port.clone());
    *port.book.lock().unwrap() = Some(book.clone());
    let lease = book.acquire(spec("worker")).await.unwrap();
    drop(lease);
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert!(book.release_errors().is_empty());
    port.book.lock().unwrap().take();
}

use std::sync::atomic::{AtomicUsize, Ordering};
struct Gate {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    resume: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl Gate {
    fn new() -> (
        Self,
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (entered, receive) = tokio::sync::oneshot::channel();
        let (resume, wait) = std::sync::mpsc::channel();
        (
            Self {
                entered: Mutex::new(Some(entered)),
                resume: Mutex::new(wait),
            },
            receive,
            resume,
        )
    }
    fn wait(&self) {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
            self.resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }
}
#[derive(Default)]
struct BlockingPort {
    acquire_gate: Option<Gate>,
    release_gate: Option<Gate>,
    retry_gate: Option<Gate>,
    acquired: AtomicUsize,
    released: AtomicUsize,
    retries: AtomicUsize,
    uncertain: bool,
}
impl SandboxLeasePort for BlockingPort {
    fn acquire(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        self.acquired.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.acquire_gate {
            gate.wait();
        }
        if self.uncertain {
            return Err(LeaseError::ProvisioningUncertain {
                worker: spec.worker_id.clone(),
                reason: "partial fixture provisioning".into(),
            });
        }
        Ok(())
    }
    fn release(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        if let Some(gate) = &self.release_gate {
            gate.wait();
        }
        self.released.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn retry_release(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        self.retries.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.retry_gate {
            gate.wait();
        }
        Ok(())
    }
}
fn poll_pending<F: std::future::Future>(future: std::pin::Pin<&mut F>) {
    assert!(
        future
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
            .is_pending()
    );
}
#[tokio::test]
async fn close_during_blocked_provision_wakes_caller_and_owns_late_cleanup() {
    let (gate, entered, resume) = Gate::new();
    let port = Arc::new(BlockingPort {
        acquire_gate: Some(gate),
        ..Default::default()
    });
    let book = book(port.clone());
    let mut pending = Box::pin(book.acquire(spec("late")));
    poll_pending(pending.as_mut());
    entered.await.unwrap();
    assert_eq!(book.cleanup_report().pending, 1);
    assert!(matches!(
        book.acquire(spec("late")).await,
        Err(LeaseError::WorkerReserved(_))
    ));
    let report = book.shutdown(Duration::ZERO).await.unwrap();
    assert!(report.timed_out);
    assert_eq!(report.pending, 1);
    assert!(!report.is_clean());
    assert!(matches!(pending.await, Err(LeaseError::Closed)));
    assert_eq!(port.released.load(Ordering::SeqCst), 0);
    resume.send(()).unwrap();
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(book.acquisitions(), 1);
    assert_eq!(book.releases(), 1);
    assert_eq!(port.released.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn dropping_acquire_retains_capacity_until_late_cleanup_then_fifo_dispatches() {
    let (gate, entered, resume) = Gate::new();
    let port = Arc::new(BlockingPort {
        acquire_gate: Some(gate),
        ..Default::default()
    });
    let book = book(port.clone());
    let mut abandoned = Box::pin(book.acquire(spec("abandoned")));
    poll_pending(abandoned.as_mut());
    entered.await.unwrap();
    drop(abandoned);
    let mut first = Box::pin(book.acquire(spec("first")));
    let mut second = Box::pin(book.acquire(spec("second")));
    poll_pending(first.as_mut());
    poll_pending(second.as_mut());
    assert_eq!(book.queued_count(), 2);
    assert_eq!(port.acquired.load(Ordering::SeqCst), 1);
    resume.send(()).unwrap();
    let first = first.await.unwrap();
    assert_eq!(book.queued_count(), 1);
    assert_eq!(port.acquired.load(Ordering::SeqCst), 2);
    assert_eq!(port.released.load(Ordering::SeqCst), 1);
    drop(first);
    let second = second.await.unwrap();
    assert_eq!(port.acquired.load(Ordering::SeqCst), 3);
    drop(second);
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(port.released.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn timeout_cannot_free_a_blocked_backend_reservation() {
    let (gate, entered, resume) = Gate::new();
    let port = Arc::new(BlockingPort {
        acquire_gate: Some(gate),
        ..Default::default()
    });
    let book = book(port.clone());
    let mut pending = Box::pin(book.acquire_with_timeout(spec("timed"), Duration::from_millis(20)));
    poll_pending(pending.as_mut());
    entered.await.unwrap();
    assert!(matches!(
        pending.await,
        Err(LeaseError::WaitTimedOut { .. })
    ));
    assert_eq!(book.active_count(), 1);
    assert_eq!(book.cleanup_report().pending, 1);
    assert!(matches!(
        book.acquire(spec("timed")).await,
        Err(LeaseError::WorkerReserved(_))
    ));
    resume.send(()).unwrap();
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(port.released.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn blocked_release_does_not_block_drop_close_or_free_capacity() {
    let (gate, entered, resume) = Gate::new();
    let port = Arc::new(BlockingPort {
        release_gate: Some(gate),
        ..Default::default()
    });
    let book = book(port.clone());
    let lease = book.acquire(spec("worker")).await.unwrap();
    drop(lease);
    entered.await.unwrap();
    assert_eq!(book.active_count(), 1);
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert!(book.shutdown(Duration::ZERO).await.unwrap().timed_out);
    assert_eq!(book.releases(), 0);
    resume.send(()).unwrap();
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(port.released.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn partial_provision_and_dropped_retry_keep_single_cleanup_owner() {
    let (gate, entered, resume) = Gate::new();
    let port = Arc::new(BlockingPort {
        uncertain: true,
        retry_gate: Some(gate),
        ..Default::default()
    });
    let book = book(port.clone());
    assert!(matches!(
        book.acquire(spec("partial")).await,
        Err(LeaseError::ProvisioningUncertain { .. })
    ));
    assert_eq!(book.cleanup_report().quarantined, 1);
    assert_eq!(book.acquisitions(), 0);
    let mut retry = Box::pin(book.retry_quarantined(1));
    poll_pending(retry.as_mut());
    entered.await.unwrap();
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    drop(retry);
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert!(book.shutdown(Duration::ZERO).await.unwrap().timed_out);
    resume.send(()).unwrap();
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(port.retries.load(Ordering::SeqCst), 1);
    assert_eq!(book.releases(), 1);
}
#[tokio::test]
async fn queued_shared_members_join_only_after_origin_commit() {
    let port = Arc::new(BlockingPort::default());
    let book = book(port.clone());
    let held = book.acquire(spec("held")).await.unwrap();
    let shared = |id| LeaseSpec::shared(id, IsolationRequirement::ProcessTree, "grant", "group");
    let mut first = Box::pin(book.acquire(shared("first")));
    let mut second = Box::pin(book.acquire(shared("second")));
    poll_pending(first.as_mut());
    poll_pending(second.as_mut());
    drop(held);
    let first = first.await.unwrap();
    let second = second.await.unwrap();
    assert_eq!(book.active_count(), 1);
    assert_eq!(port.acquired.load(Ordering::SeqCst), 2);
    drop((first, second));
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(port.released.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn shared_member_arriving_during_dispatched_provision_waits_for_commit() {
    let (gate, entered, resume) = Gate::new();
    let port = Arc::new(BlockingPort {
        acquire_gate: Some(gate),
        ..Default::default()
    });
    let book = book(port.clone());
    let shared = |id| LeaseSpec::shared(id, IsolationRequirement::ProcessTree, "grant", "group");
    let mut first = Box::pin(book.acquire(shared("first")));
    poll_pending(first.as_mut());
    entered.await.unwrap();
    let mut second = Box::pin(book.acquire(shared("second")));
    poll_pending(second.as_mut());
    assert_eq!(port.acquired.load(Ordering::SeqCst), 1);
    resume.send(()).unwrap();
    let first = first.await.unwrap();
    let second = second.await.unwrap();
    assert_eq!(book.active_count(), 1);
    assert_eq!(port.acquired.load(Ordering::SeqCst), 1);
    drop((first, second));
    assert!(
        book.shutdown(Duration::from_secs(5))
            .await
            .unwrap()
            .is_clean()
    );
    assert_eq!(port.released.load(Ordering::SeqCst), 1);
}
