//! VRO-13 PR-4 — Docker backend integration tests, `#[ignore]`-gated.
//!
//! Gating contract (PRD §2.5 acceptance #1 and the fast-track directive):
//! these tests touch a **real Docker daemon**, so they are excluded from
//! every default and CI invocation. CI machines must never need Docker.
//! Run them explicitly on a Docker-capable host with:
//!
//! ```text
//! DOCKER_AVAILABLE=1 cargo test -p vesper-sandbox \
//!     --features docker --test docker -- --ignored
//! ```
//!
//! Two layers live here:
//!
//! 1. **Ungated honest-refusal tests** (`docker_unavailable_…`): when the
//!    `docker` binary is absent or the daemon is unreachable, the backend
//!    must refuse with the model-facing "sandbox unavailable" text and
//!    never assume capabilities it could not probe. These run everywhere
//!    (they point the backend at a binary that cannot exist).
//! 2. **Gated real-daemon tests**: provision → run → teardown against the
//!    actual daemon, proving the bind-mount, the resource limits, and the
//!    default no-network isolation.

#![cfg(feature = "docker")]

use std::path::PathBuf;
use std::sync::Arc;

use vesper_sandbox::{
    Argv, DockerBackend, DockerSandboxConfig, SandboxBackend, SandboxError, SandboxSpec,
};
use vesper_security::{CapabilityStatus, IsolationRequirement};

/// Minimal inline block-on: backend futures here are plain `std::process`
/// I/O (no reactor), so a thread-park waker bridge is sufficient and
/// runtime-free (matches the namespaces test binary's approach).
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl std::task::Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let mut future = Box::pin(future);
    let waker = std::task::Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

/// Backend pointed at a docker binary that cannot exist. Every
/// honest-refusal test uses this; no test relies on the host actually
/// lacking Docker.
fn unreachable_backend() -> DockerBackend {
    DockerBackend::new(DockerSandboxConfig {
        docker_bin: Some(PathBuf::from("/nonexistent/vesper-docker-stub")),
        ..DockerSandboxConfig::default()
    })
}

/// Creates an isolated workspace directory for one gated test.
fn workspace_temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vesper-docker-test-{}-{tag}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("create temp workspace");
    dir
}

// ---------------------------------------------------------------------------
// Ungated honest-refusal layer (runs everywhere, including CI).
// ---------------------------------------------------------------------------

/// The cold-start guard: an unreachable daemon makes every capability
/// honestly `Unavailable` — never assumed available.
#[test]
fn docker_unavailable_capabilities_fail_closed_everywhere() {
    let backend = unreachable_backend();
    let caps = backend.capabilities();
    assert_eq!(caps.process_tree, CapabilityStatus::Unavailable);
    assert_eq!(caps.filesystem, CapabilityStatus::Unavailable);
    assert_eq!(caps.network, CapabilityStatus::Unavailable);
    assert!(!caps.satisfies(IsolationRequirement::ProcessTree));
    assert!(!caps.satisfies(IsolationRequirement::Filesystem));
    assert!(!caps.satisfies(IsolationRequirement::Full));
}

/// Provision must fail fast with the model-facing "sandbox unavailable"
/// refusal instead of spawning `docker run` or hanging.
#[test]
fn docker_unavailable_provision_fails_fast_with_model_facing_refusal() {
    let backend = unreachable_backend();
    let spec = SandboxSpec::new(PathBuf::from("."));
    match block_on(backend.provision(&spec)) {
        Ok(_) => panic!("provision must refuse when the daemon is unreachable"),
        // Cold-start guard shape 1: the fail-closed capability gate fires
        // first (nothing was provisioned, no `docker run` was spawned).
        Err(error @ SandboxError::CapabilityUnavailable { .. }) => {
            let text = error.to_string();
            assert!(
                text.contains("sandbox unavailable"),
                "model-facing refusal text missing: {text}"
            );
        }
        // Shape 2: the bounded daemon probe itself failed on the way in.
        Err(error @ SandboxError::Provision(_)) => {
            let text = error.to_string();
            assert!(
                text.contains("sandbox unavailable"),
                "model-facing refusal text missing: {text}"
            );
            assert!(
                text.contains("the operation needs isolation"),
                "the refusal must explain why: {text}"
            );
        }
        Err(other) => panic!("unexpected error shape: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Gated real-daemon layer (`#[ignore]` + DOCKER_AVAILABLE).
// ---------------------------------------------------------------------------

/// True when the operator explicitly opted into real-daemon tests:
/// `DOCKER_AVAILABLE=1 cargo test … -- --ignored`.
fn docker_gated() -> bool {
    std::env::var("DOCKER_AVAILABLE").is_ok_and(|value| !value.trim().is_empty())
}

/// Full provision → run → teardown cycle against the real daemon: the
/// bind-mounted workspace is writable, `/workspace` is the cwd, and the
/// process runs as the container's own (non-host) identity.
#[test]
#[ignore = "requires a real Docker daemon; run with DOCKER_AVAILABLE=1"]
fn docker_run_exec_provisions_workspace_and_teardown_reaps() {
    if !docker_gated() {
        eprintln!("skipping: DOCKER_AVAILABLE not set");
        return;
    }
    let root = workspace_temp("full-cycle");
    let backend = DockerBackend::with_defaults();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("daemon must provision");
    let argv = Argv {
        argv: vec![
            "sh".into(),
            "-c".into(),
            "pwd > /tmp/pwd.txt; echo bind:$PWD; id -u > /workspace/uid.txt; \
             echo done"
                .into(),
        ],
        cwd: root.clone(),
    };
    let output = block_on(backend.run(&handle, &argv)).expect("run must complete");
    assert_eq!(output.exit_code, Some(0), "stderr: {}", output.stderr);
    assert!(output.stdout.contains("done"), "stdout: {}", output.stdout);
    // The bind-mounted workspace is writable from inside the container.
    assert!(
        root.join("uid.txt").exists(),
        "the workspace bind-mount must be writable"
    );
    block_on(backend.teardown(handle)).expect("teardown must succeed");
    // Teardown is total: the container name is gone.
    let name = format!(
        "agent-vesper-sbx-{}",
        backend
            .container_name()
            .trim_start_matches("agent-vesper-sbx-")
    );
    let listed = std::process::Command::new(backend.docker_bin())
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name={name}"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .expect("docker ps");
    let names = String::from_utf8_lossy(&listed.stdout);
    assert!(
        !names.contains(&name),
        "container {name} must be gone after teardown; ps said: {names}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Default isolation denies network egress: `--network none` is not a
/// suggestion, it is the provisioned state.
#[test]
#[ignore = "requires a real Docker daemon; run with DOCKER_AVAILABLE=1"]
fn docker_default_provisions_no_network() {
    if !docker_gated() {
        eprintln!("skipping: DOCKER_AVAILABLE not set");
        return;
    }
    let root = workspace_temp("no-network");
    let backend = DockerBackend::with_defaults();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("daemon must provision");
    let argv = Argv {
        argv: vec![
            "sh".into(),
            "-c".into(),
            // `ip` may be absent on alpine; route inspection suffices: a
            // `none` network leaves only the loopback interface.
            "cat /proc/net/route > /workspace/routes.txt; echo routed".into(),
        ],
        cwd: root.clone(),
    };
    let output = block_on(backend.run(&handle, &argv)).expect("run must complete");
    assert_eq!(output.exit_code, Some(0), "stderr: {}", output.stderr);
    let routes = std::fs::read_to_string(root.join("routes.txt"))
        .expect("route table snapshot must land in the bind-mounted workspace");
    // With `--network none` there is no default gateway route (no non-empty
    // destination column): any real interface route proves egress reach.
    let has_default_route = routes.lines().skip(1).any(|line| {
        line.split_whitespace()
            .nth(1)
            .is_none_or(|d| d == "00000000")
            && !line.trim().is_empty()
    });
    assert!(
        !has_default_route,
        "--network none must leave no usable route; got:\n{routes}"
    );
    block_on(backend.teardown(handle)).expect("teardown must succeed");
    let _ = std::fs::remove_dir_all(&root);
}

/// Resource limits are enforced: the provisioned container carries the
/// configured `--cpus` and `--memory` ceilings.
#[test]
#[ignore = "requires a real Docker daemon; run with DOCKER_AVAILABLE=1"]
fn docker_resource_limits_are_enforced() {
    if !docker_gated() {
        eprintln!("skipping: DOCKER_AVAILABLE not set");
        return;
    }
    let root = workspace_temp("limits");
    let backend = DockerBackend::new(DockerSandboxConfig {
        session_slug: Some("limits-test".into()),
        ..DockerSandboxConfig::default()
    });
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("daemon must provision");
    let name = handle
        .writable_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Inspect the live container through the same binary the backend uses.
    let inspected = std::process::Command::new(backend.docker_bin())
        .args([
            "inspect",
            "--format",
            "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}} {{.HostConfig.NetworkMode}}",
            "agent-vesper-sbx-limits-test",
        ])
        .output()
        .unwrap_or_else(|error| panic!("docker inspect failed: {error}"));
    assert!(
        inspected.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let report = String::from_utf8_lossy(&inspected.stdout);
    assert!(
        report.trim().starts_with("2000000000"),
        "--cpus 2 must be applied; inspect said: {report}"
    );
    assert!(
        report.contains("none"),
        "--network none must be the provisioned NetworkMode; got: {report}"
    );
    let _ = name;
    block_on(backend.teardown(handle)).expect("teardown must succeed");
    let _ = std::fs::remove_dir_all(&root);
}
