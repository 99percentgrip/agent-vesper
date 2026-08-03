//! BM25 normalization. Verbatim port of `mem0/utils/scoring.py`.
//!
//! Sigmoid normalization of raw BM25 scores to `[0, 1]`, with query-length-
//! adaptive midpoint/steepness parameters. The raw score comes from SQLite
//! FTS5's `bm25(table)` auxiliary function (which returns a negative value
//! where lower = better); we negate before applying the sigmoid so higher
//! similarity wins.

/// Weight contributed by a single entity boost. Mirrors
/// `ENTITY_BOOST_WEIGHT = 0.5` in the oracle.
pub const ENTITY_BOOST_WEIGHT: f32 = 0.5;

/// Returns `(midpoint, steepness)` for sigmoid normalization based on the
/// number of lemmatized query terms. Verbatim from
/// `mem0/utils/scoring.py:get_bm25_params`.
#[must_use]
pub fn get_bm25_params(num_terms: usize) -> (f32, f32) {
    if num_terms <= 3 {
        (5.0, 0.7)
    } else if num_terms <= 6 {
        (7.0, 0.6)
    } else if num_terms <= 9 {
        (9.0, 0.5)
    } else if num_terms <= 15 {
        (10.0, 0.5)
    } else {
        (12.0, 0.5)
    }
}

/// Logistic-sigmoid normalization. Verbatim from
/// `mem0/utils/scoring.py:normalize_bm25`.
#[must_use]
pub fn normalize_bm25(raw_score: f32, midpoint: f32, steepness: f32) -> f32 {
    1.0 / (1.0 + (-steepness * (raw_score - midpoint)).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_table_matches_oracle() {
        assert_eq!(get_bm25_params(1), (5.0, 0.7));
        assert_eq!(get_bm25_params(3), (5.0, 0.7));
        assert_eq!(get_bm25_params(6), (7.0, 0.6));
        assert_eq!(get_bm25_params(9), (9.0, 0.5));
        assert_eq!(get_bm25_params(15), (10.0, 0.5));
        assert_eq!(get_bm25_params(99), (12.0, 0.5));
    }

    #[test]
    fn sigmoid_at_midpoint_is_half() {
        let (midpoint, steepness) = get_bm25_params(5);
        let at_midpoint = normalize_bm25(midpoint, midpoint, steepness);
        assert!((at_midpoint - 0.5).abs() < 1e-5);
    }

    #[test]
    fn sigmoid_is_monotonic() {
        let (midpoint, steepness) = get_bm25_params(5);
        let lo = normalize_bm25(midpoint - 2.0, midpoint, steepness);
        let hi = normalize_bm25(midpoint + 2.0, midpoint, steepness);
        assert!(lo < 0.5);
        assert!(hi > 0.5);
        assert!(lo < hi);
    }

    #[test]
    fn sigmoid_bounded_to_unit_interval() {
        let (midpoint, steepness) = get_bm25_params(5);
        for raw in [-50.0_f32, -1.0, 0.0, 5.0, 10.0, 50.0, 1000.0] {
            let n = normalize_bm25(raw, midpoint, steepness);
            assert!((0.0..=1.0).contains(&n), "raw={raw} normalized={n}");
        }
    }
}
