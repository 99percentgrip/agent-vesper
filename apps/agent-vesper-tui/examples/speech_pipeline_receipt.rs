//! Explicit two-sentence listening/timing probe; never plays without --play-two.
//! --serial waits for the first player drain before enqueueing the second unit.
#[cfg(not(feature = "voice-kokoro"))]
fn main() {
    eprintln!("requires voice-kokoro");
}

#[cfg(feature = "voice-kokoro")]
fn main() {
    use agent_vesper_tui::{
        voice_conversation::EngineSelection,
        voice_playback::PlaybackOwner,
        voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker},
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.iter().any(|arg| arg == "--play-two") {
        eprintln!("Requires explicit user approval and --play-two; no playback performed.");
        return;
    }
    let serial = args.iter().any(|arg| arg == "--serial");
    println!(
        "Real selected-path Kokoro am_michael; serial={serial}; no microphone/provider; transport is not audibility."
    );
    let start = Instant::now();
    let worker = SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::new(PlaybackOwner::new(
            agent_vesper_tui::resolve_player_for_preview(),
            None,
        )),
    );
    let enqueue = |segment| {
        worker.enqueue(SpeechJob {
            segment,
            text: match segment {
                1 => "This is the first sentence in the timing check.",
                _ => "This is the second sentence in the timing check.",
            }
            .into(),
        })
    };
    enqueue(1);
    if !serial {
        enqueue(2);
    }
    let mut completed = 0;
    let mut last_stage = String::new();
    while start.elapsed() < Duration::from_secs(60) {
        let stage = worker.stage_status();
        // Ignore the changing elapsed numerals when detecting stage transitions.
        let labels: String = stage
            .chars()
            .filter(|c| !c.is_ascii_digit() && *c != '.')
            .collect();
        if labels != last_stage && !stage.is_empty() {
            println!("t={:.3}s {stage}", start.elapsed().as_secs_f64());
            last_stage = labels;
        }
        for outcome in worker.drain() {
            println!("t={:.3}s {outcome:?}", start.elapsed().as_secs_f64());
            match outcome {
                SpeechOutcome::Spoke { .. } => {
                    completed += 1;
                    if serial && completed == 1 {
                        enqueue(2);
                    }
                }
                SpeechOutcome::Failed { .. } | SpeechOutcome::Stale { .. } => {
                    worker.stop();
                    panic!("probe did not complete");
                }
                SpeechOutcome::Progress { .. } => {}
            }
        }
        if completed == 2 {
            worker.shutdown();
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    worker.stop();
    panic!("probe deadline exceeded");
}
