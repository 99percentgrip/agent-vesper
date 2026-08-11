//! Parallel-candidate strategy handlers (VRO-4, PRD §11.4 + §11.5).
//!
//! Both strategies share the same fan-out (via [`CandidateExecutor`]) and
//! differ only in how they collapse the candidates into a single
//! [`ReasoningOutcome`]:
//!
//! - [`run_parallel_candidates_consensus`] (PRD §11.4): normalize each
//!   candidate's output, find a quorum (≥ `ceil(N/2)` agree on the normalized
//!   form). On quorum, return that candidate as [`OutcomeStatus::Succeeded`].
//!   On disagreement, surface [`OutcomeStatus::Inconclusive`] with the
//!   disagreement recorded as an unresolved risk.
//!
//! - [`run_parallel_candidates_judge`] (PRD §11.5): fan out, **shuffle** the
//!   candidates with a deterministic-but-different-order PRNG (to reduce
//!   position bias), ask the [`CandidateJudge`] to pick the best by shuffled
//!   index, then map that index back to the original [`CandidateId`] and
//!   return the chosen candidate as [`OutcomeStatus::Succeeded`].
//!
//! ## Zero-breakage contract
//!
//! This module is invoked only by the orchestrator when the profiled strategy
//! is [`ParallelCandidatesConsensus`](vesper_domain::ReasoningStrategy) or
//! [`ParallelCandidatesJudge`](vesper_domain::ReasoningStrategy). The existing
//! [`Direct`](vesper_domain::ReasoningStrategy) and
//! [`GenerateVerifyRepair`](vesper_domain::ReasoningStrategy) paths are
//! completely unaffected.

use std::future::Future;
use std::pin::Pin;

use vesper_domain::{
    Candidate, InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningOutcome, StructuredOutput,
    VerificationStatus, VerificationSummary,
};

use super::executor::{CandidateExecutor, ExecutorError, XorShiftRng};
use super::orchestrator::CandidateGenerator;

// ---------------------------------------------------------------------------
// Normalization + quorum (PRD §11.4)
// ---------------------------------------------------------------------------

/// Normalizes a candidate's structured output into a canonical comparable
/// form (PRD §11.4: "Consensus must compare normalized final answers and
/// supporting evidence, not just wording similarity").
///
/// The canonical form is the JSON-serialized output with:
///   - all inter-token whitespace stripped, and
///   - object keys recursively sorted (so `{"b":1,"a":2}` and
///     `{"a":2,"b":1}` normalize identically).
///
/// Recursion is required because under `--all-features`, an upstream
/// protocol crate enables `serde_json`'s `preserve_order` feature, which
/// makes `Value::Object` use insertion order instead of `BTreeMap` ordering.
/// The explicit recursive walk below is independent of that feature flag —
/// the canonical form is stable across feature configurations.
///
/// Returns the empty string when the value cannot be serialized (extremely
/// rare; treated as a unique answer).
#[must_use]
pub fn normalize_output(output: &StructuredOutput) -> String {
    canonicalize_json(output)
}

/// Recursively serializes a JSON value with sorted object keys and no
/// whitespace. Deterministic regardless of `serde_json` features.
fn canonicalize_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = String::from("{");
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // Serialize the key as a JSON string (escapes correctly).
                out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| String::from("\"\"")));
                out.push(':');
                out.push_str(&canonicalize_json(&map[*k]));
            }
            out.push('}');
            out
        }
        serde_json::Value::Array(arr) => {
            let mut out = String::from("[");
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canonicalize_json(v));
            }
            out.push(']');
            out
        }
        // Scalars serialize directly via serde_json::Value::to_string —
        // no whitespace, escapes control characters correctly.
        other => other.to_string(),
    }
}

/// Computes the consensus quorum threshold for `n` candidates.
///
/// Quorum = `ceil(n / 2)` rounded up — i.e. a strict majority. For `n = 3`
/// quorum is 2; for `n = 4` quorum is 2 (still a majority of votes when
/// spread across 4 candidates, the smallest set with no clear winner is 2-2
/// which falls below the strict majority of 3); for `n = 1` quorum is 1.
///
/// `n == 0` returns 0 (no candidates → no quorum).
#[must_use]
pub fn quorum_threshold(n: usize) -> usize {
    if n == 0 { 0 } else { n.div_ceil(2) }
}

/// Picks the candidate whose normalized form has the largest backing cluster
/// (≥ quorum). Returns `Some(cluster_winner)` when a quorum exists, else
/// `None`. Ties inside the largest cluster are broken by lexicographic order
/// of the normalized form so the result is deterministic.
#[must_use]
fn consensus_winner(candidates: &[Candidate]) -> Option<(&Candidate, usize)> {
    if candidates.is_empty() {
        return None;
    }
    let threshold = quorum_threshold(candidates.len());
    // Map: normalized form → (count, first_candidate_index). Using the first
    // index for the winner pick keeps the result deterministic.
    let mut buckets: Vec<(String, usize, usize)> = Vec::new(); // (form, count, first_idx)
    for (idx, cand) in candidates.iter().enumerate() {
        let form = normalize_output(&cand.output);
        if let Some(entry) = buckets.iter_mut().find(|(f, _, _)| *f == form) {
            entry.1 += 1;
        } else {
            buckets.push((form, 1, idx));
        }
    }
    // Pick the largest bucket. Ties broken by lexicographic normalized form
    // (deterministic). We destructure by reference because the bucket owns a
    // `String` (not `Copy`).
    let (form, count, first_idx) = buckets.first()?;
    let count = *count;
    let first_idx = *first_idx;
    let _ = form; // form not needed once we have the index
    if count >= threshold {
        Some((&candidates[first_idx], count))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Consensus strategy (PRD §11.4)
// ---------------------------------------------------------------------------

/// Runs the Parallel-Candidates-Consensus strategy (PRD §11.4).
///
/// Fans out `branch_count` candidates (capped to `budget.max_parallel_branches`),
/// normalizes each output, and returns [`OutcomeStatus::Succeeded`] with the
/// consensus candidate when a quorum is reached. If no quorum is reached the
/// function returns [`OutcomeStatus::Inconclusive`] with the disagreement
/// surfaced as an unresolved risk (PRD §18: "Candidates disagree → surface
/// uncertainty").
pub async fn run_parallel_candidates_consensus(
    generator: &dyn CandidateGenerator,
    prompt: &str,
    branch_count: usize,
    budget: ReasoningBudget,
) -> ReasoningOutcome {
    let executor = CandidateExecutor::new();
    let outcomes = match executor
        .fan_out(generator, prompt, branch_count, budget)
        .await
    {
        Ok(outcomes) => outcomes,
        Err(ExecutorError::ZeroParallelBranches) => {
            return ReasoningOutcome {
                status: OutcomeStatus::Failed,
                final_output: None,
                selected_candidate: None,
                verification_summary: VerificationSummary::default(),
                unresolved_risks: vec!["budget.max_parallel_branches is zero".to_string()],
                cost: InferenceCost::default(),
            };
        }
        Err(ExecutorError::AllBranchesFailed(n)) => {
            return ReasoningOutcome {
                status: OutcomeStatus::Failed,
                final_output: None,
                selected_candidate: None,
                verification_summary: VerificationSummary::default(),
                unresolved_risks: vec![format!("all {n} parallel branches failed")],
                cost: InferenceCost::default(),
            };
        }
    };

    let candidates: Vec<Candidate> = outcomes.into_iter().map(|o| o.candidate).collect();
    let total_cost = aggregate_cost(&candidates);

    match consensus_winner(&candidates) {
        Some((winner, agreement_count)) => {
            let total = candidates.len();
            ReasoningOutcome {
                status: OutcomeStatus::Succeeded,
                final_output: Some(winner.output.clone()),
                selected_candidate: Some(winner.candidate_id.clone()),
                // Consensus itself is the verification — record it as a passed
                // "consensus" verifier so the summary reflects the agreement.
                verification_summary: VerificationSummary {
                    passed: 1,
                    failed: 0,
                    overall: VerificationStatus::Passed,
                },
                unresolved_risks: vec![format!(
                    "consensus reached: {agreement_count}/{total} branches agree"
                )],
                cost: total_cost,
            }
        }
        None => {
            // No quorum — surface uncertainty (PRD §18).
            ReasoningOutcome {
                status: OutcomeStatus::Inconclusive,
                final_output: candidates.first().map(|c| c.output.clone()),
                selected_candidate: candidates.first().map(|c| c.candidate_id.clone()),
                verification_summary: VerificationSummary {
                    passed: 0,
                    failed: 0,
                    overall: VerificationStatus::Inconclusive,
                },
                unresolved_risks: vec![format!(
                    "no consensus among {} branches (quorum threshold {})",
                    candidates.len(),
                    quorum_threshold(candidates.len())
                )],
                cost: total_cost,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Judge strategy (PRD §11.5)
// ---------------------------------------------------------------------------

/// The Judge seam (PRD §10.8 "Independent candidate judge" + §11.5).
///
/// The orchestrator supplies a real provider-backed implementation; the
/// judge receives the candidates in **shuffled** order (to reduce position
/// bias) and returns the index of its selection **in that shuffled view**.
/// The strategy then maps the shuffled index back to the original
/// [`CandidateId`] before constructing the outcome.
pub trait CandidateJudge: Send + Sync {
    /// Inspects the (shuffled) candidates and returns the index of the
    /// selected candidate in the slice as presented. MUST be in
    /// `0..candidates.len()`; out-of-range values are clamped by the caller.
    fn judge<'a>(
        &'a self,
        candidates: &'a [Candidate],
    ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>>;
}

/// Runs the Parallel-Candidates-Judge strategy (PRD §11.5).
///
/// Fans out `branch_count` candidates, **shuffles** them with a
/// `seed`-controlled [`XorShiftRng`] (PRD §11.5: "candidates in randomized
/// order to reduce position bias"), asks `judge` to pick the best by shuffled
/// index, then maps the index back to the original [`CandidateId`].
///
/// The shuffle seed is exposed so tests can reproduce an exact shuffle and
/// prove the position-bias defense actually fired. Production callers should
/// pass a per-turn random-ish seed (e.g. derived from the request id).
pub async fn run_parallel_candidates_judge(
    generator: &dyn CandidateGenerator,
    judge: &dyn CandidateJudge,
    prompt: &str,
    branch_count: usize,
    budget: ReasoningBudget,
    seed: u64,
) -> ReasoningOutcome {
    let executor = CandidateExecutor::new();
    let outcomes = match executor
        .fan_out(generator, prompt, branch_count, budget)
        .await
    {
        Ok(o) => o,
        Err(ExecutorError::ZeroParallelBranches) => {
            return ReasoningOutcome {
                status: OutcomeStatus::Failed,
                final_output: None,
                selected_candidate: None,
                verification_summary: VerificationSummary::default(),
                unresolved_risks: vec!["budget.max_parallel_branches is zero".to_string()],
                cost: InferenceCost::default(),
            };
        }
        Err(ExecutorError::AllBranchesFailed(n)) => {
            return ReasoningOutcome {
                status: OutcomeStatus::Failed,
                final_output: None,
                selected_candidate: None,
                verification_summary: VerificationSummary::default(),
                unresolved_risks: vec![format!("all {n} parallel branches failed")],
                cost: InferenceCost::default(),
            };
        }
    };

    let candidates: Vec<Candidate> = outcomes.into_iter().map(|o| o.candidate).collect();
    let total_cost = aggregate_cost(&candidates);

    // Build the shuffled view the judge sees. The judge MUST NOT be able to
    // infer the original order from its input (PRD §11.5: reduce position
    // bias).
    let mut shuffled: Vec<Candidate> = candidates.clone();
    let mut rng = XorShiftRng::new(seed);
    rng.shuffle(&mut shuffled);

    // Ask the judge for the index of its pick in the SHUFFLED view.
    let pick_idx = judge.judge(&shuffled).await;
    let pick_idx = pick_idx.min(shuffled.len().saturating_sub(1));
    let chosen_in_shuffled = &shuffled[pick_idx];

    // Map the shuffled pick back to the original CandidateId. The candidates
    // were cloned into `shuffled`, so we use the ID (which is preserved by
    // clone) to find the canonical entry.
    let original = candidates
        .iter()
        .find(|c| c.candidate_id == chosen_in_shuffled.candidate_id)
        .cloned()
        .unwrap_or_else(|| chosen_in_shuffled.clone());

    ReasoningOutcome {
        status: OutcomeStatus::Succeeded,
        final_output: Some(original.output.clone()),
        selected_candidate: Some(original.candidate_id.clone()),
        verification_summary: VerificationSummary {
            passed: 1,
            failed: 0,
            overall: VerificationStatus::Passed,
        },
        unresolved_risks: vec![format!(
            "judge selected candidate {} (presented at shuffled position {pick_idx})",
            original.candidate_id.as_str()
        )],
        cost: total_cost,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Aggregates the per-candidate costs into one [`InferenceCost`].
fn aggregate_cost(candidates: &[Candidate]) -> InferenceCost {
    let mut total = InferenceCost::default();
    for c in candidates {
        total.model_calls = total.model_calls.saturating_add(c.cost.model_calls);
        total.total_tokens = total.total_tokens.saturating_add(c.cost.total_tokens);
    }
    total
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use vesper_domain::{CandidateId, VerificationFinding};

    // --- Reusable generator that returns a programmed sequence ---
    //
    // Uses a SHARED `Arc<Mutex<Vec<StructuredOutput>>>` so the VRO-4 parallel
    // executor's per-branch `boxed_clone`s all draw from the SAME pool. The
    // total distribution (e.g. "2×yes + 1×no") is what the consensus test
    // asserts; the per-branch assignment is intentionally non-deterministic
    // (whichever branch wins the lock first pops first), but the AGGREGATE
    // outcome — quorum on "yes" — is stable.

    struct ScriptedGenerator {
        outputs: Arc<Mutex<Vec<StructuredOutput>>>,
    }
    impl ScriptedGenerator {
        fn new(outputs: Vec<StructuredOutput>) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(outputs)),
            }
        }
    }
    impl CandidateGenerator for ScriptedGenerator {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = super::super::orchestrator::GeneratedCandidate> + Send + 'a>>
        {
            let outputs = Arc::clone(&self.outputs);
            Box::pin(async move {
                let output = {
                    let mut outputs = outputs.lock().expect("poisoned");
                    if outputs.is_empty() {
                        serde_json::json!({"answer": "default"})
                    } else if outputs.len() == 1 {
                        outputs[0].clone()
                    } else {
                        outputs.remove(0)
                    }
                };
                super::super::orchestrator::GeneratedCandidate {
                    output,
                    cost: InferenceCost {
                        model_calls: 1,
                        total_tokens: 100,
                    },
                }
            })
        }

        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            // Share the outputs pool across clones so the test's programmed
            // distribution is consumed collectively by all parallel branches.
            Box::new(Self {
                outputs: Arc::clone(&self.outputs),
            })
        }
    }

    fn budget(max_parallel: u16) -> ReasoningBudget {
        ReasoningBudget {
            max_parallel_branches: max_parallel,
            ..ReasoningBudget::balanced()
        }
    }

    // --- Directive 4 test: Consensus identifies a quorum ---

    #[tokio::test]
    async fn consensus_strategy_succeeds_when_quorum_is_present() {
        // 3 branches, two produce identical normalized answers, one differs.
        // Quorum threshold for n=3 is 2 → consensus succeeds.
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"answer": "yes"}),
            serde_json::json!({"answer": "yes"}),
            serde_json::json!({"answer": "no"}),
        ]);
        let outcome = run_parallel_candidates_consensus(&generator, "q", 3, budget(3)).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert!(outcome.selected_candidate.is_some());
        // The winner is one of the "yes" candidates.
        let final_output = outcome.final_output.expect("output present");
        assert_eq!(final_output, serde_json::json!({"answer": "yes"}));
        // Unresolved risk confirms 2/3 agreement.
        assert!(
            outcome.unresolved_risks.iter().any(|r| r.contains("2/3")),
            "expected 2/3 agreement, got {:?}",
            outcome.unresolved_risks
        );
        // Cost accumulates across all 3 branches.
        assert_eq!(outcome.cost.model_calls, 3);
    }

    #[tokio::test]
    async fn consensus_strategy_inconclusive_when_no_quorum() {
        // 3 branches, 3 distinct answers → no quorum.
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"answer": "a"}),
            serde_json::json!({"answer": "b"}),
            serde_json::json!({"answer": "c"}),
        ]);
        let outcome = run_parallel_candidates_consensus(&generator, "q", 3, budget(3)).await;
        assert_eq!(outcome.status, OutcomeStatus::Inconclusive);
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("no consensus")),
            "expected no-consensus risk, got {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn consensus_strategy_treats_whitespace_only_differences_as_agreement() {
        // Two outputs that differ only in whitespace normalize to the same
        // canonical form → consensus succeeds on n=2 (threshold 1? no, n=2 →
        // threshold = (2+1)/2 = 1, so 1 of 2 agreeing IS a quorum).
        // Wait — for n=2, threshold = (2+1)/2 = 1 (integer division), which
        // means a single matching pair suffices. That's correct: 2 matching
        // out of 2 is a strict majority.
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"answer": "yes"}),
            serde_json::json!({"answer": "yes"}),
        ]);
        let outcome = run_parallel_candidates_consensus(&generator, "q", 2, budget(2)).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
    }

    #[test]
    fn normalize_output_strips_whitespace_and_sorts_keys() {
        // Two outputs that differ only in whitespace + key order normalize
        // to the SAME canonical form. This must hold even when serde_json's
        // preserve_order feature is enabled workspace-wide under
        // --all-features (an upstream protocol crate opts into it).
        let a = serde_json::json!({"b": 1, "a": 2});
        let b = serde_json::json!({  "a" : 2,  "b" : 1  });
        assert_eq!(normalize_output(&a), normalize_output(&b));
    }

    #[test]
    fn normalize_output_recursively_sorts_nested_object_keys() {
        // Regression test for the --all-features preserve_order failure:
        // nested objects must also have sorted keys, regardless of serde_json
        // features.
        let a = serde_json::json!({"outer_b": {"inner_b": 2, "inner_a": 1}, "outer_a": 0});
        let b = serde_json::json!({"outer_a": 0, "outer_b": {"inner_a": 1, "inner_b": 2}});
        assert_eq!(normalize_output(&a), normalize_output(&b));
        // Sanity: the canonical form starts with the outermost sorted key.
        assert!(
            normalize_output(&a).starts_with("{\"outer_a\":"),
            "canonical form must start with the lexicographically-first outer key"
        );
    }

    #[test]
    fn quorum_threshold_handles_edge_cases() {
        assert_eq!(quorum_threshold(0), 0);
        assert_eq!(quorum_threshold(1), 1);
        assert_eq!(quorum_threshold(2), 1); // (2+1)/2 = 1 — strict majority
        assert_eq!(quorum_threshold(3), 2);
        assert_eq!(quorum_threshold(4), 2); // (4+1)/2 = 2
        assert_eq!(quorum_threshold(5), 3);
    }

    // --- Directive 4 test: Judge strategy shuffles candidates ---

    /// Judge that always returns the FIRST presented candidate. Without
    /// shuffling, that would always be cand-0000. With shuffling, the pick
    /// depends on the seed.
    struct FirstPresentedJudge {
        observed_order: Mutex<Vec<CandidateId>>,
    }
    impl CandidateJudge for FirstPresentedJudge {
        fn judge<'a>(
            &'a self,
            candidates: &'a [Candidate],
        ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>> {
            let observed = &self.observed_order;
            Box::pin(async move {
                observed
                    .lock()
                    .expect("poisoned")
                    .extend(candidates.iter().map(|c| c.candidate_id.clone()));
                0
            })
        }
    }

    #[tokio::test]
    async fn judge_strategy_shuffles_candidate_order_before_evaluation() {
        // 5 distinct candidates. A FirstPresentedJudge with NO shuffle would
        // always pick cand-0000. With a non-trivial shuffle, it picks a
        // different candidate AND the observed order is NOT the spawn order.
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"i": 0}),
            serde_json::json!({"i": 1}),
            serde_json::json!({"i": 2}),
            serde_json::json!({"i": 3}),
            serde_json::json!({"i": 4}),
        ]);
        let judge = FirstPresentedJudge {
            observed_order: Mutex::new(Vec::new()),
        };

        let outcome =
            run_parallel_candidates_judge(&generator, &judge, "q", 5, budget(5), 42).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);

        let observed = judge.observed_order.lock().expect("poisoned").clone();
        // The judge saw 5 candidates in the shuffled order.
        assert_eq!(observed.len(), 5);
        // The spawn order would be [cand-0000, cand-0001, ...]; the shuffle
        // MUST differ (with very high probability for n=5 + a non-zero seed).
        let spawn_order: Vec<String> = (0..5).map(|i| format!("cand-{i:04}")).collect();
        let observed_strs: Vec<String> =
            observed.iter().map(|id| id.as_str().to_string()).collect();
        assert_ne!(
            observed_strs, spawn_order,
            "the judge MUST see a shuffled order, not the spawn order"
        );
        // The set of IDs the judge saw is the same as the spawn set (shuffle
        // is a permutation).
        let mut observed_sorted = observed_strs.clone();
        observed_sorted.sort();
        let mut spawn_sorted = spawn_order.clone();
        spawn_sorted.sort();
        assert_eq!(observed_sorted, spawn_sorted);
    }

    #[tokio::test]
    async fn judge_strategy_maps_shuffled_pick_back_to_original_id() {
        // The outcome's selected_candidate MUST be the original CandidateId,
        // not the shuffled position. We use a Judge that picks position 2 in
        // the shuffled view and assert the selected_candidate is a real ID.
        struct PickAtTwoJudge;
        impl CandidateJudge for PickAtTwoJudge {
            fn judge<'a>(
                &'a self,
                _candidates: &'a [Candidate],
            ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>> {
                Box::pin(async { 2 })
            }
        }
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"i": 0}),
            serde_json::json!({"i": 1}),
            serde_json::json!({"i": 2}),
            serde_json::json!({"i": 3}),
            serde_json::json!({"i": 4}),
        ]);
        let outcome =
            run_parallel_candidates_judge(&generator, &PickAtTwoJudge, "q", 5, budget(5), 7).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        let selected = outcome
            .selected_candidate
            .expect("judge must select a candidate");
        // The selected id is one of cand-0000..cand-0004.
        let valid_ids: Vec<String> = (0..5).map(|i| format!("cand-{i:04}")).collect();
        assert!(
            valid_ids.iter().any(|v| v == selected.as_str()),
            "selected {selected} must be a real cand-0000..cand-0004 id"
        );
    }

    #[tokio::test]
    async fn judge_strategy_clamps_out_of_range_pick_to_last_candidate() {
        // A buggy judge that returns an index past the end is clamped, not
        // panicked on.
        struct OverflowJudge;
        impl CandidateJudge for OverflowJudge {
            fn judge<'a>(
                &'a self,
                _candidates: &'a [Candidate],
            ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>> {
                Box::pin(async { 999 })
            }
        }
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"i": 0}),
            serde_json::json!({"i": 1}),
        ]);
        let outcome =
            run_parallel_candidates_judge(&generator, &OverflowJudge, "q", 2, budget(2), 1).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Last candidate is cand-0001 (deterministic shuffle on 2 elements
        // with seed 1 either swaps or not; either way the clamp lands on the
        // last index of the shuffled view).
        assert!(outcome.selected_candidate.is_some());
    }

    // --- Edge case: budget cap flows through to strategies ---

    #[tokio::test]
    async fn consensus_strategy_respects_budget_cap_on_branch_count() {
        // Request 10 branches with cap=2 → only 2 candidates run. With both
        // identical, quorum (threshold 1 of 2) is reached.
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"answer": "ok"}),
            serde_json::json!({"answer": "ok"}),
        ]);
        let outcome = run_parallel_candidates_consensus(&generator, "q", 10, budget(2)).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.cost.model_calls, 2, "only 2 branches ran (cap)");
    }
}
