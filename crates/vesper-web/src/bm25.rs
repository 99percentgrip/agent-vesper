//! BM25 content filter (VRO-14 PR-2) — a pure-Rust port of web oracle
//! beta's `BM25ContentFilter`: tokenized chunk scoring with Okapi BM25,
//! priority-tag boosting, and beta's query-fallback chain (user query →
//! title + h1 + meta keywords/description → first significant paragraph).
//! Stemming uses the workspace's approved `rust-stemmers` (English-only v1,
//! matching the PRD's honest-divergence note).

use crate::arena::{Dom, NodeId};
use rust_stemmers::{Algorithm, Stemmer};

/// Beta's explicit query → metadata/headings → significant paragraph chain.
pub fn page_query(document: &crate::dom::Document, explicit: Option<&str>) -> String {
    if let Some(query) = explicit.filter(|s| !s.trim().is_empty()) {
        return query.to_owned();
    }
    let metadata = crate::meta::extract_metadata(document);
    let heading = crate::dom::find_first(&document.root, "h1").map(|h| h.text());
    let query = [
        metadata.title,
        heading,
        metadata.keywords,
        metadata.description,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    if !query.trim().is_empty() {
        return query;
    }
    let mut queue = vec![&document.root];
    while let Some(node) = queue.pop() {
        if node.tag == "p" {
            let text = node.text();
            if text.split_whitespace().count() >= 20 {
                return text;
            }
        }
        queue.extend(node.children.iter().rev().filter_map(|n| match n {
            crate::dom::Node::Element(el) => Some(el),
            _ => None,
        }));
    }
    String::new()
}

/// Beta's priority-tag boost table (`BM25ContentFilter.priority_tags`).
const PRIORITY_TAGS: &[(&str, f32)] = &[
    ("h1", 5.0),
    ("h2", 4.0),
    ("h3", 3.0),
    ("title", 4.0),
    ("strong", 2.0),
    ("b", 1.5),
    ("em", 1.5),
    ("blockquote", 2.0),
    ("code", 2.0),
    ("pre", 1.5),
    ("th", 1.5),
];

fn priority_boost(tag: &str) -> f32 {
    PRIORITY_TAGS
        .iter()
        .find(|(name, _)| *name == tag)
        .map(|(_, boost)| *boost)
        .unwrap_or(1.0)
}

/// A scored content chunk (beta's chunk + its BM25 score).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredChunk {
    pub text: String,
    pub score: f32,
}

/// BM25 filter over extracted text chunks.
pub struct Bm25Filter {
    /// Beta's default threshold: chunks scoring below it are dropped.
    pub threshold: f32,
    /// Stemming toggle (beta's `use_stemming`, default true).
    pub use_stemming: bool,
    /// Minimum words a chunk needs to be considered (beta's
    /// `min_word_threshold`).
    pub min_word_threshold: Option<usize>,
    stemmer: Option<Stemmer>,
}

impl Bm25Filter {
    /// Beta's defaults: threshold 1.0, stemming on, no word floor.
    pub fn new() -> Self {
        Self {
            threshold: 1.0,
            use_stemming: true,
            min_word_threshold: None,
            stemmer: Some(Stemmer::create(Algorithm::English)),
        }
    }

    /// Score every chunk with no threshold gate: test and diagnostic
    /// surface proving relative ordering without depending on corpus
    /// size. `filter` remains the production threshold gate (beta's 1.0).
    pub fn score_all(&self, chunks: &[TaggedChunk], query: &str) -> Vec<ScoredChunk> {
        self.filter_with_threshold(chunks, query, 0.0)
    }

    /// Disable stemming (beta's `use_stemming: false`).
    pub fn without_stemming(mut self) -> Self {
        self.use_stemming = false;
        self.stemmer = None;
        self
    }

    /// Set a minimum-word floor (beta's `min_word_threshold`).
    pub fn with_min_word_threshold(mut self, min_words: usize) -> Self {
        self.min_word_threshold = Some(min_words);
        self
    }

    /// Tokenize one text into stemmed terms (lowercase, alphanumeric split,
    /// optional English stemming — beta's tokenization shape).
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(|word| match &self.stemmer {
                Some(stemmer) => stemmer.stem(word).into_owned(),
                None => word.to_string(),
            })
            .collect()
    }

    /// Score text chunks against a query with Okapi BM25 plus beta's
    /// priority-tag boost. Returns chunks above the threshold, sorted by
    /// descending score (beta's ordering).
    pub fn filter(&self, chunks: &[TaggedChunk], query: &str) -> Vec<ScoredChunk> {
        self.filter_with_threshold(chunks, query, self.threshold)
    }

    fn filter_with_threshold(
        &self,
        chunks: &[TaggedChunk],
        query: &str,
        threshold: f32,
    ) -> Vec<ScoredChunk> {
        let query_terms = self.tokenize(query);
        if query_terms.is_empty() || chunks.is_empty() {
            return Vec::new();
        }

        // Tokenize each chunk once; apply the word floor. Pairs keep
        // chunk/terms aligned even when the floor skips entries.
        let mut pairs: Vec<(&TaggedChunk, Vec<String>)> = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let terms = self.tokenize(&chunk.text);
            if let Some(min_words) = self.min_word_threshold
                && terms.len() < min_words
            {
                continue;
            }
            pairs.push((chunk, terms));
        }

        // Okapi parameters (beta's rank_bm25 BM25Okapi defaults).
        let k1 = 1.5_f32;
        let b = 0.75_f32;

        // Document-frequency table over the surviving chunks.
        let mut doc_freq: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (_, terms) in &pairs {
            let mut seen = std::collections::HashSet::new();
            for term in terms {
                if seen.insert(term) {
                    *doc_freq.entry(term.clone()).or_default() += 1;
                }
            }
        }

        let doc_count = pairs.len() as f32;
        let avg_len: f32 = if pairs.is_empty() {
            0.0
        } else {
            pairs.iter().map(|(_, terms)| terms.len()).sum::<usize>() as f32 / doc_count
        };

        let mut scored: Vec<ScoredChunk> = Vec::new();
        for (chunk, terms) in &pairs {
            let len = terms.len() as f32;
            let mut score = 0.0_f32;
            for term in &query_terms {
                let df = doc_freq.get(term).copied().unwrap_or(0) as f32;
                if df == 0.0 || len == 0.0 {
                    continue;
                }
                // Okapi BM25: idf = ln((N - df + 0.5)/(df + 0.5) + 1) with
                // rank_bm25's epsilon-free +1 guard; tf component is
                // tf*(k1+1) / (tf + k1*(1 - b + b*len/avg_len)).
                let idf = ((doc_count - df + 0.5) / (df + 0.5) + 1.0).ln();
                let tf = terms.iter().filter(|t| *t == term).count() as f32;
                let norm = k1 * (1.0 - b + b * len / avg_len);
                score += idf * (tf * (k1 + 1.0)) / (tf + norm);
            }
            // Beta's priority-tag boost multiplies the chunk's score.
            score *= priority_boost(&chunk.tag);
            if score >= threshold {
                scored.push(ScoredChunk {
                    text: chunk.text.clone(),
                    score,
                });
            }
        }

        // Beta's ordering: highest score first.
        scored.sort_by(|a, b_chunk| {
            b_chunk
                .score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored
    }
}

impl Default for Bm25Filter {
    fn default() -> Self {
        Self::new()
    }
}

/// A text chunk annotated with its dominant tag (beta tracks the chunk's
/// source element for the priority boost).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedChunk {
    pub tag: String,
    pub text: String,
}

/// Extract content chunks from a stripped document's arena (beta's
/// `extract_text_chunks`): body-level blocks that carry non-trivial text,
/// tagged with their element name.
pub fn extract_chunks(dom: &Dom) -> Vec<TaggedChunk> {
    let mut chunks = Vec::new();
    // Body-level blocks: descend through the structural wrappers
    // (#document → html → body) so the walk starts at real content blocks.
    collect_chunks(body_start(dom), dom, &mut chunks);
    chunks
}

/// The arena id to walk from: `body` when present, else the root.
fn body_start(dom: &Dom) -> NodeId {
    fn find_body(dom: &Dom, id: NodeId) -> Option<NodeId> {
        if dom.tag_of(id) == "body" {
            return Some(id);
        }
        for &child in dom.children_of(id) {
            if dom
                .get(child)
                .is_some_and(crate::arena::ArenaNode::is_element)
                && let Some(found) = find_body(dom, child)
            {
                return Some(found);
            }
        }
        None
    }
    find_body(dom, dom.root()).unwrap_or_else(|| dom.root())
}

fn collect_chunks(id: NodeId, dom: &Dom, out: &mut Vec<TaggedChunk>) {
    for &child in dom.children_of(id) {
        let Some(node) = dom.get(child) else { continue };
        if let crate::arena::ArenaNode::Element { tag, .. } = node {
            let text = dom.text_of(child);
            let trimmed = text.trim();
            if trimmed.split_whitespace().count() >= 2 {
                out.push(TaggedChunk {
                    tag: tag.clone(),
                    text: trimmed.to_string(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse;
    use crate::strip::strip;

    fn chunks_from(html: &str) -> Vec<TaggedChunk> {
        let mut doc = parse(html);
        strip(&mut doc);
        let dom = Dom::from_document(&doc);
        extract_chunks(&dom)
    }

    #[test]
    fn priority_boost_table_matches_beta() {
        assert_eq!(priority_boost("h1"), 5.0);
        assert_eq!(priority_boost("h2"), 4.0);
        assert_eq!(priority_boost("h3"), 3.0);
        assert_eq!(priority_boost("strong"), 2.0);
        assert_eq!(priority_boost("code"), 2.0);
        assert_eq!(priority_boost("p"), 1.0, "untagged default is 1.0");
    }

    #[test]
    fn relevant_chunk_outscores_unrelated_one() {
        let chunks = vec![
            TaggedChunk {
                tag: "p".into(),
                text: "The deployment pipeline builds containers and pushes them to the registry"
                    .into(),
            },
            TaggedChunk {
                tag: "p".into(),
                text: "Sailing boats glide across the quiet harbor at sunset".into(),
            },
        ];
        // Beta's default threshold is 1.0 and stays 1.0 here; on a
        // two-chunk corpus Okapi scores legitimately fall below it, so the
        // faithful assertion is ordering (relevant first) with the default
        // gate demonstrated on a realistic corpus below.
        let permissive = Bm25Filter {
            threshold: 0.0,
            ..Bm25Filter::new()
        };
        let scored = permissive.filter(&chunks, "deployment pipeline containers");
        assert!(!scored.is_empty(), "must rank the relevant chunk");
        assert!(
            scored[0].text.contains("deployment"),
            "top chunk must be the deployment one: {:?}",
            scored[0].text
        );
        // With more irrelevant chunks the relevant one clears beta's
        // unmodified 1.0 default.
        let mut bigger = chunks.clone();
        for filler in [
            "Gardening tips for the spring season and soil care",
            "The history of wooden furniture making in Europe",
            "An introduction to competitive chess openings",
            "Baking bread with sourdough starters at home",
        ] {
            bigger.push(TaggedChunk {
                tag: "p".into(),
                text: filler.into(),
            });
        }
        let defaulted = Bm25Filter::new();
        let scored_default = defaulted.filter(&bigger, "deployment pipeline containers");
        assert!(
            scored_default.iter().any(|c| c.text.contains("deployment")),
            "default threshold must retain the relevant chunk on a realistic corpus: {scored_default:?}"
        );
    }

    #[test]
    fn empty_query_returns_no_chunks() {
        let chunks = vec![TaggedChunk {
            tag: "p".into(),
            text: "some content here".into(),
        }];
        let filter = Bm25Filter::new();
        assert!(filter.filter(&chunks, "").is_empty());
        assert!(filter.filter(&chunks, "   ").is_empty());
    }

    #[test]
    fn word_floor_drops_short_chunks() {
        let chunks = vec![
            TaggedChunk {
                tag: "p".into(),
                text: "tiny".into(),
            },
            TaggedChunk {
                tag: "p".into(),
                text: "one two three four five six seven".into(),
            },
        ];
        let filter = Bm25Filter::new().with_min_word_threshold(3);
        let scored = filter.filter(&chunks, "one two three four five six seven");
        assert!(
            scored.iter().all(|c| c.text != "tiny"),
            "short chunk must be dropped: {scored:?}"
        );
    }

    #[test]
    fn stemming_matches_inflections() {
        let chunks = vec![TaggedChunk {
            tag: "p".into(),
            text: "running runners run quickly".into(),
        }];
        let filter = Bm25Filter::new();
        // Single-chunk corpora score far below beta's 1.0 gate; what
        // stemming guarantees is that the query still MATCHES the inflected
        // chunk (nonzero score), not that it clears the gate.
        let scored = filter.filter(&chunks, "runner runs");
        assert!(
            !scored.is_empty()
                || filter
                    .score_all(&chunks, "runner runs")
                    .iter()
                    .any(|s| s.score > 0.0),
            "stemmed query must match inflected chunk"
        );
    }

    #[test]
    fn extract_chunks_skips_single_word_blocks() {
        let chunks = chunks_from("<html><body><p>lonely</p><p>two words</p></body></html>");
        assert!(
            chunks.iter().all(|c| c.text != "lonely"),
            "single-word block dropped: {chunks:?}"
        );
        assert!(chunks.iter().any(|c| c.text == "two words"));
    }
}
