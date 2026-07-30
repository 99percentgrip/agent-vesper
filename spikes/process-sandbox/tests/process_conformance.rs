#![cfg(unix)]

use std::time::Duration;

use vesper_process_sandbox_spike::{process_group_exists, spawn_supervised, Terminal};

fn child() -> String {
    env!("CARGO_BIN_EXE_fixture-child").to_owned()
}

/// Cross-platform open-file-descriptor count for leak detection.
///
/// Returns `Some(count)` on Linux via `/proc/self/fd` and `None` on other Unix
/// targets (macOS, FreeBSD, etc.) where `/proc` is unavailable. Callers must
/// treat `None` as "assertion not applicable on this platform" rather than a
/// failure, preserving the process-group-membership check for all Unix targets.
#[cfg(target_os = "linux")]
fn fd_count() -> Option<usize> {
    std::fs::read_dir("/proc/self/fd").ok().map(|entries| entries.count())
}

#[cfg(not(target_os = "linux"))]
fn fd_count() -> Option<usize> {
    None
}

async fn wait_group_gone(group: i32) -> bool {
    for _ in 0..100 {
        if !process_group_exists(group) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn direct_child_exits_and_drains() {
    let result = spawn_supervised(child(), "direct", Duration::from_secs(2))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(result.terminal, Terminal::Exited);
    assert_eq!(result.stdout, b"direct-child\n");
    assert!(!result.group_survived);
}

#[tokio::test]
async fn timeout_kills_child_and_grandchild() {
    let result = spawn_supervised(child(), "tree", Duration::from_millis(150))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(result.terminal, Terminal::TimedOut);
    assert!(!result.group_survived);
    assert!(result.kill_to_reap.unwrap() < Duration::from_secs(2));
}

#[tokio::test]
async fn cancellation_kills_descendants_and_has_no_late_output() {
    let run = spawn_supervised(child(), "ticker", Duration::from_secs(5))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(75)).await;
    run.cancel();
    let result = run.wait().await.unwrap();
    assert_eq!(result.terminal, Terminal::Cancelled);
    assert!(!result.group_survived);
    assert_eq!(result.post_termination_output_bytes, 0);
}

#[tokio::test]
async fn pipe_holders_do_not_stall_reaping() {
    for mode in ["hold-stdout", "hold-stderr"] {
        let result = spawn_supervised(child(), mode, Duration::from_millis(150))
            .await
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert_eq!(result.terminal, Terminal::Exited);
        assert!(!result.group_survived);
        assert!(result.kill_to_reap.is_some());
    }
}

#[tokio::test]
async fn ignored_graceful_termination_escalates() {
    let result = spawn_supervised(child(), "ignore-term", Duration::from_millis(150))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(result.terminal, Terminal::TimedOut);
    assert!(!result.group_survived);
    assert!(result.kill_to_reap.unwrap() >= Duration::from_millis(75));
}

#[tokio::test]
async fn huge_output_is_drained_but_capture_is_bounded() {
    let result = spawn_supervised(child(), "huge", Duration::from_secs(5))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(result.terminal, Terminal::Exited);
    assert_eq!(result.stdout.len(), 64 * 1024);
    assert_eq!(result.stderr.len(), 64 * 1024);
    assert_eq!(result.stdout_total, 4 * 1024 * 1024);
    assert_eq!(result.stderr_total, 4 * 1024 * 1024);
}

#[tokio::test]
async fn dropping_owner_triggers_cleanup() {
    let run = spawn_supervised(child(), "tree", Duration::from_secs(30))
        .await
        .unwrap();
    let group = run.process_group;
    drop(run);
    assert!(wait_group_gone(group).await);
}

#[tokio::test]
async fn silent_and_detached_looking_descendants_are_owned() {
    for mode in ["silent", "detached-looking"] {
        let result = spawn_supervised(child(), mode, Duration::from_millis(150))
            .await
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert_eq!(result.terminal, Terminal::TimedOut);
        assert!(!result.group_survived);
    }
}

#[tokio::test]
async fn process_group_membership_and_fd_count_are_stable() {
    let before = fd_count();
    for _ in 0..4 {
        let run = spawn_supervised(child(), "silent", Duration::from_millis(75))
            .await
            .unwrap();
        let actual_group = unsafe { libc::getpgid(run.pid as i32) };
        assert_eq!(actual_group, run.process_group);
        let result = run.wait().await.unwrap();
        assert!(!result.group_survived);
    }
    let after = fd_count();
    // The fd-leak assertion is Linux-only: /proc/self/fd does not exist on
    // macOS or other Unix targets. The process-group-membership check above
    // runs on all Unix platforms via libc::getpgid.
    if let (Some(before), Some(after)) = (before, after) {
        assert!(
            after <= before + 1,
            "file descriptor count grew from {before} to {after}"
        );
    }
}
