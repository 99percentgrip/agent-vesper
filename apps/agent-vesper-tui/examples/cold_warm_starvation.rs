//! VRO-17 cold/warm starvation measurement harness (2026-09-23).
//!
//! Fixed texts (no provider), both voices, timed no-device player sink.
//! Runs the REAL SpeechWorker → Kokoro engine → PlaybackOwner chain.
//! Cold = run 1 in a fresh process; warm = repeats in the same process
//! reusing the same worker (the production F9 host's lifetime model).

use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(not(unix))]
fn main() {
    eprintln!("requires Unix and voice-kokoro");
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let voice = args.get(1).cloned().unwrap_or_else(|| "am_michael".into());
    let text_kind = args.get(2).cloned().unwrap_or_else(|| "short".into());
    let runs: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);

    let text = match text_kind.as_str() {
        "short" => "Please reply now. I am ready for the test.",
        "para" => {
            "Rust ownership helps prevent memory-safety bugs by making resource ownership explicit at compile time. Each value has one owner, and borrowing rules control temporary access without allowing unsafe aliasing. When the owner goes out of scope, the value is released automatically. These rules prevent many use-after-free, double-free, and data-race errors before the program runs."
        }
        other => panic!("unknown text kind {other}"),
    };

    // No-device timed player double on an isolated PATH prefix.
    let dir = tempfile::tempdir().unwrap();
    let stamps = dir.path().join("stamps.log");
    // Env for the player double: pass via the child's own environment by
    // writing the stamp path to a fixed location the script reads from a
    // file instead (avoid set_var entirely).
    let boot_marker = dir.path().join("stampfile-path.txt");
    std::fs::write(&boot_marker, stamps.display().to_string()).unwrap();
    std::fs::create_dir_all(dir.path().join("bin")).unwrap();
    let player = dir.path().join("bin/aplay");
    // Real-rate drain: read stdin in small slices sleeping to emulate
    // 32,000 B/s playback, timestamping first byte and EOF.
    #[rustfmt::skip]
    let player_script = "#!/usr/bin/env python3\nimport sys, time, os, select\nrate = 32000.0\nSTAMP_FILE = open(os.path.join(os.path.dirname(os.path.dirname(os.path.realpath(__file__))), 'stampfile-path.txt')).read().strip()\nlg = open(STAMP_FILE + '.boot', 'w')\nlg.write('argv=%r pid=%d' % (sys.argv, os.getpid()))\nlg.flush(); os.fsync(lg.fileno()); lg.close()\nf = open(STAMP_FILE, 'a')\nt0 = time.monotonic()\nconsumed = 0.0\nfirst = None\nfd = sys.stdin.fileno()\nwhile True:\n    r, _, _ = select.select([fd], [], [], 0.05)\n    if r:\n        chunk = os.read(fd, 4096)\n        if not chunk:\n            break\n        if first is None:\n            first = time.monotonic()\n            f.write('FIRST %.4f' % (first - t0))\n        consumed += len(chunk)\n    target = consumed / rate\n    now = time.monotonic() - t0\n    if target > now:\n        time.sleep(min(target - now, 0.1))\nf.write('EOF %.4f bytes %d' % (time.monotonic() - t0, int(consumed)))\nf.close()\n";
    std::fs::write(&player, player_script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Keep the workers alive across runs (warm = same worker/engine).
    let mut workers = Vec::new();
    println!("# voice={voice} text={text_kind} runs={runs}");
    println!("# player={:?} exists={}", player, player.is_file());
    for run in 1..=runs {
        let label = if run == 1 { "cold" } else { "warm" };
        std::fs::write(&stamps, "").unwrap();
        let t0 = Instant::now();
        let playback = Arc::new(agent_vesper_tui::voice_playback::PlaybackOwner::new(
            player.clone(),
            None,
        ));
        let worker = agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
            agent_vesper_tui::voice_conversation::EngineSelection::Neural {
                voice_id: voice.clone(),
            },
            Arc::clone(&playback),
        );
        // PRODUCTION SHAPE: enqueue IMMEDIATELY at spawn (a fast short
        // reply arrives while the lazily-constructed host is still
        // building its engine). Readiness is recorded opportunistically
        // while we wait for the outcome.
        let t_enqueue = t0.elapsed();
        worker.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
            segment: 0,
            text: text.to_owned(),
        });
        let mut events: Vec<(String, Duration)> = Vec::new();
        let t_terminal;
        loop {
            for outcome in worker.drain() {
                match outcome {
                    agent_vesper_tui::voice_speech_worker::SpeechOutcome::Progress {
                        through_bytes,
                        ..
                    } => events.push((format!("progress/{through_bytes}"), t0.elapsed())),
                    agent_vesper_tui::voice_speech_worker::SpeechOutcome::Spoke {
                        samples, ..
                    } => events.push((format!("spoke/{samples}"), t0.elapsed())),
                    agent_vesper_tui::voice_speech_worker::SpeechOutcome::Stale { .. } => {
                        events.push(("stale".into(), t0.elapsed()))
                    }
                    agent_vesper_tui::voice_speech_worker::SpeechOutcome::Failed {
                        error, ..
                    } => events.push((format!("failed/{error}"), t0.elapsed())),
                }
            }
            let finished = events
                .iter()
                .any(|(k, _)| k.starts_with("spoke") || k.starts_with("failed") || k == "stale");
            if finished {
                t_terminal = t0.elapsed();
                break;
            }
            if t0.elapsed() > Duration::from_secs(240) {
                t_terminal = t0.elapsed();
                events.push(("timeout".into(), t_terminal));
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        println!("{label}: enqueue={t_enqueue:?} terminal={t_terminal:?}");
        for (k, v) in &events {
            println!("  {k} @ {v:?}");
        }
        let boot =
            std::fs::read_to_string(format!("{}.boot", stamps.display())).unwrap_or_default();
        for line in boot.lines() {
            println!("  boot {line}");
        }
        for line in std::fs::read_to_string(&stamps).unwrap().lines() {
            println!("  player {line}");
        }
        workers.push(worker); // keep the engine warm for the next run
    }
}
