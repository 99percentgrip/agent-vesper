//! Pronunciation: text normalization → espeak-ng IPA → phoneme-string
//! post-processing → model input IDs (the Kokoro input contract).
//!
//! Provenance and limits (honest, per directive §6):
//!
//! - The acoustic model consumes **IPA phoneme IDs**, not text. The
//!   officially maintained Kokoro binding (kokoro-js, Apache-2.0) passes
//!   espeak-ng IPA output to the model; this module reimplements that
//!   pipeline's *algorithm* (normalization rule order, punctuation-
//!   preserving section split, phoneme post-processing) in Rust from the
//!   published behavior — no source port, no JS runtime, no Python.
//! - **espeak-ng is the phonemizer only** (pronunciation symbols out; the
//!   baseline spoken audio is never used). It is an external system
//!   component invoked at a process boundary: bounded stdin/stdout, fixed
//!   argv, no shell, no device output. GPL-3.0 obligations stay with the
//!   user-installed binary; Vesper copies and bundles nothing.
//! - espeak-ng `--ipa` is not the full upstream G2P: unusual words,
//!   uncovered abbreviations, and names may be mispronounced. That is a
//!   recorded limitation of this route — never claimed as parity.
//! - "Unknown symbols" are not silently dropped by *this* code: the
//!   upstream tokenizer's own normalizer strips any character outside the
//!   vocabulary. This module applies that defined behavior explicitly and
//!   reports the stripped count; entirely-stripped input is an error.
//!   Text hygiene (pure core) protects content before it reaches here;
//!   it does not repair pronunciation.

use std::process::{Command, Stdio};

use vesper_domain::ProviderId;
use vesper_voice::error::VoiceError;

use crate::vocab::PhonemeVocab;

/// Bounded input to one phonemization pass (mirrors the subprocess
/// adapter's text bound; the sentence gate keeps units far below this).
pub const MAX_TEXT_BYTES: usize = 8192;
/// Bounded espeak-ng stdout accepted per pass.
const MAX_IPA_BYTES: usize = 64 * 1024;

/// Punctuation kept as section delimiters (all verified in the vocab).
/// Parentheses were normalized to «» first; «» act as delimiters and are
/// dropped at assembly exactly as the upstream tokenizer strips them.
const KEPT_PUNCT: &str = ";:,.!?—…\"“”()«»";

fn is_kept_punct(c: char) -> bool {
    KEPT_PUNCT.contains(c)
}

/// One normalization+split outcome.
enum Section {
    /// Raw text — espeak-ng phonemizes this part.
    Text(String),
    /// Punctuation (+attached whitespace) — passes through, minus «».
    Kept(String),
}

/// Splits `text` into text and punctuation sections. A punctuation run
/// includes its immediately surrounding whitespace (the official
/// `(\s*[punct]+\s*)+` shape); bare whitespace between words is NOT a
/// split point — "plain words" stays one espeak call.
fn split_sections(text: &str) -> Vec<Section> {
    let chars: Vec<char> = text.chars().collect();
    let mut attached = vec![false; chars.len()];
    for (index, ch) in chars.iter().enumerate() {
        if is_kept_punct(*ch) {
            attached[index] = true;
            // Expand left over whitespace.
            let mut left = index;
            while left > 0 && chars[left - 1].is_whitespace() {
                left -= 1;
                attached[left] = true;
            }
            // Expand right over whitespace.
            let mut right = index;
            while right + 1 < chars.len() && chars[right + 1].is_whitespace() {
                right += 1;
                attached[right] = true;
            }
        }
    }
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut current_attached = None;
    for (index, ch) in chars.iter().enumerate() {
        let this_attached = attached[index];
        match current_attached {
            None => {
                current.push(*ch);
                current_attached = Some(this_attached);
            }
            Some(same) if same == this_attached => current.push(*ch),
            Some(_) => {
                sections.push(if current_attached == Some(true) {
                    Section::Kept(std::mem::take(&mut current))
                } else {
                    Section::Text(std::mem::take(&mut current))
                });
                current.push(*ch);
                current_attached = Some(this_attached);
            }
        }
    }
    if !current.is_empty() {
        sections.push(if current_attached == Some(true) {
            Section::Kept(current)
        } else {
            Section::Text(current)
        });
    }
    sections
}

/// Text normalization (speech-only, bounded): the same rule order as the
/// official pipeline — quotes/brackets, CJK punctuation, whitespace,
/// abbreviations, casual words, numbers/currency/times, possessives,
/// hyphenated letters. Canonical assistant text in history is never
/// touched; this output exists only to feed pronunciation.
pub fn normalize_text(input: &str) -> String {
    let mut t = input.to_owned();
    // 1. Quotes and brackets (parens become «» which carry no
    //    pronunciation; the vocabulary has no bracket symbols).
    t = t
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace('\u{00AB}', "\u{201C}")
        .replace('\u{00BB}', "\u{201D}")
        .replace(['\u{201C}', '\u{201D}'], "\"")
        .replace('(', "\u{00AB}")
        .replace(')', "\u{00BB}");
    // 2. Uncommon punctuation marks → ASCII equivalents.
    for (from, to) in [
        ('\u{3001}', ", "),
        ('\u{3002}', ". "),
        ('\u{FF01}', "! "),
        ('\u{FF0C}', ", "),
        ('\u{FF1A}', ": "),
        ('\u{FF1B}', "; "),
        ('\u{FF1F}', "? "),
    ] {
        t = t.replace(from, to);
    }
    // 3. Whitespace normalization: non-space/newline whitespace → space;
    //    runs → one space; spaces-only between newlines removed.
    t = t
        .chars()
        .map(|c| {
            if c != ' ' && c != '\n' && c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>();
    while t.contains("  ") {
        t = t.replace("  ", " ");
    }
    t = strip_blank_line_spaces(&t);
    // 4. Abbreviations (case rules from the official table).
    t = replace_word(&t, "Dr.", "Doctor", |after| {
        after.starts_with(' ') && after[1..].chars().next().is_some_and(char::is_uppercase)
    });
    t = replace_word(&t, "Mr.", "Mister", |_| true);
    t = replace_word(&t, "Ms.", "Miss", |_| true);
    t = replace_word(&t, "Mrs.", "Mrs", |_| true);
    t = replace_word_ci(&t, "etc.", "etc", |after| {
        !(after.starts_with(' ') && after[1..].chars().next().is_some_and(char::is_uppercase))
    });
    // 5. Casual words: yeah/yea → ye'a (initial y/Y preserved).
    t = replace_yeah(&t);
    // 6. Numbers, times, years, currency, decimals, ranges.
    t = rewrite_numbers(&t);
    // 7. Possessives: word-final s after a consonant → 'S; X'S → X's.
    t = rewrite_possessives(&t);
    // 8. Initialisms: letter-dot runs → hyphens ("U.S.A." → "U-S-A.").
    t = rewrite_letter_dots(&t);
    t.trim().to_owned()
}

/// Removes space-only line interiors without a regex dependency.
fn strip_blank_line_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        out.push_str(line.trim_matches(' '));
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

/// Replaces exact-case word occurrences whose suffix satisfies `allow`.
fn replace_word(text: &str, from: &str, to: &str, allow: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(from) {
        let at_word_start = index == 0
            || !rest[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let after = &rest[index + from.len()..];
        let at_word_end = !after.chars().next().is_some_and(char::is_alphanumeric);
        if at_word_start && at_word_end && allow(after) {
            out.push_str(&rest[..index]);
            out.push_str(to);
            rest = after;
        } else {
            out.push_str(&rest[..index + from.len()]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Case-insensitive word replacement (etc. → etc).
fn replace_word_ci(text: &str, from: &str, to: &str, allow: impl Fn(&str) -> bool) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0usize;
    let mut index = 0usize;
    while let Some(found) = lower[index..].find(from) {
        let at = index + found;
        let after = &text[at + from.len()..];
        let at_word_start = at == 0
            || !text[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let at_word_end = !after.chars().next().is_some_and(char::is_alphanumeric);
        if at_word_start && at_word_end && allow(after) {
            out.push_str(&text[copied..at]);
            out.push_str(to);
            copied = at + from.len();
        }
        index = at + from.len();
    }
    out.push_str(&text[copied..]);
    out
}

/// yeah/yea → ye'a (initial y or Y preserved).
fn replace_yeah(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0usize;
    let mut index = 0usize;
    while index < lower.len() {
        let found = lower[index..]
            .find("yeah")
            .map(|f| (f, 4usize))
            .or_else(|| lower[index..].find("yea").map(|f| (f, 3usize)));
        let Some((found, word_len)) = found else {
            break;
        };
        let at = index + found;
        let after = &text[at + word_len..];
        let boundary = (at == 0
            || !text[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric))
            && !after.chars().next().is_some_and(char::is_alphanumeric);
        if boundary {
            out.push_str(&text[copied..at]);
            out.push(text[at..].chars().next().expect("non-empty slice"));
            out.push_str("e'a");
            copied = at + word_len;
        }
        index = at + word_len;
    }
    out.push_str(&text[copied..]);
    out
}

/// Number/time/year/currency rewriting (rule 6 of the official table).
fn rewrite_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        // Currency first: "$5", "£1.50", "$3 million".
        if (c == '$' || c == '\u{00A3}')
            && i + 1 < chars.len()
            && let Some((replacement, consumed)) = rewrite_money(&chars[i..])
        {
            out.push_str(&replacement);
            i += consumed;
            continue;
        }
        if c.is_ascii_digit() {
            // Time shape h:mm / h:ss before generic number handling.
            if let Some((replacement, end)) = try_time(&chars, i) {
                out.push_str(&replacement);
                i = end;
                continue;
            }
            // Collect the number token: digits with at most one '.'.
            let start = i;
            let mut seen_dot = false;
            while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !seen_dot)) {
                if chars[i] == '.' {
                    seen_dot = true;
                }
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if seen_dot {
                // Decimal → "X point D D D".
                let (whole, frac) = token.split_once('.').unwrap_or((token.as_str(), ""));
                out.push_str(whole);
                if !frac.is_empty() {
                    out.push_str(" point ");
                    for digit in frac.chars() {
                        out.push(digit);
                        out.push(' ');
                    }
                    out.pop();
                }
                continue;
            }
            if token.len() == 4 {
                // Year (optional trailing 's' stays attached).
                let suffix_is_s = i < chars.len() && chars[i] == 's';
                out.push_str(&split_year(&token, suffix_is_s));
                if suffix_is_s {
                    i += 1;
                }
                continue;
            }
            // Plain integer: espeak reads small numbers natively.
            out.push_str(&token);
            continue;
        }
        // Comma inside digits: "4,500" → "4500".
        if c == ','
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_digit()
        {
            i += 1;
            continue;
        }
        // Digit-hyphen-digit → " to ".
        if c == '-'
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_digit()
        {
            out.push_str(" to ");
            i += 1;
            continue;
        }
        // Digit + S (e.g. "4500S") → "4500 S".
        if c == 'S' && i > 0 && chars[i - 1].is_ascii_digit() {
            out.push_str(" S");
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Time rewriting `h:mm` (1–12, minutes 00–59, no surrounding colons or
/// leading digit): "X o'clock" / "X oh Y" / "X Y". Returns the
/// replacement and the scan end (after the minutes).
fn try_time(chars: &[char], start: usize) -> Option<(String, usize)> {
    // Not preceded by a digit or ':' (the (?<!:) and \b rules).
    if start > 0 && (chars[start - 1].is_ascii_digit() || chars[start - 1] == ':') {
        return None;
    }
    let mut j = start;
    while j < chars.len() && chars[j].is_ascii_digit() && j - start < 2 {
        j += 1;
    }
    let hour: String = chars[start..j].iter().collect();
    let h: u32 = hour.parse().ok()?;
    if !(1..=12).contains(&h) {
        return None;
    }
    if j >= chars.len() || chars[j] != ':' {
        return None;
    }
    j += 1;
    let min_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() && j - min_start < 2 {
        j += 1;
    }
    if j - min_start != 2 {
        return None;
    }
    let m: u32 = chars[min_start..j]
        .iter()
        .collect::<String>()
        .parse()
        .ok()?;
    if m > 59 {
        return None;
    }
    // Not followed by ':' (the (?!:) rule).
    if j < chars.len() && chars[j] == ':' {
        return None;
    }
    let hour_text = h.to_string();
    let replacement = if m == 0 {
        format!("{hour_text} o'clock")
    } else if m < 10 {
        format!("{hour_text} oh {m}")
    } else {
        format!("{hour_text} {m}")
    };
    Some((replacement, j))
}

/// Year → spoken words ("1990" → "nineteen ninety"); years before 1100
/// or ending 00..09 stay literal; trailing 's' is preserved.
fn split_year(token: &str, suffix_s: bool) -> String {
    let year: u32 = token.parse().unwrap_or(0);
    let suffix = if suffix_s { "s" } else { "" };
    if year < 1100 || year % 1000 < 10 {
        return format!("{token}{suffix}");
    }
    let left = &token[0..2];
    let right = token[2..4].parse::<u32>().unwrap_or(0);
    if (100..=999).contains(&(year % 1000)) {
        if right == 0 {
            return format!("{left} hundred{suffix}");
        }
        if right < 10 {
            return format!("{left} oh {right}{suffix}");
        }
    }
    format!("{left} {right}{suffix}")
}

/// Money rewriting: returns (replacement, consumed chars) for "$5",
/// "£1.50", "$3 million" shapes (magnitude only without decimals, per
/// the official pattern alternatives).
fn rewrite_money(chars: &[char]) -> Option<(String, usize)> {
    let mut i = 1usize; // past the currency sign
    let mut digits = String::new();
    let mut frac = String::new();
    let mut seen_dot = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            if seen_dot {
                frac.push(c);
            } else {
                digits.push(c);
            }
            i += 1;
        } else if c == '.' && !seen_dot && !digits.is_empty() {
            seen_dot = true;
            i += 1;
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    let bill = if chars[0] == '$' { "dollar" } else { "pound" };
    // Magnitude words only in the no-decimal alternative.
    if frac.is_empty() {
        let rest: String = chars[i..].iter().take(12).collect();
        for word in [" hundred", " thousand", " trillion", " billion", " million"] {
            if rest.starts_with(word) {
                let plural = if digits == "1" { "" } else { "s" };
                return Some((
                    format!("{digits}{word} {bill}{plural}"),
                    i + word.chars().count(),
                ));
            }
        }
        let suffix = if digits == "1" { "" } else { "s" };
        return Some((format!("{digits} {bill}{suffix}"), i));
    }
    // Fractional alternative: exactly one or two cent digits.
    if frac.len() > 2 {
        return None;
    }
    let mut cents = frac.clone();
    while cents.len() < 2 {
        cents.push('0');
    }
    let d: u32 = cents.parse().unwrap_or(0);
    let coins = if chars[0] == '$' {
        if d == 1 { "cent" } else { "cents" }
    } else if d == 1 {
        "penny"
    } else {
        "pence"
    };
    let plural = if digits == "1" { "" } else { "s" };
    Some((format!("{digits} {bill}{plural} and {d} {coins}"), i))
}

/// Possessive rules (official lookbehinds, exact case):
/// `[A-Z consonant] + optional apostrophe + word-final lowercase s → 'S`
/// (forces espeak's /z/ reading for pluralized acronyms, e.g. "CDs" →
/// "CD'S"); `X' + word-final S → s`. Lowercase possessives ("cats",
/// "Miss", "dollars") are never rewritten — the lookbehind class is
/// uppercase-only in the official table.
fn rewrite_possessives(text: &str) -> String {
    const CONSONANT_UPPER: &str = "BCDFGHJNPQRSTVWXYZ";
    let chars: Vec<char> = text.chars().collect();
    let is_upper_consonant = |c: char| c.is_ascii_uppercase() && CONSONANT_UPPER.contains(c);
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == 's' && i > 0 && (i + 1 >= chars.len() || !chars[i + 1].is_alphanumeric()) {
            // Optional apostrophe before the s; the consonant before that.
            let (apostrophe, consonant_index) = if chars[i - 1] == '\'' && i >= 2 {
                (Some(chars[i - 1]), Some(i - 2))
            } else {
                (None, Some(i - 1))
            };
            if let Some(index) = consonant_index
                && is_upper_consonant(chars[index])
            {
                out.push('\'');
                out.push('S');
                i += 1;
                continue;
            }
            let _ = apostrophe;
        }
        if c == 'S'
            && i >= 2
            && chars[i - 1] == '\''
            && chars[i - 2] == 'X'
            && (i + 1 >= chars.len() || !chars[i + 1].is_alphanumeric())
        {
            // X'S → X's.
            out.push('s');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Initialism rule: dot between two letters → hyphen ("U.S.A." →
/// "U-S-A.", "e.g. this" → "e-g this" for runs of ≥2 letter-dots).
fn rewrite_letter_dots(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let is_letter_dot =
        |i: usize| i >= 2 && chars[i - 2].is_ascii_alphabetic() && chars[i - 1] == '.';
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '.' && i > 0 && chars[i - 1].is_ascii_alphabetic() {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            let between_letters = next.is_some_and(|n| n.is_ascii_alphabetic());
            // Run of (letter, dot) pairs behind us (≥2) with a following
            // lowercase word → hyphen; or letter.dot.letter → hyphen.
            let run_behind = is_letter_dot(i);
            let lower_word_after =
                next == Some(' ') && chars.get(i + 2).is_some_and(|n| n.is_ascii_lowercase());
            if between_letters || (run_behind && lower_word_after) {
                let _ = prev;
                out.push('-');
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// IPA post-processing over the concatenated phoneme string (the official
/// rule list; fixed-string scans instead of lookaround regexes).
pub fn postprocess_phonemes(ps: &str) -> String {
    let t = ps
        // "kokoro" re-spelling (the upstream's own espeak fixups).
        .replace(
            "k\u{259}k\u{02C8}o\u{02D0}\u{0279}o\u{028A}",
            "k\u{02C8}o\u{028A}k\u{259}\u{0279}o\u{028A}",
        )
        .replace(
            "k\u{259}k\u{02C8}\u{0254}\u{02D0}\u{0279}\u{259}\u{028A}",
            "k\u{02C8}\u{259}\u{028A}k\u{259}\u{0279}\u{259}\u{028A}",
        )
        .replace('\u{02B2}', "j")
        .replace('r', "\u{0279}")
        .replace('x', "k")
        .replace('\u{026C}', "l");
    let t = insert_hundred_space(&t);
    let t = fix_plural_z(&t);
    fix_ninety(&t).trim().to_owned()
}

/// Space before "hˈʌndɹɪd" when preceded by [a-z ɹ ː].
fn insert_hundred_space(text: &str) -> String {
    let needle = "h\u{02C8}\u{028C}nd\u{0279}\u{026A}d";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(needle) {
        let boundary = index > 0
            && rest[..index]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '\u{0279}' || c == '\u{02D0}');
        if boundary {
            out.push_str(&rest[..index]);
            out.push(' ');
        } else {
            out.push_str(&rest[..index]);
        }
        out.push_str(needle);
        rest = &rest[index + needle.len()..];
    }
    out.push_str(rest);
    out
}

/// " z" → "z" before punctuation/whitespace/end.
fn fix_plural_z(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == ' ' && i + 1 < chars.len() && chars[i + 1] == 'z' {
            let boundary =
                i + 2 >= chars.len() || is_kept_punct(chars[i + 2]) || chars[i + 2] == ' ';
            if boundary {
                out.push('z');
                i += 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// "nˈaɪn" + "ti" (not followed by ː) → "nˈaɪn" + "di".
fn fix_ninety(text: &str) -> String {
    let base = "n\u{02C8}a\u{026A}n";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(base) {
        let after = &rest[index + base.len()..];
        if let Some(stripped) = after.strip_prefix("ti") {
            let long = stripped.starts_with('\u{02D0}');
            out.push_str(&rest[..index]);
            out.push_str(base);
            out.push_str(if long { "ti" } else { "di" });
            rest = stripped;
            continue;
        }
        out.push_str(&rest[..index + base.len()]);
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Kokoro provider identity for errors raised in this module.
fn kokoro_provider() -> ProviderId {
    ProviderId::new(crate::PROVIDER_ID).expect("static id fits")
}

/// Unavailability error with the phonemizer context.
fn phonemizer_unavailable(reason: String) -> VoiceError {
    VoiceError::Unavailable {
        provider: kokoro_provider(),
        reason: vesper_domain::BoundedString::new(reason).unwrap_or_else(|_| {
            vesper_domain::BoundedString::new("phonemizer unavailable").expect("fits")
        }),
    }
}

/// Runs espeak-ng over the text sections and assembles the final phoneme
/// string (kept punctuation verbatim minus «», espeak IPA for text,
/// official post-processing). One process per pass, bounded stdin/stdout.
pub fn phonemize_blocking(text: &str, espeak: &std::path::Path) -> Result<String, VoiceError> {
    if text.trim().is_empty() {
        return Err(VoiceError::InvalidInput(
            "refusing to phonemize empty text".into(),
        ));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(VoiceError::ResourceExhausted(
            "phonemizer text exceeded its bound".into(),
        ));
    }
    let normalized = normalize_text(text);
    if normalized.is_empty() {
        return Err(VoiceError::InvalidInput(
            "text normalized to nothing pronounceable".into(),
        ));
    }
    let sections = split_sections(&normalized);
    let lines: Vec<String> = sections
        .iter()
        .filter_map(|section| match section {
            Section::Text(text) => {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_owned())
            }
            Section::Kept(_) => None,
        })
        .collect();
    let espeak_lines = if lines.is_empty() {
        Vec::new()
    } else {
        // TERMINATING NEWLINE (pronunciation repair, 2026-09-23):
        // espeak-ng `--stdin` treats input without a final newline as a
        // truncated final line and degrades the LAST word's pronunciation
        // — measured: "Replying fast now" → nˈoʊ ("no"), "I'm here
        // ready" → ɹˈiːd ("read"), "The test" → tˈɛs ("tes"); the same
        // inputs with a trailing newline yield nˈaʊ / ɹˈɛdi / tˈɛst.
        // Every line gets the terminator, matching shell/pipe usage the
        // reference pipeline relies on.
        run_espeak(espeak, &format!("{}\n", lines.join("\n")))?
    };
    let mut phoneme_stream = String::new();
    let mut espeak_index = 0usize;
    for section in &sections {
        match section {
            Section::Kept(kept) => {
                // «» are dropped exactly as the upstream tokenizer's own
                // normalizer strips them (defined behavior, not an error).
                phoneme_stream.extend(
                    kept.chars()
                        .filter(|c| *c != '\u{00AB}' && *c != '\u{00BB}'),
                );
            }
            Section::Text(text) => {
                if text.trim().is_empty() {
                    continue;
                }
                let line = espeak_lines.get(espeak_index).cloned().unwrap_or_default();
                espeak_index += 1;
                phoneme_stream.push_str(&line.join(" "));
            }
        }
    }
    Ok(postprocess_phonemes(&phoneme_stream))
}

/// One bounded espeak-ng `--ipa` invocation (stdin in, stdout out, no
/// shell, no device output). Returns whitespace-split symbols per input
/// line. The caller owns the process deadline via its blocking executor;
/// the child is killed and reaped on drop (process-group scoped).
fn run_espeak(espeak: &std::path::Path, input: &str) -> Result<Vec<Vec<String>>, VoiceError> {
    use std::io::Read as _;
    use std::io::Write as _;
    let mut command = Command::new(espeak);
    command
        .args(["--ipa", "-q", "-v", "en-us", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        phonemizer_unavailable(format!(
            "pronunciation engine unavailable ({}): install espeak-ng",
            error.kind()
        ))
    })?;
    struct KillOnDrop<'a>(&'a mut std::process::Child);
    impl Drop for KillOnDrop<'_> {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let guarded = KillOnDrop(&mut child);
    {
        let mut stdin = guarded
            .0
            .stdin
            .take()
            .ok_or_else(|| phonemizer_unavailable("phonemizer stdin unavailable".into()))?;
        stdin
            .write_all(input.as_bytes())
            .map_err(|_| phonemizer_unavailable("phonemizer stdin write failed".into()))?;
    }
    let mut stdout = guarded
        .0
        .stdout
        .take()
        .ok_or_else(|| phonemizer_unavailable("phonemizer stdout unavailable".into()))?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stdout
            .read(&mut chunk)
            .map_err(|_| phonemizer_unavailable("phonemizer read failed".into()))?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.len() > MAX_IPA_BYTES {
            return Err(VoiceError::ResourceExhausted(
                "phonemizer output exceeded its bound".into(),
            ));
        }
    }
    let status = guarded
        .0
        .wait()
        .map_err(|_| phonemizer_unavailable("phonemizer wait failed".into()))?;
    if !status.success() {
        return Err(phonemizer_unavailable(format!(
            "pronunciation engine failed (exit {})",
            status
                .code()
                .map_or("unknown".into(), |code| code.to_string())
        )));
    }
    let text = String::from_utf8_lossy(&raw);
    Ok(text
        .lines()
        .map(|line| line.split_whitespace().map(str::to_owned).collect())
        .collect())
}

/// Maps a final phoneme string to model input IDs using the pinned vocab,
/// applying the defined upstream stripping behavior for out-of-vocabulary
/// characters (reported via [`IdsOutcome::stripped`]).
///
/// # Errors
/// [`VoiceError::InvalidInput`] when the input would produce no IDs;
/// [`VoiceError::ResourceExhausted`] beyond the model context.
pub fn ids_from_phonemes(vocab: &PhonemeVocab, phonemes: &str) -> Result<IdsOutcome, VoiceError> {
    let mut ids = Vec::new();
    let mut stripped = 0usize;
    for ch in phonemes.chars() {
        match vocab.id_for(ch) {
            Some(id) => ids.push(id),
            None => stripped += 1,
        }
    }
    if ids.is_empty() {
        return Err(VoiceError::InvalidInput(
            "text produced no pronounceable phonemes".into(),
        ));
    }
    if ids.len() > MAX_PHONEME_IDS {
        return Err(VoiceError::ResourceExhausted(
            "phoneme sequence exceeded the model context".into(),
        ));
    }
    Ok(IdsOutcome {
        // Model wrapper contract: [0, *ids, 0] (bos/eos, from the pinned
        // tokenizer's TemplateProcessing single-sequence template).
        input_ids: std::iter::once(0i64)
            .chain(ids)
            .chain(std::iter::once(0i64))
            .collect(),
        stripped,
    })
}

/// The model's context ceiling (512 total including bos/eos).
pub const MAX_PHONEME_IDS: usize = 510;

/// Phoneme→ID conversion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdsOutcome {
    /// `[0, *ids, 0]` — the wrapper contract from the reference model.
    pub input_ids: Vec<i64>,
    /// Characters the upstream vocabulary cannot represent (the
    /// tokenizer's own normalizer would strip them; surfaced, not hidden).
    pub stripped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_quotes_brackets_and_whitespace() {
        assert_eq!(normalize_text("it’s fine…"), "it's fine…");
        assert_eq!(normalize_text("a  (b) c"), "a \u{00AB}b\u{00BB} c");
        assert_eq!(normalize_text("line1\n  \nline2"), "line1\n\nline2");
    }

    #[test]
    fn expands_abbreviations_and_casual_words() {
        assert_eq!(normalize_text("Dr. Smith"), "Doctor Smith");
        assert_eq!(normalize_text("Mr. Jones went"), "Mister Jones went");
        assert_eq!(normalize_text("Ms. Lee"), "Miss Lee");
        assert_eq!(normalize_text("Mrs. Fox"), "Mrs Fox");
        assert_eq!(normalize_text("etc. then"), "etc then");
        assert_eq!(normalize_text("Yeah, sure."), "Ye'a, sure.");
    }

    #[test]
    fn rewrites_numbers_times_and_years() {
        // Hour >12 stays literal (espeak reads it as a number).
        assert_eq!(normalize_text("at 14:30"), "at 14:30");
        assert_eq!(normalize_text("at 9:05"), "at 9 oh 5");
        assert_eq!(normalize_text("at 9:00"), "at 9 o'clock");
        assert_eq!(normalize_text("4,500 lines"), "4500 lines");
        // Year → digit pairs ("19 90"); espeak speaks each pair as a
        // two-digit number, producing the official "nineteen ninety".
        assert_eq!(normalize_text("in 1990"), "in 19 90");
        assert_eq!(normalize_text("in 2020"), "in 20 20");
        // Hundreds years: "18 hundred" (right = 0 → "hundred" shape).
        assert_eq!(normalize_text("in 1800"), "in 18 hundred");
        // Years before 1100 or ending 00..09 in the last two digits < 10
        // via year%1000<10 stay literal.
        assert_eq!(normalize_text("in 1050"), "in 1050");
        assert_eq!(normalize_text("3.14 approx"), "3 point 1 4 approx");
        assert_eq!(normalize_text("10-15 items"), "10 to 15 items");
    }

    #[test]
    fn rewrites_currency() {
        assert_eq!(normalize_text("cost $5"), "cost 5 dollars");
        assert_eq!(normalize_text("cost $1"), "cost 1 dollar");
        assert_eq!(normalize_text("cost $1.50"), "cost 1 dollar and 50 cents");
        assert_eq!(normalize_text("£2.10 fine"), "2 pounds and 10 pence fine");
        assert_eq!(normalize_text("$3 million deal"), "3 million dollars deal");
        // Three cent digits: outside the money pattern (exactly .dd),
        // the number is a plain decimal → "point" reading.
        assert_eq!(normalize_text("cost $1.500"), "cost $1 point 5 0 0");
    }

    #[test]
    fn possessives_follow_the_official_lookbehinds() {
        // Uppercase-consonant + word-final s → 'S (pluralized acronyms).
        assert_eq!(normalize_text("three CDs"), "three CD'S");
        // Vowel-final acronyms are untouched by the lookbehind class
        // ([BCDFGHJ-NP-TV-Z] has no A/E/I/O/U).
        assert_eq!(normalize_text("the APIs work"), "the APIs work");
        // Lowercase possessives are untouched by the official rule.
        assert_eq!(normalize_text("the cats sat"), "the cats sat");
        assert_eq!(normalize_text("repo's history"), "repo's history");
        assert_eq!(normalize_text("Miss Lee"), "Miss Lee");
        assert_eq!(normalize_text("dollars"), "dollars");
        // X'S → X's.
        assert_eq!(normalize_text("UX'S flow"), "UX's flow");
    }

    #[test]
    fn initialisms_become_hyphenated() {
        assert_eq!(normalize_text("U.S.A. based"), "U-S-A. based");
        assert_eq!(normalize_text("U.S. army"), "U-S. army");
    }

    #[test]
    fn split_sections_keeps_punctuation() {
        // "hello" / ", " / "world" / "!" — four alternating sections.
        let sections = split_sections("hello, world!");
        assert_eq!(sections.len(), 4);
        // Bare whitespace is NOT a split point (official pattern requires
        // a punctuation char; spaces only attach to punct runs).
        let sections = split_sections("plain words");
        assert_eq!(sections.len(), 1);
        // "wait" / "... " / "ok" — three sections.
        let sections = split_sections("wait... ok");
        assert_eq!(sections.len(), 3);
    }

    #[test]
    fn postprocess_fixes_known_espeak_outputs() {
        // r → ɹ (espeak emits ASCII r inside some words).
        assert_eq!(postprocess_phonemes("ɹˈɛdr"), "ɹˈɛd\u{0279}");
        // plural z before punctuation: " z," → "z,"
        assert_eq!(postprocess_phonemes("ˌʌndɚstˈʊd z,"), "ˌʌndɚstˈʊdz,");
        // ninety: nˈaɪnti → nˈaɪndi (but nˈaɪntiː unchanged)
        assert_eq!(postprocess_phonemes("nˈaɪnti"), "nˈaɪndi");
        assert_eq!(postprocess_phonemes("nˈaɪntiː"), "nˈaɪntiː");
        // hundred spacing: "faɪvhʌndɹɪd" → "faɪv hʌndɹɪd"
        assert_eq!(postprocess_phonemes("fˈaɪvhˈʌndɹɪd"), "fˈaɪv hˈʌndɹɪd");
    }

    #[test]
    fn ids_use_wrapper_contract_and_count_stripped() {
        let vocab = PhonemeVocab::from_json_str(
            r#"{"model":{"vocab":{"$":0,";":1,"ˈ":156,"ʌ":140,"n":80,"d":30}}}"#,
        )
        .expect("vocab");
        let outcome = ids_from_phonemes(&vocab, "ˈʌnd").expect("ids");
        assert_eq!(outcome.input_ids, vec![0, 156, 140, 80, 30, 0]);
        assert_eq!(outcome.stripped, 0);
        let stripped_outcome = ids_from_phonemes(&vocab, "ˈʌ~nd").expect("ids");
        assert_eq!(stripped_outcome.stripped, 1);
        assert!(ids_from_phonemes(&vocab, "~").is_err());
    }

    #[test]
    fn phoneme_context_ceiling_is_enforced() {
        let vocab =
            PhonemeVocab::from_json_str(r#"{"model":{"vocab":{"$":0,"n":80}}}"#).expect("vocab");
        let long = "n".repeat(600);
        assert!(matches!(
            ids_from_phonemes(&vocab, &long),
            Err(VoiceError::ResourceExhausted(_))
        ));
    }
}

#[cfg(test)]
mod pronunciation_repair_tests {
    use super::*;

    /// Resolves the real phonemizer the way production does (PATH),
    /// so these tests run wherever espeak-ng is installed and SKIP
    /// (visibly, via eprintln) only where it truly is absent. A bare
    /// relative path never exists as a file — the original guard made
    /// every test vacuously pass; fixed with the repair's red proof.
    fn which_espeak() -> Option<std::path::PathBuf> {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("espeak-ng"))
                .find(|candidate| candidate.is_file())
        })
    }

    /// RED-first proof (2026-09-23 pronunciation repair): espeak-ng
    /// `--stdin` without a terminating newline truncates the final word's
    /// pronunciation. Alex's three reported defects all land on the LAST
    /// word of their speech piece: reply→"ripple" family (degraded final
    /// syllable), ready→"read", now→"no". These tests pin the exact
    /// post-fix phonemes; reverting the newline makes each FAIL.
    #[test]
    fn final_word_diphthong_is_not_truncated_now() {
        let espeak = which_espeak();
        let Some(espeak) = espeak else {
            eprintln!("skipping: espeak-ng not present");
            return;
        };
        let phonemes = phonemize_blocking("Replying fast now.", &espeak).unwrap();
        assert!(
            phonemes.contains("n\u{02C8}a\u{028A}"),
            "'now' must keep its diphthong, got: {phonemes}"
        );
        assert!(
            !phonemes.contains("n\u{02C8}o\u{028A}"),
            "'now' must not degrade to 'no': {phonemes}"
        );
    }

    #[test]
    fn final_syllable_is_not_dropped_ready() {
        let espeak = which_espeak();
        let Some(espeak) = espeak else {
            eprintln!("skipping: espeak-ng not present");
            return;
        };
        let phonemes = phonemize_blocking("I'm here ready,", &espeak).unwrap();
        assert!(
            phonemes.contains("\u{0279}\u{02C8}\u{025B}di"),
            "'ready' must keep its final syllable, got: {phonemes}"
        );
        assert!(
            !phonemes.contains("\u{0279}\u{02C8}i\u{02D0}d"),
            "'ready' must not degrade to 'read': {phonemes}"
        );
    }

    #[test]
    fn final_consonant_is_not_dropped_test_and_tool() {
        let espeak = which_espeak();
        let Some(espeak) = espeak else {
            eprintln!("skipping: espeak-ng not present");
            return;
        };
        let phonemes = phonemize_blocking("sent without using any tool.", &espeak).unwrap();
        assert!(
            phonemes.contains("t\u{02C8}u\u{02D0}l"),
            "'tool' must keep its final l, got: {phonemes}"
        );
        let phonemes = phonemize_blocking("the short test.", &espeak).unwrap();
        assert!(
            phonemes.contains("t\u{02C8}\u{025B}st"),
            "'test' must keep its final consonant cluster, got: {phonemes}"
        );
    }

    /// Mid-sentence words were never affected (newline was present via
    /// section joins); pinned so a future regression can't hide there.
    #[test]
    fn mid_sentence_words_keep_correct_phonemes() {
        let espeak = which_espeak();
        let Some(espeak) = espeak else {
            eprintln!("skipping: espeak-ng not present");
            return;
        };
        let phonemes = phonemize_blocking(
            "The reply is ready, and this completes your long test now.",
            &espeak,
        )
        .unwrap();
        for expected in [
            "\u{0279}\u{1D7B}pl\u{02C8}a\u{026A}", // reply
            "\u{0279}\u{02C8}\u{025B}di",          // ready (mid-sentence)
            "n\u{02C8}a\u{028A}",                  // now (final, post-fix)
        ] {
            assert!(
                phonemes.contains(expected),
                "missing {expected:?} in {phonemes}"
            );
        }
    }
}
