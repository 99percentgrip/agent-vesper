//! VRO-13 PR-3 integration tests for `vesper-sandbox`.
//!
//! These exercise the real namespaces backend end-to-end via the
//! `sandbox_init` supervisor binary when the host can provision namespaces,
//! and report explicit skips when it cannot (containers with restricted
//! seccomp, `kernel.unprivileged_userns_clone=0`, etc.). The tests are
//! honest about the environment: they never fake capability output, and
//! they verify the fail-closed stub path in `stub_fail_closed.rs`.

#![cfg(target_os = "linux")]

use std::path::PathBuf;

use vesper_sandbox::{Argv, SandboxSpec, default_backend};
use vesper_security::CapabilityStatus;

/// Locates or builds the `sandbox_init` supervisor and installs it beside
/// this test binary, where the library's sibling lookup finds it. No env
/// mutation and no unsafe code: workspace lints forbid `unsafe` in tests.
fn ensure_supervisor() -> Option<PathBuf> {
    // A pre-set override always wins.
    if let Ok(path) = std::env::var("VESPER_SANDBOX_INIT") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    // Walk up from the test binary (target/debug/deps/) to the target dir
    // and build the supervisor on demand if it is not there yet.
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    let mut target = None;
    for _ in 0..8 {
        let candidate = dir.join("target");
        if candidate.is_dir() {
            target = Some(candidate);
            break;
        }
        if !dir.pop() {
            return None;
        }
    }
    let target = target?;
    if !target.join("debug/sandbox_init").exists() {
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--bin", "sandbox_init", "-p", "vesper-sandbox"])
            .arg("--target-dir")
            .arg(&target)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
    }
    // Place the supervisor beside this test binary so the library's
    // sibling lookup finds it: no env mutation, no unsafe.
    let sibling = exe.parent()?.join("sandbox_init");
    std::fs::copy(target.join("debug/sandbox_init"), &sibling).ok()?;
    sibling.exists().then_some(sibling)
}

/// Whether the current host can actually provision unprivileged namespaces.
fn namespaces_available() -> bool {
    let Some(supervisor) = ensure_supervisor() else {
        return false;
    };
    // The supervisor's own probe is the honest oracle: run it and require
    // exit 0 (every namespace provisioned) before any test proceeds.
    std::process::Command::new(&supervisor)
        .arg("probe")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// A workspace root the sandbox may write to, unique per test.
fn temp_workspace(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vesper-sandbox-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test workspace");
    dir
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

#[test]
fn probe_reports_honest_capabilities() {
    if !namespaces_available() {
        eprintln!("skip: namespace provisioning unavailable on this host");
        return;
    }
    let backend = default_backend();
    let caps = backend.capabilities();
    assert_eq!(caps.backend, "linux-namespaces");
    assert_ne!(caps.filesystem, CapabilityStatus::Unavailable);
    assert_ne!(caps.process_tree, CapabilityStatus::Unavailable);
}

#[test]
fn sandbox_runs_id_as_uid0_inside_user_namespace() {
    if !namespaces_available() {
        eprintln!("skip: namespace provisioning unavailable on this host");
        return;
    }
    let root = temp_workspace("id");
    let backend = default_backend();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("provision sandbox");
    let out = block_on(backend.run(
        &handle,
        &Argv {
            argv: vec!["/bin/sh".into(), "-c".into(), "id -u".into()],
            cwd: root.clone(),
        },
    ))
    .expect("run inside sandbox");
    assert_eq!(out.exit_code, Some(0));
    assert!(
        out.stdout.trim() == "0",
        "payload must run as uid 0 inside the user namespace, got: {}",
        out.stdout
    );
    assert!(!out.timed_out);
    drop(handle);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn readonly_system_trees_are_not_writable() {
    if !namespaces_available() {
        eprintln!("skip: namespace provisioning unavailable on this host");
        return;
    }
    let root = temp_workspace("ro");
    let backend = default_backend();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("provision sandbox");
    let out = block_on(backend.run(
        &handle,
        &Argv {
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "echo x > /usr/vesper-sandbox-ro-check".into(),
            ],
            cwd: root.clone(),
        },
    ))
    .expect("run inside sandbox");
    assert_ne!(
        out.exit_code,
        Some(0),
        "writing to /usr must fail inside the sandbox, exit: {:?}",
        out.exit_code
    );
    drop(handle);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn writable_root_is_writable_inside_the_sandbox() {
    if !namespaces_available() {
        eprintln!("skip: namespace provisioning unavailable on this host");
        return;
    }
    let root = temp_workspace("rw");
    let backend = default_backend();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("provision sandbox");
    let out = block_on(backend.run(
        &handle,
        &Argv {
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "echo vesper > marker.txt && cat marker.txt".into(),
            ],
            cwd: root.clone(),
        },
    ))
    .expect("run inside sandbox");
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stdout.trim(), "vesper");
    // The write must be visible on the host at the real workspace root.
    let marker = root.join("marker.txt");
    assert!(marker.exists(), "workspace write did not reach the host");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn drop_teardown_kills_supervisor_and_leaves_no_processes() {
    if !namespaces_available() {
        eprintln!("skip: namespace provisioning unavailable on this host");
        return;
    }
    let root = temp_workspace("teardown");
    let backend = default_backend();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("provision sandbox");
    let supervisor_pid = handle.pid();
    assert!(supervisor_pid > 1);
    drop(handle);
    // After teardown the supervisor must be gone: no zombies, no orphans.
    let alive = std::process::Command::new("ps")
        .arg("-p")
        .arg(supervisor_pid.to_string())
        .output()
        .expect("ps available");
    assert!(
        !alive.status.success(),
        "supervisor {supervisor_pid} still alive after drop"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn network_is_unreachable_inside_the_sandbox() {
    if !namespaces_available() {
        eprintln!("skip: namespace provisioning unavailable on this host");
        return;
    }
    let root = temp_workspace("net");
    let backend = default_backend();
    let spec = SandboxSpec::new(root.clone());
    let handle = block_on(backend.provision(&spec)).expect("existed sandbox");
    let out = block_on(backend.run(
        &handle,
        &Argv {
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "ip link show 2>/dev/null || ls /sys/class/net".into(),
            ],
            cwd: root.clone(),
        },
    ))
    .expect("run inside sandbox");
    // A fresh network namespace has only the loopback device, which is
    // DOWN until configured. No routed interface means no network access.
    assert!(
        out.stdout.contains("lo") && !out.stdout.contains("eth0"),
        "sandbox network must be loopback-only, got: {}",
        out.stdout
    );
    drop(handle);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "requires real namespaces; unavailable isolation fails acceptance"]
fn namespace_security_and_timeout_acceptance() {
    assert!(namespaces_available(), "real namespace supervisor required");
    let root = temp_workspace("security-timeout");
    let outside = temp_workspace("outside-canary");
    std::fs::write(outside.join("canary"), "untouched").unwrap();
    let backend = default_backend();
    let mut spec = SandboxSpec::new(root.clone());
    spec.timeout_seconds = 1;
    let handle = block_on(backend.provision(&spec)).unwrap();
    let command = format!(
        "printf own > result; test ! -e '{}/canary'; test ! -d /home; test ! -d /root; test \"$(awk '/CapEff:/ {{print $2}}' /proc/self/status)\" = 0000000000000000; test \"$(awk '/CapBnd:/ {{print $2}}' /proc/self/status)\" = 0000000000000000; printf confined",
        outside.display()
    );
    let result = block_on(backend.run(
        &handle,
        &Argv {
            cwd: root.clone(),
            argv: vec!["/bin/sh".into(), "-ec".into(), command],
        },
    ))
    .unwrap();
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    assert!(result.stdout.contains("confined"));
    assert_eq!(std::fs::read_to_string(root.join("result")).unwrap(), "own");
    assert_eq!(
        std::fs::read_to_string(outside.join("canary")).unwrap(),
        "untouched"
    );
    block_on(backend.teardown(handle)).unwrap();

    let handle = block_on(backend.provision(&spec)).unwrap();
    let started = std::time::Instant::now();
    let result = block_on(backend.run(
        &handle,
        &Argv {
            cwd: root.clone(),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "dd if=/dev/zero bs=8192 count=32 >&2; printf visible; sleep 30 & wait".into(),
            ],
        },
    ))
    .unwrap();
    assert!(result.timed_out, "{result:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(result.stdout.contains("visible"));
    assert!(result.stderr.len() < 66000);
    let pid = handle.pid();
    block_on(backend.teardown(handle)).unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}
