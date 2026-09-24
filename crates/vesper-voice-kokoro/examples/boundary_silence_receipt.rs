//! VRO-17 §2 recon (isolated): quantify the per-piece generated-silence
//! stacking at artificial boundaries — the leading/trailing quiet runs of
//! consecutive pieces concatenate. Compare current 28/48 pieces against
//! longer natural sentence units at the ENGINE level (no production change).
//! Read-only; no device; bounded.

use std::path::PathBuf;
use std::time::Instant;

const QUIET: i16 = 400;

fn main() {
    let root = vesper_voice_kokoro::pack_root();
    let engine = match vesper_voice_kokoro::engine::EngineState::load(&root).expect("state") {
        vesper_voice_kokoro::engine::EngineState::Ready(engine) => engine,
        vesper_voice_kokoro::engine::EngineState::NotReady(problem) => {
            panic!("{}", problem.description())
        }
    };
    let voice = vesper_voice_kokoro::engine::StylePack::load(&root, "am_michael").expect("voice");
    let espeak = espeak_path();

    let long = "Here is the summary of the unit. The first repair removed the fatal error flag from the player argv, because the documented default recovers device underruns instead of aborting mid stream. The second repair preserved the original write error, so a failure now names the actual layer instead of inventing a device hypothesis. The third repair contained dead streams, so a failed turn no longer poisons the next one. Together these changes restored speech for every consecutive turn in the controlled suite.";

    // Variant A: current production pieces.
    let current: Vec<&str> = split_like_production(long);
    // Variant B: whole natural sentences (hygiene units as single pieces —
    // the pre-latency-repair steady-state shape).
    let sentences: Vec<&str> = long
        .split_inclusive(['.', '!', '?'])
        .map(str::trim_start)
        .filter(|s| !s.trim().is_empty())
        .collect();

    for (name, pieces) in [("current-28/48", &current), ("whole-sentences", &sentences)] {
        println!("=== {name} ({} pieces) ===", pieces.len());
        let mut total_infer = 0.0f64;
        let mut total_audio = 0.0f64;
        let mut boundary_stacks = 0.0f64;
        let mut prev_trail = None;
        let mut first_piece_audio = None;
        for (n, piece) in pieces.iter().enumerate() {
            let started = Instant::now();
            let output = engine
                .synthesize_blocking(piece, &voice, &espeak)
                .expect("synth");
            let infer = started.elapsed().as_secs_f64();
            let audio = output.model_rate_samples as f64 / 24_000.0;
            let (lead, trail) = quiet_envelope(&output, 24_000);
            if n == 0 {
                first_piece_audio = Some(audio);
            }
            if let Some(previous) = prev_trail.replace(trail) {
                boundary_stacks += previous + lead;
            }
            total_infer += infer;
            total_audio += audio;
            println!(
                "  piece {n}: chars={} infer={infer:.2}s audio={audio:.2}s RTF={:.2} lead={lead:.2}s trail={trail:.2}s",
                piece.chars().count(),
                if audio > 0.0 { infer / audio } else { 0.0 },
            );
        }
        println!(
            "  TOTAL infer={total_infer:.1}s audio={total_audio:.1}s agg-RTF={:.2}",
            total_infer / total_audio
        );
        println!(
            "  generated quiet stacked at the {} artificial boundaries: {boundary_stacks:.1}s total ({:.2}s avg)",
            pieces.len() - 1,
            boundary_stacks / (pieces.len() - 1).max(1) as f64
        );
        println!(
            "  onset trade-off: first-piece audio (playable after one inference) = {:.2}s",
            first_piece_audio.unwrap_or(0.0)
        );
        println!();
    }
}

/// Leading/trailing quiet-run durations at the model rate, over the
/// canonical samples (approximation: amplitude threshold, not phonetics).
fn quiet_envelope(output: &vesper_voice_kokoro::engine::SynthesisOutput, rate: u32) -> (f64, f64) {
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
    let rate = rate as usize;
    let lead = samples.iter().take_while(|s| s.abs() < QUIET).count() as f64 / rate as f64;
    let trail = samples.iter().rev().take_while(|s| s.abs() < QUIET).count() as f64 / rate as f64;
    (lead, trail)
}

fn espeak_path() -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("espeak-ng"))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("espeak-ng"))
}

/// Production-mirroring splitter (12,28)/(24,48) word/clause cuts.
fn split_like_production(mut text: &str) -> Vec<&str> {
    if text.chars().count() <= 32 {
        return vec![text];
    }
    let mut pieces = Vec::new();
    loop {
        let remaining = text.chars().count();
        let threshold = if pieces.is_empty() { 32 } else { 48 };
        if remaining <= threshold {
            break;
        }
        let (minimum, target) = if pieces.is_empty() {
            (12, 28)
        } else {
            (24, 48)
        };
        let mut word_cut = None;
        let mut clause_cut = None;
        for (chars, (index, ch)) in text.char_indices().enumerate() {
            if chars > target {
                break;
            }
            if ch.is_whitespace() && chars >= minimum {
                word_cut = Some(index + ch.len_utf8());
                if text[..index].ends_with([',', ';', ':', '—']) {
                    clause_cut = word_cut;
                }
            }
        }
        let Some(cut) = clause_cut.or(word_cut) else {
            break;
        };
        pieces.push(&text[..cut]);
        text = &text[cut..];
    }
    pieces.push(text);
    pieces
}
