//! R3 defect reproduction: run a neural synthesis through the exact
//! production call shape the TUI uses (`speak_unit` = `block_on` of the
//! adapter's synthesize future ON the calling thread, then a bounded
//! stream-drain loop), inside a tokio-runtime context like the TUI's
//! `apply_agent_progress`. Debug profile, timed, hard timeouts: a hang
//! reproduces Alex's "not replying" as a timeout, success as PCM counts.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use vesper_voice::ports::VoiceTts as _;

fn main() {
    println!("repro: neural synthesis on the TUI call path (debug build)");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let load_started = Instant::now();
        let adapter = Arc::new(
            vesper_voice_kokoro::KokoroTts::new(Arc::new(
                vesper_voice::composition::blocking::ThreadPoolExecutor::new(1),
            ))
            .expect("adapter"),
        );
        println!(
            "[t+{:?}] adapter constructed (engine loads lazily on first use)",
            load_started.elapsed()
        );

        let profile = vesper_voice::ports::VoiceProfile {
            voice_id: vesper_domain::BoundedString::<128>::new("am_michael".to_owned())
                .expect("fits"),
            label: vesper_domain::BoundedString::<128>::new("Michael".to_owned()).expect("fits"),
            sample_rate_hz: vesper_voice::audio::CANONICAL_SAMPLE_RATE_HZ,
        };

        // speak_unit shape: block_on(synthesize(...)) on THIS thread.
        let synth_started = Instant::now();
        let cancel = vesper_voice::VoiceCancel::new();
        let opened = tokio::time::timeout(
            Duration::from_secs(90),
            adapter.synthesize(
                "Understood. This is a voice reply from Michael.",
                &profile,
                &cancel,
            ),
        )
        .await;
        let mut stream = match opened {
            Ok(Ok(stream)) => {
                println!(
                    "[t+{:?}] synthesize() open OK; draining stream…",
                    synth_started.elapsed()
                );
                stream
            }
            Ok(Err(error)) => {
                println!("OPEN FAILED after {:?}: {error}", synth_started.elapsed());
                return;
            }
            Err(_) => {
                println!("OPEN TIMEOUT (>90s) — hang reproduced");
                std::process::exit(4);
            }
        };

        let mut audio = 0usize;
        let mut finished = false;
        loop {
            match tokio::time::timeout(Duration::from_secs(90), stream.next()).await {
                Ok(Some(Ok(vesper_voice::ports::TtsChunk::Audio(frame)))) => {
                    audio += frame.sample_count();
                }
                Ok(Some(Ok(vesper_voice::ports::TtsChunk::Finished))) => {
                    finished = true;
                    break;
                }
                Ok(Some(Err(error))) => {
                    println!("MID-STREAM ERROR: {error}");
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    println!("STREAM POLL TIMEOUT (>90s) after {audio} samples — hang reproduced");
                    std::process::exit(4);
                }
            }
        }
        println!(
            "DONE: {audio} canonical samples, finished={finished}, total {:?}",
            synth_started.elapsed()
        );
    });
}
