//! Hybrid scoring. Verbatim port of `mem0/utils/scoring.py:score_and_rank`
//! plus the cosine-similarity and entity-boost math from
//! `mem0/memory/main.py:_compute_entity_boosts`.
//!
//! Final formula (oracle V3):
//! ```text
//! combined = min((semantic + bm25_normalized + entity_boost) / max_possible, 1.0)
//! ```
//! where `max_possible` adapts to which signals are active:
//! - semantic only            → 1.0
//! - semantic + entity        → 1.5
//! - semantic + BM25          → 2.0
//! - semantic + BM25 + entity → 2.5

use std::collections::HashMap;

use crate::bm25::ENTITY_BOOST_WEIGHT;
use crate::types::{MemoryHit, ScoreBreakdown};

/// Cosine similarity between two equal-length f32 slices. Returns 0.0 for
/// zero vectors. Vectors of mismatched length return 0.0 (defensive — the
/// store guarantees equal dims at insert time).
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// One scored candidate produced internally before formatting into a `MemoryHit`.
#[derive(Debug, Clone)]
pub(crate) struct ScoredCandidate {
    pub memory_id: String,
    pub semantic_score: f32,
    pub payload_data: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub hash: Option<String>,
    pub attributed_to: Option<String>,
    pub extras: std::collections::BTreeMap<String, serde_json::Value>,
    pub scope_user_id: Option<String>,
    pub scope_agent_id: Option<String>,
    pub scope_run_id: Option<String>,
}

/// Score and rank candidates additively, returning top-k `MemoryHit`s.
/// Mirrors `mem0/utils/scoring.py:score_and_rank`.
///
/// Threshold gates `semantic_score` BEFORE combining: a candidate below
/// threshold is excluded even if BM25/entity would boost it.
#[must_use]
pub(crate) fn score_and_rank(
    semantic_results: &[ScoredCandidate],
    bm25_scores: &HashMap<String, f32>,
    entity_boosts: &HashMap<String, f32>,
    threshold: f32,
    top_k: usize,
    explain: bool,
) -> Vec<MemoryHit> {
    let has_bm25 = !bm25_scores.is_empty();
    let has_entity = !entity_boosts.is_empty();
    let mut max_possible = 1.0_f32;
    if has_bm25 {
        max_possible += 1.0;
    }
    if has_entity {
        max_possible += ENTITY_BOOST_WEIGHT;
    }

    let mut scored: Vec<(f32, &ScoredCandidate, f32, f32, f32)> = Vec::new();
    for cand in semantic_results {
        if cand.semantic_score < threshold {
            continue;
        }
        let bm25 = bm25_scores.get(&cand.memory_id).copied().unwrap_or(0.0);
        let entity = entity_boosts.get(&cand.memory_id).copied().unwrap_or(0.0);
        let raw = cand.semantic_score + bm25 + entity;
        let combined = (raw / max_possible).min(1.0);
        scored.push((combined, cand, bm25, entity, raw));
    }

    // Stable sort by combined score descending.
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    scored
        .into_iter()
        .take(top_k)
        .map(|(combined, cand, bm25, entity, raw)| MemoryHit {
            id: cand.memory_id.clone(),
            memory: cand.payload_data.clone(),
            score: combined,
            hash: cand.hash.clone(),
            created_at: cand.created_at.clone(),
            updated_at: cand.updated_at.clone(),
            attributed_to: cand
                .attributed_to
                .as_deref()
                .and_then(crate::types::Attribution::parse),
            scope: crate::types::Scope {
                user_id: cand.scope_user_id.clone(),
                agent_id: cand.scope_agent_id.clone(),
                run_id: cand.scope_run_id.clone(),
            },
            extras: cand.extras.clone(),
            score_details: explain.then_some(ScoreBreakdown {
                semantic_score: cand.semantic_score,
                bm25_score: bm25,
                entity_boost: entity,
                raw_score: raw,
                max_possible_score: max_possible,
                final_score: combined,
                threshold,
            }),
        })
        .collect()
}

/// Entity-boost math from `mem0/memory/main.py:_compute_entity_boosts`.
///
/// `boost = similarity * ENTITY_BOOST_WEIGHT * memory_count_weight` where
/// `memory_count_weight = 1/(1+0.001*(num_linked-1)^2)`. Bounded to
/// `[0, ENTITY_BOOST_WEIGHT]` per memory; takes the max boost across all
/// matching query entities.
#[must_use]
#[allow(dead_code)]
pub fn entity_boost(similarity: f32, num_linked: usize) -> f32 {
    if similarity < 0.5 {
        return 0.0;
    }
    let n = num_linked.max(1) as f32;
    let memory_count_weight = 1.0 / (1.0 + 0.001 * (n - 1.0).powi(2));
    similarity * ENTITY_BOOST_WEIGHT * memory_count_weight
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn cand(id: &str, semantic: f32) -> ScoredCandidate {
        ScoredCandidate {
            memory_id: id.to_string(),
            semantic_score: semantic,
            payload_data: format!("payload-{id}"),
            created_at: None,
            updated_at: None,
            hash: None,
            attributed_to: None,
            extras: BTreeMap::new(),
            scope_user_id: None,
            scope_agent_id: None,
            scope_run_id: None,
        }
    }

    #[test]
    fn cosine_basic() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-5);
        assert!((cosine(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-5);
        assert!((cosine(&[1.0, 1.0], &[1.0, 1.0]) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_handles_mismatch_and_zero() {
        assert_eq!(cosine(&[1.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn divisor_adapts_to_active_signals() {
        let cands = vec![cand("a", 0.9)];
        // Semantic only → max_possible = 1.0
        let only_sem = score_and_rank(&cands, &HashMap::new(), &HashMap::new(), 0.0, 10, false);
        assert!(only_sem[0].score > 0.89);

        // + BM25 → 2.0
        let mut bm25 = HashMap::new();
        bm25.insert("a".to_string(), 0.8_f32);
        let with_bm = score_and_rank(&cands, &bm25, &HashMap::new(), 0.0, 10, true);
        let det = with_bm[0].score_details.as_ref().unwrap();
        assert!((det.max_possible_score - 2.0).abs() < 1e-5);
        assert!((det.final_score - (0.9 + 0.8) / 2.0).abs() < 1e-5);

        // + entity → 2.5
        let mut ent = HashMap::new();
        ent.insert("a".to_string(), 0.3_f32);
        let with_both = score_and_rank(&cands, &bm25, &ent, 0.0, 10, true);
        let det = with_both[0].score_details.as_ref().unwrap();
        assert!((det.max_possible_score - 2.5).abs() < 1e-5);
        assert!((det.final_score - (0.9 + 0.8 + 0.3) / 2.5).abs() < 1e-5);
    }

    #[test]
    fn threshold_gates_before_combining() {
        let cands = vec![cand("a", 0.05)];
        let mut bm25 = HashMap::new();
        bm25.insert("a".to_string(), 1.0_f32);
        let out = score_and_rank(&cands, &bm25, &HashMap::new(), 0.1, 10, false);
        assert!(out.is_empty(), "below-threshold candidate must be dropped");
    }

    #[test]
    fn top_k_truncates_after_sort() {
        let cands = vec![cand("a", 0.4), cand("b", 0.9), cand("c", 0.7)];
        let out = score_and_rank(&cands, &HashMap::new(), &HashMap::new(), 0.0, 2, false);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "b");
        assert_eq!(out[1].id, "c");
    }

    #[test]
    fn entity_boost_bounded_and_decays_with_count() {
        let single = entity_boost(0.9, 1);
        let many = entity_boost(0.9, 100);
        assert!(single <= ENTITY_BOOST_WEIGHT);
        assert!(many < single, "hyper-connected entity must decay");
        assert_eq!(entity_boost(0.4, 1), 0.0, "below similarity floor");
    }
}
