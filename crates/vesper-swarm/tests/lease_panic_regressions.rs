//! Backend unwinds must not poison bookkeeping or lose uncertain resources.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_swarm::sandbox::{LeaseBook, LeaseError, LeaseSpec, SandboxLeasePort};
struct Port {
    acquire_panics: AtomicBool,
    cleanup_panics: AtomicBool,
}
impl SandboxLeasePort for Port {
    fn acquire(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        assert!(
            !self.acquire_panics.load(Ordering::SeqCst),
            "private-acquire-canary"
        );
        Ok(())
    }
    fn release(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        assert!(
            !self.cleanup_panics.load(Ordering::SeqCst),
            "private-release-canary"
        );
        Ok(())
    }
    fn retry_release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        self.release(spec)
    }
}
fn book(port: Arc<Port>) -> LeaseBook {
    LeaseBook::new(
        2,
        IsolationRequirement::None,
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
    LeaseSpec::isolated(id, IsolationRequirement::None)
}
#[tokio::test]
async fn acquisition_panic_reserves_uncertain_boundary_and_closes_admission() {
    let port = Arc::new(Port {
        acquire_panics: AtomicBool::new(true),
        cleanup_panics: AtomicBool::new(false),
    });
    let book = book(port);
    let error = book
        .acquire(spec("uncertain"))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("panicked"));
    assert!(!error.contains("canary"));
    assert_eq!(book.active_count(), 1);
    assert_eq!(book.acquisitions(), 0);
    assert!(matches!(
        book.acquire(spec("next")).await,
        Err(LeaseError::Closed)
    ));
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 1);
    assert_eq!(book.active_count(), 0);
    assert!(matches!(
        book.acquire(spec("next")).await,
        Err(LeaseError::Closed)
    ));
}
#[tokio::test]
async fn release_and_retry_panics_preserve_book_and_wake_waiters_closed() {
    use std::future::Future;
    let port = Arc::new(Port {
        acquire_panics: AtomicBool::new(false),
        cleanup_panics: AtomicBool::new(true),
    });
    let book = book(port.clone());
    let first = book.acquire(spec("first")).await.unwrap();
    let second = book.acquire(spec("second")).await.unwrap();
    let mut waiting = Box::pin(book.acquire(spec("waiting")));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(waiting.as_mut().poll(&mut cx).is_pending());
    drop(first);
    assert!(
        !book
            .settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .timed_out
    );
    assert!(matches!(waiting.await, Err(LeaseError::Closed)));
    assert_eq!(book.active_count(), 2);
    assert_eq!(book.releases(), 0);
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert_eq!(book.active_count(), 2);
    // A second backend panic during holder unwind must not double-panic abort.
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = second;
            panic!("holder unwind");
        }))
        .is_err()
    );
    assert_eq!(book.active_count(), 2);
    assert_eq!(
        book.settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .quarantined,
        2
    );
    port.cleanup_panics.store(false, Ordering::SeqCst);
    assert_eq!(book.retry_quarantined(2).await.unwrap(), 2);
    assert_eq!(book.releases(), 2);
    assert_eq!(book.active_count(), 0);
}
