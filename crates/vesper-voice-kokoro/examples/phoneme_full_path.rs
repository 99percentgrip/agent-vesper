//! Isolated: full production-path reproduction of the empty-phoneme error.
//! Real HygieneGate -> real speech_pieces split -> real phonemize_blocking
//! -> real ids_from_phonemes, over formatting-bearing MULTI-SENTENCE input
//! with paragraph structure (the shape Alex's long replies have).
//! Read-only; no model run, no audio, no device.

fn main() {
    let vocab =
        vesper_voice_kokoro::vocab::PhonemeVocab::load_verified(&vesper_voice_kokoro::pack_root())
            .expect("vocab");
    let espeak = std::path::PathBuf::from(
        std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("espeak-ng"))
                    .find(|candidate| candidate.is_file())
            })
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "espeak-ng".into()),
    );

    // Long answer with headings, bullets, a table, inline code, and
    // paragraph breaks — streamed through the gate chunk by chunk like
    // assistant deltas.
    let answer = "Here is the summary of the repair work completed today.

## Verification

The gates all passed. | Gate | Result | shows every row green. \
Run `cargo test --features voice-kokoro` twice for stability.

* item one drained bounded
* item two settled once

Approximately ~3 files changed and the value is ^2 for the exponent. \
Cost was $5 at 50% off.

Done. Let me know what you observe.";

    let mut gate =
        vesper_voice::hygiene::HygieneGate::new(vesper_voice::config::CaptureBudget::default());
    // Stream in delta-sized chunks.
    let mut units = Vec::new();
    for chunk in answer.split_inclusive(|c: char| c.is_whitespace()) {
        // Chunk at ~word granularity like real deltas (chunk = a few words).
        units.extend(gate.push(chunk).expect("gate"));
    }
    units.extend(gate.finalize().expect("finalize"));

    println!("gated units: {}", units.len());
    let mut failures = 0;
    for unit in &units {
        let text = unit.text.as_str();
        if text.trim().is_empty() {
            println!(
                "  unit {} (EMPTY — dropped, markers {:?})",
                unit.segment, unit.markers
            );
            continue;
        }
        // The production worker splits each unit into pieces.
        for (index, piece) in split_like_production(text).iter().enumerate() {
            match vesper_voice_kokoro::phonemize::phonemize_blocking(piece, &espeak) {
                Err(error) => {
                    failures += 1;
                    println!(
                        "  unit {} piece {} PHONEMIZE-ERR {error}\n    text={piece:?}",
                        unit.segment, index
                    );
                }
                Ok(phonemes) => {
                    if let Err(error) =
                        vesper_voice_kokoro::phonemize::ids_from_phonemes(&vocab, &phonemes)
                    {
                        failures += 1;
                        println!(
                            "  unit {} piece {} ZERO-IDS {error}\n    piece={piece:?}\n    phonemes={phonemes:?}",
                            unit.segment, index
                        );
                    }
                }
            }
        }
    }
    println!("\nfailures: {failures}");
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
