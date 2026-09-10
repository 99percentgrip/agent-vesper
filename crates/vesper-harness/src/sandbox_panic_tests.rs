//! Unwinding backend failures are refusals, not permission to reuse a route.
use super::*;
use vesper_sandbox::{ExecOutput, SandboxError, SandboxFuture, SandboxHandle};

struct PanickingProvision(bool);
impl SandboxBackend for PanickingProvision {
    fn capabilities(&self) -> vesper_agent::sandbox_route::SandboxCapabilities {
        vesper_sandbox::UnavailableBackend.capabilities()
    }
    fn provision<'a>(
        &'a self,
        _: &'a SandboxSpec,
    ) -> SandboxFuture<'a, Result<SandboxHandle, SandboxError>> {
        assert!(!self.0, "private-construction-canary");
        Box::pin(async { panic!("private-poll-canary") })
    }
    fn run<'a>(
        &'a self,
        _: &'a SandboxHandle,
        _: &'a Argv,
    ) -> SandboxFuture<'a, Result<ExecOutput, SandboxError>> {
        panic!("must not run")
    }
    fn teardown<'a>(&'a self, _: SandboxHandle) -> SandboxFuture<'a, Result<(), SandboxError>> {
        panic!("no handle was returned")
    }
}
struct NeverCancelled;
impl vesper_agent::CancellationSignal for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}
#[test]
fn construction_and_poll_panics_quarantine_before_another_provision() {
    let cancellation: Arc<dyn vesper_agent::CancellationSignal> = Arc::new(NeverCancelled);
    for immediate in [false, true] {
        let port = BackendPort::new(
            Arc::new(PanickingProvision(immediate)),
            SandboxDemand::none(),
        );
        let error = port
            .run_command("unused", Path::new("."), 1, &cancellation)
            .unwrap_err()
            .to_string();
        assert!(error.contains("panicked during provision"));
        assert!(!error.contains("canary"));
        let next = port
            .run_command("unused", Path::new("."), 1, &cancellation)
            .unwrap_err()
            .to_string();
        assert!(next.contains("route quarantined"));
        assert!(!next.contains("provision failed"));
    }
}
#[test]
fn run_and_cleanup_panic_boundaries_return_errors_without_payloads() {
    for phase in ["run", "teardown"] {
        let port = BackendPort::new(
            Arc::new(vesper_sandbox::UnavailableBackend),
            SandboxDemand::none(),
        );
        let result: Result<(), SandboxError> =
            port.backend_call(phase, || panic!("private-canary"));
        let message = result.unwrap_err().to_string();
        assert!(message.contains(phase));
        assert!(!message.contains("private-canary"));
        assert!(
            port.cleanup_unverified
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }
}
#[test]
fn ordinary_provision_refusal_does_not_quarantine() {
    let port = BackendPort::new(
        Arc::new(vesper_sandbox::UnavailableBackend),
        SandboxDemand::none(),
    );
    let cancellation: Arc<dyn vesper_agent::CancellationSignal> = Arc::new(NeverCancelled);
    assert!(
        port.run_command("unused", Path::new("."), 1, &cancellation)
            .is_err()
    );
    assert!(
        !port
            .cleanup_unverified
            .load(std::sync::atomic::Ordering::Acquire)
    );
}
