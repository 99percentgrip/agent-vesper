//! Local deterministic embedding via feature hashing.
//!
//! A zero-dependency, zero-network embedding implementation that produces
//! fixed-dimension vectors from text using the hashing trick (a.k.a. feature
//! hashing). Each word is hashed to a position in the vector with a sign
//! derived from a second hash, accumulating frequency. The result is
//! normalized to unit length for cosine similarity.
//!
//! Quality is lower than neural embeddings (it's a bag-of-words model with
//! no contextual information), but it is:
//! - **Zero network**: no API call, no API key, no latency.
//! - **Deterministic**: same text always produces the same vector.
//! - **Immediately functional**: the cognitive memory system works out of
//!   the box without any provider configuration.
//!
//! The hybrid retrieval pipeline compensates for the weaker embedding signal
//! via the FTS5 BM25 keyword score and the entity-boost graph, which do not
//! depend on embeddings.

use std::collections::HashMap;

use crate::error::Result;
use crate::ports::{EmbedAction, EmbeddingPort};

/// Local feature-hashing embedder. Produces `dim`-dimensional vectors.
#[derive(Debug, Clone)]
pub struct LocalHashEmbedder {
    dim: usize,
}

impl LocalHashEmbedder {
    /// Creates a new embedder with the given output dimension.
    #[must_use]
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }

    /// Hash a string to a `u64` using a simple but well-distributed
    /// polynomial rolling hash. No external dependency needed.
    fn hash(s: &str) -> u64 {
        // FNV-1a: fast, good distribution for short strings.
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

impl EmbeddingPort for LocalHashEmbedder {
    fn embed(&self, text: &str, _action: EmbedAction) -> Result<Vec<f32>> {
        let mut vec = vec![0.0_f32; self.dim];

        // Tokenize: lowercase, split on non-alphanumeric, drop stopword-like
        // single chars and empties. Count word frequencies.
        let mut freq: HashMap<String, f32> = HashMap::new();
        for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if word.len() < 2 {
                continue;
            }
            *freq.entry(word.to_string()).or_insert(0.0) += 1.0;
        }

        // Signed feature hashing: each word gets hashed to an index (position)
        // and a second hash determines the sign (+1 or -1). This reduces
        // collision bias compared to unsigned hashing.
        for (word, count) in &freq {
            let h1 = Self::hash(word);
            let idx = (h1 % self.dim as u64) as usize;
            let h2 = Self::hash(&format!("{word}_sign"));
            let sign = if h2 % 2 == 0 { 1.0 } else { -1.0 };
            vec[idx] += sign * count;
        }

        // L2-normalize so cosine similarity is bounded to [-1, 1].
        let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        Ok(vec)
    }

    /// Distinct model name (Gap 11) so the composition boundary detects a
    /// swap from this in-process embedder to a neural one via the
    /// `cognition_meta.embedding_model` key — not just by dimension (which
    /// two distinct embedders can happen to share).
    fn model_name(&self) -> &str {
        "local-hash-embedder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_produces_identical_embedding() {
        let emb = LocalHashEmbedder::new(256);
        let a = emb
            .embed("User likes Rust programming", EmbedAction::Add)
            .unwrap();
        let b = emb
            .embed("User likes Rust programming", EmbedAction::Search)
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_produces_different_embedding() {
        let emb = LocalHashEmbedder::new(256);
        let a = emb.embed("User likes Rust", EmbedAction::Add).unwrap();
        let b = emb.embed("User dislikes Python", EmbedAction::Add).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn embedding_is_unit_length() {
        let emb = LocalHashEmbedder::new(256);
        let v = emb
            .embed("hello world this is a test", EmbedAction::Add)
            .unwrap();
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn similar_text_has_higher_cosine_than_dissimilar() {
        let emb = LocalHashEmbedder::new(1024);
        let target = emb
            .embed(
                "Marcus works at Shopify as a Senior Engineer",
                EmbedAction::Search,
            )
            .unwrap();
        let similar = emb
            .embed("Marcus is a Senior Engineer", EmbedAction::Add)
            .unwrap();
        let dissimilar = emb
            .embed("The weather is sunny today", EmbedAction::Add)
            .unwrap();

        let cos_sim = crate::score::cosine(&target, &similar);
        let cos_dis = crate::score::cosine(&target, &dissimilar);
        assert!(
            cos_sim > cos_dis,
            "similar text should have higher cosine: {cos_sim} vs {cos_dis}"
        );
    }

    #[test]
    fn empty_text_produces_zero_vector() {
        let emb = LocalHashEmbedder::new(64);
        let v = emb.embed("", EmbedAction::Add).unwrap();
        assert!(v.iter().all(|x| *x == 0.0));
    }
}
