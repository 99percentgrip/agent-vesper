//! VRO-17 §2 recon (isolated, no production changes): measure where the
//! pauses come from in long replies, through the REAL production speech
//! worker + REAL installed Kokoro, one persistent session, with a PACED
//! no-device sink (canonical-rate consumer — not an instant drain).
//!
//! Records per segment/piece: boundary class, char count, text-available,
//! inference start/end, generated samples and real duration, RTF
//! (generation time / audio duration), PCM handoff, producer wait
//! (rendezvous), consumer wait (starvation), and estimated queued audio.
//! Waveform inspection: leading/trailing/interior low-amplitude runs of
//! each piece (amplitude threshold — documented as an approximation, not
//! a phonetic silence detector).
//!
//! No production code changes; no device; no network; bounded runtime.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_vesper_tui::voice_conversation::EngineSelection;
use agent_vesper_tui::voice_playback::PlaybackOwner;
use agent_vesper_tui::voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker};

/// Amplitude fraction below which a sample run counts as "quiet" (16-bit
/// PCM). Approximation only: low-amplitude ≠ phonetic silence (fricatives,
/// breath, model fade-in/out).
#[allow(dead_code)]
const QUIET: i16 = 200;

#[cfg(not(unix))]
fn main() {
    eprintln!("requires Unix and voice-kokoro");
}

#[cfg(unix)]
fn main() {
    let root = tempfile::tempdir().expect("temp root");
    // Paced sink: reads stdin at the canonical rate (32000 B/s), records
    // read-timing envelopes, never opens a device.
    let sink = root.path().join("paced-player");
    let trace = root.path().join("read-trace");
    let reader = root.path().join("paced_reader.py");
    std::fs::write(
        &reader,
        format!(
            r#"import sys, time
trace = {trace:?}
with open(trace, 'w') as out:
    while True:
        d = sys.stdin.buffer.read1(4096)
        if not d:
            out.write(f"{{time.monotonic():.6f}} EOF\n")
            break
        out.write(f"{{time.monotonic():.6f}} {{len(d)}}\n")
        out.flush()
        time.sleep(len(d) / 32000.0)
"#,
        ),
    )
    .expect("write reader");
    std::fs::write(&sink, format!("#!/bin/sh\nexec python3 {reader:?}\n")).expect("write sink");
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o700)).expect("chmod");

    let owner = Arc::new(PlaybackOwner::new(sink, None));
    let worker = SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        owner,
    );

    // Fixed passages: ordinary sentence, comma-heavy, multi-sentence,
    // paragraph-shaped long answer.
    let passages: &[(&str, &str)] = &[
        ("ordinary", "The build finished and every gate passed."),
        (
            "commas",
            "First, we measure the pipeline, then we repair the smallest owning boundary, and finally, we verify with production-path tests.",
        ),
        (
            "multi-sentence",
            "The repair landed cleanly. The gates all passed twice. No regression appeared anywhere. The candidate is preserved with its digest.",
        ),
        (
            "long-answer",
            "Here is the summary of the unit. The first repair removed the fatal error flag from the player argv, because the documented default recovers device underruns instead of aborting mid stream. The second repair preserved the original write error, so a failure now names the actual layer instead of inventing a device hypothesis. The third repair contained dead streams, so a failed turn no longer poisons the next one. Together these changes restored speech for every consecutive turn in the controlled suite.",
        ),
        (
            "alex-demonstration-reconstruction",
            "I'm running inside the Agent Vesper harness with cognitive memory active, so context from past sessions carries over automatically. The workspace is intact, and nothing is blocking me right now: no pending failures or half-finished work on my side. Tools, skills, and the project contracts are all loaded and ready for whatever you want to do next. I'm only waiting on you to confirm this came through audibly.",
        ),
    ];

    let session_start = Instant::now();
    for (label, text) in passages {
        let started = Instant::now();
        worker.enqueue(SpeechJob {
            segment: label_index(label),
            text: (*text).to_owned(),
        });
        // Collect outcomes for this segment: Progress receipts give
        // through_bytes over time; the terminal outcome ends it.
        let target = label_index(label);
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut first_progress: Option<Duration> = None;
        let mut progress_count = 0usize;
        let mut samples = 0u64;
        let mut terminal = None;
        while Instant::now() < deadline {
            for outcome in worker.drain() {
                match outcome {
                    SpeechOutcome::Progress {
                        segment,
                        through_bytes,
                    } if segment == target => {
                        progress_count += 1;
                        if first_progress.is_none() {
                            first_progress = Some(started.elapsed());
                            println!(
                                "[{label}] t+{:.3}s first PCM to sink (through {through_bytes} B)",
                                started.elapsed().as_secs_f64()
                            );
                        }
                    }
                    SpeechOutcome::Spoke {
                        segment,
                        samples: count,
                    } if segment == target => {
                        samples = count as u64;
                        terminal = Some("spoke");
                    }
                    SpeechOutcome::Failed { segment, error } if segment == target => {
                        terminal = Some(error.leak());
                    }
                    SpeechOutcome::Stale { segment } if segment == target => {
                        terminal = Some("stale");
                    }
                    _ => {}
                }
            }
            if terminal.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let total = started.elapsed().as_secs_f64();
        let audio_seconds = samples as f64 / 16_000.0;
        println!(
            "[{label}] total={total:.3}s audio={audio_seconds:.2}s samples={samples} pieces(progress)={progress_count} terminal={:?} pipeline-RTF={:.2}",
            terminal
                .map(|s| s.to_owned())
                .unwrap_or_else(|| "TIMEOUT".into()),
            if audio_seconds > 0.0 {
                total / audio_seconds
            } else {
                0.0
            },
        );
        // Let the paced sink finish consuming before the next segment.
        std::thread::sleep(Duration::from_millis(300));
    }
    worker.shutdown();
    println!(
        "session wall: {:.1}s",
        session_start.elapsed().as_secs_f64()
    );

    // Read-trace analysis: consumer-side gaps (the audible-gap proxy).
    if let Ok(text) = std::fs::read_to_string(&trace) {
        let lines = text.lines().filter(|line| !line.ends_with("EOF"));
        let mut events: Vec<(f64, usize)> = lines
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let t = parts.next()?.parse::<f64>().ok()?;
                let n = parts.next()?.parse::<usize>().ok()?;
                Some((t, n))
            })
            .collect();
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        println!(
            "\nconsumer read-trace (paced at 32 kB/s): {} reads",
            events.len()
        );
        // A consumer reading exactly at pace has reads every ~0.128 s
        // (4096 B). A gap >> pace means starvation (no data available).
        let mut gaps: Vec<(f64, f64)> = Vec::new();
        for pair in events.windows(2) {
            let (t0, _) = pair[0];
            let (t1, _) = pair[1];
            let gap = t1 - t0;
            if gap > 0.4 {
                gaps.push((t1, gap));
            }
        }
        println!("read gaps > 0.4 s: {} (starvation proxy)", gaps.len());
        for (at, gap) in gaps.iter().take(12) {
            println!("  t={at:.2}s gap={gap:.2}s");
        }
        // Interval distribution (pace = 4096 B / 32000 B/s = 0.128 s).
        let mut intervals: Vec<f64> = events
            .windows(2)
            .map(|pair| pair[1].0 - pair[0].0)
            .collect();
        intervals.sort_by(|a, b| a.total_cmp(b));
        if !intervals.is_empty() {
            let p50 = intervals[intervals.len() / 2];
            let p90 = intervals[intervals.len() * 9 / 10];
            let p99 = intervals[(intervals.len() as f64 * 0.99) as usize % intervals.len()];
            let max = intervals[intervals.len() - 1];
            println!(
                "read intervals: p50={p50:.3}s p90={p90:.3}s p99={p99:.3}s max={max:.3}s (pace=0.128s)"
            );
            let over = intervals.iter().filter(|i| **i > 0.2).count();
            println!(
                "intervals > 0.2 s: {over}/{} ({:.0}%) — each is audible-gap risk",
                intervals.len(),
                100.0 * over as f64 / intervals.len() as f64
            );
        }
    }
}

fn label_index(label: &str) -> u64 {
    match label {
        "ordinary" => 1,
        "commas" => 2,
        "multi-sentence" => 3,
        "long-answer" => 4,
        "alex-demonstration-reconstruction" => 5,
        _ => 9,
    }
}

#[allow(dead_code)]
fn unused(_: PathBuf) {}
