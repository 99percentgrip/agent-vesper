//! Device-free Preview-path first-PCM timing through the production speech worker.
//! Uses the installed Kokoro pack and a temporary sink; never opens an audio device.
#[cfg(not(all(unix, feature = "voice-kokoro")))]
fn main() {
    eprintln!("requires Unix and voice-kokoro");
}

#[cfg(all(unix, feature = "voice-kokoro"))]
fn main() {
    use agent_vesper_tui::{
        voice_conversation::EngineSelection,
        voice_playback::PlaybackOwner,
        voice_speech_worker::{SpeechJob, SpeechOutcome, SpeechWorker},
    };
    use std::{
        os::unix::fs::PermissionsExt,
        sync::Arc,
        time::{Duration, Instant},
    };

    const PREVIEW_PHRASE: &str = "This is a preview of the selected voice.";
    let temp = tempfile::tempdir().expect("temporary root");
    let marker = temp.path().join("first-pcm");
    let player = temp.path().join("player");
    std::fs::write(
        &player,
        format!(
            "#!/bin/sh\nhead -c 2 >/dev/null\ntouch '{}'\ncat >/dev/null\n",
            marker.display()
        ),
    )
    .expect("write sink");
    std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o700))
        .expect("make sink executable");

    let process_start = Instant::now();
    let worker = SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::new(PlaybackOwner::new(player, None)),
    );
    let preparation_deadline = Instant::now() + Duration::from_secs(20);
    while !worker.stage_status().is_empty() {
        assert!(
            Instant::now() < preparation_deadline,
            "worker preparation deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    println!(
        "worker_ready_s={:.3}",
        process_start.elapsed().as_secs_f64()
    );

    for attempt in 1..=2 {
        let _ = std::fs::remove_file(&marker);
        let start = Instant::now();
        worker.enqueue(SpeechJob {
            segment: attempt,
            text: PREVIEW_PHRASE.into(),
        });
        let deadline = start + Duration::from_secs(30);
        let mut first_pcm = None;
        let mut settled = false;
        while Instant::now() < deadline {
            if first_pcm.is_none() && marker.exists() {
                let elapsed = start.elapsed().as_secs_f64();
                println!("attempt={attempt} enqueue_to_first_pcm_s={elapsed:.3}");
                first_pcm = Some(elapsed);
            }
            for outcome in worker.drain() {
                match outcome {
                    SpeechOutcome::Progress { .. } => {}
                    SpeechOutcome::Spoke { segment, samples } if segment == attempt => {
                        println!(
                            "attempt={attempt} enqueue_to_settled_s={:.3} samples={samples}",
                            start.elapsed().as_secs_f64()
                        );
                        assert!(first_pcm.is_some(), "PCM marker must precede settlement");
                        settled = true;
                    }
                    SpeechOutcome::Spoke { .. } => {}
                    SpeechOutcome::Failed { error, .. } => panic!("preview failed: {error}"),
                    SpeechOutcome::Stale { .. } => panic!("preview unexpectedly became stale"),
                }
            }
            if settled {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(settled, "preview timing deadline on attempt {attempt}");
    }
    worker.shutdown();
}
