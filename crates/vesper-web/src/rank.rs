//! Map relevance ranking — the cosine-similarity port of web oracle
//! alpha's `map-cosine.ts` (VRO-14 PRD §1.5).
//!
//! Alpha's scheme, kept faithful: the query is lowercased and split on
//! non-word characters; for each query word we count its (case-insensitive)
//! occurrences in the link's text-plus-URL and normalize by the length of
//! that haystack, producing one dimension per query word. Two links are
//! compared via cosine similarity of those character-count vectors against
//! the query's own self-vector (dimensions in query-word order).
//!
//! With no query, ranking degrades to stable identity ordering (input
//! order preserved) — alpha returns the list unranked in that case.

use crate::links::LinkRecord;

/// Score one haystack (link text + URL, lowercased) against the query
/// word list, producing one normalized count per query word.
fn text_to_vector(haystack: &str, query_words: &[&str]) -> Vec<f32> {
    if haystack.is_empty() {
        return vec![0.0; query_words.len()];
    }
    let haystack_lower = haystack.to_lowercase();
    let haystack_len = haystack_lower.chars().count().max(1) as f32;
    query_words
        .iter()
        .map(|word| {
            if word.is_empty() {
                return 0.0;
            }
            let occurrences = haystack_lower.match_indices(word).count() as f32;
            occurrences / haystack_len
        })
        .collect()
}

/// Cosine similarity of two equal-length vectors (0 when either is zero).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        0.0
    } else {
        dot / (mag_a * mag_b)
    }
}

/// Split a query into alpha's `\W+`-style word list (non-word boundaries).
fn query_words(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// One ranked link (score + stable input index for tie-breaking).
#[derive(Debug, Clone, PartialEq)]
pub struct RankedLink {
    pub link: LinkRecord,
    pub score: f32,
    /// Input position, preserved for stable ordering on equal scores.
    pub input_index: usize,
}

/// Rank links by cosine similarity of (link text + URL) against the
/// query (alpha's `performCosineSimilarity` shape). With an empty query
/// the input order is returned unchanged (identity ordering contract).
pub fn rank_links(links: Vec<LinkRecord>, query: &str) -> Vec<RankedLink> {
    let words = query_words(query);
    if words.is_empty() {
        return links
            .into_iter()
            .enumerate()
            .map(|(input_index, link)| RankedLink {
                link,
                score: 0.0,
                input_index,
            })
            .collect();
    }

    // The query's self-vector uses the query text itself as haystack.
    let word_refs: Vec<&str> = words.iter().map(String::as_str).collect();
    let query_vector = text_to_vector(&query.to_lowercase(), &word_refs);

    let mut ranked: Vec<RankedLink> = links
        .into_iter()
        .enumerate()
        .map(|(input_index, link)| {
            let text: &str = link.text.as_str();
            let haystack = format!("{text} {}", link.url);
            let link_vector = text_to_vector(&haystack, &word_refs);
            let score = cosine_similarity(&link_vector, &query_vector);
            RankedLink {
                link,
                score,
                input_index,
            }
        })
        .collect();

    // Stable sort by descending score; equal scores keep input order.
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str, text: &str) -> LinkRecord {
        LinkRecord {
            url: url.to_string(),
            text: text.to_string(),
            rel: Vec::new(),
            nofollow: false,
        }
    }

    #[test]
    fn empty_query_preserves_input_order() {
        let links = vec![
            link("https://x.invalid/z", "zeta"),
            link("https://x.invalid/a", "alpha"),
            link("https://x.invalid/m", "middle"),
        ];
        let ranked = rank_links(links, "");
        let urls: Vec<&str> = ranked.iter().map(|r| r.link.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://x.invalid/z",
                "https://x.invalid/a",
                "https://x.invalid/m"
            ],
            "no query must mean identity ordering"
        );
    }

    #[test]
    fn whitespace_only_query_preserves_input_order() {
        let links = vec![
            link("https://x.invalid/a", "a"),
            link("https://x.invalid/b", "b"),
        ];
        let ranked = rank_links(links, "   ");
        assert_eq!(ranked[0].link.url, "https://x.invalid/a");
        assert_eq!(ranked[1].link.url, "https://x.invalid/b");
    }

    #[test]
    fn matching_link_outranks_unrelated_link() {
        // Recorded set: three links, one obviously about the query.
        let links = vec![
            link("https://docs.invalid/contact", "Contact page"),
            link("https://docs.invalid/guide/install", "Installation guide"),
            link("https://docs.invalid/blog/news", "Company news"),
        ];
        let ranked = rank_links(links, "installation guide");
        assert_eq!(
            ranked[0].link.url,
            "https://docs.invalid/guide/install",
            "the install guide must rank first: {:?}",
            ranked
                .iter()
                .map(|r| (r.link.url.clone(), r.score))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn scores_are_zero_for_unrelated_links() {
        let links = vec![link("https://x.invalid/zzz", "zzz")];
        let ranked = rank_links(links, "query");
        assert_eq!(ranked[0].score, 0.0);
    }

    #[test]
    fn link_text_contributes_to_score() {
        // Same URL shape, different anchor text: the matching text wins.
        let links = vec![
            link("https://shop.invalid/item/1", "completely unrelated"),
            link("https://shop.invalid/item/2", "pricing details"),
        ];
        let ranked = rank_links(links, "pricing");
        assert_eq!(ranked[0].link.url, "https://shop.invalid/item/2");
    }

    #[test]
    fn equal_scores_keep_input_order() {
        // Two identical-haystack links must not swap.
        let links = vec![
            link("https://x.invalid/a", "docs"),
            link("https://x.invalid/b", "docs"),
        ];
        let ranked = rank_links(links, "docs");
        assert_eq!(ranked[0].link.url, "https://x.invalid/a");
        assert_eq!(ranked[1].link.url, "https://x.invalid/b");
    }

    #[test]
    fn cosine_self_similarity_is_one() {
        let v = vec![0.3, 0.4, 0.5];
        let s = cosine_similarity(&v, &v);
        assert!((s - 1.0).abs() < 1e-6, "self similarity {s}");
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn word_split_matches_alpha_nonword_boundaries() {
        let words = query_words("Install-Guide, 2nd edition!");
        assert_eq!(words, vec!["install", "guide", "2nd", "edition"]);
    }
}
