//! VRO-17 §3 recon (isolated, no production changes): classify what the
//! real phonemization + vocabulary path yields for formatting-shaped
//! speech units. Pure phonemize/vocab layer — no model run, no audio.
//!
//! Answers two questions from Alex's device evidence:
//!  1. Which formatting-only unit shapes reach `ids_from_phonemes` with a
//!     phoneme string that maps to ZERO vocab ids (the exact
//!     "text produced no pronounceable phonemes" constructor)?
//!  2. Does a piece boundary split (28/48 chars) ever strand punctuation
//!     such that a piece's text section is empty (kept-punct-only piece)?
//!
//! Not a production pipeline: reads the public crate API only, prints a
//! bounded classification table, exits. No audio, no device, no network.

fn main() {
    let espeak = espeak_path();
    let vocab = vocab_for_recon();

    // (label, text) matrix: prose plus the formatting shapes the hygiene
    // gate can emit when a sentence contains or sits near paragraph
    // structure. All synthetic; no private text.
    let cases: &[(&str, &str)] = &[
        (
            "ordinary prose",
            "The build finished and all checks passed.",
        ),
        (
            "commas",
            "First, we measure, then we repair, and finally we verify.",
        ),
        ("numbers", "Version 2.4.1 ships with 3 fixes and 12 tests."),
        ("standalone number", "42"),
        ("decimal", "3.14159"),
        ("heading text", "Overview of the repair"),
        ("bullet lead-in", "First item the pipeline drains bounded"),
        ("table row", "Stage Result Time"),
        ("rule neighbors", "Before the rule after the rule"),
        ("ellipsis only-ish", "Wait … then continue."),
        ("ellipsis mid", "It worked… mostly."),
        ("quotes", "He said \"done\" and stopped."),
        ("parens", "(aside)"),
        ("dash clause", "One — two — three."),
        ("accents", "Naïve café résumé façade."),
        ("code marker", "```rust"),
        ("redaction", "the token was removed"),
        ("kept-punct only", "—"),
        ("comma only", ","),
        ("period only", "."),
        ("ellipsis only", "…"),
        ("colon only", ":"),
        ("semicolons", "; ; ;"),
        ("markdown bullet", "- item one follows here"),
        ("numbered list", "1. First step then second"),
        ("plus/minus", "+ - * / ="),
        ("backtick text", "`code` stays"),
        ("underscore", "snake_case_name_here"),
        ("slash path", "src/lib.rs and tests/"),
        ("percent", "50% complete"),
        ("ampersand", "a & b"),
        ("at sign", "user at host dot com"),
        ("hash", "# section"),
        ("caret/tilda", "^ ~"),
        ("empty-ish", " "),
    ];

    println!("case\tphon_ok\tphon_len\tids\tstripped\tclass");
    let mut empty_ids = 0usize;
    let mut no_text_section = 0usize;
    for (label, text) in cases {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            println!("{label}\t-\t-\t-\t-\tEMPTY (hygiene drops; no unit)");
            continue;
        }
        match vesper_voice_kokoro::phonemize::phonemize_blocking(text, &espeak) {
            Err(error) => println!("{label}\tERR\t-\t-\t-\t{error}"),
            Ok(phonemes) => {
                match vesper_voice_kokoro::phonemize::ids_from_phonemes(&vocab, &phonemes) {
                    Err(error) => {
                        empty_ids += 1;
                        println!(
                            "{label}\tok\t{}\tERR\t-\tZERO-IDS ({error})",
                            phonemes.chars().count()
                        );
                    }
                    Ok(outcome) => {
                        let ids_len = outcome.input_ids.len().saturating_sub(2);
                        let class = if ids_len == 0 {
                            "zero"
                        } else if outcome.stripped > 0 {
                            "partial-strip"
                        } else {
                            "clean"
                        };
                        println!(
                            "{label}\tok\t{}\t{}\t{}\t{class}",
                            phonemes.chars().count(),
                            ids_len,
                            outcome.stripped
                        );
                        if phonemes.chars().all(|c| !c.is_alphanumeric()) {
                            no_text_section += 1;
                        }
                    }
                }
            }
        }
    }
    println!("\nsummary: zero-id cases = {empty_ids}; punctuation-dominated = {no_text_section}");

    // Piece-boundary stranding: does 28/48 splitting ever emit a piece
    // whose TEXT sections are empty (only kept punctuation), which would
    // fail ids_from_phonemes even though the sentence is speakable?
    let paragraphs = [
        "Alpha beta gamma delta epsilon zeta eta theta, iota kappa — lambda mu nu.",
        "One two three four five six seven eight. Nine ten eleven twelve thirteen.",
        "River mountain valley forest, desert ocean glacier; plain and plateau.",
    ];
    println!("\npiece-boundary stranding check (speech_pieces shape, 28/48):");
    for text in paragraphs {
        let pieces = split_like_production(text);
        for (index, piece) in pieces.iter().enumerate() {
            let has_word = piece.chars().any(|c| c.is_alphanumeric());
            if !has_word {
                println!(
                    "  piece {index} of {:?} is punctuation-only: {piece:?}",
                    &text[..20.min(text.len())]
                );
            }
        }
        println!(
            "  {:?} -> {} pieces",
            &text[..24.min(text.len())],
            pieces.len()
        );
    }
}

/// Loads the REAL pinned vocabulary from the installed pack (read-only;
/// same source the engine uses) so id counts are production-real.
fn vocab_for_recon() -> vesper_voice_kokoro::vocab::PhonemeVocab {
    let root = vesper_voice_kokoro::pack_root();
    let vocab = vesper_voice_kokoro::vocab::PhonemeVocab::load_verified(&root)
        .expect("installed pack vocabulary");
    println!(
        "# vocab loaded from installed pack ({} entries)",
        vocab.len()
    );
    vocab
}

/// Mirrors the production first/successor thresholds WITHOUT touching
/// production: word/clause cuts at (12,28) then (24,48) char targets.
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
