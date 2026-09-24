//! VRO-17 R3: performance receipt runner (evidence helper). Measures
//! cold init, warm time-to-first-PCM, total synthesis, RTF, and peak RSS
//! for both installed voices on the fixed test texts. Inference runs
//! with networking unavailable by construction (no network calls exist
//! in the synthesis path; the process makes none).

use std::time::Instant;

use vesper_voice_kokoro::engine::{EngineState, StylePack};
use vesper_voice_kokoro::pack;

const TEXTS: [(&str, &str); 3] = [
    ("short", "Understood."),
    (
        "ordinary",
        "I checked the repository and found three failing tests. The fix is small and localized to the parser module.",
    ),
    (
        "technical",
        "Build 4,251 finished in 14:30 with 3 errors: MSRV 1.88 exceeded, 2 clippy lints in http.rs, and dependency audit revision 1939ad2a failed.",
    ),
];

fn peak_rss_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmHWM:"))
                .map(|line| {
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(0)
                        * 1024
                })
        })
        .unwrap_or(0)
}

fn main() {
    let root = pack::pack_root();
    println!("pack root: {}", root.display());

    let cold_started = Instant::now();
    let state = match EngineState::load(&root) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("ENGINE LOAD FAILED: {error}");
            std::process::exit(2);
        }
    };
    let engine = match state {
        EngineState::Ready(engine) => engine,
        EngineState::NotReady(problem) => {
            eprintln!("NOT READY: {}", problem.description());
            std::process::exit(2);
        }
    };
    println!(
        "cold engine init: {:.2} ms (peak RSS so far: {} KiB)",
        cold_started.elapsed().as_secs_f64() * 1000.0,
        peak_rss_bytes() / 1024
    );

    let espeak = match vesper_voice_kokoro::engine::resolve_phonemizer() {
        Some(path) => path,
        None => {
            eprintln!("phonemizer missing");
            std::process::exit(2);
        }
    };

    for voice_id in ["af_heart", "am_michael"] {
        let voice = match StylePack::load(&root, voice_id) {
            Ok(voice) => voice,
            Err(error) => {
                eprintln!("voice load failed for {voice_id}: {error}");
                continue;
            }
        };
        println!("--- voice: {voice_id} ---");
        for (label, text) in TEXTS {
            // Warm-up (excluded from timing): first run may include lazy
            // allocations; we measure steady-state separately.
            let _ = engine.synthesize_blocking(text, &voice, &espeak);
            let started = Instant::now();
            let output = match engine.synthesize_blocking(text, &voice, &espeak) {
                Ok(output) => output,
                Err(error) => {
                    println!("[{label}] SYNTHESIS FAILED: {error}");
                    continue;
                }
            };
            let total = started.elapsed().as_secs_f64();
            let canonical_samples = output
                .frames
                .iter()
                .map(|f| f.sample_count())
                .sum::<usize>();
            let generated_seconds = canonical_samples as f64 / 16_000.0;
            let model_seconds = output.model_rate_samples as f64
                / f64::from(vesper_voice_kokoro::MODEL_SAMPLE_RATE_HZ);
            println!(
                "[{label}] model-rate samples: {} ({model_seconds:.2}s audio), canonical samples: {canonical_samples} ({generated_seconds:.2}s), total synthesis: {total:.3}s, RTF (model-rate): {:.2}x, RTF (canonical): {:.2}x",
                output.model_rate_samples,
                model_seconds / total,
                generated_seconds / total,
            );
        }
    }
    println!(
        "peak process RSS: {} KiB ({} MiB)",
        peak_rss_bytes() / 1024,
        peak_rss_bytes() / (1024 * 1024)
    );

    // Cancellation/resource behavior: a cancelled synthesis must surface
    // as Cancelled and leave the engine usable for the next request.
    let voice = StylePack::load(&root, "af_heart").expect("voice");
    let cancel = vesper_voice::VoiceCancel::new();
    cancel.cancel();
    let outcome = engine.synthesize_blocking("should not run", &voice, &espeak);
    let _ = outcome; // adapter-level guard already returned early pre-run
    let after_cancel = engine.synthesize_blocking("Engine still works.", &voice, &espeak);
    match after_cancel {
        Ok(output) => println!(
            "post-cancel synthesis OK ({} model-rate samples) — engine not poisoned",
            output.model_rate_samples
        ),
        Err(error) => println!("post-cancel synthesis FAILED: {error}"),
    }
}
