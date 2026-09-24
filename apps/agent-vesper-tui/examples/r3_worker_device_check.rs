//! R3 worker device check (evidence helper): drives the REAL
//! SpeechWorker with the REAL pack and the REAL aplay, exactly the
//! production wiring. Prints worker outcomes with timings so a silent
//! speaker path shows up as bytes=0 / never-Drained.
//!
//! NOTE: this PLAYS audio through the default device.

use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    println!("worker device check: real pack + real aplay (audio will play)");
    let root = match std::env::var_os("XDG_DATA_HOME") {
        Some(data) => std::path::PathBuf::from(data)
            .join("agent-vesper")
            .join("voice-pack"),
        None => std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"))
            .join(".local/share/agent-vesper/voice-pack"),
    };
    let state = match vesper_voice_kokoro::engine::EngineState::load(&root) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("ENGINE LOAD FAILED: {error}");
            std::process::exit(2);
        }
    };
    match state {
        vesper_voice_kokoro::engine::EngineState::Ready(_) => println!("engine: Ready"),
        vesper_voice_kokoro::engine::EngineState::NotReady(problem) => {
            eprintln!("NOT READY: {}", problem.description());
            std::process::exit(2);
        }
    }
    // Player path: same resolution the host uses.
    let player = std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("aplay"))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("aplay"));
    println!("player: {}", player.display());
    let playback = Arc::new(agent_vesper_tui::voice_playback::PlaybackOwner::new(
        player, None,
    ));
    let worker = agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
        agent_vesper_tui::voice_conversation::EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::clone(&playback),
    );
    let started = Instant::now();
    worker.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
        segment: 1,
        text: "Understood. The voice worker is speaking through your speakers now.".into(),
    });
    let deadline = started + Duration::from_secs(120);
    let mut progress_bytes = 0u64;
    let mut spoke = false;
    while Instant::now() < deadline {
        for outcome in worker.drain() {
            match outcome {
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Progress {
                    segment: _,
                    through_bytes,
                } => {
                    if progress_bytes == 0 {
                        println!(
                            "[t+{:?}] FIRST bytes handed to player: {through_bytes}",
                            started.elapsed()
                        );
                    }
                    progress_bytes = through_bytes;
                }
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Spoke { samples, .. } => {
                    println!(
                        "[t+{:?}] SPOKE: {samples} samples ({progress_bytes} bytes total)",
                        started.elapsed()
                    );
                    spoke = true;
                }
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Stale { .. } => {
                    println!("[t+{:?}] STALE (unexpected)", started.elapsed());
                }
                agent_vesper_tui::voice_speech_worker::SpeechOutcome::Failed { error, .. } => {
                    println!("[t+{:?}] FAILED: {error}", started.elapsed());
                    std::process::exit(3);
                }
            }
        }
        if spoke {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !spoke {
        println!("NEVER SPOKE within 120 s — device path broken");
        std::process::exit(4);
    }
    worker.shutdown();
    println!("worker device check OK");
}
