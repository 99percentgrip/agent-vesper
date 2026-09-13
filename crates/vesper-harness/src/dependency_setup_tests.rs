use super::*;

#[test]
fn runtime_preferences_are_bounded_and_never_created_by_reads() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("runtime.json");
    assert_eq!(read_engine(&path).unwrap(), None);
    assert!(!path.exists());
    for invalid in [
        "{}".to_owned(),
        "x".repeat(4097),
        r#"{"binary":"relative","connection":null,"managed_machine":false}"#.into(),
        r#"{"binary":"/bin/podman","connection":"--url=evil","managed_machine":true}"#.into(),
        r#"{"binary":"/bin/podman","connection":"someone-else","managed_machine":true}"#.into(),
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert!(read_engine(&path).is_err());
    }
}

#[test]
fn linux_distribution_selection_never_executes_os_release_text() {
    for id in ["ubuntu", "debian", "fedora"] {
        assert!(linux_package(&format!("ID=\"{id}\"\nPRETTY_NAME=ignored")).is_ok());
    }
    for body in [
        "ID=unknown",
        "ID=$(touch canary)",
        "ID=arch\nID_LIKE=debian",
        "",
    ] {
        assert!(linux_package(body).is_err());
    }
}

#[cfg(unix)]
fn fixture() -> (tempfile::TempDir, Engine) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("os"), "linux\n").unwrap();
    std::fs::write(root.path().join("endpoint"), "unix:///run/docker.sock\n").unwrap();
    let binary = root.path().join("engine");
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/dependency_cli_fixture.sh"),
        &binary,
    )
    .unwrap();
    (
        root,
        Engine {
            binary,
            connection: None,
            managed_machine: false,
            verified: false,
        },
    )
}

#[cfg(unix)]
#[tokio::test]
async fn engine_health_requires_a_responding_linux_runtime() {
    let (root, engine) = fixture();
    health(&engine).await.unwrap();
    std::fs::write(root.path().join("os"), "windows\n").unwrap();
    assert!(health(&engine).await.is_err());
    std::fs::write(root.path().join("unhealthy"), "").unwrap();
    assert!(health(&engine).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn browser_probe_cleans_up_after_failure_and_never_hides_cleanup_failure() {
    let (root, mut engine) = fixture();
    engine.connection = Some(MACHINE.into());
    verify_browser(&engine, "sha256:fixture").await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("calls")).unwrap();
    assert!(calls.contains("--connection agent-vesper run --pull=never"));
    assert!(calls.contains("--network none --read-only --cap-drop=ALL"));
    assert!(calls.contains("--connection agent-vesper rm -f vesper-setup-"));
    assert!(!calls.contains("--mount"));
    std::fs::write(root.path().join("browser-failed"), "").unwrap();
    assert!(verify_browser(&engine, "sha256:fixture").await.is_err());
    let calls = std::fs::read_to_string(root.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|l| l.contains(" rm -f ")).count(), 2);
    std::fs::remove_file(root.path().join("browser-failed")).unwrap();
    std::fs::write(root.path().join("cleanup-failed"), "").unwrap();
    assert!(
        verify_browser(&engine, "sha256:fixture")
            .await
            .unwrap_err()
            .contains("cleanup")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn setup_process_timeout_is_bounded() {
    let (_root, engine) = fixture();
    let mut command = engine.command();
    command.arg("hang");
    let started = std::time::Instant::now();
    assert!(run(command, 1).await.unwrap_err().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[cfg(unix)]
#[tokio::test]
async fn an_unhealthy_first_engine_does_not_mask_a_healthy_alternative() {
    let (first, first_engine) = fixture();
    let (_second, second_engine) = fixture();
    std::fs::write(first.path().join("unhealthy"), "").unwrap();
    assert_eq!(
        select_healthy(&[first_engine, second_engine.clone()]).await,
        Some(second_engine)
    );
}

#[tokio::test]
#[ignore = "explicit contained runtime acceptance; requires isolated engine storage and a pinned image"]
async fn real_dependency_browser_readiness() {
    let binary = std::env::var_os("VESPER_DEPENDENCY_TEST_ENGINE")
        .expect("explicit test engine")
        .into();
    let image = std::env::var("VESPER_DEPENDENCY_TEST_IMAGE").expect("explicit pinned image");
    assert!(vesper_config::is_digest_pinned_image(&image));
    let engine = Engine {
        binary,
        connection: None,
        managed_machine: false,
        verified: false,
    };
    health(&engine).await.unwrap();
    verify_browser(&engine, &image).await.unwrap();
}

#[tokio::test]
async fn cancellation_before_setup_has_no_io_or_progress() {
    assert!(
        setup_cancellable(|_| panic!("no progress after cancellation"), || true)
            .await
            .unwrap_err()
            .contains("stopped")
    );
}

#[test]
fn corrupt_or_missing_download_never_passes_verification() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("package");
    assert!(verify_package(&path, "unused").is_err());
    std::fs::write(&path, b"abc").unwrap();
    verify_package(
        &path,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    )
    .unwrap();
    std::fs::write(&path, b"altered").unwrap();
    assert!(
        verify_package(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        )
        .unwrap_err()
        .contains("checksum")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn managed_machine_resume_is_idempotent_and_runtime_never_initializes() {
    let (root, mut engine) = fixture();
    engine.connection = Some(MACHINE.into());
    engine.managed_machine = true;
    std::fs::write(root.path().join("unhealthy"), "").unwrap();
    start_machine(&engine).await.unwrap();
    start_machine(&engine).await.unwrap();
    let before = std::fs::read_to_string(root.path().join("calls")).unwrap();
    assert_eq!(
        before
            .lines()
            .filter(|line| line.contains("machine init"))
            .count(),
        1
    );
    assert_eq!(
        before
            .lines()
            .filter(|line| line.contains("machine start"))
            .count(),
        1
    );
    assert!(!before.contains("machine rm"));
    engine.verified = true;
    std::fs::write(root.path().join("unhealthy"), "").unwrap();
    ensure_engine(&engine).await.unwrap();
    let after = std::fs::read_to_string(root.path().join("calls")).unwrap();
    assert_eq!(
        after
            .lines()
            .filter(|line| line.contains("machine init"))
            .count(),
        1
    );
    assert_eq!(
        after
            .lines()
            .filter(|line| line.contains("machine start --update-connection=false agent-vesper"))
            .count(),
        2
    );
    engine.verified = false;
    std::fs::write(root.path().join("unhealthy"), "").unwrap();
    assert!(ensure_engine(&engine).await.is_err());
}
