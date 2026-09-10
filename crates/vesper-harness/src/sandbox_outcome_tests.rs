//! Offline outcome arbitration: cleanup failure must never become tool success.
use super::*;
use vesper_sandbox::{ExecOutput, SandboxError};
fn output() -> Result<ExecOutput, SandboxError> {
    Ok(ExecOutput {
        exit_code: Some(0),
        stdout: "visible stdout".into(),
        stderr: "visible stderr".into(),
        timed_out: false,
    })
}
#[test]
fn failed_teardown_overrides_success_and_retains_output() {
    let result = finish_run(
        output(),
        Err(SandboxError::Teardown("descendant still alive".into())),
        false,
    );
    let error = result.unwrap_err().to_string();
    assert!(error.contains("descendant still alive"));
    assert!(error.contains("visible stdout"));
    assert!(error.contains("visible stderr"));
}
#[test]
fn simultaneous_run_and_teardown_errors_are_both_reported() {
    let error = finish_run(
        Err(SandboxError::Run("execution failed".into())),
        Err(SandboxError::Teardown("cleanup failed".into())),
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("execution failed"));
    assert!(error.contains("cleanup failed"));
}
#[test]
fn cleanup_failure_is_not_hidden_by_cancellation() {
    let error = finish_run(
        output(),
        Err(SandboxError::Teardown("cleanup failed".into())),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cleanup failed"));
}
#[test]
fn late_cancel_never_reports_success() {
    assert!(matches!(
        finish_run(output(), Ok(()), true),
        Err(SandboxRunError::Cancelled)
    ));
}
#[test]
fn successful_cleanup_preserves_normal_output_and_timeout() {
    let result = finish_run(output(), Ok(()), false).unwrap();
    assert_eq!(result.output, "visible stdout\n[stderr]\nvisible stderr");
    assert!(!result.timed_out);
    let mut timed = output().unwrap();
    timed.timed_out = true;
    assert!(finish_run(Ok(timed), Ok(()), false).unwrap().timed_out);
}

#[test]
fn quarantined_route_refuses_before_backend_provisioning() {
    struct NeverCancelled;
    impl vesper_agent::CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    let port = BackendPort::new(
        Arc::new(vesper_sandbox::UnavailableBackend),
        SandboxDemand::none(),
    );
    port.cleanup_unverified
        .store(true, std::sync::atomic::Ordering::Release);
    let cancellation: Arc<dyn vesper_agent::CancellationSignal> = Arc::new(NeverCancelled);
    let error = port
        .run_command("must not execute", Path::new("."), 1, &cancellation)
        .unwrap_err()
        .to_string();
    assert!(error.contains("route quarantined"));
    assert!(!error.contains("provision failed"));
    assert!(
        port.cleanup_unverified
            .load(std::sync::atomic::Ordering::Acquire)
    );
}
