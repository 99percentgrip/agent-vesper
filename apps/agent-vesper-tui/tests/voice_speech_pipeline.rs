//! Device-free production-worker ordering regression. The subprocess isolates PATH.
#![cfg(unix)]
use agent_vesper_tui::voice_conversation::EngineSelection;
use agent_vesper_tui::voice_playback::PlaybackOwner;
use agent_vesper_tui::voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker};
use std::{
    fs,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn synthesis_overlaps_playback_and_stop_discards_lookahead() {
    if std::env::var_os("VESPER_PIPELINE_FIXTURE").is_some() {
        run_fixture();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let python = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(python.status.success());
    let python = String::from_utf8(python.stdout).unwrap();
    let script = format!("#!{}\n", python.trim());
    // Synthesis-only executable: WAV stdout, never sound or a real voice engine.
    executable(
        &dir.path().join("espeak-ng"),
        &(script.clone()
            + r#"import io, os, sys, wave, time
from pathlib import Path
root = Path(os.environ['VESPER_PIPELINE_FIXTURE'])
text = sys.stdin.read()
if len(text) > 120:
    time.sleep(6)
if text.startswith("because a blocked"):
    time.sleep(0.5)
(root / ('synth-' + text)).touch()
buf = io.BytesIO()
with wave.open(buf, 'wb') as wav:
    wav.setnchannels(1)
    wav.setsampwidth(2)
    wav.setframerate(16000)
    wav.writeframes(b'\x01\x00' * 1600)
sys.stdout.buffer.write(buf.getvalue())
"#),
    );
    executable(
        &dir.path().join("player"),
        &(script
            + r#"import os, sys, time
from pathlib import Path
root = Path(os.environ['VESPER_PIPELINE_FIXTURE'])
with (root / 'player-starts').open('a') as out:
    out.write('start\n')
    out.flush()
sys.stdin.buffer.read(2)
(root / 'player-first-pcm').touch()
sys.stdin.buffer.read()
(root / 'player-draining').touch()
# Deliberately remain alive until Stop kills this fixture; no device is opened.
if not (root / 'release-player').exists():
    time.sleep(15)
"#),
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "synthesis_overlaps_playback_and_stop_discards_lookahead",
            "--nocapture",
        ])
        .env("VESPER_PIPELINE_FIXTURE", dir.path())
        .env("PATH", dir.path())
        .env("HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unit_boundary_gap_next_unit_synthesis_must_overlap_previous_drain() {
    if std::env::var_os("VESPER_PIPELINE_FIXTURE").is_some() {
        run_boundary_fixture();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let python = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(python.status.success());
    let python = String::from_utf8(python.stdout).unwrap();
    let script = format!("#!{}\n", python.trim());
    // Engine: ~0.2 s per unit except the third (2.5 s) — the uncovered
    // regime Alex's recording shows: a successor whose synthesis is
    // SLOWER than the previous unit's playback. The previous unit's
    // audio (1.5 s) cannot cover a 2.5 s synthesis, so continuity
    // requires the synthesis to have started EARLIER (banked ahead).
    executable(
        &dir.path().join("espeak-ng"),
        &(script.clone()
            + r#"import io, sys, wave, time
text = sys.stdin.read()
time.sleep(2.5 if text.startswith("Third") else 0.2)
buf = io.BytesIO()
with wave.open(buf, 'wb') as wav:
    wav.setnchannels(1)
    wav.setsampwidth(2)
    wav.setframerate(16000)
    wav.writeframes(b'\x01\x00' * 24000)
sys.stdout.buffer.write(buf.getvalue())
"#),
    );
    // Player: consumes at the canonical real-time rate and logs monotonic
    // start/eof instants, so the measured inter-unit gap is the player-side
    // silence (a software proxy; acoustic certification stays user-side).
    executable(
        &dir.path().join("player"),
        &(script
            + r#"import sys, time
with open('player-boundary.log', 'a') as log:
    log.write(f'start {time.monotonic()}\n')
    log.flush()
    while True:
        d = sys.stdin.buffer.read(4096)
        if not d:
            log.write(f'eof {time.monotonic()}\n')
            log.flush()
            sys.exit(0)
        time.sleep(len(d) / 32000.0)
"#),
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "unit_boundary_gap_next_unit_synthesis_must_overlap_previous_drain",
            "--nocapture",
        ])
        .env("VESPER_PIPELINE_FIXTURE", dir.path())
        .env("PATH", dir.path())
        .env("HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_boundary_fixture() {
    let root = std::path::PathBuf::from(std::env::var_os("VESPER_PIPELINE_FIXTURE").unwrap());
    let worker = SpeechWorker::spawn(
        EngineSelection::System {
            voice_name: "en".into(),
        },
        Arc::new(PlaybackOwner::new(root.join("player"), None)),
    );
    for (segment, text) in [
        (0usize, "First sentence speaks now."),
        (1, "Second sentence follows here."),
        (2, "Third sentence arrives last."),
    ] {
        worker.enqueue(SpeechJob {
            segment: segment as u64,
            text: text.into(),
        });
    }
    // Wait until every unit settles (each unit is one whole piece).
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut spoke = Vec::new();
    while Instant::now() < deadline && spoke.len() < 3 {
        for outcome in worker.drain() {
            match outcome {
                SpeechOutcome::Spoke { segment, .. } => spoke.push(segment),
                SpeechOutcome::Failed { segment, error } => {
                    panic!("segment {segment} failed: {error}")
                }
                SpeechOutcome::Stale { segment } => {
                    panic!("segment {segment} went stale without a stop")
                }
                SpeechOutcome::Progress { .. } => {}
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    worker.shutdown();
    spoke.sort_unstable();
    assert_eq!(spoke, vec![0, 1, 2], "all three units must speak");
    let log = fs::read_to_string(root.join("player-boundary.log")).unwrap();
    let starts: Vec<f64> = log
        .lines()
        .filter_map(|l| l.strip_prefix("start "))
        .filter_map(|v| v.parse().ok())
        .collect();
    let eofs: Vec<f64> = log
        .lines()
        .filter_map(|l| l.strip_prefix("eof "))
        .filter_map(|v| v.parse().ok())
        .collect();
    assert_eq!(starts.len(), 3, "one player stream per unit: {log}");
    assert_eq!(eofs.len(), 3, "one drain per unit: {log}");
    // Inter-unit silence: player spawn of unit N+1 minus audio end of N.
    // Unit 3's synthesis (2.5 s) exceeds unit 2's audio (1.5 s), so the
    // boundary is covered only if unit 3 was synthesized while earlier
    // units were still playing (the bounded bank), not started from rest.
    let gap = starts[2] - eofs[1];
    assert!(
        (starts[1] - eofs[0]) < 1.0,
        "sanity: unit 2 boundary must be covered (gap {:.3})",
        starts[1] - eofs[0]
    );
    assert!(
        gap < 0.8,
        "unit 3 began {gap:.3}s after unit 2's audio ended: next-unit \
         synthesis did not overlap the previous unit's playback+drain"
    );
}

fn executable(path: &std::path::Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn run_fixture() {
    let root = std::path::PathBuf::from(std::env::var_os("VESPER_PIPELINE_FIXTURE").unwrap());
    let worker = SpeechWorker::spawn(
        EngineSelection::System {
            voice_name: "en".into(),
        },
        Arc::new(PlaybackOwner::new(root.join("player"), None)),
    );
    for segment in 1..=3 {
        worker.enqueue(SpeechJob {
            segment,
            text: segment.to_string(),
        });
    }
    assert!(
        wait_for(|| root.join("player-draining").exists()),
        "first player must reach drain"
    );
    let overlapped = wait_for(|| root.join("synth-2").exists());
    // Synthesis may now run up to the bounded two-unit bank ahead
    // (boundary continuity repair), so a third synth marker may appear;
    // the enforced bounds are: banked audio never starts a player
    // (player-starts stays 1 below) and stale generations never speak.
    assert!(
        overlapped,
        "second synthesis must start before first player drains"
    );
    let start = Instant::now();
    worker.stop();
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "Stop blocked on playback"
    );
    let mut outcomes = Vec::new();
    assert!(
        wait_for(|| {
            outcomes.extend(worker.drain());
            !worker.is_busy()
                && outcomes
                    .iter()
                    .filter(|outcome| matches!(outcome, SpeechOutcome::Stale { .. }))
                    .count()
                    == 3
        }),
        "all stopped jobs must settle"
    );
    assert_eq!(
        fs::read_to_string(root.join("player-starts"))
            .unwrap()
            .lines()
            .count(),
        1,
        "stale prefetched audio must never start a player"
    );
    assert!(
        !outcomes
            .iter()
            .any(|outcome| matches!(outcome, SpeechOutcome::Spoke { .. })),
        "Stop cannot claim playback completion"
    );
    for segment in 1..=3 {
        assert_eq!(outcomes.iter().filter(|outcome| matches!(outcome, SpeechOutcome::Stale { segment: id } if *id == segment)).count(), 1, "exactly one stale outcome for {segment}: {outcomes:?}");
    }
    // Fresh generations still play in FIFO order after Stop; no permanent latch.
    fs::write(root.join("release-player"), "").unwrap();
    for segment in 4..=5 {
        worker.enqueue(SpeechJob {
            segment,
            text: segment.to_string(),
        });
    }
    let mut spoke = Vec::new();
    assert!(
        wait_for(|| {
            for outcome in worker.drain() {
                match outcome {
                    SpeechOutcome::Spoke { segment, samples } => {
                        assert_eq!(samples, 1600);
                        spoke.push(segment);
                    }
                    SpeechOutcome::Progress { through_bytes, .. } => {
                        assert_eq!(through_bytes, 3200)
                    }
                    unexpected => panic!("fresh generation failed: {unexpected:?}"),
                }
            }
            spoke.len() == 2 && !worker.is_busy()
        }),
        "fresh generation must settle"
    );
    assert_eq!(spoke, vec![4, 5]);
    assert_eq!(
        fs::read_to_string(root.join("player-starts"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    // A long validated sentence must not wait for one whole-sentence inference.
    fs::remove_file(root.join("player-first-pcm")).unwrap();
    worker.enqueue(SpeechJob { segment: 0, text: "Keep the mutex guard out of blocking audio playback because a blocked writer can prevent Stop from acquiring the lock and leave the interface unresponsive until the player drains the pipe.".into() });
    let early_pcm = wait_for(|| root.join("player-first-pcm").exists());
    worker.stop();
    // Sentence-level successors make the successor inference as long as one
    // whole sentence (the espeak fixture models 6 s for >120-char pieces);
    // Stop still wins for the PLAYER immediately — settle here only waits
    // out the single in-flight inference, bounded by the fixture's own
    // modeled latency plus margin.
    let settle = Instant::now() + Duration::from_secs(12);
    while worker.is_busy() && Instant::now() < settle {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!worker.is_busy(), "long unit must settle after Stop");
    assert!(
        early_pcm,
        "first PCM must arrive before slow whole-sentence synthesis completes"
    );
    let terminal: Vec<_> = worker
        .drain()
        .into_iter()
        .filter(|outcome| !matches!(outcome, SpeechOutcome::Progress { .. }))
        .collect();
    assert_eq!(
        terminal,
        vec![SpeechOutcome::Stale { segment: 0 }],
        "partial-sentence Stop must settle exactly once"
    );
    // A complete fragmented unit reuses one player and settles only once.
    let before = fs::read_to_string(root.join("player-starts"))
        .unwrap()
        .lines()
        .count();
    worker.enqueue(SpeechJob { segment: 0, text: "Keep the mutex guard out of blocking audio playback because a blocked writer can prevent Stop from acquiring the lock and leave the interface unresponsive until the player drains the pipe.".into() });
    let mut progress = Vec::new();
    let mut done = Vec::new();
    // Sentence-level successors: the successor inference spans one whole
    // sentence; the fixture models 6 s for >120-char pieces. Bounded wait
    // consistent with that modeled latency (unchanged invariant: settle
    // exactly once; only the inference length changed).
    let sentence_deadline = Instant::now() + Duration::from_secs(12);
    let mut settled = false;
    while Instant::now() < sentence_deadline && !settled {
        for outcome in worker.drain() {
            match outcome {
                SpeechOutcome::Progress {
                    segment: 0,
                    through_bytes,
                } => progress.push(through_bytes),
                other => done.push(other),
            }
        }
        settled = !worker.is_busy() && !done.is_empty();
        if !settled {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert!(settled, "fresh turn segment must settle");
    // Onset protection: the first piece is clause-sized (early PCM), and
    // the successor is sentence-level — so a long sentence uses exactly
    // 2 pieces under the superseding policy (not >=4 micro-pieces).
    assert!(
        progress.len() >= 2,
        "long unit must still produce early first-piece PCM"
    );
    assert_eq!(
        progress,
        (1..=progress.len())
            .map(|piece| u64::try_from(piece).unwrap() * 3200)
            .collect::<Vec<_>>(),
        "progress must stay cumulative across every piece"
    );
    assert_eq!(
        done,
        vec![SpeechOutcome::Spoke {
            segment: 0,
            samples: progress.len() * 1600
        }]
    );
    assert_eq!(
        fs::read_to_string(root.join("player-starts"))
            .unwrap()
            .lines()
            .count(),
        before + 1
    );
    worker.shutdown();
}
