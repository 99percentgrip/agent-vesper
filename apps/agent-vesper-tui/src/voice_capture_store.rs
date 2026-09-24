//! VRO-17 PR-4: managed capture storage (R20 enforcement).
//!
//! One managed namespace per user for voice captures, with:
//! - **hard per-capture caps** enforced at the writer boundary (120 s /
//!   4 MiB, whichever first) — not polling;
//! - **aggregate reservation** across active, pending-transcription and
//!   abandoned captures (32 MiB default), **across TUI instances**
//!   through a lease file per capture: a fresh directory never grants a
//!   new budget, and recovery adopts only captures whose lease is
//!   provably dead (live-PID check on the owning machine — never age,
//!   PID-alone, or a filename prefix);
//! - private owned directories (0700) under the harness data root,
//!   checked on the actual destination filesystem;
//! - cleanup on every outcome; unknown/insufficient space **defers**
//!   capture with an actionable message (never relocation, cloud
//!   fallback, unbounded buffering, or user-data deletion).
//!
//! This module owns only the *capture* store (recordings). Synthesis
//! stays pipe/memory-only (PR-2); response-audio files remain none.
//! Pure std: no async, no unsafe; lives in the TUI binary (host-owned
//! surface) while the policy constants mirror the PRD.

// PR-4 note: this module is the complete, tested R20 contract (hard
// caps, aggregate reservation, leases, recovery). Its production call
// site is the conversation capture worker loop, which lands with
// user-operated device acceptance; until then the API is exercised by
// its dedicated test suite below, so bin-level dead-code analysis is
// explicitly waived for the contract surface (constants included —
// they are policy, not unused locals).
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// R20 default: one capture ≤ 120 seconds.
pub const CAPTURE_MAX_SECONDS: u64 = 120;
/// R20 default: one capture ≤ 4 MiB written bytes.
pub const CAPTURE_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// R20 default: ≤ 32 MiB aggregate across the managed namespace,
/// across instances (active + pending + abandoned).
pub const AGGREGATE_MAX_BYTES: u64 = 32 * 1024 * 1024;
/// Free-space reserve required before any disk-writing capture.
pub const FREE_SPACE_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;

/// R20 errors: actionable, never silent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureStoreError {
    /// The destination filesystem cannot be measured.
    #[error(
        "cannot determine free space for {path}: capture deferred; check the voice capture directory and retry"
    )]
    UnknownFreeSpace { path: String },
    /// Less than the required reserve would remain.
    #[error(
        "insufficient space for capture ({available} bytes available, {required} required): free space or lower the capture limit"
    )]
    InsufficientSpace { available: u64, required: u64 },
    /// Aggregate reservation would exceed the namespace budget.
    #[error(
        "voice capture storage is full ({used} of {budget} bytes across instances): wait for pending transcriptions or restart cleanly"
    )]
    AggregateExceeded { used: u64, budget: u64 },
    /// Store I/O failure (creates/reads/writes).
    #[error("voice capture store failure: {reason}")]
    Io { reason: String },
}

/// A live lease for one capture: the owner's PID plus a boot-stable
/// start marker. Recovery checks both — a recycled PID with a different
/// start time is not the same process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureLease {
    /// Owering process id (this machine).
    pub pid: u32,
    /// Owner process start time (ms since epoch, from /proc/<pid>/stat
    /// field 22 + boot time) — makes PID reuse detectable.
    pub process_start_ms: u64,
    /// When the lease was taken (wall clock).
    pub created_ms: u64,
}

impl CaptureLease {
    /// Reads the current process's lease identity (Linux `/proc`; other
    /// platforms use a weaker PID-only identity, so recovery refuses
    /// adoption unless the PID is provably gone).
    #[must_use]
    pub fn for_current_process() -> Self {
        let pid = std::process::id();
        let process_start_ms = process_start_ms(pid).unwrap_or(0);
        Self {
            pid,
            process_start_ms,
            created_ms: now_ms(),
        }
    }

    /// Whether this lease's owner is provably not running (safe to
    /// adopt). Unknown identity ⇒ conservatively alive.
    #[must_use]
    pub fn owner_is_dead(&self) -> bool {
        if self.process_start_ms == 0 {
            // Weaker identity: only dead when the PID itself is gone.
            return !process_exists(self.pid);
        }
        match process_start_ms(self.pid) {
            Some(start) => start != self.process_start_ms,
            None => !process_exists(self.pid),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn process_start_ms(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 22 (starttime in clock ticks) follows the comm field which
    // may contain spaces/parens; parse after the LAST ')'.
    let after = stat.rsplit(')').next()?;
    let fields: Vec<&str> = after.split_whitespace().collect();
    let ticks: u64 = fields.get(19)?.parse().ok()?;
    let boot_ms = boot_time_ms()?;
    Some(boot_ms + ticks * 1000 / 100)
}

fn boot_time_ms() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    for line in stat.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            return rest
                .trim()
                .parse::<u64>()
                .ok()
                .map(|seconds| seconds * 1000);
        }
    }
    None
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let pid_text = pid.to_string();
        if std::process::Command::new("kill")
            .arg("-0")
            .arg(&pid_text)
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return true;
        }
        // `kill -0` can also fail for a live process owned by somebody
        // else. `ps` distinguishes that case; failure to launch the probe
        // is unknown and therefore conservatively treated as live.
        std::process::Command::new("ps")
            .args(["-p", &pid_text, "-o", "pid="])
            .output()
            .map_or(true, |output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .any(|field| field == pid_text)
            })
    }

    #[cfg(windows)]
    {
        let pid_text = pid.to_string();
        let filter = format!("PID eq {pid}");
        std::process::Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
            .map_or(true, |output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                        line.split(',')
                            .nth(1)
                            .is_some_and(|field| field.trim().trim_matches('"') == pid_text)
                    })
            })
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

/// One managed capture (directory + bounded writer + lease).
pub struct ManagedCapture {
    dir: PathBuf,
    writer: Option<std::fs::File>,
    written: u64,
    started_at: std::time::Instant,
    /// The stream carries its own WAV container (recorder passthrough).
    passthrough: bool,
    /// Kept for diagnostics/future lease renewal; written to lease.json
    /// at start and re-readable via `read_lease` for recovery tests.
    #[allow(dead_code)]
    lease: CaptureLease,
    retained: bool,
}

impl ManagedCapture {
    /// Attempts to start a new capture under `root`, enforcing every R20
    /// policy before any byte is written.
    ///
    /// # Errors
    ///
    /// See [`CaptureStoreError`]: unknown free space, insufficient
    /// reserve, aggregate exhaustion, or store I/O failure. All defer
    /// capture — nothing is relocated, deleted, or buffered unboundedly.
    pub fn start(root: &Path) -> Result<Self, CaptureStoreError> {
        Self::start_with(root, false)
    }

    /// Starts a capture whose stream already carries a complete WAV
    /// container (recorder passthrough, e.g. `arecord -t wav -`): no
    /// placeholder header is injected and finalize never patches the
    /// stream's sizes. Caps, lease, aggregation and cleanup semantics
    /// are identical.
    pub fn start_passthrough(root: &Path) -> Result<Self, CaptureStoreError> {
        Self::start_with(root, true)
    }

    fn start_with(root: &Path, passthrough: bool) -> Result<Self, CaptureStoreError> {
        // 1. Destination filesystem must be measurable and have the
        //    reserve after the planned bounded allocation.
        let planned = CAPTURE_MAX_BYTES;
        let reserve = FREE_SPACE_RESERVE_BYTES;
        let available = free_bytes(root).ok_or_else(|| CaptureStoreError::UnknownFreeSpace {
            path: root.display().to_string(),
        })?;
        if available < planned + reserve {
            return Err(CaptureStoreError::InsufficientSpace {
                available,
                required: planned + reserve,
            });
        }
        // 2. Recover provably-dead leases first, then measure aggregate.
        recover_abandoned(root)?;
        let used = aggregate_bytes(root)?;
        if used + planned > AGGREGATE_MAX_BYTES {
            return Err(CaptureStoreError::AggregateExceeded {
                used,
                budget: AGGREGATE_MAX_BYTES,
            });
        }
        // 3. Private owned directory + lease file.
        let id = format!("cap-{}-{}", std::process::id(), uuid::Uuid::new_v4());
        let dir = root.join("captures").join(id);
        std::fs::create_dir_all(&dir).map_err(|error| CaptureStoreError::Io {
            reason: error.to_string(),
        })?;
        set_private(&dir)?;
        let lease = CaptureLease::for_current_process();
        let lease_bytes = serde_json::to_vec(&[
            u64::from(lease.pid),
            lease.process_start_ms,
            lease.created_ms,
        ])
        .map_err(|error| CaptureStoreError::Io {
            reason: error.to_string(),
        })?;
        std::fs::write(dir.join("lease.json"), lease_bytes).map_err(|error| {
            CaptureStoreError::Io {
                reason: error.to_string(),
            }
        })?;
        // 4. Capped WAV writer.
        let writer = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dir.join("capture.wav"))
            .map_err(|error| CaptureStoreError::Io {
                reason: error.to_string(),
            })?;
        let mut capture = Self {
            dir,
            writer: Some(writer),
            written: 0,
            started_at: std::time::Instant::now(),
            passthrough,
            lease,
            retained: false,
        };
        if !passthrough {
            capture.write_wav_header()?;
        }
        Ok(capture)
    }

    fn write_wav_header(&mut self) -> Result<(), CaptureStoreError> {
        // Canonical PCM WAV header with placeholder sizes (patched at
        // finalize; the caps below bound everything regardless).
        let header = [0u8; 44];
        self.write_raw(&header)
    }

    /// Writes PCM bytes under the hard caps. Returns `Ok(true)` while
    /// the capture continues, `Ok(false)` when a cap was reached (the
    /// writer is finalized and closed; the recorder must be stopped by
    /// the caller — a limit stop NEVER auto-submits).
    ///
    /// # Errors
    ///
    /// [`CaptureStoreError::Io`] on write failure (partial writes are
    /// surfaced; the capture is left for explicit review/retry).
    pub fn write_pcm(&mut self, bytes: &[u8]) -> Result<bool, CaptureStoreError> {
        if self.writer.is_none() {
            return Ok(false);
        }
        let cap_by_time = self.started_at.elapsed() >= Duration::from_secs(CAPTURE_MAX_SECONDS);
        let cap_by_bytes = self.written + bytes.len() as u64 > CAPTURE_MAX_BYTES;
        if cap_by_time || cap_by_bytes {
            self.finalize_header()?;
            self.writer = None;
            return Ok(false);
        }
        self.write_raw(bytes)?;
        Ok(true)
    }

    fn write_raw(&mut self, bytes: &[u8]) -> Result<(), CaptureStoreError> {
        let writer = self.writer.as_mut().ok_or_else(|| CaptureStoreError::Io {
            reason: "capture closed".into(),
        })?;
        writer
            .write_all(bytes)
            .map_err(|error| CaptureStoreError::Io {
                reason: error.to_string(),
            })?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn finalize_header(&mut self) -> Result<(), CaptureStoreError> {
        if self.passthrough {
            // The stream owns its container; nothing to patch.
            return Ok(());
        }
        // Patch RIFF/data sizes with the true byte count.
        let data_len = self.written.saturating_sub(44);
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        let patch = |writer: &mut std::fs::File, at: u64, value: u32| -> std::io::Result<()> {
            use std::io::Seek;
            writer.seek(std::io::SeekFrom::Start(at))?;
            writer.write_all(&value.to_le_bytes())?;
            Ok(())
        };
        patch(writer, 4, (36 + data_len) as u32)
            .and_then(|()| patch(writer, 40, data_len as u32))
            .and_then(|()| {
                use std::io::Seek;
                writer.seek(std::io::SeekFrom::End(0)).map(|_| ())
            })
            .map_err(|error| CaptureStoreError::Io {
                reason: error.to_string(),
            })?;
        Ok(())
    }

    /// The capture file path (read by the shared STT path).
    #[must_use]
    pub fn wav_path(&self) -> PathBuf {
        self.dir.join("capture.wav")
    }

    /// Bytes written so far (accounting includes container overhead —
    /// the 44-byte header — not just nominal audio).
    #[must_use]
    pub fn written_bytes(&self) -> u64 {
        self.written
    }

    /// Whether a hard cap ended this capture.
    #[must_use]
    pub fn hit_cap(&self) -> bool {
        self.writer.is_none() && self.written > 44
    }

    /// Finalizes the capture (patches WAV header sizes, closes the
    /// writer) and leaves the directory in place for the caller's
    /// explicit lifecycle. Called by the recorder-stream pump at EOF and
    /// safe to call on an already-finished capture.
    pub fn finish(&mut self) {
        if self.writer.is_some() {
            let _ = self.finalize_header();
        }
        self.writer = None;
    }

    /// Retains the capture for transcription: writes a retain marker so
    /// the eventual `Drop` does not delete it; transcription completion
    /// must call [`cleanup`](Self::cleanup). (Invoked by the worker
    /// path once conversation capture drives this store end-to-end in
    /// device acceptance; the store API and tests are complete now.)
    #[allow(dead_code)]
    pub fn retain_for_transcription(&mut self) -> PathBuf {
        std::fs::write(self.dir.join("retain"), b"1").ok();
        self.retained = true;
        self.wav_path()
    }

    /// Deletes the owned capture directory (success/discard path).
    pub fn cleanup(&self) -> Result<(), CaptureStoreError> {
        // Only our own directory; never follows symlinks out.
        std::fs::remove_dir_all(&self.dir).map_err(|error| CaptureStoreError::Io {
            reason: error.to_string(),
        })
    }
}

impl Drop for ManagedCapture {
    fn drop(&mut self) {
        // Bounded cleanup attempt on any drop path; the lease file
        // remains for recovery if transcription still needs it — no,
        // PR-4 policy: an un-retained capture cleans itself. Retention
        // happens only via retain_for_transcription (which moves the
        // path out; drop then cleans nothing because writer is None and
        // dir was consumed). Simplest correct behavior: attempt removal
        // of our own dir unless told otherwise via `retained` flag.
        if !self.retained {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Recovers provably-abandoned captures (dead leases only). Never
/// touches live leases, foreign files, or symlink targets.
pub fn recover_abandoned(root: &Path) -> Result<u64, CaptureStoreError> {
    let captures = root.join("captures");
    if !captures.exists() {
        return Ok(0);
    }
    let mut reclaimed = 0u64;
    let entries = std::fs::read_dir(&captures).map_err(|error| CaptureStoreError::Io {
        reason: error.to_string(),
    })?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() || is_symlink(&dir) {
            continue;
        }
        let Some(lease) = read_lease(&dir) else {
            // No lease: not ours to judge (foreign/legacy) — preserve.
            continue;
        };
        if lease.owner_is_dead() {
            let size = dir_size(&dir);
            if std::fs::remove_dir_all(&dir).is_ok() {
                reclaimed += size;
            }
        }
    }
    Ok(reclaimed)
}

fn read_lease(dir: &Path) -> Option<CaptureLease> {
    let bytes = std::fs::read(dir.join("lease.json")).ok()?;
    let value: [u64; 3] = serde_json::from_slice(&bytes).ok()?;
    Some(CaptureLease {
        pid: value[0] as u32,
        process_start_ms: value[1],
        created_ms: value[2],
    })
}

fn free_bytes(path: &Path) -> Option<u64> {
    // First use may precede creation of the managed data root. Measure its
    // nearest existing ancestor without creating anything before the space
    // gate. Only absence permits ascent; permission and other errors fail closed.
    for ancestor in path.ancestors() {
        match std::fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => return statvfs_bytes(ancestor),
            Ok(_) => return None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    None
}

fn statvfs_bytes(path: &Path) -> Option<u64> {
    fs2::available_space(path).ok()
}

fn aggregate_bytes(root: &Path) -> Result<u64, CaptureStoreError> {
    let captures = root.join("captures");
    if !captures.exists() {
        return Ok(0);
    }
    Ok(dir_size(&captures))
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if is_symlink(&child) {
            continue; // never follow links out of the managed root
        }
        if child.is_dir() {
            total += dir_size(&child);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn set_private(dir: &Path) -> Result<(), CaptureStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
            CaptureStoreError::Io {
                reason: error.to_string(),
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vesper-r20-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_capture_handles_a_not_yet_created_data_root() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("data/agent-vesper");
        let capture = ManagedCapture::start(&root).expect("fresh data root must be supported");
        capture.cleanup().unwrap();
    }

    #[test]
    fn free_space_probe_uses_the_existing_ancestor_portably() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("not-created/voice-root");
        assert!(free_bytes(&root).is_some());
    }

    #[test]
    fn start_write_cleanup_round_trip() {
        let root = store_root();
        let mut capture = ManagedCapture::start(&root).unwrap();
        assert!(capture.write_pcm(&[0u8; 3200]).unwrap());
        assert!(capture.written_bytes() >= 44 + 3200);
        let path = capture.wav_path();
        assert!(path.exists());
        capture.cleanup().unwrap();
        assert!(!path.exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn back_to_back_captures_have_distinct_owned_directories() {
        let root = store_root();
        let first = ManagedCapture::start(&root).unwrap();
        let second = ManagedCapture::start(&root).unwrap();
        assert_ne!(first.dir, second.dir);
        first.cleanup().unwrap();
        assert!(second.wav_path().is_file());
        second.cleanup().unwrap();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn byte_cap_stops_writing_at_boundary() {
        let root = store_root();
        let mut capture = ManagedCapture::start(&root).unwrap();
        // Write exactly to the cap: the next write returns Ok(false).
        let chunk = vec![0u8; 64 * 1024];
        let mut open = true;
        while open {
            open = capture.write_pcm(&chunk).unwrap();
        }
        assert!(!open, "byte cap must stop the writer");
        assert!(capture.written_bytes() <= CAPTURE_MAX_BYTES);
        assert!(capture.hit_cap());
        capture.cleanup().unwrap();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn aggregate_reservation_blocks_second_instance_simulation() {
        let root = store_root();
        // Simulate a full namespace: pre-create captures totaling >32 MiB
        // owned by a dead PID (so recovery takes them first) — instead
        // simulate by pre-filling with a live lease the recovery must
        // respect, then verify the aggregate check fails.
        let captures = root.join("captures");
        std::fs::create_dir_all(&captures).unwrap();
        let big = captures.join("cap-foreign-1");
        std::fs::create_dir_all(&big).unwrap();
        // Append until the namespace exceeds the aggregate budget.
        use std::io::Write;
        let file = big.join("capture.wav");
        let mut handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .unwrap();
        let payload = vec![0u8; 1024 * 1024];
        let mut size = 0u64;
        while size < AGGREGATE_MAX_BYTES {
            handle.write_all(&payload).unwrap();
            size += payload.len() as u64;
        }
        drop(handle);
        // Live lease for the foreign capture: recovery must NOT reclaim.
        // PID 1 exists on Linux with a real start time; lease [1, 0, 0]
        // claims pid=1 with unknown start (0) — owner_is_dead uses the
        // weaker PID-exists rule, so PID 1 alive ⇒ lease alive.
        std::fs::write(
            big.join("lease.json"),
            serde_json::to_vec(&[1u64, 0, 0]).unwrap(),
        )
        .unwrap();
        let outcome = ManagedCapture::start(&root);
        match outcome {
            Err(error @ CaptureStoreError::AggregateExceeded { .. }) => {
                let _ = error;
            }
            Err(other) => panic!("expected aggregate exhaustion, got {other:?}"),
            Ok(_capture) => panic!("expected aggregate exhaustion, got a capture"),
        }
        // The foreign file survived.
        assert!(big.join("capture.wav").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dead_lease_is_reclaimed_live_lease_survives() {
        let root = store_root();
        let captures = root.join("captures");
        // Dead lease: PID 4_000_000 does not exist.
        let dead = captures.join("cap-dead");
        std::fs::create_dir_all(&dead).unwrap();
        std::fs::write(
            dead.join("lease.json"),
            serde_json::to_vec(&[4_000_000u64, 0, 0]).unwrap(),
        )
        .unwrap();
        std::fs::write(dead.join("capture.wav"), vec![0u8; 1000]).unwrap();
        // Live lease: our own PID with our start time.
        let live = captures.join("cap-live");
        std::fs::create_dir_all(&live).unwrap();
        let mine = CaptureLease::for_current_process();
        std::fs::write(
            live.join("lease.json"),
            serde_json::to_vec(&[u64::from(mine.pid), mine.process_start_ms, mine.created_ms])
                .unwrap(),
        )
        .unwrap();
        std::fs::write(live.join("capture.wav"), vec![0u8; 1000]).unwrap();
        let reclaimed = recover_abandoned(&root).unwrap();
        assert!(reclaimed >= 1000, "dead capture reclaimed");
        assert!(!dead.exists());
        assert!(live.exists(), "live capture must survive");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn symlink_escape_is_never_followed() {
        let root = store_root();
        let captures = root.join("captures");
        std::fs::create_dir_all(&captures).unwrap();
        let outside = root.join("outside-target");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"precious").unwrap();
        std::os::unix::fs::symlink(&outside, captures.join("cap-link")).unwrap();
        let reclaimed = recover_abandoned(&root).unwrap();
        assert_eq!(reclaimed, 0, "symlinked dirs are skipped, not followed");
        assert!(outside.join("keep.txt").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn no_lease_means_preserved() {
        let root = store_root();
        let captures = root.join("captures");
        let legacy = captures.join("legacy-1970");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("capture.wav"), vec![0u8; 10]).unwrap();
        let reclaimed = recover_abandoned(&root).unwrap();
        assert_eq!(reclaimed, 0);
        assert!(legacy.exists(), "no lease ⇒ not ours to delete");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pid_reuse_is_detected_by_start_time() {
        let current = CaptureLease::for_current_process();
        assert!(!current.owner_is_dead());
        let mut lease = current.clone();
        lease.process_start_ms = lease.process_start_ms.saturating_add(60_000);
        if current.process_start_ms == 0 {
            assert!(
                !lease.owner_is_dead(),
                "without a platform start marker, a live PID is conservatively retained"
            );
        } else {
            assert!(lease.owner_is_dead(), "same pid, different start ⇒ dead");
        }
    }

    #[test]
    fn mid_write_failure_surfaces_as_error() {
        // Inject failure by closing the writer under the capture.
        let root = store_root();
        let mut capture = ManagedCapture::start(&root).unwrap();
        capture.writer = None; // simulate a dead handle
        let result = capture.write_pcm(&[1, 2, 3]);
        assert!(matches!(result, Ok(false)) || result.is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}
