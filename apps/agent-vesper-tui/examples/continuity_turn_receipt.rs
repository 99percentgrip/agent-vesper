//! VRO-17 long-response continuity receipt: real HygieneGate -> production
//! SpeechWorker -> real installed Kokoro -> canonical-rate no-device sink.
//! Synthetic text only; no network, microphone, speaker, or saved settings.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_vesper_tui::voice_conversation::EngineSelection;
use agent_vesper_tui::voice_playback::PlaybackOwner;
use agent_vesper_tui::voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker};
use vesper_voice::config::CaptureBudget;
use vesper_voice::hygiene::HygieneGate;

const PASSAGE: &str = "I'm running inside the Agent Vesper harness with cognitive memory active, so context from past sessions carries over automatically. The workspace is intact, and nothing is blocking me right now: no pending failures or half-finished work on my side. Tools, skills, and the project contracts are all loaded and ready for whatever you want to do next. I'm only waiting on you to confirm this came through audibly.";

fn main() {
    let root = tempfile::tempdir().expect("temp root");
    let sink = root.path().join("paced-player");
    let trace = root.path().join("trace");
    let script = root.path().join("paced.py");
    std::fs::write(
        &script,
        format!(
            "import sys,time\np={trace:?}\nwith open(p,'a') as f:\n while True:\n  d=sys.stdin.buffer.read1(4096)\n  if not d:\n   f.write(f'{{time.monotonic():.6f}} EOF\\n');f.flush();break\n  f.write(f'{{time.monotonic():.6f}} {{len(d)}}\\n');f.flush();time.sleep(len(d)/32000.0)\n"
        ),
    )
    .unwrap();
    std::fs::write(&sink, format!("#!/bin/sh\nexec python3 {script:?}\n")).unwrap();
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o700)).unwrap();

    let worker = SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::new(PlaybackOwner::new(sink, None)),
    );

    for (label, chunks) in [
        ("available", vec![PASSAGE.to_owned()]),
        ("word-deltas", word_chunks(PASSAGE)),
        ("subword-automatic-ally", subword_chunks(PASSAGE)),
    ] {
        let before = read_events(&trace).len();
        let mut gate = HygieneGate::new(CaptureBudget::default());
        let mut units = Vec::new();
        for chunk in chunks {
            units.extend(gate.push(&chunk).unwrap());
        }
        units.extend(gate.finalize().unwrap());
        let unit_count = units.len();
        let reconstructed = units.iter().map(|u| u.text.as_str()).collect::<String>();
        println!(
            "CASE {label}: units={unit_count} reconstructed_bytes={}",
            reconstructed.len()
        );
        let started = Instant::now();
        for unit in units {
            worker.enqueue(SpeechJob {
                segment: unit.segment,
                text: unit.text.as_str().to_owned(),
            });
        }
        let mut terminals = 0usize;
        let mut first_pcm = None;
        let mut samples = 0usize;
        let deadline = Instant::now() + Duration::from_secs(150);
        while terminals < unit_count && Instant::now() < deadline {
            for outcome in worker.drain() {
                match outcome {
                    SpeechOutcome::Progress { .. } => {
                        first_pcm.get_or_insert(started.elapsed());
                    }
                    SpeechOutcome::Spoke { samples: n, .. } => {
                        terminals += 1;
                        samples += n;
                    }
                    SpeechOutcome::Failed { error, .. } => panic!("{label} failed: {error}"),
                    SpeechOutcome::Stale { segment } => panic!("{label} stale: {segment}"),
                };
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(terminals, unit_count, "{label} did not settle");
        let events = read_events(&trace);
        let slice = &events[before..];
        let gaps: Vec<f64> = slice
            .windows(2)
            .filter_map(|pair| {
                let expected = pair[0].1 as f64 / 32000.0;
                let gap = pair[1].0 - pair[0].0 - expected;
                (gap > 0.25).then_some(gap)
            })
            .collect();
        println!(
            "CASE {label}: first_pcm={:.3}s total={:.3}s audio={:.3}s gaps={gaps:?} peak_buffer_bound={}B",
            first_pcm.unwrap_or_default().as_secs_f64(),
            started.elapsed().as_secs_f64(),
            samples as f64 / 16000.0,
            16 * 1024 * 1024 * 2
        );
    }
    worker.shutdown();
}

fn word_chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch.is_whitespace() {
            out.push(text[start..=i].to_owned());
            start = i + ch.len_utf8();
        }
    }
    if start < text.len() {
        out.push(text[start..].to_owned());
    }
    out
}

fn subword_chunks(text: &str) -> Vec<String> {
    let needle = "automatically";
    let Some(at) = text.find(needle) else {
        return vec![text.to_owned()];
    };
    vec![
        text[..at + "automatic".len()].to_owned(),
        text[at + "automatic".len()..].to_owned(),
    ]
}

fn read_events(path: &std::path::Path) -> Vec<(f64, usize)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let time = fields.next()?.parse().ok()?;
            let bytes = fields.next()?.parse().ok()?;
            Some((time, bytes))
        })
        .collect()
}
