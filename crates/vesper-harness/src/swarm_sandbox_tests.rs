//! Refusal and ownership tests; successful OS isolation is a separate explicit gate.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use vesper_agent::sandbox_route::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};
use vesper_sandbox::{Argv, ExecOutput, SandboxError, SandboxFuture};
use vesper_swarm::worker::CancelFlag;
struct Refusing {
    provisions: AtomicUsize,
    capabilities: SandboxCapabilities,
}
impl SandboxBackend for Refusing {
    fn capabilities(&self) -> SandboxCapabilities {
        self.capabilities.clone()
    }
    fn provision<'a>(
        &'a self,
        _: &'a SandboxSpec,
    ) -> SandboxFuture<'a, Result<SandboxHandle, SandboxError>> {
        Box::pin(async move {
            self.provisions.fetch_add(1, Ordering::SeqCst);
            Err(SandboxError::Provision(
                "fixture refuses provisioning".into(),
            ))
        })
    }
    fn run<'a>(
        &'a self,
        _: &'a SandboxHandle,
        _: &'a Argv,
    ) -> SandboxFuture<'a, Result<ExecOutput, SandboxError>> {
        Box::pin(async { panic!("refused backend must never run") })
    }
    fn teardown<'a>(&'a self, _: SandboxHandle) -> SandboxFuture<'a, Result<(), SandboxError>> {
        Box::pin(async { panic!("fixture has no handles") })
    }
}
fn backend(status: CapabilityStatus) -> Arc<Refusing> {
    Arc::new(Refusing {
        provisions: AtomicUsize::new(0),
        capabilities: SandboxCapabilities {
            backend: "refusing fixture".into(),
            process_tree: status,
            filesystem: status,
            network: status,
            strength: SecurityStrength::None,
        },
    })
}
fn demand() -> SandboxDemand {
    SandboxDemand {
        requirement: IsolationRequirement::Filesystem,
        ..SandboxDemand::none()
    }
}
fn native(root: &Path, port: Arc<Refusing>) -> Arc<NativeSandboxLeases> {
    Arc::new(
        NativeSandboxLeases::new(
            port,
            demand(),
            SandboxBackendChoice::Default,
            1,
            root.canonicalize().unwrap(),
            String::new(),
        )
        .unwrap(),
    )
}
#[test]
fn unavailable_and_missing_grants_refuse_without_creating_worker_state() {
    let root = tempfile::tempdir().unwrap();
    for status in [CapabilityStatus::Unknown, CapabilityStatus::Unavailable] {
        let port = backend(status);
        assert!(
            NativeSandboxLeases::new(
                port.clone(),
                demand(),
                SandboxBackendChoice::Default,
                1,
                root.path().canonicalize().unwrap(),
                String::new()
            )
            .is_err()
        );
        assert_eq!(port.provisions.load(Ordering::SeqCst), 0);
    }
    let mut network = demand();
    network.allow_network = true;
    assert!(
        NativeSandboxLeases::new(
            backend(CapabilityStatus::Available),
            network,
            SandboxBackendChoice::Default,
            1,
            root.path().canonicalize().unwrap(),
            String::new()
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
#[tokio::test]
async fn closed_cancelled_and_invalid_workers_do_not_prepare_or_provision() {
    let root = tempfile::tempdir().unwrap();
    let port = backend(CapabilityStatus::Available);
    let leases = native(root.path(), port.clone());
    let cancel = CancelFlag::new();
    cancel.cancel();
    assert!(leases.worker("driver", 1, cancel.signal()).await.is_err());
    assert!(
        leases
            .worker("../escape", 1, CancelFlag::new().signal())
            .await
            .is_err()
    );
    assert!(leases.shutdown(Duration::ZERO).await.unwrap().is_clean());
    assert!(
        leases
            .worker("driver", 2, CancelFlag::new().signal())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert_eq!(port.provisions.load(Ordering::SeqCst), 0);
}
#[test]
fn unused_preparation_drops_metadata_but_preserves_worker_artifact_directory() {
    let root = tempfile::tempdir().unwrap();
    let port = backend(CapabilityStatus::Available);
    let leases = native(root.path(), port.clone());
    let prepared = leases.prepare("driver-1").unwrap();
    assert_eq!(leases.port.boundaries.lock().unwrap().len(), 1);
    drop(prepared);
    assert!(leases.port.boundaries.lock().unwrap().is_empty());
    assert!(root.path().join("driver-1").is_dir());
    assert_eq!(port.provisions.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn failed_native_provision_keeps_scoped_identity_quarantined() {
    let root = tempfile::tempdir().unwrap();
    let port = backend(CapabilityStatus::Available);
    let leases = native(root.path(), port.clone());
    assert!(matches!(
        leases.worker("driver", 1, CancelFlag::new().signal()).await,
        Err(LeaseError::ProvisioningUncertain { .. })
    ));
    assert_eq!(leases.cleanup_report().quarantined, 1);
    assert_eq!(leases.port.boundaries.lock().unwrap().len(), 1);
    assert_eq!(leases.book.retry_quarantined(1).await.unwrap(), 0);
    assert_eq!(leases.cleanup_report().quarantined, 1);
    assert_eq!(port.provisions.load(Ordering::SeqCst), 1);
    assert!(!leases.shutdown(Duration::ZERO).await.unwrap().is_clean());
}
#[cfg(unix)]
#[tokio::test]
async fn filesystem_alias_refuses_before_provisioning_and_does_not_touch_target() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("canary"), "unchanged").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("driver-1")).unwrap();
    let port = backend(CapabilityStatus::Available);
    let leases = native(root.path(), port.clone());
    assert!(
        leases
            .worker("driver", 1, CancelFlag::new().signal())
            .await
            .is_err()
    );
    assert!(leases.cleanup_report().is_clean());
    assert!(leases.port.boundaries.lock().unwrap().is_empty());
    assert_eq!(port.provisions.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read_to_string(outside.path().join("canary")).unwrap(),
        "unchanged"
    );
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
}
