//! Isolated: does the PIECE splitter (28/48 word/clause cuts) ever emit a
//! piece whose ONLY content is one of the zero-phoneme symbol characters —
//! i.e. can a piece boundary STRAND a lone symbol so a speakable sentence
//! fails with "no pronounceable phonemes"? Read-only; no model, no audio.

fn main() {
    // Realistic formatting-bearing sentences that pass hygiene unchanged.
    let sentences = [
        // Pipe inside a sentence (tables get unwrapped by hygiene? no:
        // pipes are plain characters — they survive to the unit).
        "Stage | Result | Time passed as expected in the table view.",
        "Use ^ for caret anchors and ~ for home paths in the shell.",
        "Set the value to *bold* or _italic_ when formatting.",
        "Run `cargo test` with +nightly for the feature set.",
        "The mask is [a-z]+ and the separator is | here.",
        "Escaped \\n and \\t appear in the string literal.",
        "Cost: $5 at 50% off, or ~3 files.",
        // A sentence whose LAST word is a lone symbol before the period.
        "The delimiter is |.",
        "Choose either ^ or ~.",
    ];
    let zero_chars: [char; 21] = [
        '^', '~', '|', '`', '#', '@', '$', '%', '&', '*', '_', '+', '=', '<', '>', '[', ']', '{',
        '}', '\\', '/',
    ];
    println!("sentence\tpieces\tzero-only-pieces");
    for text in sentences {
        let pieces = split_like_production(text);
        let mut bad = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            // A piece is "stranded-symbol" if, after trimming kept
            // punctuation and whitespace, every remaining char is a
            // zero-phoneme symbol.
            let core: String = piece
                .chars()
                .filter(|c| !c.is_whitespace() && !";:,.!?—…\"“”()«»".contains(*c))
                .collect();
            if !core.is_empty() && core.chars().all(|c| zero_chars.contains(&c)) {
                bad.push(format!("piece {index}: {piece:?}"));
            }
        }
        println!(
            "{:?}\t{}\t{}",
            &text[..28.min(text.len())],
            pieces.len(),
            if bad.is_empty() {
                "none".to_owned()
            } else {
                bad.join("; ")
            }
        );
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
