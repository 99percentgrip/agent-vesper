//! Recovery retries require explicit backend support and preserve quarantine.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_swarm::sandbox::{LeaseBook, LeaseError, LeaseSpec, SandboxLeasePort};
struct Backend {
    ready: AtomicBool,
    attempts: AtomicUsize,
}
impl SandboxLeasePort for Backend {
    fn acquire(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        Ok(())
    }
    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        Err(LeaseError::PortRefused {
            worker: spec.worker_id.clone(),
            reason: "cleanup incomplete".into(),
        })
    }
    fn retry_release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.ready.load(Ordering::SeqCst) {
            Ok(())
        } else {
            self.release(spec)
        }
    }
}
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
#[tokio::test]
async fn recovery_preserves_failed_capacity_then_fulfils_queued_waiter_once() {
    use std::future::Future;
    let backend = Arc::new(Backend {
        ready: AtomicBool::new(false),
        attempts: AtomicUsize::new(0),
    });
    let book = book(backend.clone());
    let first = book
        .acquire(LeaseSpec::isolated(
            "first",
            IsolationRequirement::ProcessTree,
        ))
        .await
        .unwrap();
    let mut waiting = Box::pin(book.acquire(LeaseSpec::isolated(
        "second",
        IsolationRequirement::ProcessTree,
    )));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(waiting.as_mut().poll(&mut context).is_pending());
    drop(first);
    assert!(
        !book
            .settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .timed_out
    );
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert_eq!(book.active_count(), 1);
    assert_eq!(book.releases(), 0);
    assert!(waiting.as_mut().poll(&mut context).is_pending());
    backend.ready.store(true, Ordering::SeqCst);
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 1);
    let second = waiting.await.unwrap();
    assert_eq!(book.active_count(), 1);
    assert_eq!(book.acquisitions(), 2);
    assert_eq!(book.releases(), 1);
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert_eq!(backend.attempts.load(Ordering::SeqCst), 2);
    assert!(book.retry_quarantined(4097).await.is_err());
    drop(second);
    assert!(
        !book
            .settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .timed_out
    );
    book.close();
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 1);
    assert_eq!(book.active_count(), 0);
    assert_eq!(book.releases(), 2);
}
struct Unsupported;
impl SandboxLeasePort for Unsupported {
    fn acquire(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        Ok(())
    }
    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        Err(LeaseError::PortRefused {
            worker: spec.worker_id.clone(),
            reason: "failed".into(),
        })
    }
}
#[tokio::test]
async fn unsupported_cleanup_retry_never_frees_quarantine() {
    let book = book(Arc::new(Unsupported));
    drop(
        book.acquire(LeaseSpec::isolated(
            "worker",
            IsolationRequirement::ProcessTree,
        ))
        .await
        .unwrap(),
    );
    assert_eq!(
        book.settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .quarantined,
        1
    );
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert_eq!(book.active_count(), 1);
    assert_eq!(book.releases(), 0);
}

#[tokio::test]
async fn shared_quarantine_reserves_name_until_explicit_success_and_zero_budget_is_noop() {
    let backend = Arc::new(Backend {
        ready: AtomicBool::new(false),
        attempts: AtomicUsize::new(0),
    });
    let book = book(backend.clone());
    let spec = LeaseSpec::shared("a", IsolationRequirement::ProcessTree, "grant", "group");
    drop(book.acquire(spec.clone()).await.unwrap());
    assert!(
        !book
            .settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .timed_out
    );
    assert!(matches!(
        book.acquire(spec.clone()).await,
        Err(LeaseError::WorkerReserved(_))
    ));
    let mut other_member = spec.clone();
    other_member.worker_id = "other".into();
    other_member.write_path = "/w/other".into();
    assert!(matches!(
        book.acquire(other_member).await,
        Err(LeaseError::SharedRefused { .. })
    ));
    assert_eq!(book.retry_quarantined(0).await.unwrap(), 0);
    assert!(book.retry_quarantined(4097).await.is_err());
    assert_eq!(backend.attempts.load(Ordering::SeqCst), 0);
    backend.ready.store(true, Ordering::SeqCst);
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 1);
    let lease = book.acquire(spec).await.unwrap();
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 0);
    assert_eq!(backend.attempts.load(Ordering::SeqCst), 1);
    drop(lease);
    assert!(
        !book
            .settle(std::time::Duration::from_secs(5))
            .await
            .unwrap()
            .timed_out
    );
    book.close();
    assert!(matches!(
        book.acquire(LeaseSpec::isolated(
            "closed",
            IsolationRequirement::ProcessTree
        ))
        .await,
        Err(LeaseError::Closed)
    ));
    assert_eq!(book.retry_quarantined(1).await.unwrap(), 1);
}
