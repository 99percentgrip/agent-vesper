//! Explicit cleanup reports errors; local child fixtures do not claim namespace isolation.
use super::*;
fn handle(command: Option<Vec<String>>) -> SandboxHandle {
    let child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    SandboxHandle {
        child: Mutex::new(child),
        teardown_command: command,
        stdout: Mutex::new(None),
        writable_root: std::env::temp_dir(),
        timeout_seconds: 1,
    }
}
#[tokio::test]
async fn namespace_explicit_teardown_refuses_poisoned_process_ownership() {
    let handle = handle(None);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = handle.child.lock().unwrap();
        panic!("fixture lock poisoning");
    }));
    // Keep the test process bounded even before the production fix: terminate
    // this fixture while retaining poison as the ownership failure under test.
    {
        let mut child = handle.child.lock().unwrap_err().into_inner();
        child.kill().unwrap();
        child.wait().unwrap();
    }
    let result = LinuxNamespacesBackend::new().teardown(handle).await;
    assert!(matches!(result, Err(SandboxError::Teardown(_))));
}
#[test]
fn explicit_supervisor_cleanup_reaps_and_is_idempotent() {
    let handle = handle(None);
    handle.terminate_supervisor().unwrap();
    assert!(handle.child.lock().unwrap().try_wait().unwrap().is_some());
    handle.terminate_supervisor().unwrap();
}
#[cfg(feature = "docker")]
#[tokio::test]
async fn docker_nonzero_cleanup_is_not_verified_success() {
    let backend = DockerBackend::new(Default::default());
    for command in [
        Some(vec!["sh".into(), "-c".into(), "exit 7".into()]),
        Some(Vec::new()),
        None,
    ] {
        assert!(matches!(
            backend.teardown(handle(command)).await,
            Err(SandboxError::Teardown(_))
        ));
    }
}
#[cfg(feature = "docker")]
#[tokio::test]
async fn docker_success_and_missing_cleanup_executable_are_distinct() {
    let backend = DockerBackend::new(Default::default());
    backend
        .teardown(handle(Some(vec![
            "sh".into(),
            "-c".into(),
            "exit 0".into(),
        ])))
        .await
        .unwrap();
    let result = backend
        .teardown(handle(Some(vec![
            "/vesper-nonexistent-test-cleanup".into(),
        ])))
        .await;
    assert!(matches!(result, Err(SandboxError::Teardown(_))));
}
