//! Isolated: what units does the real hygiene gate emit for Markdown
//! formatting lines, and does any emitted unit reach phonemization with
//! zero pronounceable content (the exact production error)?
//! Real gate + real phonemize + real vocab. Read-only; no model, no audio.

fn main() {
    let espeak = espeak_path();
    let vocab =
        vesper_voice_kokoro::vocab::PhonemeVocab::load_verified(&vesper_voice_kokoro::pack_root())
            .expect("vocab");

    let answers: &[(&str, &str)] = &[
        (
            "table",
            "Here is the table.\n\n| Gate | Result |\n| --- | --- |\n| fmt | pass |\n\nAll rows checked.",
        ),
        (
            "heading+rule",
            "Intro line.\n\n## Section\n\n---\n\nBody text follows here.\n\nDone.",
        ),
        (
            "bullets",
            "Summary.\n\n* first point here\n* second point here\n* third point\n\nEnd.",
        ),
        (
            "numbers-list",
            "Steps.\n\n1. Do the first thing\n2. Then the second\n\nReady.",
        ),
        ("stars", "Note.\n\n***\n\nAfter the rule."),
    ];

    for (label, answer) in answers {
        println!("=== {label} ===");
        let mut gate =
            vesper_voice::hygiene::HygieneGate::new(vesper_voice::config::CaptureBudget::default());
        // Stream by whitespace-terminated tokens (delta-like).
        let mut units = Vec::new();
        for chunk in answer.split_inclusive(|c: char| c.is_whitespace()) {
            units.extend(gate.push(chunk).expect("gate"));
        }
        units.extend(gate.finalize().expect("finalize"));

        for unit in &units {
            let text = unit.text.as_str();
            let mut verdict = String::new();
            for (index, piece) in split_like_production(text).iter().enumerate() {
                match vesper_voice_kokoro::phonemize::phonemize_blocking(piece, &espeak) {
                    Err(error) => verdict.push_str(&format!(" P{index}!PHON({error})")),
                    Ok(phonemes) => {
                        if let Err(error) =
                            vesper_voice_kokoro::phonemize::ids_from_phonemes(&vocab, &phonemes)
                        {
                            verdict.push_str(&format!(" P{index}!ZERO({error})"));
                        }
                    }
                }
            }
            println!(
                "  unit {} {:?} markers={:?}{}",
                unit.segment,
                &text[..text.len().min(46)],
                unit.markers,
                if verdict.is_empty() {
                    String::new()
                } else {
                    format!(" -->{verdict}")
                }
            );
        }
    }
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

fn espeak_path() -> std::path::PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("espeak-ng"))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("espeak-ng"))
}
