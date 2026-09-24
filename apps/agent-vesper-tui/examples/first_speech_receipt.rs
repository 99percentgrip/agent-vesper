//! Explicit long-unit latency comparison. --measure uses a no-device sink;
//! --play-one adds one authorized production playback. Never a provider call.
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
    use vesper_voice::ports::{VoiceProfile, VoiceTts};
    let mode = std::env::args().nth(1).unwrap_or_default();
    if !matches!(mode.as_str(), "--measure" | "--play-one") {
        eprintln!("Explicit --measure (no device) or separately authorized --play-one required.");
        return;
    }
    const TEXT: &str = "Keep the mutex guard out of blocking audio playback because a blocked writer can prevent Stop from acquiring the lock and leave the interface unresponsive until the player drains the pipe.";
    let adapter = vesper_voice_kokoro::KokoroTts::new(Arc::new(
        vesper_voice::composition::blocking::ThreadPoolExecutor::new(1),
    ))
    .unwrap();
    adapter.prepare().unwrap();
    let profile = VoiceProfile {
        voice_id: vesper_domain::BoundedString::new("am_michael").unwrap(),
        label: vesper_domain::BoundedString::new("Michael").unwrap(),
        sample_rate_hz: 16000,
    };
    let cancel = vesper_voice::VoiceCancel::new();
    let baseline_start = Instant::now();
    let baseline_stream =
        vesper_voice::test_util::block_on(adapter.synthesize(TEXT, &profile, &cancel)).unwrap();
    println!(
        "warm whole-unit synthesis open: {:.3}s; {} text bytes; no playback",
        baseline_start.elapsed().as_secs_f64(),
        TEXT.len()
    );
    drop(baseline_stream);
    drop(adapter);
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("first-pcm");
    let player = if mode == "--play-one" {
        agent_vesper_tui::resolve_player_for_preview()
    } else {
        let player = temp.path().join("player");
        std::fs::write(
            &player,
            format!(
                "#!/bin/sh\nhead -c 2 >/dev/null\ntouch '{}'\ncat >/dev/null\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o700)).unwrap();
        player
    };
    let worker = SpeechWorker::spawn(
        EngineSelection::Neural {
            voice_id: "am_michael".into(),
        },
        Arc::new(PlaybackOwner::new(player, None)),
    );
    let wait = Instant::now();
    while !worker.stage_status().is_empty() {
        assert!(
            wait.elapsed() < Duration::from_secs(20),
            "worker preparation deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let start = Instant::now();
    worker.enqueue(SpeechJob {
        segment: 1,
        text: TEXT.into(),
    });
    let mut first = false;
    let mut first_send = false;
    while start.elapsed() < Duration::from_secs(60) {
        if !first_send && worker.stage_status().contains("Sending PCM") {
            println!(
                "t={:.3}s first observed Sending PCM stage; not acoustic onset",
                start.elapsed().as_secs_f64()
            );
            first_send = true;
        }
        if !first && marker.exists() {
            println!(
                "t={:.3}s first PCM observed by fixture player",
                start.elapsed().as_secs_f64()
            );
            first = true;
        }
        for outcome in worker.drain() {
            println!("t={:.3}s {outcome:?}", start.elapsed().as_secs_f64());
            match outcome {
                SpeechOutcome::Spoke { .. } => {
                    worker.shutdown();
                    return;
                }
                SpeechOutcome::Failed { .. } | SpeechOutcome::Stale { .. } => {
                    worker.stop();
                    panic!("speech probe failed");
                }
                SpeechOutcome::Progress { .. } => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    worker.stop();
    panic!("speech probe deadline");
}
