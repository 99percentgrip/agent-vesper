#![cfg(target_os = "linux")]

use std::process::Command;

use tempfile::tempdir;
use vesper_process_sandbox_spike::require_bubblewrap;

#[test]
fn required_mode_refuses_missing_bubblewrap() {
    let error = require_bubblewrap("/definitely/missing/bwrap").unwrap_err();
    assert!(error
        .to_string()
        .contains("required Bubblewrap unavailable"));
}

#[test]
fn bubblewrap_pid_and_network_namespaces_work() {
    require_bubblewrap("bwrap").unwrap();
    let status = Command::new("bwrap")
        .args([
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "/bin/sh",
            "-c",
            "test $$ -eq 2 && test \"$(awk -F: 'NR>2 {gsub(/ /,\"\",$1); print $1}' /proc/net/dev)\" = lo",
        ])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn bubblewrap_limits_workspace_write_scope() {
    require_bubblewrap("bwrap").unwrap();
    let root = tempdir().unwrap();
    let writable = root.path().join("workspace");
    let denied = root.path().join("outside");
    std::fs::create_dir_all(&writable).unwrap();
    std::fs::create_dir_all(&denied).unwrap();
    let status = Command::new("bwrap")
        .args(["--unshare-pid", "--unshare-net", "--ro-bind", "/", "/"])
        .arg("--bind")
        .arg(&writable)
        .arg(&writable)
        .args(["--dev", "/dev", "--proc", "/proc", "/bin/sh", "-c"])
        .arg(format!(
            "touch '{}/allowed' && ! touch '{}/denied'",
            writable.display(),
            denied.display()
        ))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(writable.join("allowed").exists());
    assert!(!denied.join("denied").exists());
}
