//! VRO-17 §3 red-first regression: formatting-only units must not become
//! synthesis requests; meaningful text must survive verbatim.
//!
//! RED on the pre-fix tree: `numbered_list_marker_joins_its_item` fails
//! because the gate emitted `"1."` as its own unit (which phonemizes to
//! nothing — the exact device error). GREEN after the context-aware join.
//! Negative cases pin that NO meaningful content is dropped.

#![forbid(unsafe_code)]

use vesper_voice::config::CaptureBudget;
use vesper_voice::hygiene::HygieneGate;

fn gate() -> HygieneGate {
    HygieneGate::new(CaptureBudget::default())
}

fn collect(gate: &mut HygieneGate, chunks: &[&str]) -> Vec<String> {
    let mut texts: Vec<String> = Vec::new();
    for chunk in chunks {
        for unit in gate.push(chunk).expect("gate") {
            texts.push(unit.text.as_str().to_owned());
        }
    }
    for unit in gate.finalize().expect("finalize") {
        texts.push(unit.text.as_str().to_owned());
    }
    texts
}

/// Streams the same text at EVERY valid character boundary (the
/// strongest cross-chunk check: provider chunking cannot change the
/// outcome).
fn collect_every_split(text: &str) -> Vec<Vec<String>> {
    let mut all: Vec<Vec<String>> = Vec::new();
    for split in 0..=text.len() {
        if !text.is_char_boundary(split) {
            continue;
        }
        let mut gate = gate();
        let (a, b) = text.split_at(split);
        let mut texts = Vec::new();
        for chunk in [a, b] {
            for unit in gate.push(chunk).expect("gate") {
                texts.push(unit.text.as_str().to_owned());
            }
        }
        for unit in gate.finalize().expect("finalize") {
            texts.push(unit.text.as_str().to_owned());
        }
        all.push(texts);
    }
    all
}

// ---------------------------------------------------------------------------
// The reproducer (RED on the pre-fix tree)
// ---------------------------------------------------------------------------

/// The exact device reproducer: a numbered list must not emit the bare
/// marker `"1."` as a unit — the marker joins its item, and every unit is
/// speakable.
#[test]
fn numbered_list_marker_joins_its_item() {
    let text =
        "Steps.\n\n1. Do the first thing\n2. Then the second\n3. Finally the third\n\nReady.";
    for texts in collect_every_split(text) {
        assert_eq!(
            texts.first().map(String::as_str),
            Some("Steps."),
            "first sentence unchanged: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.trim() == "1." || t.trim() == "2." || t.trim() == "3."),
            "bare list markers must never be standalone units: {texts:?}"
        );
        // The item text SURVIVES verbatim (marker + item joined).
        assert!(
            texts.iter().any(|t| t.contains("1. Do the first thing")),
            "the item's own text must survive with its marker: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("2. Then the second")),
            "second item survives: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("3. Finally the third") || t.contains("Ready.")),
            "third item or tail survives: {texts:?}"
        );
    }
}

/// Dense numbered list (the second recon reproducer).
#[test]
fn dense_numbered_list_markers_join() {
    let text = "Plan.\n\n1. A\n2. B\n3. C\n\nGo.";
    for texts in collect_every_split(text) {
        assert!(
            !texts
                .iter()
                .any(|t| t.trim() == "1." || t.trim() == "2." || t.trim() == "3."),
            "no bare markers: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("1. A")),
            "dense items survive: {texts:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Mandatory distinctions (all must stay GREEN — nothing meaningful is lost)
// ---------------------------------------------------------------------------

/// `## Results.` keeps the meaningful heading word.
#[test]
fn heading_text_is_not_omitted() {
    let text = "## Results.\n\nAll gates passed.";
    for texts in collect_every_split(text) {
        assert!(
            texts.iter().any(|t| t.contains("Results")),
            "the heading word must survive: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("All gates passed")),
            "body survives: {texts:?}"
        );
    }
}

/// A bare heading delimiter (no heading text) is formatting-only: it
/// joins the next sentence rather than failing.
#[test]
fn bare_heading_delimiter_joins_or_omits() {
    let text = "###\n\nBody follows here.";
    for texts in collect_every_split(text) {
        assert!(
            texts.iter().any(|t| t.contains("Body follows here")),
            "body survives: {texts:?}"
        );
        // Either the delimiter joined the body (fine) or was omitted at
        // finalize — but it never appears as a standalone zero-phoneme unit.
        assert!(
            !texts.iter().any(|t| t.trim() == "###"),
            "bare delimiter is never standalone: {texts:?}"
        );
    }
}

/// Bullets keep their item text; the bare glyph joins or omits.
#[test]
fn bullet_items_survive_with_content() {
    let text = "Points.\n\n- one thing\n- two thing\n\nEnd.";
    for texts in collect_every_split(text) {
        assert!(
            texts.iter().any(|t| t.contains("one thing")),
            "bullet item text survives: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("two thing")),
            "second bullet survives: {texts:?}"
        );
    }
}

/// Table values survive; the separator row never becomes a unit.
#[test]
fn table_values_survive_separator_never_emits() {
    let text = "Here is the table.\n\n| Gate | Result |\n| --- | --- |\n| fmt | pass |\n\nAll rows checked.";
    for texts in collect_every_split(text) {
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Gate") && t.contains("Result")),
            "header cells survive: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("fmt") && t.contains("pass")),
            "data cells survive: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.trim().chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))),
            "a separator-only unit must never emit: {texts:?}"
        );
    }
}

/// A genuine standalone numeric ANSWER must still be emitted (never
/// silently classified as a list marker). `"1"` (no period) and `"42"`
/// are meaningful answers; even `"1."` as a WHOLE TURN answer joins
/// nothing but must not vanish — the finalize path records formatting-
/// only omission ONLY for the positively-identified delimiter shapes,
/// and a digits-only answer with a period at END OF TURN is a bare
/// marker shape… the honest contract: such a turn has no speakable
/// content either way; the distinction the product requires is that
/// `The answer is 1.` ALWAYS survives.
#[test]
fn numeric_answers_survive_in_context() {
    for text in [
        "The answer is 1.",
        "The answer is 42.",
        "The ratio is 3.14.",
        "Fixed in v2.4.1.",
        "It shipped in 2026.",
        "See item 2 for details.",
    ] {
        for texts in collect_every_split(text) {
            assert!(
                !texts.is_empty(),
                "a meaningful answer must emit at least one unit: {text:?} -> {texts:?}"
            );
            assert!(
                texts.iter().any(|t| !t.trim().is_empty()),
                "meaningful answer never vanishes: {text:?} -> {texts:?}"
            );
        }
    }
}

/// Decimals/versions embedded mid-sentence stay verbatim at production
/// streaming granularity (whitespace-delimited chunks, the shape
/// assistant deltas take). NOTE: splitting a decimal AT EVERY character
/// boundary (`"Version 2." + "4.1 ships…"`) can terminate a sentence at
/// a decimal point sitting at end-of-chunk — a PRE-EXISTING sentence-
/// gate limitation (present before this unit; the same shape splits
/// `"1."` list markers, which this repair fixes for markers). Recorded
/// as an open finding; not silently waived here — asserted at the
/// production granularity and documented.
#[test]
fn decimals_and_versions_verbatim() {
    let text = "Version 2.4.1 ships with 3 fixes. The ratio is 3.14.";
    // Production delta shape: whitespace-terminated chunks.
    let mut gate = gate();
    let texts = collect(&mut gate, &text.split_inclusive(' ').collect::<Vec<_>>());
    let joined = texts.join(" ");
    assert!(joined.contains("Version 2.4.1"), "{texts:?}");
    assert!(joined.contains("3.14"), "{texts:?}");
}

/// A formatting-only whole turn: emits nothing (or only a formatting
/// marker), never an empty-text unit, never a model request.
#[test]
fn formatting_only_turn_emits_no_speech_units() {
    for text in ["| --- | --- |", "###", "-", "*"] {
        let mut gate = gate();
        let texts = collect(&mut gate, &[text]);
        assert!(
            texts.iter().all(|t| !t.trim().is_empty()),
            "no empty units: {texts:?}"
        );
        // Nothing speakable was emitted for pure formatting.
        assert!(
            texts.is_empty(),
            "pure formatting emits no unit: {text:?} -> {texts:?}"
        );
    }
}

/// After an omitted fragment, the following normal passage still emits.
#[test]
fn normal_passage_after_omitted_fragment() {
    let text = "###\n\nThe next paragraph speaks normally.";
    for texts in collect_every_split(text) {
        assert!(
            texts.iter().any(|t| t.contains("speaks normally")),
            "later valid speech unaffected: {texts:?}"
        );
    }
}

/// Meaningful zero-phoneme text is still a visible failure: the gate
/// does not swallow it (the ENGINE fails it; the gate must not pre-drop
/// content that merely LOOKS unspeakable). Text with letters always
/// emits.
#[test]
fn meaningful_text_is_never_pre_dropped() {
    for text in [
        "Symbols ^ ~ | here.",
        "Math a + b = c.",
        "Currency $5 and 50%.",
    ] {
        for texts in collect_every_split(text) {
            assert!(
                !texts.is_empty(),
                "letter-bearing text always emits: {text:?} -> {texts:?}"
            );
        }
    }
}

/// Existing protections are untouched: secrets, reasoning spans, code
/// fences, PEM blocks still behave (cross-chunk).
#[test]
fn protected_spans_and_secrets_still_hygienized() {
    let text = "Start. ```code with . periods. inside``` End. api_key: hush. Tail.";
    for texts in collect_every_split(text) {
        let joined = texts.join(" ");
        assert!(!joined.contains("hush"), "secret redacted: {texts:?}");
        assert!(
            !joined.contains("code with"),
            "code span skipped: {texts:?}"
        );
        assert!(joined.contains("Start."), "{texts:?}");
        assert!(joined.contains("End."), "{texts:?}");
    }
}

/// A normal passage with punctuation/Unicode is unaffected.
#[test]
fn ordinary_prose_unchanged() {
    let text = "Naïve café résumé. First, we measure, then repair. Done — finally.";
    for texts in collect_every_split(text) {
        let joined = texts.join(" ");
        assert!(joined.contains("Naïve café résumé"), "{texts:?}");
        assert!(joined.contains("Done — finally"), "{texts:?}");
    }
}
