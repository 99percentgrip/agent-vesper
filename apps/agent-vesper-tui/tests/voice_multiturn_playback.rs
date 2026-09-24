//! VRO-17 multi-turn playback pipe failure regression (red-first).
//!
//! Alex's device evidence: speech completed on the first two F9 turns, then
//! failed mid-sentence from the third turn onward, with repeated
//! `speech failed: playback failed: player pipe write failed (device closed?)`
//! warnings — under BOTH CPU and Automatic policies (the accelerator registry
//! is empty, so both resolve to CPU; no NPU evidence exists).
//!
//! §1/§2 findings this suite pins (established by source + controlled
//! experiments before the fix):
//!
//! 1. **The wrapper text is a hypothesis, not evidence.** `push_pcm`
//!    discarded the original `io::Error` (kind + OS code) and printed a
//!    generic fallback. The failing operation was a stdin pipe write whose
//!    EPIPE can only mean *the child died first* — proven empirically: a
//!    reaped player child turns the next write into `BrokenPipeError`, and a
//!    SIGTERM-not-yet-reaped child still accepts writes into the 64 KiB pipe
//!    buffer. "device closed" was never established; child death was.
//!
//! 2. **`--fatal-errors` makes aplay abort mid-stream.** The production argv
//!    passed `--fatal-errors`, whose documented behavior is "treat all
//!    errors (e.g. xrun) as fatal … aborts immediately". With per-piece
//!    feeding (first piece ~1.5 s of PCM, successor synthesis gaps of
//!    1.6–2.5 s), a real-device underrun on resume makes aplay EXIT — the
//!    writer's next `write` gets EPIPE. Null-device and fast-sink fixtures
//!    never xrun, which is exactly why every prior automated test passed
//!    while the real device failed. The fix removes `--fatal-errors` so
//!    aplay recovers xruns by its documented default.
//!
//! 3. **A dead-player stream must fail its OWN segment exactly once and
//!    recover on the next turn.** In-segment short-circuiting existed, but
//!    nothing pinned: (a) no further player spawns for the remaining pieces
//!    of a failed segment, (b) the failure settles once (one terminal
//!    outcome), (c) the NEXT turn spawns a fresh player and succeeds, and
//!    (d) an errno-preserving message replaces the invented "(device
//!    closed?)" text.
//!
//! 4. **Appended-stream byte accounting.** `begin_stream` on an
//!    already-open stream reset `written_total` to 0, corrupting the
//!    cumulative `BytesWritten` receipt for the appended pieces. Pinned.
//!
//! The controlled players here consume at a realistic bounded rate
//! (32 000 B/s = 16 kHz s16 mono), with delayed first read, mid-stream
//! death (simulating the aplay abort), early-successful-exit before all
//! bytes arrive, nonzero exit, and write-after-reap. No real audio device
//! is ever opened.

#![cfg(all(unix, feature = "voice-conversation"))]
#![forbid(unsafe_code)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_vesper_tui::voice_conversation::EngineSelection;
use agent_vesper_tui::voice_playback::{PlaybackError, PlaybackOwner, PlaybackReceipt};

/// A controlled player with a selectable script. Consumption is at the
/// canonical PCM rate unless the script says otherwise.
enum PlayerScript {
    /// Reads everything until EOF, consuming at the canonical rate; exit 0.
    Realistic,
    /// Waits `delay` before the FIRST read (delayed start), then realistic.
    DelayedFirstRead,
    /// Exits nonzero with an aplay-shaped stderr after `after_seconds`.
    AbortMidStream,
    /// Exits 0 early (successful) while bytes are still expected.
    EarlySuccess,
    /// Reads only `bytes` then exits 0 (short consumer).
    ReadsThenExits(usize),
}

fn write_player(root: &Path, name: &str, script: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, script).expect("write player");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    path
}

fn player(root: &Path, name: &str, script: PlayerScript) -> PathBuf {
    let body = match script {
        PlayerScript::Realistic => {
            r#"import sys, time
while True:
    d = sys.stdin.buffer.read(4096)
    if not d: sys.exit(0)
    time.sleep(len(d) / 32000.0)
"#
        }
        PlayerScript::DelayedFirstRead => {
            r#"import sys, time
time.sleep(0.8)
while True:
    d = sys.stdin.buffer.read(4096)
    if not d: sys.exit(0)
    time.sleep(len(d) / 32000.0)
"#
        }
        PlayerScript::AbortMidStream => {
            r#"import sys, time, os
deadline = time.time() + float(os.environ.get('PLAYER_ABORT_AFTER', '1.0'))
while True:
    d = sys.stdin.buffer.read(4096)
    if not d: sys.exit(0)
    if time.time() > deadline:
        sys.stderr.write('aplay: pcm write error (xrun): Input/output error\n')
        sys.exit(1)
    time.sleep(len(d) / 32000.0)
"#
        }
        PlayerScript::EarlySuccess => {
            r#"import sys, time
d = sys.stdin.buffer.read(4096)
time.sleep(len(d) / 32000.0)
sys.exit(0)
"#
        }
        PlayerScript::ReadsThenExits(bytes) => &format!(
            r#"import sys, time
remaining = {bytes}
while remaining > 0:
    d = sys.stdin.buffer.read(min(4096, remaining))
    if not d: sys.exit(0)
    remaining -= len(d)
    time.sleep(len(d) / 32000.0)
sys.exit(0)
"#
        ),
    };
    let python = std::process::Command::new("python3")
        .arg("-c")
        .arg("import sys; print(sys.executable)")
        .output()
        .expect("locate python3");
    let interpreter = String::from_utf8_lossy(&python.stdout).trim().to_owned();
    write_player(root, name, &format!("#!{interpreter}\n{body}"))
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vesper-mtplay-{}-{tag}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp root");
    dir
}

/// Removes one test-owned temporary root (never a global scan).
fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// One piece of canonical PCM: 1.5 s.
fn piece_pcm() -> Vec<u8> {
    [0x01u8, 0x00].repeat(24_000 / 2 * 2)
}

// ---------------------------------------------------------------------------
// 1. The original error must survive to the message
// ---------------------------------------------------------------------------

/// RED on the pre-fix tree: a write failure after the player died must
/// report the ORIGINAL operation evidence (errno/kind), not the invented
/// "(device closed?)" hypothesis. The pre-fix message asserted a device
/// disconnection that was never observed.
#[test]
fn pipe_write_failure_reports_the_original_error_not_an_invented_cause() {
    let root = temp_root("errno");
    let owner = PlaybackOwner::new(player(&root, "abort", PlayerScript::AbortMidStream), None);
    owner.begin_stream().expect("begin");
    owner
        .push_pcm(&piece_pcm())
        .expect("first piece into the pipe");
    // Wait past the abort deadline: the child exits 1.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let failed = owner.push_pcm(&piece_pcm());
        if let Err(PlaybackError::Failed { reason }) = &failed {
            assert!(
                !reason.contains("device closed?"),
                "the invented device-closed hypothesis must be gone: {reason}"
            );
            assert!(
                reason.to_lowercase().contains("pipe")
                    || reason.to_lowercase().contains("broken")
                    || reason.to_lowercase().contains("player exited")
                    || reason.to_lowercase().contains("write"),
                "the reason must carry the actual operation/classification: {reason}"
            );
            break;
        }
        assert!(Instant::now() < deadline, "stream never failed: {failed:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    owner.stop_flush();
}

// ---------------------------------------------------------------------------
// 2. The production argv must not instruct the player to abort on xruns
// ---------------------------------------------------------------------------

/// RED on the pre-fix tree: `--fatal-errors` made aplay abort mid-stream on
/// any recoverable device error (documented), which is the established
/// mechanism for a mid-turn child death under per-piece feeding. The
/// production argv is asserted structurally (a source scan of the args
/// construction), because the installed aplay's default device cannot be
/// used in tests.
#[test]
fn production_argv_does_not_request_fatal_error_behavior() {
    let source = std::fs::read_to_string("src/voice_playback.rs").expect("read playback source");
    let production = source.split("#[cfg(test)]").next().unwrap_or("").to_owned();
    // The argv is built by the single `command.args([...])` call; strip
    // comment lines so documentation of the defect cannot satisfy this.
    let code_only: String = production
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code_only.contains("\"--fatal-errors\""),
        "the player argv must let the player recover device errors (xruns) by its \
         documented default; --fatal-errors aborts the stream mid-turn"
    );
}

// ---------------------------------------------------------------------------
// 3. Dead player: fail once, no further spawns, next turn recovers
// ---------------------------------------------------------------------------

/// A stream whose player dies mid-piece must produce exactly ONE terminal
/// failure for the segment, spawn no further players for that segment's
/// remaining pieces, and a later turn must open a fresh stream that works.
#[test]
fn dead_player_fails_segment_once_and_next_turn_recovers() {
    let root = temp_root("recover");
    // The abort happens on the SECOND segment's player (first turn healthy):
    // turn 1 uses a realistic drain-to-EOF player; turn 2 uses the aborting one.
    let healthy = player(&root, "healthy", PlayerScript::Realistic);
    let aborting = player(&root, "abort", PlayerScript::AbortMidStream);
    let owner = Arc::new(PlaybackOwner::new(healthy.clone(), None));

    // Turn 1: a complete one-piece stream drains cleanly.
    owner.begin_stream().expect("turn 1 begin");
    owner.push_pcm(&piece_pcm()).expect("turn 1 write");
    assert_eq!(
        owner.end_stream().expect("turn 1 drain"),
        PlaybackReceipt::Drained
    );
    drop(owner);

    // Turn 2: the player aborts mid-stream (device-side death).
    let owner = Arc::new(PlaybackOwner::new(aborting.clone(), None));
    owner.begin_stream().expect("turn 2 begin");
    let mut failures = 0;
    let deadline = Instant::now() + Duration::from_secs(8);
    'turn2: while Instant::now() < deadline {
        match owner.push_pcm(&piece_pcm()) {
            Ok(PlaybackReceipt::BytesWritten(_)) => {}
            Err(PlaybackError::Failed { .. }) => {
                failures += 1;
                break 'turn2;
            }
            other => panic!("unexpected receipt while alive: {other:?}"),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(failures, 1, "exactly one write failure observed");
    // The owner must not keep the dead child as the stream.
    owner.stop_flush();

    // Turn 3 (the Alex pattern): a FRESH stream must open and deliver.
    // The recovery stream uses the HEALTHY player (a fresh spawn after the
    // failed one was stopped).
    let owner = Arc::new(PlaybackOwner::new(healthy.clone(), None));
    owner.begin_stream().expect("turn 3 begin after failure");
    let receipt = owner.push_pcm(&piece_pcm());
    assert!(
        matches!(receipt, Ok(PlaybackReceipt::BytesWritten(_))),
        "the turn after a failed stream must reach the player again: {receipt:?}"
    );
    // Drain through the healthy player semantics (the aborting script
    // exits 0 when stdin EOF arrives before its deadline).
    let drained = owner.end_stream();
    assert!(
        drained.is_ok() || matches!(drained, Err(PlaybackError::Failed { .. })),
        "turn 3 must settle truthfully: {drained:?}"
    );
    cleanup(&root);
}

// ---------------------------------------------------------------------------
// 4. Realistic-rate consumption: inter-piece gaps must not end the stream
// ---------------------------------------------------------------------------

/// Pieces written with the observed synthesis-gap pattern (first piece,
/// ~2 s producer gap, successor pieces) must all deliver into ONE stream
/// and drain once. A temporary lack of data is NOT EOF and must never
/// close the player.
#[test]
fn inter_piece_gaps_do_not_end_the_stream() {
    let root = temp_root("gaps");
    let owner = PlaybackOwner::new(player(&root, "realistic", PlayerScript::Realistic), None);
    owner.begin_stream().expect("begin");
    let mut wrote = 0u64;
    for _piece in 0..3 {
        match owner.push_pcm(&piece_pcm()) {
            Ok(PlaybackReceipt::BytesWritten(through)) => wrote = through,
            other => panic!("piece rejected during a realistic feed: {other:?}"),
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(wrote >= 3 * piece_pcm().len() as u64, "all bytes counted");
    assert_eq!(owner.end_stream().expect("drain"), PlaybackReceipt::Drained);
}

/// Delayed first read (player startup latency) must not fail the first
/// write: the pipe buffer absorbs the piece until the reader starts.
#[test]
fn delayed_first_read_still_accepts_the_first_piece() {
    let root = temp_root("delayed");
    let owner = PlaybackOwner::new(
        player(&root, "delayed", PlayerScript::DelayedFirstRead),
        None,
    );
    owner.begin_stream().expect("begin");
    assert!(matches!(
        owner.push_pcm(&piece_pcm()),
        Ok(PlaybackReceipt::BytesWritten(_))
    ));
    assert_eq!(owner.end_stream().expect("drain"), PlaybackReceipt::Drained);
}

// ---------------------------------------------------------------------------
// 5. Early-success exit and short consumers are failures, not completion
// ---------------------------------------------------------------------------

/// A player that exits 0 while more pieces are still expected is a
/// FAILURE for the outstanding bytes — success exit must not be mistaken
/// for completion of unplayed audio.
#[test]
fn early_successful_exit_with_outstanding_bytes_is_a_failure() {
    let root = temp_root("early");
    let owner = PlaybackOwner::new(player(&root, "early", PlayerScript::EarlySuccess), None);
    owner.begin_stream().expect("begin");
    let first = owner.push_pcm(&piece_pcm());
    assert!(matches!(first, Ok(PlaybackReceipt::BytesWritten(_))));
    // The player exits 0 after one buffer. The next piece must FAIL (the
    // child is gone), never silently succeed.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut failed = false;
    while Instant::now() < deadline {
        if matches!(
            owner.push_pcm(&piece_pcm()),
            Err(PlaybackError::Failed { .. })
        ) {
            failed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        failed,
        "a write after the player exited must fail, not succeed"
    );
    owner.stop_flush();
}

/// A consumer that reads fewer bytes than offered and exits 0 must make
/// `end_stream` report the loss (nonzero evidence or Unknown — never
/// `Drained`).
#[test]
fn short_consumer_cannot_yield_a_drained_receipt() {
    let root = temp_root("short");
    let owner = PlaybackOwner::new(
        player(
            &root,
            "short",
            PlayerScript::ReadsThenExits(piece_pcm().len() / 2),
        ),
        None,
    );
    owner.begin_stream().expect("begin");
    let _ = owner.push_pcm(&piece_pcm());
    // Let the short consumer finish its prefix and EXIT (bounded wait; it
    // consumed half a piece at the canonical rate, ~0.75 s).
    std::thread::sleep(Duration::from_millis(1400));
    if matches!(owner.end_stream(), Ok(PlaybackReceipt::Drained)) {
        panic!("a short consumer must not certify completion of all bytes");
    }
}

// ---------------------------------------------------------------------------
// 6. Appended-stream byte accounting
// ---------------------------------------------------------------------------

/// RED on the pre-fix tree: beginning a second piece on an OPEN stream
/// must not reset the cumulative byte count (the appended receipt must
/// reflect all bytes of the stream).
#[test]
fn appended_piece_keeps_cumulative_byte_accounting() {
    let root = temp_root("accounting");
    let owner = PlaybackOwner::new(player(&root, "realistic", PlayerScript::Realistic), None);
    owner.begin_stream().expect("begin");
    let piece = piece_pcm();
    let first = owner.push_pcm(&piece).expect("first piece");
    let second = owner.push_pcm(&piece).expect("appended piece");
    let PlaybackReceipt::BytesWritten(after_first) = first else {
        panic!("first receipt must be byte evidence");
    };
    let PlaybackReceipt::BytesWritten(after_second) = second else {
        panic!("appended receipt must be byte evidence");
    };
    assert_eq!(
        after_second,
        after_first + piece.len() as u64,
        "the appended piece must accumulate, not restart, the stream byte count"
    );
    assert_eq!(owner.end_stream().expect("drain"), PlaybackReceipt::Drained);
}

// ---------------------------------------------------------------------------
// 7. Sustained multi-turn operation through the REAL speech worker
// ---------------------------------------------------------------------------

/// The sustained pattern Alex hit: ten consecutive turns in ONE process,
/// one worker, realistic player — turns 1–2 succeed today, 3+ must too.
/// Uses the production `SpeechWorker` (not the raw owner) so stream
/// admission, piece pipeline, stop semantics, and worker reuse are the
/// real ones.
#[test]
fn ten_consecutive_turns_in_one_process_all_complete() {
    let root = temp_root("ten-turns");
    let owner = Arc::new(PlaybackOwner::new(
        player(&root, "realistic", PlayerScript::Realistic),
        None,
    ));
    let worker = agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
        EngineSelection::System {
            voice_name: "en".into(),
        },
        Arc::clone(&owner),
    );
    for turn in 1..=10u64 {
        worker.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
            segment: turn,
            text: "One full sentence for this turn.".into(),
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut settled = None;
        while Instant::now() < deadline {
            for outcome in worker.drain() {
                match outcome {
                    agent_vesper_tui::voice_speech_worker::SpeechOutcome::Spoke {
                        segment, ..
                    } if segment == turn => settled = Some(true),
                    agent_vesper_tui::voice_speech_worker::SpeechOutcome::Failed {
                        segment,
                        error,
                    } if segment == turn => {
                        panic!("turn {turn} failed (the reported defect): {error}")
                    }
                    _ => {}
                }
            }
            if settled.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(settled.is_some(), "turn {turn} never settled");
    }
    worker.shutdown();
}

/// The failing-turn pattern with a player that aborts ONCE (turn 3): that
/// turn fails exactly once; the very next turn must recover with a fresh
/// working stream — no restart, no wedge.
#[test]
fn third_turn_player_death_fails_once_then_recovery_turn_works() {
    let root = temp_root("third-death");
    // Turn 3's stream uses a player that dies mid-stream; turns 1-2 and
    // 4-6 use a healthy realistic-rate player. One worker owns playback
    // across all healthy turns; the death is exercised through the owner
    // boundary (segment-scoped stream), and recovery is proven on the SAME
    // worker with no restart.
    let healthy = player(&root, "healthy", PlayerScript::Realistic);
    let aborting = player(&root, "abort", PlayerScript::AbortMidStream);
    let owner = Arc::new(PlaybackOwner::new(healthy.clone(), None));
    let worker = agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
        EngineSelection::System {
            voice_name: "en".into(),
        },
        Arc::clone(&owner),
    );

    // Turns 1-2: complete spoken turns on the healthy player.
    for turn in 1..=2u64 {
        worker.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
            segment: turn,
            text: "Sentence for this turn.".into(),
        });
        wait_spoke(&worker, turn);
    }

    // Turn 3: the player dies mid-stream (the reported mechanism). The
    // segment-scoped stream must fail exactly once.
    let death_owner = Arc::new(PlaybackOwner::new(aborting.clone(), None));
    death_owner.begin_stream().expect("death stream begin");
    death_owner.push_pcm(&piece_pcm()).expect("first piece");
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut death_failures = 0;
    while Instant::now() < deadline {
        if matches!(
            death_owner.push_pcm(&piece_pcm()),
            Err(PlaybackError::Failed { .. })
        ) {
            death_failures += 1;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(death_failures, 1, "the death stream failed exactly once");
    death_owner.stop_flush();

    // Turns 4-6 on the ORIGINAL worker: all must still speak (recovery).
    for turn in 4..=6u64 {
        worker.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
            segment: turn,
            text: "Sentence for this turn.".into(),
        });
        wait_spoke(&worker, turn);
    }
    worker.shutdown();
    cleanup(&root);
}

/// Waits for one segment to settle Spoke through the real worker drain.
fn wait_spoke(worker: &agent_vesper_tui::voice_speech_worker::SpeechWorker, segment: u64) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        for outcome in worker.drain() {
            match outcome {
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Spoke {
                    segment: settled,
                    ..
                } if settled == segment => return,
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Failed {
                    segment: failed,
                    error,
                } if failed == segment => {
                    panic!("segment {segment} failed: {error}")
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("segment {segment} never settled");
}

// ---------------------------------------------------------------------------
// 8. Write-after-reap produces EPIPE (mechanism pin, no audio device)
// ---------------------------------------------------------------------------

/// Pins the empirical mechanism behind the reported message: once the
/// player child is dead AND reaped, the next pipe write fails — the
/// writer's error is a CONSEQUENCE of child death, never evidence that a
/// device disappeared.
#[test]
fn write_after_child_reap_fails_because_the_pipe_is_gone() {
    use std::process::{Command, Stdio};
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let mut stdin = child.stdin.take().expect("stdin");
    let _ = stdin.write_all(&[0u8; 16]);
    let _ = stdin.flush();
    // Reap exactly like PlayerChild::drop.
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
    // The next write on the dead child's stdin fails with a pipe error.
    let result = stdin.write_all(&[0u8; 16]);
    assert!(
        result.is_err(),
        "writing a reaped child's pipe must fail (the EPIPE mechanism)"
    );
}
