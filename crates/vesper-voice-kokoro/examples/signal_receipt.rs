//! Device-free real-model signal regression. Reads the installed verified pack;
//! never opens a player, microphone or network connection and writes no audio.
//! Not an acoustic acceptance test. Run explicitly with --features ort.

#[cfg(not(feature = "ort"))]
fn main() {
    eprintln!("signal_receipt requires --features ort");
    std::process::exit(2);
}

#[cfg(feature = "ort")]
fn main() {
    use vesper_voice_kokoro::engine::{EngineState, StylePack, resolve_phonemizer};
    let root = vesper_voice_kokoro::pack_root();
    let EngineState::Ready(engine) = EngineState::load(&root).expect("verified installed pack")
    else {
        panic!("pack not ready");
    };
    let phonemizer = resolve_phonemizer().expect("installed phonemizer");
    let mut valid = true;
    for voice_id in ["am_michael", "af_heart"] {
        let voice = StylePack::load(&root, voice_id).expect("verified voice");
        let output = engine
            .synthesize_blocking(
                "This is a preview of the selected voice.",
                &voice,
                &phonemizer,
            )
            .expect("synthesis");
        let samples: Vec<i16> = output
            .frames
            .iter()
            .flat_map(|frame| {
                frame
                    .bytes()
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
            })
            .collect();
        let peak = samples
            .iter()
            .map(|s| i32::from(*s).abs())
            .max()
            .unwrap_or(0);
        let nonzero = samples.iter().filter(|s| **s != 0).count();
        let rms = (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>()
            / samples.len().max(1) as f64)
            .sqrt();
        println!(
            "voice={voice_id} samples={} nonzero={nonzero} peak={peak} rms={rms:.3}",
            samples.len()
        );
        // Fixed nonsensitive phrase: reject the scale-loss regression, not a
        // general loudness policy. Human audibility still requires listening.
        valid &= peak >= 256 && rms >= 32.0;
    }
    assert!(
        valid,
        "canonical PCM signal is near-zero: float-to-s16 scale regression"
    );
}
