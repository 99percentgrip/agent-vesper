//! A worker identity owns at most one active, quarantined or queued lease.
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};
use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_swarm::sandbox::{LeaseBook, LeaseError, LeaseSpec, SandboxLeasePort};
#[derive(Default)]
struct Port {
    acquisitions: AtomicUsize,
    fail_release: bool,
}
impl SandboxLeasePort for Port {
    fn acquire(&self, _: &LeaseSpec) -> Result<(), LeaseError> {
        self.acquisitions.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        if self.fail_release {
            Err(LeaseError::PortRefused {
                worker: spec.worker_id.clone(),
                reason: "still alive".into(),
            })
        } else {
            Ok(())
        }
    }
}
fn book(capacity: usize, port: Arc<Port>) -> LeaseBook {
    LeaseBook::new(
        capacity,
        IsolationRequirement::None,
        SandboxCapabilities {
            backend: "test".into(),
            process_tree: CapabilityStatus::Available,
            filesystem: CapabilityStatus::Available,
            network: CapabilityStatus::Available,
            strength: SecurityStrength::Full,
        },
        port,
    )
    .unwrap()
}
fn isolated(id: &str) -> LeaseSpec {
    LeaseSpec::isolated(id, IsolationRequirement::None)
}
#[tokio::test]
async fn duplicate_active_worker_refuses_even_with_capacity_or_disjoint_shared_paths() {
    for shared in [false, true] {
        let port = Arc::new(Port::default());
        let book = book(2, port.clone());
        let spec = if shared {
            LeaseSpec::shared("same", IsolationRequirement::None, "", "group")
        } else {
            isolated("same")
        };
        let first = book.acquire(spec.clone()).await.unwrap();
        let mut duplicate = spec.clone();
        duplicate.write_path = "/w/different".into();
        assert!(book.acquire(duplicate).await.is_err());
        assert_eq!(port.acquisitions.load(Ordering::SeqCst), 1);
        assert_eq!(book.active_count(), 1);
        drop(first);
        assert!(
            book.settle(std::time::Duration::from_secs(5))
                .await
                .unwrap()
                .is_clean()
        );
        drop(book.acquire(spec).await.unwrap());
    }
}
#[tokio::test]
async fn duplicate_queued_identity_refuses_immediately_and_drop_frees_reservation() {
    let port = Arc::new(Port::default());
    let book = book(1, port.clone());
    let first = book.acquire(isolated("first")).await.unwrap();
    let mut queued = Box::pin(book.acquire(isolated("queued")));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(queued.as_mut().poll(&mut cx).is_pending());
    let mut duplicate = Box::pin(book.acquire(isolated("queued")));
    assert!(matches!(
        duplicate.as_mut().poll(&mut cx),
        Poll::Ready(Err(_))
    ));
    assert_eq!(book.queued_count(), 1);
    assert_eq!(port.acquisitions.load(Ordering::SeqCst), 1);
    drop(queued);
    drop(duplicate);
    assert_eq!(book.queued_count(), 0);
    drop(first);
    drop(book.acquire(isolated("queued")).await.unwrap());
}
#[tokio::test]
async fn quarantined_origin_identity_cannot_open_a_second_boundary() {
    let port = Arc::new(Port {
        acquisitions: AtomicUsize::new(0),
        fail_release: true,
    });
    let book = book(2, port.clone());
    drop(book.acquire(isolated("same")).await.unwrap());
    assert!(book.acquire(isolated("same")).await.is_err());
    assert_eq!(book.active_count(), 1);
    assert_eq!(port.acquisitions.load(Ordering::SeqCst), 1);
}
