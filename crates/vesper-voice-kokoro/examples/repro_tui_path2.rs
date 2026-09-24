//! R3 defect reproduction #2: the FULL production shape — including the
//! ORT run serialized behind the adapter's synthesis lock on the SAME
//! blocking pool the TUI's worker threads use, then a second unit while
//! the first stream was cancelled mid-drain (Alex's stop-during-speech
//! flow). Any hang/timeout = the "not replying" defect.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use vesper_voice::ports::VoiceTts as _;

fn main() {
    println!("repro #2: two sequential units + mid-stream cancel (TUI shape)");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let adapter = Arc::new(
            vesper_voice_kokoro::KokoroTts::new(Arc::new(
                vesper_voice::composition::blocking::ThreadPoolExecutor::new(1),
            ))
            .expect("adapter"),
        );
        let profile = |voice: &str| vesper_voice::ports::VoiceProfile {
            voice_id: vesper_domain::BoundedString::<128>::new(voice.to_owned()).expect("fits"),
            label: vesper_domain::BoundedString::<128>::new("v".to_owned()).expect("fits"),
            sample_rate_hz: vesper_voice::audio::CANONICAL_SAMPLE_RATE_HZ,
        };

        // Unit 1: cancel AFTER audio starts (stop-during-speech).
        let cancel1 = vesper_voice::VoiceCancel::new();
        let michael1 = profile("am_michael");
        let t1 = Instant::now();
        let mut s1 = adapter
            .synthesize("First unit that will be interrupted.", &michael1, &cancel1)
            .await
            .expect("open 1");
        let first = s1.next().await;
        println!(
            "[t+{:?}] unit1 first chunk: {}",
            t1.elapsed(),
            matches!(&first, Some(Ok(_)))
        );
        cancel1.cancel();
        // speak_unit breaks the loop on cancel; the stream drops here.
        drop(s1);
        println!("[t+{:?}] unit1 cancelled + dropped", t1.elapsed());

        // Unit 2 must NOT hang or be poisoned by unit 1's cancellation.
        let cancel2 = vesper_voice::VoiceCancel::new();
        let michael_profile = profile("am_michael");
        let t2 = Instant::now();
        let opened = tokio::time::timeout(
            Duration::from_secs(90),
            adapter.synthesize(
                "Second unit must speak normally.",
                &michael_profile,
                &cancel2,
            ),
        )
        .await;
        let mut s2 = match opened {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                println!("UNIT2 OPEN FAILED after {:?}: {error}", t2.elapsed());
                std::process::exit(5);
            }
            Err(_) => {
                println!("UNIT2 OPEN TIMEOUT — poisoning/hang reproduced");
                std::process::exit(5);
            }
        };
        let mut audio = 0usize;
        loop {
            match tokio::time::timeout(Duration::from_secs(90), s2.next()).await {
                Ok(Some(Ok(vesper_voice::ports::TtsChunk::Audio(frame)))) => {
                    audio += frame.sample_count();
                }
                Ok(Some(Ok(vesper_voice::ports::TtsChunk::Finished))) => break,
                Ok(Some(Err(error))) => {
                    println!("UNIT2 MID-STREAM ERROR: {error}");
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    println!("UNIT2 STREAM TIMEOUT — hang reproduced");
                    std::process::exit(5);
                }
            }
        }
        println!(
            "UNIT2 DONE: {audio} samples in {:?} — engine not poisoned",
            t2.elapsed()
        );

        // Unit 3: the OTHER voice (Heart), proving per-voice style loads
        // work after cancellation on Michael.
        let t3 = Instant::now();
        let cancel3 = vesper_voice::VoiceCancel::new();
        let heart_profile = profile("af_heart");
        let mut s3 = adapter
            .synthesize(
                "Heart answers after the interruption.",
                &heart_profile,
                &cancel3,
            )
            .await
            .expect("open 3");
        let mut audio3 = 0usize;
        while let Some(item) = tokio::time::timeout(Duration::from_secs(90), s3.next())
            .await
            .unwrap_or(None)
        {
            match item {
                Ok(vesper_voice::ports::TtsChunk::Audio(frame)) => audio3 += frame.sample_count(),
                Ok(vesper_voice::ports::TtsChunk::Finished) => break,
                Err(error) => {
                    println!("UNIT3 ERROR: {error}");
                    break;
                }
            }
        }
        println!("UNIT3 (Heart) DONE: {audio3} samples in {:?}", t3.elapsed());
    });
}
