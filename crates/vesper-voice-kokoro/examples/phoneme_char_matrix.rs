//! Isolated follow-up: which SPECIFIC characters produce zero pronounceable
//! phonemes through the real phonemizer+vocab (the exact error constructor),
//! and which of them survive the hygiene gate inside a sentence-shaped unit.
//! Read-only, no model run, no audio.

fn main() {
    let espeak = espeak_path();
    let vocab =
        vesper_voice_kokoro::vocab::PhonemeVocab::load_verified(&vesper_voice_kokoro::pack_root())
            .expect("vocab");

    // Common Markdown/formatting/symbol characters, each tested ALONE and
    // inside a prose sentence (the sentence case shows whether hygienized
    // realistic output can still strand a zero-phoneme section).
    let chars: Vec<(char, &str)> = vec![
        ('^', "caret"),
        ('~', "tilde"),
        ('|', "pipe"),
        ('`', "backtick"),
        ('#', "hash"),
        ('@', "at"),
        ('$', "dollar"),
        ('%', "percent"),
        ('&', "ampersand"),
        ('*', "star"),
        ('_', "underscore"),
        ('+', "plus"),
        ('=', "equals"),
        ('<', "less"),
        ('>', "greater"),
        ('[', "bracket"),
        (']', "bracket-close"),
        ('{', "brace"),
        ('}', "brace-close"),
        ('\\', "backslash"),
        ('/', "slash"),
        ('-', "hyphen"),
        ('—', "em-dash"),
        ('…', "ellipsis"),
        ('·', "middot"),
        ('•', "bullet"),
        ('→', "arrow"),
        ('⇒', "double-arrow"),
        ('✓', "check"),
        ('✗', "cross"),
        ('€', "euro"),
        ('£', "pound"),
        ('°', "degree"),
        ('§', "section"),
        ('¶', "pilcrow"),
        ('†', "dagger"),
        ('*', "asterisk-again"),
    ];

    println!("char\talone_ids\tembedded_ids\tembedded_class");
    for (ch, name) in chars {
        let alone = zero_ids(&vocab, &espeak, &ch.to_string());
        let embedded_text = format!("The value is {ch} here now.");
        let embedded = match phon(&espeak, &embedded_text) {
            Err(e) => format!("PHON-ERR({e})"),
            Ok(p) => match vesper_voice_kokoro::phonemize::ids_from_phonemes(&vocab, &p) {
                Err(_) => "ZERO".to_owned(),
                Ok(o) => format!("{} (stripped {})", o.input_ids.len() - 2, o.stripped),
            },
        };
        println!(
            "{name} ({ch})\t{alone}\t{embedded}\t{}",
            if alone == "ZERO" { "ALONE-ZERO" } else { "ok" }
        );
    }

    // Hygiene-gate interaction: does a sentence ending in these chars reach
    // a SpeakUnit? (The gate collapses whitespace and keeps the chars.)
    println!("\nhygiene interaction: sentence containing caret/tilde");
    for text in [
        "Set the mask to ^~ done.",
        "Approximately ~3 files matched ^2.",
        "The caret ^ and tilde ~ appear here.",
    ] {
        // The hygiene gate is engine-independent; exercise its public push:
        let mut sentence_gate =
            vesper_voice::hygiene::HygieneGate::new(vesper_voice::config::CaptureBudget::default());
        let units = sentence_gate.push(text).expect("gate");
        for unit in &units {
            println!(
                "  unit text={:?} markers={:?}",
                unit.text.as_str(),
                unit.markers
            );
        }
        let final_units = sentence_gate.finalize().expect("finalize");
        for unit in &final_units {
            println!("  final unit text={:?}", unit.text.as_str());
        }
    }
}

fn zero_ids(
    vocab: &vesper_voice_kokoro::vocab::PhonemeVocab,
    espeak: &std::path::Path,
    text: &str,
) -> String {
    match phon(espeak, text) {
        Err(e) => format!("PHON-ERR({e})"),
        Ok(p) => match vesper_voice_kokoro::phonemize::ids_from_phonemes(vocab, &p) {
            Err(_) => "ZERO".to_owned(),
            Ok(o) => (o.input_ids.len().saturating_sub(2)).to_string(),
        },
    }
}

fn phon(espeak: &std::path::Path, text: &str) -> Result<String, vesper_voice::error::VoiceError> {
    vesper_voice_kokoro::phonemize::phonemize_blocking(text, espeak)
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
