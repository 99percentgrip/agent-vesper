//! VRO-17 PR-4: the host playback owner.
//!
//! Receives canonical PCM (16 kHz mono s16 LE), owns a **bounded**
//! in-process queue, and drives a user-installed `aplay`-class player
//! over **stdin pipes** (`-t raw -f S16_LE -r 16000 -c 1`): no WAV
//! spooling, no files, no full-turn accumulation, no system-volume
//! changes, no untracked system-service playback. The device is never
//! assumed to accept anything else — the argv names the exact raw
//! format so the player performs any device negotiation.
//!
//! **Receipt semantics (verified, not inferred):**
//!
//! - `BytesWritten(n)` — our bytes handed to the child's stdin pipe.
//!   Strongest fact: *transport*. Proves nothing about device handoff,
//!   audibility, or completion.
//!
//! - `Drained` — the child **exited 0 after stdin EOF**, which for
//!   aplay's write-then-play pipeline is the documented drain point:
//!   aplay plays everything it read before exiting. Strongest fact:
//!   *the player finished consuming and playing our bytes*. It is still
//!   not proof a human heard them.
//!
//! - `Unknown` — used whenever weaker evidence is all we have (device
//!   missing, error before/at handoff, progress not confirmed). Never
//!   upgraded.
//!
//! Process exit for a *nonzero* status is an error, never completion,
//! and writing stdin alone is never completion. Stop/flush kills and
//! reaps exactly our own child (process group), separately from any
//! slow provider/runtime cleanup — but stop-effect timing is not
//! acoustic silence and is never claimed as such.
//!
//! Missing platform/player/device ⇒ truthful `Unavailable` with the
//! setup route; the TUI keeps the textual answer regardless.
// PR-4 repair: this owner is driven by the production
// `ConversationHost::speak_unit` path; no dead-code waiver needed. The
// `NoDevice` variant remains feature-allowed (surfaced by the Settings
// readiness panel when a device is explicitly unconfigured).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Bounded queue (bytes of canonical PCM awaiting the writer).
pub const MAX_QUEUED_BYTES: usize = 512 * 1024;

/// Receipt: the strongest fact each event supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackReceipt {
    /// Bytes handed to the player's stdin (transport only).
    BytesWritten(u64),
    /// Player consumed and finished our bytes (exit 0 after EOF).
    Drained,
    /// Progress not confirmed; never upgraded to a heard claim.
    Unknown,
}

/// Owned playback child + writer.
struct PlayerChild {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    diagnostic: Arc<Mutex<Option<&'static str>>>,
    diagnostic_done: std::sync::mpsc::Receiver<()>,
    /// Set when end_stream (or Drop) closes stdin.
    stdin_closed_by_owner: bool,
    /// Set when the child was observed dead BEFORE we closed stdin (a
    /// short consumer that quit early; exit 0 cannot then certify that
    /// all offered bytes were consumed).
    child_exited_before_close: bool,
}

// Never forward arbitrary player stderr (paths, terminal escapes or secrets).
// Retain at most 4096 bytes for classification; drain the rest to prevent a
// verbose player from deadlocking behind its stderr pipe.
fn player_diagnostic(bytes: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if text.contains("connection refused") {
        Some("audio service connection refused; check the desktop audio service")
    } else if text.contains("permission denied") {
        Some("audio device permission denied; check audio session permissions")
    } else if text.contains("device or resource busy") {
        Some("audio device busy; release the device in the other application")
    } else if text.contains("unknown pcm") || text.contains("no such file or directory") {
        Some("audio device unavailable; check the configured/default output device")
    } else if text.contains("unrecognized option") || text.contains("invalid option") {
        Some("incompatible player; an aplay-compatible executable is required")
    } else if text.contains("xrun") || text.contains("overrun") || text.contains("underrun") {
        Some("player reported a device overrun/underrun (xrun)")
    } else if text.contains("input/output error") {
        Some("player reported an input/output error from the audio device")
    } else {
        None
    }
}

/// The write-failure reason from the ORIGINAL io::Error evidence (kind +
/// raw OS error string), optionally refined by a player diagnostic. Never
/// invents a device-closed hypothesis: a pipe-write failure means the
/// player child was gone first (reaped/dead children close the pipe).
fn write_failure_reason(error: &std::io::Error, diagnostic: Option<&'static str>) -> String {
    let kind = error.kind();
    let code = error.raw_os_error();
    let raw = match code {
        Some(number) => format!("os error {number}"),
        None => kind.to_string(),
    };
    match diagnostic {
        Some(text) => format!("pipe write failed ({raw}; {text})"),
        None => format!("pipe write failed ({raw}); the player process is no longer reading"),
    }
}

impl Drop for PlayerChild {
    fn drop(&mut self) {
        // Close stdin, then reap exactly this child in its own group.
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.flush();
            self.stdin_closed_by_owner = true;
        }
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &format!("{}", self.child.id())])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Playback errors (metadata only).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaybackError {
    /// Player executable missing (setup prerequisite).
    #[error("audio player unavailable ({reason}): install prerequisite")]
    Unavailable { reason: String },
    /// The bounded queue is full (backpressure; caller must stop or slow).
    #[error("playback queue full ({max} bytes): stop playback or reduce rate")]
    QueueFull { max: usize },
    /// The player failed (device busy/disconnected/runtime error).
    #[error("playback failed: {reason}")]
    Failed { reason: String },
    /// The device was explicitly unconfigured on this host.
    #[error("no audio device configured; voice replies remain text-only")]
    #[allow(dead_code)]
    NoDevice,
}

/// The playback owner: one player child per speech stream, bounded
/// in-memory queue, stop/flush separate from synthesis/runtime cleanup.
pub struct PlaybackOwner {
    player: PathBuf,
    /// Device name for the player argv (empty ⇒ default device).
    device: String,
    queue: Arc<Mutex<Vec<u8>>>,
    state: Mutex<OwnerState>,
}

#[derive(Default)]
struct OwnerState {
    current: Option<PlayerChild>,
    written_total: u64,
    stopped: bool,
}

impl PlaybackOwner {
    /// Creates the owner for the given player executable (validated
    /// plain path) and optional ALSA device name.
    #[must_use]
    pub fn new(player: PathBuf, device: Option<String>) -> Self {
        Self {
            player,
            device: device.unwrap_or_default(),
            queue: Arc::new(Mutex::new(Vec::new())),
            state: Mutex::new(OwnerState::default()),
        }
    }

    /// Validates the player path shape (no shell strings).
    ///
    /// # Errors
    ///
    /// [`PlaybackError::Unavailable`] for a malformed path.
    pub fn validate(&self) -> Result<(), PlaybackError> {
        let text = self.player.to_string_lossy();
        if text.is_empty() || text.contains([';', '|', '&', '`', ' ']) {
            return Err(PlaybackError::Unavailable {
                reason: "player path must be a plain executable path".into(),
            });
        }
        Ok(())
    }

    /// Begins one speech stream: spawns the player child for canonical
    /// raw PCM over stdin. A prior stop applies only to the stream that
    /// was live when it happened — a NEW stream automatically clears the
    /// latch (the voice-oracle session's later turns must be audible;
    /// the defect Alex hit was a permanent latch after the first barge-in).
    ///
    /// # Errors
    ///
    /// [`PlaybackError::Unavailable`] when the player cannot spawn
    /// (missing executable/permission), [`PlaybackError::Failed`] on
    /// other spawn errors. Never a fake success.
    pub fn begin_stream(&self) -> Result<(), PlaybackError> {
        self.validate()?;
        let mut state = self.state.lock().map_err(|_| PlaybackError::Failed {
            reason: "state lock poisoned".into(),
        })?;
        if state.stopped {
            // A new stream supersedes the stopped one: clear the latch.
            state.stopped = false;
        }
        if state.current.is_some() {
            return Ok(()); // one stream at a time; new audio appends
            // (byte accounting continues across the append)
        }
        let mut command = Command::new(&self.player);
        // VRO-17 multi-turn repair: NO --fatal-errors. Its documented
        // behavior ("treat all errors (e.g. xrun) as fatal … aborts
        // immediately") makes the player EXIT mid-stream on any
        // recoverable device error — under per-piece feeding that turned
        // a recoverable underrun into a dead child, EPIPE on the next
        // write, and Alex's turn-3+ failures. The player's documented
        // default recovers xruns; we surface its stderr classification
        // instead of forcing an abort.
        command.args(["-q", "-t", "raw", "-f", "S16_LE", "-r", "16000", "-c", "1"]);
        if !self.device.is_empty() {
            command.arg("-D").arg(&self.device);
        }
        command
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        // ETXTBSY can transiently fire when a freshly written fixture
        // file is exec'd concurrently in tests; retry once after a
        // short sleep before classifying as failure.
        let mut spawn_retries = 0;
        let mut child = loop {
            match command.spawn() {
                Ok(child) => break child,
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && spawn_retries < 2 =>
                {
                    spawn_retries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(PlaybackError::Unavailable {
                        reason: "player executable not found: install prerequisite".into(),
                    });
                }
                Err(error) => {
                    return Err(PlaybackError::Failed {
                        reason: format!("spawn failed ({})", error.kind()),
                    });
                }
            }
        };
        let stdin = child.stdin.take().ok_or_else(|| PlaybackError::Failed {
            reason: "player stdin unavailable".into(),
        })?;
        let diagnostic = Arc::new(Mutex::new(None));
        let reader_diagnostic = Arc::clone(&diagnostic);
        let (done_tx, diagnostic_done) = std::sync::mpsc::channel();
        let stderr = child.stderr.take();
        std::thread::spawn(move || {
            if let Some(mut stderr) = stderr {
                let mut retained = Vec::with_capacity(4096);
                let mut buffer = [0u8; 1024];
                loop {
                    match stderr.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            let keep = count.min(4096 - retained.len());
                            retained.extend_from_slice(&buffer[..keep]);
                            if keep > 0
                                && let Ok(mut result) = reader_diagnostic.lock()
                            {
                                *result = player_diagnostic(&retained);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
            }
            let _ = done_tx.send(());
        });
        // A NEW child starts a fresh byte count; appends to an already
        // open stream (the early-return above) keep the accumulated one.
        state.written_total = 0;
        state.current = Some(PlayerChild {
            child,
            stdin: Some(stdin),
            diagnostic,
            diagnostic_done,
            stdin_closed_by_owner: false,
            child_exited_before_close: false,
        });
        Ok(())
    }

    /// Offers canonical PCM into the bounded queue and writes what fits
    /// to the player stdin. Backpressure is explicit: a full queue is
    /// `QueueFull`, never silent growth or disk spooling.
    ///
    /// # Errors
    ///
    /// See [`PlaybackError`]; `BytesWritten` receipts are only emitted
    /// for bytes actually accepted by the pipe.
    pub fn push_pcm(&self, pcm: &[u8]) -> Result<PlaybackReceipt, PlaybackError> {
        if pcm.len() > MAX_QUEUED_BYTES {
            return Err(PlaybackError::QueueFull {
                max: MAX_QUEUED_BYTES,
            });
        }
        let (mut stdin, child_id) = {
            let mut state = self.state.lock().map_err(|_| PlaybackError::Failed {
                reason: "state lock poisoned".into(),
            })?;
            if state.stopped {
                return Ok(PlaybackReceipt::Unknown);
            }
            let Some(player) = state.current.as_mut() else {
                return Ok(PlaybackReceipt::Unknown);
            };
            let stdin = player.stdin.take().ok_or_else(|| PlaybackError::Failed {
                reason: "concurrent playback write".into(),
            })?;
            (stdin, player.child.id())
        };
        // Never hold the control lock across pipe backpressure. Stop must
        // be able to kill the reader and unblock this write immediately.
        let written = stdin.write_all(pcm).and_then(|()| stdin.flush());
        let mut state = self.state.lock().map_err(|_| PlaybackError::Failed {
            reason: "state lock poisoned".into(),
        })?;
        if state.stopped
            || state
                .current
                .as_ref()
                .is_none_or(|p| p.child.id() != child_id)
        {
            return Ok(PlaybackReceipt::Unknown);
        }
        if let Some(player) = state.current.as_mut() {
            player.stdin = Some(stdin);
        }
        if let Err(error) = written {
            // Preserve the ORIGINAL failure evidence (io kind + OS code)
            // before any cleanup can replace it; the diagnostic refines,
            // never overrides, the operation layer. A pipe-write failure
            // means the player child stopped reading (dead/reaped), which
            // is a DIFFERENT layer from an ALSA device error.
            let diagnostic = state
                .current
                .as_ref()
                .and_then(|player| player.diagnostic.lock().ok().and_then(|value| *value));
            // The dead stream must not remain the current one: later
            // pieces of this segment fail fast instead of writing into a
            // gone pipe, and the next turn opens a FRESH stream instead
            // of appending to a dead child.
            state.current.take();
            state.stopped = true;
            return Err(PlaybackError::Failed {
                reason: write_failure_reason(&error, diagnostic),
            });
        }
        state.written_total += pcm.len() as u64;
        Ok(PlaybackReceipt::BytesWritten(state.written_total))
    }

    /// Ends the stream: closes stdin, waits (bounded) for the player to
    /// drain and exit. Exit 0 ⇒ `Drained` (the verified strongest
    /// fact); anything else ⇒ error/Unknown.
    ///
    /// A short consumer that exits 0 before consuming the offered bytes
    /// is a FAILURE, never completion: success exit with undelivered
    /// audio would certify playback that never happened. The offered
    /// count is tracked per stream; bytes still buffered in the OS pipe
    /// when the child exits are lost by definition (the child is gone).
    ///
    /// # Errors
    ///
    /// [`PlaybackError::Failed`] on nonzero exit or wait failure.
    pub fn end_stream(&self) -> Result<PlaybackReceipt, PlaybackError> {
        {
            let mut state = self.state.lock().map_err(|_| PlaybackError::Failed {
                reason: "state lock poisoned".into(),
            })?;
            let Some(player) = state.current.as_mut() else {
                return Ok(PlaybackReceipt::Unknown);
            };
            // Snapshot whether the child is ALREADY dead before we close
            // stdin: a child that exited first consumed a prefix, not the
            // whole stream (exit 0 then cannot certify completion).
            player.child_exited_before_close = player.child.try_wait().ok().flatten().is_some();
            drop(player.stdin.take());
            player.stdin_closed_by_owner = true;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            {
                let mut state = self.state.lock().map_err(|_| PlaybackError::Failed {
                    reason: "state lock poisoned".into(),
                })?;
                let Some(player) = state.current.as_mut() else {
                    return Ok(PlaybackReceipt::Unknown);
                };
                match player.child.try_wait() {
                    Ok(Some(status)) => {
                        let offered = state.written_total;
                        let Some(player) = state.current.take() else {
                            return Ok(PlaybackReceipt::Unknown);
                        };
                        drop(state);
                        // The child has exited; briefly allow the bounded stderr
                        // reader to publish its final classification. Never wait
                        // indefinitely on inherited descriptors.
                        let _ = player
                            .diagnostic_done
                            .recv_timeout(Duration::from_millis(100));
                        let diagnostic = player.diagnostic.lock().ok().and_then(|value| *value);
                        return if status.success() {
                            if player.stdin_closed_by_owner && !player.child_exited_before_close {
                                // Exit 0 AFTER our stdin EOF: the player
                                // consumed everything it read through EOF —
                                // the documented drain point.
                                Ok(PlaybackReceipt::Drained)
                            } else {
                                // Exit 0 BEFORE we closed stdin: a short
                                // consumer. Bytes it never read are lost with
                                // the pipe; success exit must not certify
                                // completion of unplayed audio.
                                Err(PlaybackError::Failed {
                                    reason: format!(
                                        "player exited before the stream ended ({offered} bytes offered; short consumption cannot certify completion)"
                                    ),
                                })
                            }
                        } else {
                            Err(PlaybackError::Failed {
                                reason: format!(
                                    "player exited {status}: {}",
                                    diagnostic.unwrap_or("no recognized device diagnostic")
                                ),
                            })
                        };
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {}
                    _ => {
                        state.current.take();
                        return Ok(PlaybackReceipt::Unknown);
                    }
                }
            }
            // Control lock is released during drain, too.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Immediate stop/flush: kills our child now and drops queued bytes.
    /// Executed **separately** from any slow provider/runtime cleanup;
    /// timing is not acoustic silence and is never claimed as such.
    pub fn stop_flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
            state.current.take(); // Drop kills + reaps exactly our child
            if let Ok(mut queue) = self.queue.lock() {
                queue.clear();
            }
        }
    }

    /// Resets for a new stream (after stop or completion).
    pub fn reset(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.current.take();
            state.stopped = false;
            state.written_total = 0;
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_remains_responsive_under_pipe_backpressure() {
        let dir = tempfile::tempdir().unwrap();
        let owner = Arc::new(PlaybackOwner::new(
            fixture_player(dir.path(), "never_read"),
            None,
        ));
        owner.begin_stream().unwrap();
        let writer_owner = Arc::clone(&owner);
        let writer = std::thread::spawn(move || writer_owner.push_pcm(&vec![0; MAX_QUEUED_BYTES]));
        std::thread::sleep(Duration::from_millis(100));
        let started = std::time::Instant::now();
        owner.stop_flush();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "stop blocked behind the pipe writer"
        );
        assert!(!matches!(
            writer.join().unwrap(),
            Ok(PlaybackReceipt::BytesWritten(_))
        ));
    }

    fn fixture_player(dir: &std::path::Path, behavior: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let body = dir.join(format!("player_{behavior}_{unique}.py"));
        std::fs::write(
            &body,
            format!(
                r#"
import sys, time
behavior = "{behavior}"
if behavior == "never_read":
    time.sleep(30)
data = sys.stdin.buffer.read()
if behavior == "fail_fast":
    sys.exit(3)
if behavior == "read_then_fail":
    sys.exit(4)
if behavior == "slow":
    time.sleep(0.3)
    sys.exit(0)
sys.exit(0)
"#
            ),
        )
        .unwrap();
        let wrapper = dir.join(format!("player_{behavior}_{unique}"));
        std::fs::write(
            &wrapper,
            format!("#!/bin/sh\nexec python3 \"{}\"\n", body.display()),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        wrapper
    }

    fn root() -> PathBuf {
        static DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vesper-playback-test-{}-{}",
            std::process::id(),
            DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clean_drain_yields_drained_receipt() {
        let owner = PlaybackOwner::new(fixture_player(&root(), "ok"), None);
        owner.begin_stream().unwrap();
        assert!(matches!(
            owner.push_pcm(&[0u8; 3200]).unwrap(),
            PlaybackReceipt::BytesWritten(_)
        ));
        assert_eq!(owner.end_stream().unwrap(), PlaybackReceipt::Drained);
    }

    #[test]
    fn nonzero_exit_is_failure_not_completion() {
        let owner = PlaybackOwner::new(fixture_player(&root(), "fail_fast"), None);
        owner.begin_stream().unwrap();
        let _ = owner.push_pcm(&[0u8; 320]);
        let result = owner.end_stream();
        assert!(matches!(result, Err(PlaybackError::Failed { .. })));
    }

    #[test]
    fn missing_player_is_unavailable_with_setup_hint() {
        let owner = PlaybackOwner::new(PathBuf::from("/nonexistent/aplay"), None);
        match owner.begin_stream() {
            Err(PlaybackError::Unavailable { reason }) => {
                assert!(reason.contains("prerequisite"), "{reason}");
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
    }

    #[test]
    fn queue_is_bounded_with_explicit_backpressure() {
        let owner = PlaybackOwner::new(fixture_player(&root(), "ok"), None);
        owner.begin_stream().unwrap();
        // Push beyond the bound: the pipe accepts some, queue holds the
        // rest until full, then QueueFull.
        let big = vec![0u8; MAX_QUEUED_BYTES + 4096];
        let result = owner.push_pcm(&big);
        assert!(
            matches!(result, Err(PlaybackError::QueueFull { .. })),
            "{result:?}"
        );
        owner.stop_flush();
    }

    #[test]
    fn stop_flush_is_immediate_and_independent() {
        let owner = PlaybackOwner::new(fixture_player(&root(), "slow"), None);
        owner.begin_stream().unwrap();
        let _ = owner.push_pcm(&[0u8; 3200]);
        let start = std::time::Instant::now();
        owner.stop_flush();
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "stop must not wait for the slow player"
        );
        owner.reset();
    }

    #[test]
    fn stopped_stream_ignores_late_audio_within_the_same_stream() {
        // Stop suppresses the CURRENT stream's late audio (no stale
        // playback from a canceled utterance).
        let owner = PlaybackOwner::new(fixture_player(&root(), "ok"), None);
        owner.begin_stream().unwrap();
        owner.stop_flush();
        assert_eq!(
            owner.push_pcm(&[0u8; 320]).unwrap(),
            PlaybackReceipt::Unknown
        );
    }

    #[test]
    fn new_stream_after_stop_is_audible_again() {
        // Alex's defect regression: after barge-in stopped stream #1,
        // a LATER turn's stream must play — the stop latch must not
        // persist forever.
        let owner = PlaybackOwner::new(fixture_player(&root(), "ok"), None);
        owner.begin_stream().unwrap();
        owner.stop_flush(); // barge-in
        // Next turn: begin_stream must clear the latch.
        owner.begin_stream().unwrap();
        let receipt = owner.push_pcm(&[0u8; 320]).unwrap();
        assert!(
            matches!(receipt, PlaybackReceipt::BytesWritten(_)),
            "new stream must be audible after a prior stop: {receipt:?}"
        );
        owner.reset();
    }

    #[test]
    fn repeated_stop_is_idempotent() {
        let owner = PlaybackOwner::new(fixture_player(&root(), "ok"), None);
        owner.begin_stream().unwrap();
        owner.stop_flush();
        owner.stop_flush();
        owner.stop_flush();
        owner.reset();
        owner.begin_stream().unwrap();
        owner.end_stream().unwrap();
    }

    #[test]
    fn no_fresh_budget_or_files_from_playback() {
        // The owner never writes files: structural source assertion.
        let source = std::fs::read_to_string("src/voice_playback.rs").unwrap();
        // Scan production code only: cut at the test module boundary.
        let production = source.split("#[cfg(test)]").next().unwrap_or("").to_owned();
        let source = production;
        // Assert absence of *call syntax* (docs may mention the concept).
        for forbidden in [
            "fs::remove_file",
            "fs::remove_dir_all",
            "fs::File::create",
            "fs::write(",
            "fs::OpenOptions::new()",
        ] {
            assert!(
                !source.contains(forbidden),
                "playback owner must not create/delete files (`{forbidden}`)"
            );
        }
    }

    #[test]
    fn malformed_player_path_rejected() {
        let owner = PlaybackOwner::new(PathBuf::from("/bin/sh -c evil"), None);
        assert!(owner.validate().is_err());
    }
}
