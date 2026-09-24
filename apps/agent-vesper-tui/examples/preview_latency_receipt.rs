//! Explicit, single real-output diagnostic. Never runs playback by default.
//! Same Kokoro adapter/PCM/player sequence as Preview, with per-stage timings;
//! not a Settings modal test and not evidence of human audibility.
#[cfg(not(feature = "voice-kokoro"))]
fn main() {
    eprintln!("requires voice-kokoro");
}
#[cfg(feature = "voice-kokoro")]
fn main() {
    use agent_vesper_tui::voice_playback::PlaybackOwner;
    use futures_util::StreamExt;
    use std::{sync::Arc, time::Instant};
    use vesper_voice::ports::{TtsChunk, VoiceProfile, VoiceTts};
    if std::env::args().nth(1).as_deref() != Some("--play-once") {
        eprintln!("Requires explicit user approval and --play-once; no playback performed.");
        return;
    }
    let start = Instant::now();
    let elapsed = || start.elapsed().as_secs_f64();
    let adapter = vesper_voice_kokoro::KokoroTts::new(Arc::new(
        vesper_voice::composition::blocking::ThreadPoolExecutor::new(1),
    ))
    .unwrap();
    println!("t={:.3}s adapter created", elapsed());
    adapter.prepare().unwrap();
    println!("t={:.3}s verified session ready", elapsed());
    let profile = VoiceProfile {
        voice_id: vesper_domain::BoundedString::new("am_michael").unwrap(),
        label: vesper_domain::BoundedString::new("Michael").unwrap(),
        sample_rate_hz: 16000,
    };
    let cancel = vesper_voice::VoiceCancel::new();
    let mut stream = vesper_voice::test_util::block_on(adapter.synthesize(
        "This is a preview of the selected voice.",
        &profile,
        &cancel,
    ))
    .unwrap();
    println!("t={:.3}s synthesis open complete", elapsed());
    let player = PlaybackOwner::new(agent_vesper_tui::resolve_player_for_preview(), None);
    player.begin_stream().unwrap();
    println!("t={:.3}s player spawned", elapsed());
    while let Some(chunk) = vesper_voice::test_util::block_on(stream.next()) {
        match chunk.unwrap() {
            TtsChunk::Audio(frame) => {
                println!(
                    "t={:.3}s PCM ready samples={}",
                    elapsed(),
                    frame.sample_count()
                );
                println!("t={:.3}s pipe write begins", elapsed());
                let receipt = player.push_pcm(frame.bytes()).unwrap();
                println!("t={:.3}s pipe write complete {receipt:?}", elapsed());
            }
            TtsChunk::Finished => break,
        }
    }
    let receipt = player.end_stream().unwrap();
    println!(
        "t={:.3}s player settled {receipt:?}; not human listening evidence",
        elapsed()
    );
}
