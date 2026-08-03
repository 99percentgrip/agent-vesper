//! NLP fallback layer.
//!
//! Substitutes for mem0's spaCy (`mem0/utils/lemmatization.py`,
//! `mem0/utils/entity_extraction.py`). Faithful V3 quality requires spaCy;
//! this module provides the Snowball + regex fallback ratified by ADR 0015.
//!
//! Two public capabilities:
//! - `lemmatize_for_bm25` — Snowball English stemmer. Drops stopwords and
//!   punctuation, preserves original `-ing` forms alongside the lemma
//!   (mirrors the spaCy port's tolerance for verb/noun ambiguity).
//! - `extract_entities` — regex heuristics covering four entity classes
//!   (PROPER, QUOTED, TOPIC, IDENTIFIER). Coarser than spaCy NER but bounded
//!   and dependency-free.

use std::sync::OnceLock;

use regex::Regex;
use rust_stemmers::{Algorithm, Stemmer};

use crate::types::Attribution;

/// Snowball English stemmer. Constructed once and reused.
fn stemmer() -> &'static Stemmer {
    static STEMMER: OnceLock<Stemmer> = OnceLock::new();
    STEMMER.get_or_init(|| Stemmer::create(Algorithm::English))
}

/// Common English stopwords. Matches the high-frequency set the spaCy port
/// discards via `token.is_stop`.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "then", "else", "when", "at", "by", "for", "with",
    "about", "against", "between", "into", "through", "during", "before", "after", "above",
    "below", "to", "from", "up", "down", "in", "out", "on", "off", "over", "under", "again",
    "further", "once", "here", "there", "all", "any", "both", "each", "few", "more", "most",
    "other", "some", "such", "no", "nor", "not", "only", "own", "same", "so", "than", "too",
    "very", "can", "will", "just", "dont", "should", "now", "i", "me", "my", "we", "our", "you",
    "your", "he", "him", "his", "she", "her", "it", "its", "they", "them", "their", "what",
    "which", "who", "whom", "this", "that", "these", "those", "am", "is", "are", "was", "were",
    "be", "been", "being", "have", "has", "had", "having", "do", "does", "did", "doing", "of",
    "as",
];

/// Snowball-stem token list for FTS5 BM25 indexing and querying.
///
/// Mirrors `mem0/utils/lemmatization.py:lemmatize_for_bm25` shape: lowercases,
/// drops stopwords and punctuation, and appends the original `-ing` form
/// alongside the lemma when they differ (handles noun/verb ambiguity such
/// as `meeting`/`meet`).
#[must_use]
pub fn lemmatize_for_bm25(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut tokens: Vec<String> = Vec::new();
    for raw in lower.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        if STOPWORDS.contains(&raw) {
            continue;
        }
        let lemma = stemmer().stem(raw).to_string();
        if !lemma.is_empty() {
            tokens.push(lemma.clone());
        }
        // Preserve -ing forms to mitigate Snowball's over-stemming on gerunds.
        if raw.ends_with("ing") && raw != lemma {
            tokens.push(raw.to_string());
        }
    }
    tokens.join(" ")
}

/// Four entity classes mirroring mem0's `entity_extraction.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    /// Capitalized multi-word sequences (person, place, brand, title).
    Proper,
    /// Text inside single or double quotes (titles, specific terms).
    Quoted,
    /// Multi-word noun-compound topic (coarse approximation of spaCy NOUN).
    Topic,
    /// CamelCase / snake_case technical identifiers.
    Identifier,
}

/// One extracted entity candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityCandidate {
    pub entity_type: EntityType,
    pub text: String,
}

impl EntityCandidate {
    /// Normalize for exact-dedup matching — mirrors mem0's
    /// `_normalize_entity_text`: lowercase, single-spaced, trimmed.
    #[must_use]
    pub fn normalized(&self) -> String {
        self.text
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn proper_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // 1+ capitalized words optionally joined by spaces, dots, or apostrophes.
        // ASCII-only because the workspace `regex` dep disables `unicode-perl`.
        Regex::new(r"\b[A-Z][a-zA-Z0-9]+(?:[ \t.'][A-Z][a-zA-Z0-9]+){0,3}\b")
            .expect("proper-noun regex")
    })
}

fn quoted_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)'"#)
            .expect("quoted-text regex")
    })
}

fn identifier_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // CamelCase tokens ≥5 chars OR snake_case identifiers with ≥1 underscore.
        Regex::new(r"\b(?:[A-Z][a-z]+(?:[A-Z][a-z]+){1,}|[a-zA-Z][a-zA-Z0-9]*_[a-zA-Z0-9_]+)\b")
            .expect("identifier regex")
    })
}

fn topic_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Adjective/Noun-modifier + Noun compound (coarse, ASCII whitespace).
        Regex::new(r"\b(?:[a-z]{3,}(?:ly|ish|ic|al|ive|ous|ful|less|able))[ \t]+[a-z]{3,}\b")
            .expect("topic regex")
    })
}

/// Default in-crate regex extractor. Returns deduplicated candidates in
/// stable order (PROPER, QUOTED, IDENTIFIER, TOPIC).
///
/// This is the v1 fallback ratified by ADR 0015. It is intentionally
/// conservative — it errs toward high-precision, low-recall extractions so
/// the entity-graph stays small and the boost math stays bounded.
#[must_use]
pub fn extract_entities(text: &str) -> Vec<EntityCandidate> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let push =
        |out: &mut Vec<_>, seen: &mut std::collections::HashSet<String>, entity_type, raw: &str| {
            let trimmed = raw.trim().trim_matches(|c: char| c == '.' || c == ',');
            if trimmed.len() < 2 {
                return;
            }
            let key = trimmed.to_lowercase();
            if !seen.insert(key) {
                return;
            }
            out.push(EntityCandidate {
                entity_type,
                text: trimmed.to_string(),
            });
        };

    for cap in proper_re().captures_iter(text) {
        push(&mut out, &mut seen, EntityType::Proper, &cap[0]);
    }
    for cap in quoted_re().captures_iter(text) {
        // Group 1 = double-quoted content; group 2 = single-quoted content.
        let inner = cap.get(1).or_else(|| cap.get(2));
        if let Some(m) = inner {
            push(&mut out, &mut seen, EntityType::Quoted, m.as_str());
        }
    }
    for cap in identifier_re().captures_iter(text) {
        push(&mut out, &mut seen, EntityType::Identifier, &cap[0]);
    }
    for cap in topic_re().captures_iter(text) {
        push(&mut out, &mut seen, EntityType::Topic, &cap[0]);
    }
    out
}

/// Decide an `Attribution` for a message based on its role string.
#[must_use]
pub fn attribution_for_role(role: &str) -> Option<Attribution> {
    match role {
        "user" => Some(Attribution::User),
        "assistant" => Some(Attribution::Assistant),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lemmatizer_drops_stopwords_and_preserves_ing() {
        let out = lemmatize_for_bm25("The User is attending meetings about organizations");
        // "attending" lemma is "attend"; -ing form preserved.
        assert!(out.contains("attend"));
        assert!(out.contains("attending"));
        // "the", "is", "about" dropped as stopwords.
        assert!(!out.contains(" the ") && !out.starts_with("the "));
    }

    #[test]
    fn entity_extractor_finds_quoted_and_proper() {
        let text =
            r#"Marcus was promoted at "Osteria Francescana" after adopting CamelCase_design"#;
        let entities = extract_entities(text);
        let texts: Vec<&str> = entities.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("Osteria Francescana")));
        assert!(texts.iter().any(|t| t.contains("Marcus")));
        assert!(texts.iter().any(|t| t.contains("CamelCase_design")));
    }

    #[test]
    fn entity_dedup_is_normalization_stable() {
        let e = EntityCandidate {
            entity_type: EntityType::Proper,
            text: "  New   York ".to_string(),
        };
        assert_eq!(e.normalized(), "new york");
    }
}
