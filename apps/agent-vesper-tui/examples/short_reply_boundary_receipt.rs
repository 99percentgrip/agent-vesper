//! Device-free VRO-17 short-reply boundary receipt.
//!
//! Runs the real installed Kokoro adapter through the production speech worker
//! and a file sink, then measures the low-amplitude run at the worker's one
//! artificial onset split. It opens no microphone, speaker, provider, network,
//! settings, or user-state writer.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_vesper_tui::voice_conversation::EngineSelection;
use agent_vesper_tui::voice_playback::PlaybackOwner;
use agent_vesper_tui::voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker};
use vesper_voice::config::CaptureBudget;
use vesper_voice::hygiene::HygieneGate;

const QUIET_ABS_THRESHOLD: i32 = 400;

fn main() {
    let voice_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "am_michael".to_owned());
    assert!(
        matches!(voice_id.as_str(), "am_michael" | "af_heart"),
        "voice must be am_michael or af_heart"
    );
    let root = tempfile::tempdir().expect("temporary receipt root");
    let captured = root.path().join("short-reply.pcm");
    let lengths = root.path().join("stream-lengths");
    let sink = root.path().join("pcm-sink");
    let script = root.path().join("pcm-sink.py");
    std::fs::write(
        &script,
        format!(
            "import sys\ndata=sys.stdin.buffer.read()\nopen({captured:?},'ab').write(data)\nopen({lengths:?},'a').write(str(len(data))+'\\n')\n"
        ),
    )
    .expect("write sink body");
    std::fs::write(&sink, format!("#!/bin/sh\nexec python3 {script:?}\n"))
        .expect("write sink launcher");
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o700))
        .expect("make sink executable");

    let worker = SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: voice_id.clone(),
        },
        Arc::new(PlaybackOwner::new(sink, None)),
    );
    let mut gate = HygieneGate::new(CaptureBudget::default());
    let mut units = gate
        .push("Please reply now. I am ready for the test.")
        .expect("hygiene push");
    units.extend(gate.finalize().expect("hygiene finalize"));
    assert_eq!(units.len(), 2, "fixture must cross a hygiene-unit boundary");
    for unit in &units {
        worker.enqueue(SpeechJob {
            segment: unit.segment,
            text: unit.text.as_str().to_owned(),
        });
    }

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut boundaries = Vec::new();
    let mut terminal_samples = Vec::new();
    while Instant::now() < deadline && terminal_samples.len() < units.len() {
        for outcome in worker.drain() {
            match outcome {
                SpeechOutcome::Progress { through_bytes, .. } => boundaries.push(through_bytes),
                SpeechOutcome::Spoke { samples, .. } => terminal_samples.push(samples),
                SpeechOutcome::Failed { error, .. } => panic!("speech failed: {error}"),
                SpeechOutcome::Stale { segment } => panic!("segment {segment} went stale"),
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    worker.shutdown();
    assert_eq!(
        terminal_samples.len(),
        units.len(),
        "speech receipt timed out"
    );
    let samples: usize = terminal_samples.iter().sum();
    assert!(
        boundaries.len() >= 2,
        "fixture must exercise an artificial split"
    );
    let pcm = std::fs::read(&captured).expect("captured PCM");
    assert_eq!(pcm.len(), samples * 2, "receipt and sink byte counts agree");
    let decoded: Vec<i16> = pcm
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let stream_lengths: Vec<usize> = std::fs::read_to_string(&lengths)
        .expect("stream length receipt")
        .lines()
        .map(|line| line.parse().expect("numeric stream length"))
        .collect();
    assert_eq!(
        stream_lengths.len(),
        units.len(),
        "one stream per hygiene unit"
    );
    let boundary = stream_lengths[0] / 2;
    let trailing = decoded[..boundary]
        .iter()
        .rev()
        .take_while(|sample| i32::from(**sample).abs() < QUIET_ABS_THRESHOLD)
        .count();
    let leading = decoded[boundary..]
        .iter()
        .take_while(|sample| i32::from(**sample).abs() < QUIET_ABS_THRESHOLD)
        .count();
    let stacked_seconds = (trailing + leading) as f64 / 16_000.0;
    println!(
        "PASS: real Kokoro {voice_id} short reply used {} hygiene units; unit-boundary quiet={stacked_seconds:.3}s (trailing={:.3}s, leading={:.3}s); samples={samples}",
        units.len(),
        trailing as f64 / 16_000.0,
        leading as f64 / 16_000.0,
    );
    assert!(
        stacked_seconds <= 0.21,
        "Kokoro hygiene-unit boundary retained {stacked_seconds:.3}s of stacked quiet"
    );
}
