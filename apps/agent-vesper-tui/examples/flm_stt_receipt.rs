//! VRO-17 R16: real-model no-device recognition receipt through the
//! production composition (`FlmNpuStt` + shared VAD worker + the owned
//! FLM ASR process). Run only with the real installed model present.
//!
//! Usage:
//! ```text
//! cargo run --example flm_stt_receipt --features voice-flm
//! ```
//!
//! Fixtures: canonical synthetic PCM (digital silence and an
//! envelope-modulated tone surrogate with a trailing silent tail).
//! No microphone, no speaker, no provider, no download. The surrogate
//! proves execution and silence short-circuit — NOT word accuracy.

#![cfg(feature = "voice-flm")]
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Instant;

use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::composition::blocking::ThreadPoolExecutor;
use vesper_voice::ports::VoiceStt;

fn pcm_tone_surrogate(_seconds: f64, _tail_seconds: f64) -> Vec<PcmFrame> {
    // The recorded backend-gate fixture shape (launch-gate correction):
    // three fixed-frequency sin²-enveloped segments within 4.0 s, with
    // a genuine trailing silent tail (2.65 s → 4.0 s). The installed
    // Silero defaults detect this shape (verified window [0, 1.584 s)).
    let total = (4.0 * 16_000.0) as usize;
    let segments = [
        (0.25_f64, 0.60_f64, 190.0_f64),
        (0.95, 0.70, 240.0),
        (2.10, 0.55, 210.0),
    ];
    let mut data: Vec<u8> = Vec::with_capacity(total * 2);
    for index in 0..total {
        let t = index as f64 / 16_000.0;
        let mut sample: f64 = 0.0;
        for &(start, duration, f0) in &segments {
            if t >= start && t < start + duration {
                let local = t - start;
                let envelope = (std::f64::consts::PI * local / duration).sin().max(0.0);
                sample = envelope
                    * ((2.0 * std::f64::consts::PI * f0 * local).sin()
                        + 0.5 * (2.0 * std::f64::consts::PI * 2.0 * f0 * local).sin()
                        + 0.25 * (2.0 * std::f64::consts::PI * 3.0 * f0 * local).sin())
                    / 1.75;
            }
        }
        let quantized = (sample * 22_000.0).clamp(-32_768.0, 32_767.0) as i16;
        data.extend_from_slice(&quantized.to_le_bytes());
    }
    data.chunks(2)
        .map(|chunk| PcmFrame::from_aligned(chunk.to_vec()).expect("aligned"))
        .collect()
}

fn pcm_silence(seconds: f64) -> Vec<PcmFrame> {
    let total = (seconds * 16_000.0) as usize;
    vec![0u8; total * 2]
        .chunks(2)
        .map(|chunk| PcmFrame::from_aligned(chunk.to_vec()).expect("aligned"))
        .collect()
}

fn block_on(
    adapter: &agent_vesper_tui::voice_flm::FlmNpuStt,
    audio: Vec<PcmFrame>,
    cancel: VoiceCancel,
) -> Result<vesper_voice::ports::SttTranscript, vesper_voice::VoiceError> {
    let future = adapter.transcribe(&audio, &cancel);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(result) => return result,
            std::task::Poll::Pending => {
                if std::time::Instant::now() > deadline {
                    panic!("receipt future exceeded its deadline");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] != "--i-have-the-installed-model" {
        eprintln!(
            "This probe uses the real installed FLM model. Re-run with:\n  \
             --i-have-the-installed-model"
        );
        std::process::exit(2);
    }
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let erased: Arc<dyn vesper_voice::composition::blocking::ValueExecutor> = pool;
    let adapter = agent_vesper_tui::voice_flm::FlmNpuStt::new(erased);

    // Case 1: digital silence (1 s) — must short-circuit at VAD with
    // zero ASR requests (and must NOT start the FLM service).
    let started = Instant::now();
    let silence = block_on(&adapter, pcm_silence(1.0), VoiceCancel::default());
    let silence_elapsed = started.elapsed();
    println!(
        "silence: {:?} in {:.3}s",
        silence
            .as_ref()
            .map(|t| (t.text.as_str().to_owned(), format!("{:?}", t.provenance))),
        silence_elapsed.as_secs_f64()
    );
    assert!(
        silence
            .as_ref()
            .is_ok_and(|t| t.text.as_str().trim().is_empty()),
        "silence must yield the empty no-speech transcript"
    );

    // Case 2: tone surrogate (4 s with a 2.4 s trailing tail) — VAD
    // filters, FLM recognizes, final-only transcript.
    let started = Instant::now();
    let speech = block_on(
        &adapter,
        pcm_tone_surrogate(4.0, 2.4),
        VoiceCancel::default(),
    );
    let speech_elapsed = started.elapsed();
    println!(
        "speech: {:?} in {:.3}s",
        speech
            .as_ref()
            .map(|t| (t.text.as_str().to_owned(), format!("{:?}", t.provenance))),
        speech_elapsed.as_secs_f64()
    );
    match &speech {
        Ok(transcript) => {
            assert_eq!(
                format!("{:?}", transcript.provenance),
                "InferredText",
                "a speech-positive result must be inferred text"
            );
            assert!(
                !transcript.text.as_str().trim().is_empty(),
                "the recorded gate's surrogate must produce non-empty text"
            );
        }
        Err(error) => panic!("speech-positive receipt failed: {error}"),
    }

    // Case 3: warm repeat through the same owned service.
    let started = Instant::now();
    let warm = block_on(
        &adapter,
        pcm_tone_surrogate(4.0, 2.4),
        VoiceCancel::default(),
    );
    let warm_elapsed = started.elapsed();
    println!(
        "warm repeat: ok={} in {:.3}s",
        warm.is_ok(),
        warm_elapsed.as_secs_f64()
    );
    assert!(warm.is_ok(), "the same instance must serve a second turn");

    adapter.shutdown();
    println!("PASS: composed FLM NPU recognition receipt (synthetic fixtures, no device)");
}
