//! Isolated probe: what do numeric-with-period shapes phonemize to, exactly?
//! Read-only; no model, no audio.

fn main() {
    let espeak = espeak_path();
    let vocab =
        vesper_voice_kokoro::vocab::PhonemeVocab::load_verified(&vesper_voice_kokoro::pack_root())
            .expect("vocab");
    for text in [
        "1.",
        "1",
        "2.",
        "42",
        "42.",
        "3.14",
        "v2.4.1",
        "2026.",
        "1. Do",
        "1. Do the first",
        "Do the first thing",
        "0.",
        "10.",
        "100.",
        "1000.",
    ] {
        match vesper_voice_kokoro::phonemize::phonemize_blocking(text, &espeak) {
            Err(error) => println!("{text:?} -> PHON-ERR {error}"),
            Ok(phonemes) => {
                let ids = vesper_voice_kokoro::phonemize::ids_from_phonemes(&vocab, &phonemes);
                println!(
                    "{text:?} -> phonemes={phonemes:?} ids={:?}",
                    ids.map(|o| o.input_ids.len() - 2)
                );
            }
        }
    }
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
