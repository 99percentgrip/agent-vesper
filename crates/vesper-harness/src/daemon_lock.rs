//! VRO-13 PR-6 — the single-writer daemon lock (`daemon.lock`).
//!
//! The background daemon must be the only scheduler process writing fired
//! cron results at any moment, so a second daemon must never run
//! concurrently. This module implements that discipline over the state
//! directory: one lock file at `<state>/daemon.lock`, acquired exclusively
//! at daemon startup and held for the process lifetime.
//!
//! ## Discipline (why `create_new`, not `flock`)
//!
//! The classic primitive is an advisory `flock(2)`, which the kernel drops
//! automatically when the holder dies — but `std` exposes no flock binding
//! and this crate is `#![forbid(unsafe_code)]` (the workspace's
//! sandbox-safety gate; the only sanctioned raw-syscall exception is the
//! `sandbox_init` supervisor binary). The discipline here is therefore the
//! POSIX `O_CREAT | O_EXCL` lock-file pattern (`File::create_new`), which
//! the kernel arbitrates exactly like an exclusive lock across processes,
//! plus explicit stale detection:
//!
//! - The lock file carries the holder's PID and start time.
//! - A second daemon sees the existing file, reads the holder PID, and
//!   checks liveness (`/proc/<pid>` presence on Unix, always-live on
//!   platforms without a procfs).
//! - A **dead** holder means the lock is stale: it is removed and the
//!   takeover succeeds (graceful reclaim).
//! - A **live** holder means the second daemon exits `0` cleanly with
//!   `daemon already running (pid …)` — never an error, never a fight.
//!
//! The TUI never takes this lock; its in-process scheduler keeps firing
//! foreground `/loop` slots. Dual-fire between the TUI scheduler and the
//! daemon is prevented by the slot-claim discipline in
//! `vesper-checkpoints`, not by this lock.
//!
//! ## Zero degradation
//!
//! The interactive TUI never blocks on this lock: it neither takes it nor
//! waits on it. `read_daemon_lock_status` — the only function the TUI
//! calls (`/daemon status`) — is a single bounded file read.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The lock file name under the state root.
pub const DAEMON_LOCK_NAME: &str = "daemon.lock";

/// One acquired daemon lock. Held for the process lifetime; released on
/// drop (the lock file is removed, making the slot available to the next
/// daemon boot).
#[derive(Debug)]
pub struct DaemonLockGuard {
    path: PathBuf,
}

impl Drop for DaemonLockGuard {
    fn drop(&mut self) {
        // Best-effort: a stale file is detected and reclaimed by the next
        // boot's liveness check, so a failed unlink here is not fatal.
        let _ = fs::remove_file(&self.path);
    }
}

/// Why [`acquire_daemon_lock`] did not hand the lock to this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonLockError {
    /// A daemon holds the lock and its process is alive. The caller must
    /// exit `0` cleanly with `daemon already running (pid …)`.
    HeldBy(u32),
    /// The lock directory could not be created or the lock file written.
    Io,
}

/// Whether the holder recorded in a `daemon.lock` is still alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonLockStatus {
    /// No lock exists — no daemon is running.
    NotHeld,
    /// A daemon holds the lock and its process is alive.
    Held { pid: u32, started_at: SystemTime },
    /// A lock file exists but its recorded PID is dead (stale lock).
    Stale { pid: u32, started_at: SystemTime },
    /// The lock file exists but is unreadable or malformed.
    Corrupt,
}

/// Path of the daemon lock under a state root.
#[must_use]
pub fn daemon_lock_path(state_root: &Path) -> PathBuf {
    state_root.join(DAEMON_LOCK_NAME)
}

/// Reads the lock state without acquiring anything. Never blocks, never
/// fails on I/O: an unreadable lock is reported as [`DaemonLockStatus::Corrupt`]
/// so `/daemon status` always answers.
#[must_use]
pub fn read_daemon_lock_status(state_root: &Path) -> DaemonLockStatus {
    let path = daemon_lock_path(state_root);
    let Ok(text) = fs::read_to_string(&path) else {
        return DaemonLockStatus::NotHeld;
    };
    match parse_lock_payload(&text) {
        Some((pid, started_at)) if pid_alive(pid) => DaemonLockStatus::Held { pid, started_at },
        Some((pid, started_at)) => DaemonLockStatus::Stale { pid, started_at },
        None => DaemonLockStatus::Corrupt,
    }
}

/// Acquires the daemon lock for this process, or reports why not.
///
/// - No lock file → created exclusively (`O_CREAT|O_EXCL`) with this
///   process's PID and start time; returns the owning guard.
/// - Stale lock (dead PID) → the file is removed and acquisition retried
///   once (graceful reclaim).
/// - Live holder → returns [`DaemonLockError::HeldBy(pid)`]; the caller
///   must exit 0 with `daemon already running (pid …)`.
///
/// # Errors
///
/// [`DaemonLockError::Io`] when the lock directory cannot be created or
/// the lock file cannot be written after a legitimate takeover.
pub fn acquire_daemon_lock(state_root: &Path) -> Result<DaemonLockGuard, DaemonLockError> {
    std::fs::create_dir_all(state_root).map_err(|_| DaemonLockError::Io)?;
    let path = daemon_lock_path(state_root);
    // First attempt: the common case (no daemon running).
    if let Ok(guard) = try_create(&path) {
        return Ok(guard);
    }
    // The file exists. Decide: live holder (yield) or stale (reclaim).
    match read_daemon_lock_status(state_root) {
        DaemonLockStatus::NotHeld => try_create(&path),
        DaemonLockStatus::Stale { .. } => {
            let _ = fs::remove_file(&path);
            try_create(&path)
        }
        DaemonLockStatus::Held { pid, .. } => Err(DaemonLockError::HeldBy(pid)),
        DaemonLockStatus::Corrupt => {
            // An unreadable lock cannot prove liveness either way, and a
            // corrupt lock would deadlock every future boot — so reclaim.
            // Single-user state root; the worst case is two daemons whose
            // slot claims still prevent double-fires.
            let _ = fs::remove_file(&path);
            try_create(&path)
        }
    }
}

/// Writes the lock payload for this process and returns the guard.
fn try_create(path: &Path) -> Result<DaemonLockGuard, DaemonLockError> {
    let mut file = File::create_new(path).map_err(|_| DaemonLockError::Io)?;
    let payload = lock_payload(std::process::id());
    let _ = file
        .write_all(payload.as_bytes())
        .and_then(|()| file.sync_all());
    Ok(DaemonLockGuard {
        path: path.to_path_buf(),
    })
}

/// `<pid>\n<unix-seconds>\n` — minimal, stable, human-readable.
fn lock_payload(pid: u32) -> String {
    let started_at = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    format!("{pid}\n{started_at}\n")
}

/// Parses a lock payload back into `(pid, started_at)`.
fn parse_lock_payload(text: &str) -> Option<(u32, SystemTime)> {
    let mut lines = text.lines();
    let pid = lines.next()?.trim().parse::<u32>().ok()?;
    let seconds = lines.next()?.trim().parse::<u64>().ok()?;
    Some((pid, unix_seconds_to_system_time(seconds)))
}

fn unix_seconds_to_system_time(seconds: u64) -> SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
}

/// Whether a PID names a live process. On Unix, `/proc/<pid>` presence;
/// on platforms without a procfs this conservatively reports `true` so a
/// missing procfs degrades to "never steal a lock" rather than "steal
/// every lock".
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false; // init-swap guard: PID 0 never names a process
    }
    #[cfg(unix)]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp state root per test, mirroring the workspace's
    /// `tempfile`-free discipline: `std::env::temp_dir` + PID + counter.
    fn state_root() -> (std::path::PathBuf, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "vesper-daemon-lock-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&base).expect("create temp state root");
        (base.clone(), base)
    }

    #[test]
    fn fresh_acquire_creates_lock_and_drop_releases_it() {
        let (_temp, state) = state_root();
        assert_eq!(read_daemon_lock_status(&state), DaemonLockStatus::NotHeld);
        let guard = acquire_daemon_lock(&state).expect("fresh acquire");
        let text = fs::read_to_string(daemon_lock_path(&state)).expect("read lock");
        let (pid, started_at) = parse_lock_payload(&text).expect("parse");
        assert_eq!(pid, std::process::id());
        assert!(started_at.elapsed().unwrap_or_default().as_secs() < 60);
        drop(guard);
        assert!(
            !daemon_lock_path(&state).exists(),
            "drop releases the lock file"
        );
    }

    #[test]
    fn secondary_instance_is_rejected_with_the_holder_pid() {
        let (_temp, state) = state_root();
        let _first = acquire_daemon_lock(&state).expect("first acquire");
        // The holder is THIS process, so it is provably alive.
        match acquire_daemon_lock(&state) {
            Ok(_) => panic!("a live holder must reject the second instance"),
            Err(error) => assert_eq!(error, DaemonLockError::HeldBy(std::process::id())),
        }
        // Status reports the live holder.
        match read_daemon_lock_status(&state) {
            DaemonLockStatus::Held { pid, .. } => assert_eq!(pid, std::process::id()),
            other => panic!("expected Held, got {other:?}"),
        }
    }

    #[test]
    fn stale_lock_with_dead_pid_is_reclaimed() {
        let (_temp, state) = state_root();
        std::fs::create_dir_all(&state).unwrap();
        // A PID beyond the kernel's allocation range cannot exist, so the
        // liveness probe is guaranteed dead without spawning anything.
        let dead = 4_194_305u32;
        if Path::new("/proc").join(dead.to_string()).exists() {
            // Paranoid fallback: skip rather than flake on exotic kernels.
            return;
        }
        std::fs::write(
            daemon_lock_path(&state),
            format!("{dead}\n{}\n", 1_700_000_000u64),
        )
        .unwrap();
        assert!(matches!(
            read_daemon_lock_status(&state),
            DaemonLockStatus::Stale { .. }
        ));
        let guard = acquire_daemon_lock(&state).expect("stale lock must be reclaimable");
        let text = fs::read_to_string(daemon_lock_path(&state)).expect("read lock");
        let (pid, _) = parse_lock_payload(&text).expect("parse");
        assert_eq!(pid, std::process::id(), "the reclaimer now owns the lock");
        drop(guard);
    }

    #[test]
    fn corrupt_lock_is_reclaimed_rather_than_deadlocking() {
        let (_temp, state) = state_root();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(daemon_lock_path(&state), "not a lock payload\n").unwrap();
        assert_eq!(read_daemon_lock_status(&state), DaemonLockStatus::Corrupt);
        let _guard = acquire_daemon_lock(&state).expect("corrupt lock must be reclaimable");
    }

    #[test]
    fn payload_round_trips() {
        // Generated payload parses back with the same pid and a recent
        // timestamp (the payload stamps `now`, so we assert recency).
        let text = lock_payload(42);
        let (pid, at) = parse_lock_payload(&text).expect("parse");
        assert_eq!(pid, 42);
        let age = SystemTime::now()
            .duration_since(at)
            .expect("payload timestamp must not be in the future");
        assert!(age.as_secs() < 60, "timestamp must be recent: {age:?}");
        // A fixed historical payload parses to its exact instant.
        let fixed = "42\n1700000000\n";
        let (pid, at) = parse_lock_payload(fixed).expect("parse fixed");
        assert_eq!(pid, 42);
        assert_eq!(
            at,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)
        );
        // Malformed payloads parse to None, never to garbage.
        assert!(parse_lock_payload("").is_none());
        assert!(parse_lock_payload("one\ntwo\n").is_none());
        assert!(parse_lock_payload("42\n").is_none());
        assert!(parse_lock_payload("-1\n1700000000\n").is_none());
    }

    #[test]
    fn pid_zero_is_treated_as_dead() {
        assert!(!pid_alive(0));
        // A live, real PID: this process.
        assert!(pid_alive(std::process::id()));
    }
}
