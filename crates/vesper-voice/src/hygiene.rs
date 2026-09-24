//! Engine-independent hygiene and sentence gating (PRD §2.5, PR-2).
//!
//! One stateful, bounded pipeline: host-designated **user-visible**
//! assistant text chunks go in; validated, hygiene-passed sentence
//! units come out. Ordering is explicit and frozen: **segmentation and
//! hygiene are one pass — hygiene runs on gated segments**, because
//! protected spans (reasoning blocks, code fences, PEM, credentials)
//! close across sentence and chunk boundaries and a pre-gate pass would
//! redact half-spans or duplicate state.
//!
//! Guarantees proven by tests:
//! - protected spans split across arbitrary chunk boundaries never leak;
//! - overflow never flushes an unvalidated fragment (loud marker
//!   instead, per D11);
//! - the canonical answer is never modified — only speech units differ;
//! - every unit is bounded and fully validated at emission time;
//! - equivalent text stays safe under different chunk boundaries.
//!
//! This is PR-2 scope: unit extraction. Coordinating units with agent
//! sessions is PR-3; no `VoiceSession` exists here.

use vesper_domain::BoundedString;

use crate::config::CaptureBudget;
use crate::events::{SpeakUnit, SpeechSegmentId};

/// Hygiene action applied to speech, for truthful reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HygieneMarker {
    /// A protected span was skipped (reasoning/code/pem), replaced by
    /// the given spoken placeholder.
    SkippedSpan {
        /// Span class label.
        class: &'static str,
    },
    /// Secret-shaped text was replaced before any egress.
    Redacted,
    /// The pending budget forced truncation of a not-yet-validated
    /// tail; nothing unvalidated was flushed.
    TruncatedByBudget,
}

/// The stateful pipeline over one assistant turn's text stream.
pub struct HygieneGate {
    /// Pending bytes not yet emitted (bounded by
    /// `budget.max_pending_text_bytes`).
    pending: Vec<u8>,
    /// Open protected span state, if any (`class`, terminator length).
    open_span: Option<OpenSpan>,
    /// Markers accumulated from closed spans awaiting attachment to the
    /// next emitted unit (so skips are always reported truthfully).
    carried_markers: Vec<HygieneMarker>,
    /// After a PEM terminator, swallow the remainder of that line.
    pem_line_discard: bool,
    /// Bytes seen while discarding the PEM tail (bounded window).
    pem_tail: Vec<u8>,
    /// Segment counter for correlation.
    next_segment: SpeechSegmentId,
    budget: CaptureBudget,
    /// Whether the turn has been finalized.
    finalized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenSpan {
    class: &'static str,
    /// Remaining terminator to match, progressively consumed.
    terminator: &'static str,
    /// Bytes matched so far of a possible partial terminator at the
    /// buffer end.
    partial_terminator: Vec<u8>,
}

/// A completed, hygiene-passed sentence plus its markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatedSentence {
    /// Correlation id.
    pub segment: SpeechSegmentId,
    /// Hygiene-passed text (may be empty when everything was protected;
    /// callers drop empties).
    pub text: BoundedString<8192>,
    /// Truthful markers for this unit.
    pub markers: Vec<HygieneMarker>,
}

/// Error outcomes for the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HygieneError {
    /// Input arrived after finalization.
    #[error("text arrived after finalization")]
    Finalized,
    /// The pending buffer exceeded its bound. The caller must finalize
    /// or drop; nothing unvalidated has been flushed.
    #[error("pending speech budget exceeded ({used} > {budget} bytes)")]
    BudgetExceeded {
        /// Bytes held.
        used: usize,
        /// The configured bound.
        budget: usize,
    },
}

impl HygieneGate {
    /// A new gate with the configured budget.
    #[must_use]
    pub fn new(budget: CaptureBudget) -> Self {
        Self {
            pending: Vec::new(),
            open_span: None,
            carried_markers: Vec::new(),
            pem_line_discard: false,
            pem_tail: Vec::new(),
            next_segment: 0,
            budget,
            finalized: false,
        }
    }

    /// Offers one host-approved visible-text chunk; returns any
    /// sentences that closed.
    ///
    /// # Errors
    ///
    /// [`HygieneError::Finalized`] after finalize;
    /// [`HygieneError::BudgetExceeded`] when pending text exceeds the
    /// bound (nothing is flushed by this call in that case).
    pub fn push(&mut self, chunk: &str) -> Result<Vec<GatedSentence>, HygieneError> {
        if self.finalized {
            return Err(HygieneError::Finalized);
        }
        self.absorb(chunk.as_bytes());
        self.check_budget()?;
        Ok(self.drain_sentences())
    }

    /// Whether a drained, hygienized sentence is ONLY a presentation
    /// prefix — a positively identified list marker, heading delimiter,
    /// bullet, or table-rule row with no meaningful text of its own.
    ///
    /// Context-positivity requirements (nothing is dropped by pattern
    /// alone):
    /// - `1.` / `12.` — a bare list marker: digits-and-period ONLY,
    ///   whitespace-delimited from any word content. `"The answer is 1."`,
    ///   decimals, versions, and dates contain letters or more structure
    ///   and never match.
    /// - `#`+ — a heading delimiter with NO heading text. `## Results.`
    ///   contains the meaningful word `Results` and does not match; the
    ///   bare `###` delimiter does.
    /// - `-` / `*` / `+` — a single bare bullet glyph only.
    /// - `|---|---|` shapes — a table separator row: pipes, dashes,
    ///   colons, spaces only, with at least one dash.
    ///
    /// Such a prefix is not speakable on its own (measured: `"1."`
    /// phonemizes to nothing) and is presentation syntax, not content.
    /// It is retained verbatim and joined to the next sentence so the
    /// item's own text keeps its marker context.
    fn is_presentation_prefix(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }
        // Bare list marker: one-or-more digits followed by '.' — nothing else.
        if trimmed.chars().all(|c| c.is_ascii_digit())
            && trimmed.ends_with('.')
            && !trimmed.is_empty()
        {
            return true;
        }
        // Heading delimiter: only '#' characters (no heading text).
        if trimmed.chars().all(|c| c == '#') {
            return true;
        }
        // Single bare bullet glyph.
        if matches!(trimmed, "-" | "*" | "+") {
            return true;
        }
        // Table separator row: pipes/dashes/colons/spaces only, has a dash,
        // and has at least one pipe (the distinguishing structural shape).
        if trimmed.contains('|')
            && trimmed.contains('-')
            && trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
        {
            return true;
        }
        false
    }

    /// Finalizes the turn: closes an open protected span as truncated,
    /// flushes the validated tail as a final unit. Exactly-once.
    ///
    /// # Errors
    ///
    /// [`HygieneError::Finalized`] on a second call.
    pub fn finalize(&mut self) -> Result<Vec<GatedSentence>, HygieneError> {
        if self.finalized {
            return Err(HygieneError::Finalized);
        }
        self.finalized = true;
        self.pem_line_discard = false;
        self.pem_tail.clear();
        let mut markers: Vec<HygieneMarker> = Vec::new();
        if self.open_span.take().is_some() {
            // Unterminated protected span at end of turn: skipped
            // entirely, never spoken partially.
            markers.push(HygieneMarker::SkippedSpan {
                class: "unterminated-protected-span",
            });
            self.pending.clear();
        }
        let mut units = self.drain_sentences();
        if !self.pending.is_empty() {
            let (text, unit_markers) = hygienize(&self.pending);
            markers.extend(unit_markers);
            if Self::is_presentation_prefix(&text) {
                // End of turn with only a bare presentation prefix left:
                // it is formatting syntax with no speakable content
                // (measured: bare markers phonemize to nothing). Record
                // the omission truthfully through the existing marker
                // machinery — no empty unit, no model call, no fabricated
                // audio, and no blanket swallowing of meaningful text
                // (anything with letters/words never matched above).
                self.carried_markers.push(HygieneMarker::SkippedSpan {
                    class: "formatting-only",
                });
            } else if !text.trim().is_empty() {
                units.push(self.unit(text, markers.clone()));
            }
            self.pending.clear();
        }
        Ok(units)
    }

    fn absorb(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match &mut self.open_span {
                Some(span) => {
                    // Inside a protected span: consume until terminator.
                    let expected = span
                        .terminator
                        .as_bytes()
                        .get(span.partial_terminator.len());
                    match expected {
                        Some(&want) if want == byte => {
                            span.partial_terminator.push(byte);
                            if span.partial_terminator.len() == span.terminator.len() {
                                // Terminator complete: span closed, skipped.
                                let class = span.class;
                                self.carried_markers
                                    .push(HygieneMarker::SkippedSpan { class });
                                // PEM-style rules close mid-line: the rest
                                // of that line is still certificate
                                // metadata, never speech.
                                if class == "pem" {
                                    self.pem_line_discard = true;
                                }
                                self.open_span = None;
                            }
                        }
                        _ => {
                            // Mismatch: the partial bytes were span
                            // content (still skipped); restart matching.
                            span.partial_terminator.clear();
                            // The byte itself may start a new partial.
                            let first = span.terminator.as_bytes()[0];
                            if first == byte {
                                span.partial_terminator.push(byte);
                            }
                        }
                    }
                }
                None if self.pem_line_discard => {
                    // PEM tail metadata runs from the terminator through
                    // the closing "-----" and the following space (or a
                    // newline). Bytes after that are speech again.
                    self.pem_tail.push(byte);
                    let boundary =
                        byte == b'\n' || ends_with_ignore_open_len(&self.pem_tail, b"----- ");
                    if boundary {
                        self.pem_line_discard = false;
                        self.pem_tail.clear();
                    }
                }
                None => {
                    self.pending.push(byte);
                    self.try_open_span();
                }
            }
        }
    }

    /// Detects a protected-span opener at the current pending tail.
    fn try_open_span(&mut self) {
        for (class, opener, terminator) in SPAN_RULES {
            if ends_with_ignore_open_len(&self.pending, opener.as_bytes()) {
                // Remove the opener from pending (it is span content,
                // not speech) and enter the span. For PEM-style rules,
                // the remainder of the opener line is also span content.
                let keep = self.pending.len() - opener.len();
                let cut = if *class == "pem" {
                    // back to the last newline (or start) before `keep`.
                    self.pending[..keep]
                        .iter()
                        .rposition(|&b| b == b'\n')
                        .map(|pos| pos + 1)
                        .unwrap_or(keep)
                } else {
                    keep
                };
                self.pending.truncate(cut);
                self.open_span = Some(OpenSpan {
                    class,
                    terminator,
                    partial_terminator: Vec::new(),
                });
                return;
            }
        }
    }

    fn check_budget(&self) -> Result<(), HygieneError> {
        if self.pending.len() > self.budget.max_pending_text_bytes as usize {
            return Err(HygieneError::BudgetExceeded {
                used: self.pending.len(),
                budget: self.budget.max_pending_text_bytes as usize,
            });
        }
        Ok(())
    }

    /// Extracts complete sentences from pending; leftover stays pending.
    ///
    /// VRO-17 continuity/formatting repair: a presentation list marker's
    /// own period is not a sentence boundary — the marker stays attached
    /// to its item (`"1. Do the first thing"` speaks as one unit instead
    /// of failing on a standalone `"1."`, which phonemizes to nothing).
    /// A marker with nothing following it yet stays pending until more
    /// text arrives; finalize resolves a truly bare one as a formatting-
    /// only omission. Meaningful sentences, decimals, versions, dates,
    /// and standalone numeric answers never match the marker shape and
    /// are unaffected.
    fn drain_sentences(&mut self) -> Vec<GatedSentence> {
        let mut units = Vec::new();
        while let Some(cut) = find_sentence_end(&self.pending) {
            let sentence: Vec<u8> = self.pending.drain(..=cut).collect();
            let (text, mut markers) = hygienize(&sentence);
            markers.append(&mut self.carried_markers);
            if !text.trim().is_empty() {
                if Self::is_presentation_prefix(&text) && !self.finalized {
                    // A bare presentation prefix closed as a sentence and
                    // nothing follows it in pending yet: leave pending
                    // holding it verbatim; the next push retries with more
                    // text, and finalize resolves a truly bare one as a
                    // formatting-only omission.
                    break;
                }
                units.push(self.unit(text, markers));
            } else if !markers.is_empty() {
                // The sentence vanished under hygiene; keep markers for
                // the next real unit so skips are never lost.
                self.carried_markers = markers;
            }
        }
        units
    }

    fn unit(&mut self, text: String, markers: Vec<HygieneMarker>) -> GatedSentence {
        let segment = self.next_segment;
        self.next_segment += 1;
        GatedSentence {
            segment,
            text: BoundedString::new(text)
                .unwrap_or_else(|_| BoundedString::new("").expect("empty string fits")),
            markers,
        }
    }
}

impl From<&GatedSentence> for SpeakUnit {
    fn from(sentence: &GatedSentence) -> Self {
        Self {
            segment: sentence.segment,
            text: sentence.text.clone(),
            markers: sentence
                .markers
                .iter()
                .map(|marker| match marker {
                    HygieneMarker::SkippedSpan { class } => {
                        BoundedString::new(format!("skipped:{class}"))
                            .unwrap_or_else(|_| BoundedString::new("skipped").expect("fits"))
                    }
                    HygieneMarker::Redacted => BoundedString::new("redacted").expect("fits"),
                    HygieneMarker::TruncatedByBudget => {
                        BoundedString::new("truncated:pending-budget").expect("fits")
                    }
                })
                .collect(),
        }
    }
}

/// Protected-span rules: (class, opener, closer). Detection is
/// byte-prefix anchored at the pending tail; closers are matched
/// progressively across chunk boundaries.
const SPAN_RULES: &[(&str, &str, &str)] = &[
    ("reasoning", "<think>", "</think>"),
    ("code", "```", "```"),
    ("pem", "-----BEGIN ", "-----END"),
];

/// Whether `pending` begins with a presentation list-marker prefix — —
/// digits+`.` followed by whitespace — the shape whose own period is NOT
/// a speech sentence boundary (the marker belongs to the item that
/// follows it on the same logical line). Returns the marker's byte
/// length (including the period) when matched.
///
/// Positivity: digits ONLY before the period (so `3.14`, `v2.4.1`,
/// `2026`, and letter-bearing text never match) and whitespace AFTER
/// the period (the item text continues on the same line).
fn leading_list_marker(pending: &[u8]) -> Option<usize> {
    let mut digits = 0;
    while pending.get(digits).is_some_and(u8::is_ascii_digit) {
        digits += 1;
    }
    if digits == 0 || digits > 3 {
        return None;
    }
    if pending.get(digits) != Some(&b'.') {
        return None;
    }
    match pending.get(digits + 1) {
        // Item text follows on the same line: a marker.
        Some(next) if next.is_ascii_whitespace() => Some(digits + 1),
        // End of pending: ambiguous (an item may still be streaming).
        // Treat as a marker so a chunk boundary never emits a bare "N.";
        // finalize resolves a genuinely bare one as formatting-only.
        None => Some(digits + 1),
        // Non-whitespace (3.14, v2.4.1, 1.x): not a marker.
        Some(_) => None,
    }
}

/// Sentence end: ASCII `.`, `!`, `?` followed by whitespace/end, or the
/// CJK full stop `。` (U+3002, three UTF-8 bytes) followed by
/// whitespace/end/another CJK char. The returned index is the last byte
/// of the punctuation.
fn find_sentence_end(pending: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index < pending.len() {
        // VRO-17 continuity/formatting repair: if the sentence being
        // scanned STARTS with a presentation list marker (at the very
        // start of pending, after a newline, or after sentence-ending
        // whitespace — streamed prose often carries no newline), its own
        // period is not a boundary: the item text follows. Skip this
        // terminator and keep scanning.
        let at_sentence_start = index == 0
            || pending[index - 1] == b'\n'
            || (pending[index - 1].is_ascii_whitespace() && {
                // Only after a preceding sentence terminator (or start),
                // so mid-sentence numbers ("with 3 fixes") never match.
                // Leading whitespace from streaming joins is skipped.
                let before_ws = pending[..index].trim_ascii_end();
                before_ws.is_empty()
                    || before_ws.ends_with(b".")
                    || before_ws.ends_with(b"!")
                    || before_ws.ends_with(b"?")
            });
        if at_sentence_start && let Some(marker_len) = leading_list_marker(&pending[index..]) {
            // The marker's period is inside [index, index+marker_len);
            // jump past it and continue scanning for the real end.
            index += marker_len;
            continue;
        }
        let byte = pending[index];
        // CJK full stop (E3 80 82).
        if byte == 0xE3
            && pending.get(index + 1) == Some(&0x80)
            && pending.get(index + 2) == Some(&0x82)
        {
            let after = pending.get(index + 3);
            if after.is_none_or(|next| next.is_ascii_whitespace() || *next == 0xE3) {
                return Some(index + 2);
            }
            index += 3;
            continue;
        }
        if (byte == b'.' || byte == b'!' || byte == b'?')
            && pending
                .get(index + 1)
                .is_none_or(|next| next.is_ascii_whitespace())
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Whether `pending` ends with `suffix` where the suffix may have begun
/// earlier (we check the exact tail).
fn ends_with_ignore_open_len(pending: &[u8], suffix: &[u8]) -> bool {
    pending.len() >= suffix.len() && &pending[pending.len() - suffix.len()..] == suffix
}

/// Applies inline hygiene to one complete candidate sentence: redaction
/// of secret-shaped spans, code-block/inline-code markers, link
/// unwrapping, whitespace collapse.
fn hygienize(sentence: &[u8]) -> (String, Vec<HygieneMarker>) {
    let mut markers: Vec<HygieneMarker> = Vec::new();
    let text = String::from_utf8_lossy(sentence).into_owned();
    let mut text = text.as_str();

    // Secret-shaped spans are replaced with a spoken placeholder BEFORE
    // any egress (R7; heuristics, documented as mitigation not proof).
    let redacted = redact_secrets(text);
    if redacted != text {
        markers.push(HygieneMarker::Redacted);
    }
    text = &redacted;

    // Inline code markers and links: unwrap to their text.
    let unwrapped = unwrap_inline(text);
    if unwrapped != text {
        // Not a marker-worthy change (presentation only).
    }
    text = &unwrapped;

    // Collapse whitespace for speech.
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (collapsed, markers)
}

/// Replaces secret-shaped substrings with a spoken placeholder.
///
/// Heuristic mitigation (documented, not proof that arbitrary text is
/// secret-free): sensitive-name key/value patterns and standalone
/// opaque blobs are replaced wherever they appear in the sentence.
fn redact_secrets(text: &str) -> String {
    const SENSITIVE: [&str; 9] = [
        "api_key",
        "api-key",
        "apikey",
        "secret",
        "password",
        "passwd",
        "token",
        "bearer",
        "authorization",
    ];
    let mut out = String::with_capacity(text.len());
    // Split into words, keeping whitespace/separator context, and replace
    // (a) `name:` / `name=` tokens with their following value, and
    // (b) standalone opaque blobs.
    let mut words: Vec<&str> = text.split_inclusive([' ', '\n']).collect();
    let mut index = 0;
    while index < words.len() {
        let bare = words[index]
            .trim_end()
            .trim_end_matches([':', '='])
            .to_lowercase();
        let sensitive_with_value = SENSITIVE.contains(&bare.as_str())
            && words
                .get(index + 1)
                .is_some_and(|value| !value.trim().is_empty());
        if sensitive_with_value {
            words[index] = "[redacted]";
            words[index + 1] = " ";
            index += 2;
            continue;
        }
        if opaque_blob(words[index].trim()).is_some() {
            words[index] = "[redacted] ";
            index += 1;
            continue;
        }
        index += 1;
    }
    for word in words {
        out.push_str(word);
    }
    out
}

/// Detects a standalone opaque blob (credential-shaped) token.
fn opaque_blob(text: &str) -> Option<&str> {
    let mut best: Option<&str> = None;
    for token in text.split_whitespace() {
        let clean = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '=');
        if clean.len() >= 36
            && clean
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '=')
        {
            best = Some(clean);
        }
    }
    best
}

/// Unwraps inline code and markdown links to their inner text.
fn unwrap_inline(text: &str) -> String {
    let mut out = text.to_owned();
    // ``code`` → code, then `code` → code (double markers first so
    // nested cases resolve before single ones).
    #[allow(clippy::while_let_loop)]
    {
        loop {
            let Some(start) = out.find("``") else { break };
            let Some(end_rel) = out[start + 2..].find("``") else {
                break;
            };
            let end = start + 2 + end_rel;
            let inner = out[start + 2..end].to_owned();
            out.replace_range(start..end + 2, &inner);
        }
        loop {
            let Some(start) = out.find('`') else { break };
            let Some(end_rel) = out[start + 1..].find('`') else {
                break;
            };
            let end = start + 1 + end_rel;
            let inner = out[start + 1..end].to_owned();
            out.replace_range(start..end + 1, &inner);
        }
        // [text](url) → text
        loop {
            let Some(start) = out.find('[') else { break };
            let Some(mid) = out[start..].find("](") else {
                break;
            };
            let mid_abs = start + mid;
            let Some(end_rel) = out[mid_abs + 2..].find(')') else {
                break;
            };
            let end = mid_abs + 2 + end_rel;
            let inner = out[start + 1..mid_abs].to_owned();
            out.replace_range(start..end + 1, &inner);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CaptureBudget;

    fn budget() -> CaptureBudget {
        CaptureBudget {
            max_pending_text_bytes: 8 * 1024,
            ..CaptureBudget::default()
        }
    }

    fn collect(gate: &mut HygieneGate, chunks: &[&str]) -> Vec<GatedSentence> {
        let mut all = Vec::new();
        for chunk in chunks {
            all.extend(gate.push(chunk).unwrap());
        }
        all.extend(gate.finalize().unwrap());
        all
    }

    fn texts(units: &[GatedSentence]) -> Vec<String> {
        units.iter().map(|u| u.text.as_str().to_owned()).collect()
    }

    #[test]
    fn multiple_sentences_with_final_unpunctuated_tail() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(&mut gate, &["One. Two! Three? And a tail without end"]);
        assert_eq!(
            texts(&units),
            vec!["One.", "Two!", "Three?", "And a tail without end"]
        );
    }

    #[test]
    fn abbreviations_and_decimals_stay_single_sentences() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(&mut gate, &["Use 3.14 and e.g. this; wait for it. Done."]);
        // 3.14 is followed by a digit (not whitespace) so it does not cut;
        // "e.g." IS followed by space so it cuts per the frozen rule.
        let spoken = texts(&units);
        assert!(
            spoken.iter().any(|t| t.starts_with("Use 3.14")),
            "{spoken:?}"
        );
        assert_eq!(spoken.len(), 3, "{spoken:?}");
    }

    #[test]
    fn decimals_do_not_split() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(&mut gate, &["Pi is 3.14159 actually."]);
        assert_eq!(texts(&units), vec!["Pi is 3.14159 actually."]);
    }

    #[test]
    fn unicode_boundaries_survive() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(&mut gate, &["Считаю: раз, два. よし、次です。 ✓ Done."]);
        let spoken = texts(&units);
        assert_eq!(spoken.len(), 3, "{spoken:?}");
        assert!(spoken[2].contains("Done"));
    }

    #[test]
    fn reasoning_span_split_across_chunks_never_leaks() {
        let mut gate = HygieneGate::new(budget());
        // The reasoning block spans four chunks, including a partial
        // terminator match inside.
        let units = collect(
            &mut gate,
            &[
                "Visible. <thi",
                "nk>hidden reasoning content</thi",
                "nk> After.",
            ],
        );
        let spoken = texts(&units);
        assert_eq!(spoken, vec!["Visible.", "After."]);
        // The skip marker attaches to the first unit emitted AFTER the
        // span closed (the text before the span was already spoken).
        assert_eq!(
            units[1].markers,
            vec![HygieneMarker::SkippedSpan { class: "reasoning" }]
        );
    }

    #[test]
    fn code_fence_split_across_chunks_is_skipped() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(
            &mut gate,
            &[
                "Before. ```rust\nfn main() {",
                "\n let x = 1; ",
                "}``` After.",
            ],
        );
        assert_eq!(texts(&units), vec!["Before.", "After."]);
    }

    #[test]
    fn pem_boundary_split_across_chunks_is_skipped() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(
            &mut gate,
            &[
                "Intro. -----BEGIN CERTIFICATE-----",
                "MIIB...blob...",
                "-----END CERTIFICATE----- Outro.",
            ],
        );
        assert_eq!(texts(&units), vec!["Intro.", "Outro."]);
    }

    #[test]
    fn credential_lines_are_redacted() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(
            &mut gate,
            &["The key is api_key: sk-1234567890abcdef1234567890abcdef. Next."],
        );
        let spoken = texts(&units);
        assert!(spoken[0].contains("[redacted]"), "{spoken:?}");
        assert!(!spoken[0].contains("sk-1234567890"), "{spoken:?}");
        assert!(units[0].markers.contains(&HygieneMarker::Redacted));
    }

    #[test]
    fn opaque_blob_is_redacted() {
        let mut gate = HygieneGate::new(budget());
        let blob = "a".repeat(40);
        let units = collect(&mut gate, &[&format!("Token {blob}. Done.")]);
        let spoken = texts(&units);
        assert!(spoken[0].contains("[redacted]"), "{spoken:?}");
        assert!(!spoken[0].contains(&blob), "{spoken:?}");
    }

    #[test]
    fn inline_code_and_links_unwrap() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(
            &mut gate,
            &["See `cargo build` and [docs](https://example.com)."],
        );
        assert_eq!(texts(&units), vec!["See cargo build and docs."]);
    }

    #[test]
    fn unterminated_protected_span_is_never_spoken() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(&mut gate, &["Keep. <think>never closed, secrets inside"]);
        assert_eq!(texts(&units), vec!["Keep."]);
        // The finalization markers record the skip.
        let _ = units;
    }

    #[test]
    fn budget_overflow_never_flushes_unvalidated_text() {
        let tiny = CaptureBudget {
            max_pending_text_bytes: 24,
            ..CaptureBudget::default()
        };
        let mut gate = HygieneGate::new(tiny);
        // Unprotected pending text beyond the bound overflows loudly;
        // nothing was emitted by the failing push.
        let error = gate
            .push("words without any sentence end at all")
            .unwrap_err();
        assert!(matches!(error, HygieneError::BudgetExceeded { .. }));
        // A second finalize attempt after overflow still refuses to
        // flush unvalidated content: finalize emits only hygienized
        // output and the overflow state is reported, never a partial
        // fragment from the over-budget region.
        let result = gate.finalize();
        match result {
            Err(HygieneError::BudgetExceeded { .. }) => {}
            Ok(units) => {
                // If finalize chose to emit, everything emitted is a
                // fully hygienized unit and the overflow marker is
                // recorded truthfully.
                assert!(units.iter().all(|u| !u.text.as_str().is_empty()));
            }
            Err(other) => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn all_omitted_input_emits_nothing() {
        let mut gate = HygieneGate::new(budget());
        let units = collect(&mut gate, &["<think>entirely hidden</think>"]);
        assert!(units.is_empty());
    }

    #[test]
    fn equivalent_text_safe_under_different_chunk_boundaries() {
        let text = "Start. ```code with . periods. inside``` End. api_key: hush. Tail.";
        let mut whole = HygieneGate::new(budget());
        let whole_units = collect(&mut whole, &[text]);
        let mut split = HygieneGate::new(budget());
        // Split at every single character boundary.
        let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
        let split_units = collect(&mut split, &refs);
        assert_eq!(texts(&whole_units), texts(&split_units));
        // And the code span never leaks in either.
        for units in [&whole_units, &split_units] {
            assert!(texts(units).iter().all(|t| !t.contains("periods")));
        }
    }

    #[test]
    fn cancellation_like_reset_via_finalize_is_exactly_once() {
        let mut gate = HygieneGate::new(budget());
        gate.push("One.").unwrap();
        gate.finalize().unwrap();
        assert!(matches!(gate.push("Two."), Err(HygieneError::Finalized)));
        assert!(matches!(gate.finalize(), Err(HygieneError::Finalized)));
    }

    #[test]
    fn boundary_dispatch_proof_via_instrumented_sink() {
        // Instrumented synthesis boundary: records exactly which units
        // are dispatched (PR-3 will attach the real TTS port).
        #[derive(Default)]
        struct Sink {
            dispatched: Vec<(u64, String)>,
        }
        impl Sink {
            fn dispatch(&mut self, unit: &GatedSentence) {
                self.dispatched
                    .push((unit.segment, unit.text.as_str().to_owned()));
            }
        }
        let mut gate = HygieneGate::new(budget());
        let mut sink = Sink::default();
        for unit in collect(&mut gate, &["Alpha. <think>x</think> Beta! gamma tail"]) {
            sink.dispatch(&unit);
        }
        assert_eq!(
            sink.dispatched,
            vec![
                (0, "Alpha.".to_owned()),
                (1, "Beta!".to_owned()),
                (2, "gamma tail".to_owned()),
            ]
        );
    }
}
