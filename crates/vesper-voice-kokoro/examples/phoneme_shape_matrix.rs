//! Isolated: complete the empty-phoneme matrix — which formatting shapes
//! emit units that reach ZERO pronounceable phonemes. Real gate+phonemize+
//! vocab. Read-only; no model, no audio.

fn main() {
    let espeak = espeak_path();
    let vocab =
        vesper_voice_kokoro::vocab::PhonemeVocab::load_verified(&vesper_voice_kokoro::pack_root())
            .expect("vocab");

    let answers: &[(&str, &str)] = &[
        (
            "numbered list",
            "Steps.\n\n1. Do the first thing\n2. Then the second\n3. Finally the third\n\nReady.",
        ),
        ("numbered dense", "Plan.\n\n1. A\n2. B\n3. C\n\nGo."),
        (
            "bullets alone",
            "Points.\n\n- one thing\n- two thing\n\nEnd.",
        ),
        (
            "version line",
            "Fixed in v2.4.1. The rest follows here normally.",
        ),
        ("bare version", "v2.4.1"),
        ("decimal sentence", "The ratio is 3.14. It is close enough."),
        ("ellipsis line", "Hmm… done."),
        ("section symbols", "See § 3 for the details of the change."),
        ("caret math", "The exponent is ^ 2 in this formula."),
        ("tilde approx", "About ~ 3 files changed in total."),
        ("empty-code-fence", "Look.\n\n```\n\n```\n\nDone."),
        ("table-only", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
    ];

    let mut failing_shapes = Vec::new();
    for (label, answer) in answers {
        println!("=== {label} ===");
        let mut gate =
            vesper_voice::hygiene::HygieneGate::new(vesper_voice::config::CaptureBudget::default());
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
                        if vesper_voice_kokoro::phonemize::ids_from_phonemes(&vocab, &phonemes)
                            .is_err()
                        {
                            verdict.push_str(&format!(" P{index}!ZERO"));
                        }
                    }
                }
            }
            if !verdict.is_empty() {
                failing_shapes.push((label, text.to_owned(), verdict.clone()));
            }
            println!(
                "  unit {} {:?}{}",
                unit.segment,
                &text[..text.len().min(52)],
                if verdict.is_empty() {
                    String::new()
                } else {
                    format!(" -->{verdict}")
                }
            );
        }
    }
    println!("\n=== failing shapes ({}) ===", failing_shapes.len());
    for (label, text, verdict) in &failing_shapes {
        println!("  [{label}] {text:?} -->{verdict}");
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
