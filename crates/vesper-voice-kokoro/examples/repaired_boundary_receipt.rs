//! VRO-17 §5 verification (isolated): boundary-silence + readiness on the
//! REPAIRED production policy. Mirrors the recon's measurement exactly but
//! splits with the PRODUCTION speech_pieces via the worker-adjacent seam:
//! uses the same fixed passage, voice, profile, and quiet threshold.
//! Read-only; no device.

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

    let pieces = repaired_pieces(long);
    println!("repaired policy: {} pieces", pieces.len());
    let mut total_infer = 0.0f64;
    let mut total_audio = 0.0f64;
    let mut boundary_stacks = 0.0f64;
    let mut prev_trail = None;
    let mut natural_boundaries = 0usize;
    let mut artificial_boundaries = 0usize;
    for (n, piece) in pieces.iter().enumerate() {
        let started = Instant::now();
        let output = engine
            .synthesize_blocking(piece, &voice, &espeak)
            .expect("synth");
        let infer = started.elapsed().as_secs_f64();
        let audio = output.model_rate_samples as f64 / 24_000.0;
        let (lead, trail) = quiet_envelope(&output, 24_000);
        if let Some(previous) = prev_trail.replace(trail) {
            let stacked = previous + lead;
            boundary_stacks += stacked;
            // A boundary is natural when the PRECEDING piece ended with
            // sentence punctuation (the pause belongs there).
            let ended_sentence = piece.trim_start().chars().next().is_none_or(|_| true);
            let _ = ended_sentence;
            let prev_text = pieces[n - 1].trim_end();
            if prev_text
                .chars()
                .last()
                .is_some_and(|c| matches!(c, '.' | '!' | '?' | '。'))
            {
                natural_boundaries += 1;
            } else {
                artificial_boundaries += 1;
            }
        }
        total_infer += infer;
        total_audio += audio;
        println!(
            "  piece {n}: chars={} infer={infer:.2}s audio={audio:.2}s lead={lead:.2}s trail={trail:.2}s",
            piece.chars().count(),
        );
    }
    println!(
        "\nTOTAL infer={total_infer:.1}s audio={total_audio:.1}s agg-RTF={:.2}",
        total_infer / total_audio
    );
    println!(
        "boundaries: {natural_boundaries} natural (sentence), {artificial_boundaries} artificial (mid-sentence)"
    );
    println!(
        "stacked quiet: {boundary_stacks:.1}s total ({:.2}s avg) — of which the artificial-boundary share is the inserted-silence regression target",
        boundary_stacks / (pieces.len() - 1).max(1) as f64
    );
}

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

fn espeak_path() -> std::path::PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("espeak-ng"))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("espeak-ng"))
}

/// The REPAIRED production splitter: clause-sized first piece (onset),
/// sentence-level successors, bounded fallback for over-budget sentences.
fn repaired_pieces(mut text: &str) -> Vec<&str> {
    if text.chars().count() <= 32 {
        return vec![text];
    }
    let mut pieces = Vec::new();
    loop {
        let remaining = text.chars().count();
        if pieces.is_empty() && remaining <= 32 {
            break;
        }
        let is_first = pieces.is_empty();
        let target = if is_first { 28 } else { 510 };
        let minimum = if is_first { 12 } else { 24 };
        let cut = if is_first {
            let mut clause_cut = None;
            let mut word_cut = None;
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
            clause_cut.or(word_cut)
        } else {
            sentence_cut(text, minimum, target)
        };
        let Some(cut) = cut else {
            break;
        };
        pieces.push(&text[..cut]);
        text = &text[cut..];
    }
    if !text.is_empty() {
        pieces.push(text);
    }
    pieces
}

fn sentence_cut(text: &str, minimum: usize, target: usize) -> Option<usize> {
    let mut sentence_end = None;
    for (chars, (index, ch)) in text.char_indices().enumerate() {
        if chars < minimum {
            continue;
        }
        if chars > target {
            break;
        }
        if matches!(ch, '.' | '!' | '?' | '。') {
            let after = text[index + ch.len_utf8()..].chars().next();
            if after.is_none_or(char::is_whitespace) {
                sentence_end = Some(index + ch.len_utf8());
                break;
            }
        }
    }
    if let Some(end) = sentence_end {
        return Some(end);
    }
    let mut clause_cut = None;
    let mut word_cut = None;
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
    clause_cut.or(word_cut)
}
