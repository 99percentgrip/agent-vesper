//! VRO-17 §2 recon (isolated): per-PIECE inference timing and RTF through
//! the real engine (no playback), plus waveform quiet-run inspection of the
//! generated audio at piece boundaries. One persistent engine session.
//! Read-only; no device; no network; bounded.

use std::path::PathBuf;
use std::time::Instant;

const QUIET: i16 = 400;

fn main() {
    let root = vesper_voice_kokoro::pack_root();
    let espeak = espeak_path();
    let engine = match vesper_voice_kokoro::engine::EngineState::load(&root).expect("engine state")
    {
        vesper_voice_kokoro::engine::EngineState::Ready(engine) => engine,
        vesper_voice_kokoro::engine::EngineState::NotReady(problem) => {
            panic!("engine not ready: {}", problem.description())
        }
    };
    let voice = vesper_voice_kokoro::engine::StylePack::load(&root, "am_michael").expect("voice");

    // The long-answer passage split exactly like production pieces.
    let long = "Here is the summary of the unit. The first repair removed the fatal error flag from the player argv, because the documented default recovers device underruns instead of aborting mid stream. The second repair preserved the original write error, so a failure now names the actual layer instead of inventing a device hypothesis. The third repair contained dead streams, so a failed turn no longer poisons the next one. Together these changes restored speech for every consecutive turn in the controlled suite.";

    println!("piece\tchars\tinfer_s\taudio_s\tRTF\tlead_q\ttrail_q\tmax_interior_q");
    let mut totals = (0.0f64, 0.0f64, 0usize);
    for (n, piece) in split_like_production(long).iter().enumerate() {
        let piece: &str = piece;
        let chars = piece.chars().count();
        let started = Instant::now();
        let output = engine
            .synthesize_blocking(piece, &voice, &espeak)
            .expect("synth");
        let infer = started.elapsed().as_secs_f64();
        // Canonical output is 16 kHz s16 mono; model_rate_samples is the
        // pre-resample count at 24 kHz.
        let audio = output.model_rate_samples as f64 / 24_000.0;
        let rtf = if audio > 0.0 { infer / audio } else { 0.0 };
        // Quiet-run analysis on the canonical samples.
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
        let lead = samples.iter().take_while(|s| s.abs() < QUIET).count() as f64 / 16000.0;
        let trail = samples.iter().rev().take_while(|s| s.abs() < QUIET).count() as f64 / 16000.0;
        let mut interior = 0;
        let mut run = 0;
        for s in &samples {
            if s.abs() < QUIET {
                run += 1;
                interior = interior.max(run);
            } else {
                run = 0;
            }
        }
        let interior_s = interior as f64 / 16000.0;
        println!(
            "{n}\t{chars}\t{infer:.2}\t{audio:.2}\t{rtf:.2}\t{lead:.2}\t{trail:.2}\t{interior_s:.2}"
        );
        totals.0 += infer;
        totals.1 += audio;
        totals.2 += 1;
    }
    println!(
        "\ntotal: {} pieces, infer={:.1}s, audio={:.1}s, aggregate RTF={:.2}",
        totals.2,
        totals.0,
        totals.1,
        totals.0 / totals.1
    );
    println!(
        "quiet-run notes: threshold ±{QUIET}/32768 (~1.2% FS); approximates low amplitude, \
         not phonetic silence; interior runs include natural punctuation pauses."
    );
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
