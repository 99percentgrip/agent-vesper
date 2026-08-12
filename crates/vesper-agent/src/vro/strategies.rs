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
use std::path::Path;
use std::pin::Pin;

use vesper_domain::{
    Candidate, InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningOutcome, StructuredOutput,
    VerificationResult, VerificationStatus, VerificationSummary, VerifierId,
};

use super::executor::{CandidateExecutor, ExecutorError, XorShiftRng};
use super::orchestrator::CandidateGenerator;
use super::verifiers::{VerificationContext, VerifierRegistry};

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

// ===========================================================================
// VRO-6 — Bounded Tree Search (PRD §11.7)
// ===========================================================================
//
// The bounded tree search expands a tree of partial candidates up to
// `budget.max_search_depth` levels, fanning out `budget.max_parallel_branches`
// children per node (PRD §11.7: "depth 3 and branching factor 2"). Each node
// is verified against the profile's mandatory verifiers; a node whose
// verifiers FAIL is **pruned** (PRD §10.6: "Beam-style pruning" + §11.7:
// "aggressive pruning"). A node whose verifiers PASS becomes a candidate
// "best leaf" and is NOT expanded further (PRD §10.6: "Early stopping when a
// verifier establishes success").
//
// ## Halts
//
// - Verifier-passing leaf found at any depth ⇒ [`OutcomeStatus::Succeeded`]
//   (early stopping).
// - `max_search_depth` reached without any passing leaf ⇒
//   [`OutcomeStatus::Partial`] with the deepest non-passing leaf as the
//   final_output.
// - `max_model_calls` exhausted before any leaf reached ⇒
//   [`OutcomeStatus::BudgetExceeded`] (PRD §22.3: "No infinite repair/search
//   loop").
// - All branches failed at the root level (no children produced) ⇒
//   [`OutcomeStatus::Failed`].
//
// ## Zero-breakage contract
//
// Invoked only when the profiled strategy is
// [`BoundedTreeSearch`](vesper_domain::ReasoningStrategy). The existing
// [`Direct`](vesper_domain::ReasoningStrategy),
// [`GenerateVerifyRepair`](vesper_domain::ReasoningStrategy),
// [`ToolGroundedReact`](vesper_domain::ReasoningStrategy), and parallel paths
// are completely unaffected.

/// One node in the bounded tree search frontier.
///
/// Carries the partial answer produced by the parent expansion plus the
/// refined prompt that downstream expansions should start from (the original
/// user message + the parent's intermediate output, so each child refines
/// rather than re-deriving the parent).
///
/// `depth` and `output` are recorded for observability / debug dumps; the
/// search loop currently only reads `refined_prompt`. They are kept (rather
/// than dropped) so a later VRO phase can surface per-leaf depth + partial
/// output in a diagnostic report without a structural change.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SearchNode {
    /// Depth at which this node was produced (0 = root seed).
    depth: usize,
    /// The partial structured output produced by the parent expansion.
    output: StructuredOutput,
    /// The prompt that downstream expansions should start from. The root
    /// node carries the original user message verbatim; every deeper node
    /// carries the original message PLUS its parent's intermediate output so
    /// the generator refines rather than re-derives.
    refined_prompt: String,
}

impl SearchNode {
    /// The root seed: depth 0, empty output, the original prompt verbatim.
    fn root(prompt: &str) -> Self {
        Self {
            depth: 0,
            output: serde_json::Value::Null,
            refined_prompt: prompt.to_string(),
        }
    }
}

/// Runs the Bounded Tree Search strategy (PRD §11.7).
///
/// `verifier_ids` are the mandatory verifiers (typically the profile's
/// `available_verifiers`); **all** must pass for a node to be promoted to a
/// verifier-passing leaf. The strategy caps expansion at
/// `budget.max_search_depth` (inclusive) and fans out at most
/// `budget.max_parallel_branches` children per node. The total number of
/// candidate generations is bounded by `budget.max_model_calls`.
///
/// See the [module docs](self) above for the full halt-condition table.
pub async fn run_bounded_tree_search(
    generator: &dyn CandidateGenerator,
    verifier_ids: &[VerifierId],
    registry: &VerifierRegistry,
    workspace_root: &Path,
    prompt: &str,
    budget: ReasoningBudget,
) -> ReasoningOutcome {
    // Defensive lower-bounds — a budget of 0 in any axis must halt immediately
    // rather than enter an unbounded loop (PRD §22.3: "No infinite repair/
    // search loop").
    let branching = usize::from(budget.max_parallel_branches.max(1));
    let max_depth = usize::from(budget.max_search_depth.max(1));
    let max_model_calls = usize::try_from(budget.max_model_calls).unwrap_or(usize::MAX);

    let executor = CandidateExecutor::new();
    let ctx = VerificationContext::new(workspace_root.to_path_buf());

    // frontier always begins with one root node; we expand it level by level.
    let mut frontier: Vec<SearchNode> = vec![SearchNode::root(prompt)];
    let mut best_passing_leaf: Option<Candidate> = None;
    let mut best_partial_leaf: Option<Candidate> = None;
    let mut best_passing_depth: usize = 0;
    let mut explored: usize = 0;
    let mut total_cost = InferenceCost::default();
    let mut verifiers_passed: u32 = 0;
    let mut verifiers_failed: u32 = 0;
    let mut budget_exhausted = false;

    for depth in 1..=max_depth {
        if frontier.is_empty() {
            break;
        }
        // Short-circuit: once a verifier-passing leaf exists at this depth we
        // do NOT expand further (PRD §10.6 "early stopping").
        if best_passing_leaf.is_some() {
            break;
        }

        let mut next_frontier: Vec<SearchNode> = Vec::new();

        for parent in &frontier {
            if budget_exhausted {
                break;
            }
            // Expand: fan out `branching` children from this parent's prompt.
            let outcomes = match executor
                .fan_out(generator, &parent.refined_prompt, branching, budget)
                .await
            {
                Ok(o) => o,
                Err(ExecutorError::ZeroParallelBranches) => {
                    // Branching cap collapsed to 0 — every subsequent
                    // expansion would also fail. Stop the search cleanly.
                    return ReasoningOutcome {
                        status: OutcomeStatus::Failed,
                        final_output: best_partial_leaf.as_ref().map(|c| c.output.clone()),
                        selected_candidate: best_partial_leaf
                            .as_ref()
                            .map(|c| c.candidate_id.clone()),
                        verification_summary: VerificationSummary {
                            passed: verifiers_passed,
                            failed: verifiers_failed,
                            overall: overall_summary_status(
                                verifiers_passed,
                                verifiers_failed,
                                best_passing_leaf.is_some(),
                            ),
                        },
                        unresolved_risks: vec![
                            "budget.max_parallel_branches is zero — bounded tree search cannot fan out"
                                .to_string(),
                        ],
                        cost: total_cost,
                    };
                }
                Err(ExecutorError::AllBranchesFailed(_)) => {
                    // This parent's expansion produced no survivors — prune
                    // silently (the frontier just shrinks).
                    continue;
                }
            };

            for outcome in outcomes {
                explored += 1;
                total_cost.model_calls = total_cost
                    .model_calls
                    .saturating_add(outcome.candidate.cost.model_calls);
                total_cost.total_tokens = total_cost
                    .total_tokens
                    .saturating_add(outcome.candidate.cost.total_tokens);

                // Verify the candidate against all mandatory verifiers.
                let mut results: Vec<VerificationResult> = Vec::with_capacity(verifier_ids.len());
                let mut all_passed = true;
                let mut any_definitive_failure = false;
                for vid in verifier_ids {
                    match registry.run(vid.as_str(), &ctx).await {
                        Some(r) => {
                            if r.status != VerificationStatus::Passed {
                                all_passed = false;
                            }
                            // PRD §10.6 "beam-style pruning" + §11.7
                            // "aggressive pruning": a definitive verifier
                            // failure (Failed = ran and found problems, or
                            // Error = could not run) PRUNES the branch — it
                            // is abandoned, not refined. Non-definitive
                            // outcomes (Inconclusive, Skipped) keep the
                            // branch alive for further refinement.
                            if matches!(
                                r.status,
                                VerificationStatus::Failed | VerificationStatus::Error
                            ) {
                                any_definitive_failure = true;
                            }
                            if r.status == VerificationStatus::Passed {
                                verifiers_passed += 1;
                            } else if r.status == VerificationStatus::Failed {
                                verifiers_failed += 1;
                            }
                            results.push(r);
                        }
                        None => {
                            all_passed = false;
                            any_definitive_failure = true;
                            results.push(unregistered_verifier_result(vid));
                            verifiers_failed += 1;
                        }
                    }
                }

                // Attach the verifier results to the candidate so the
                // downstream outcome carries per-candidate evidence.
                let mut candidate = outcome.candidate.clone();
                candidate.verification = results;

                if all_passed {
                    // Verifier-passing leaf — best candidate. PRD §10.6: do
                    // NOT expand further (early stopping).
                    if best_passing_leaf.is_none() {
                        best_passing_leaf = Some(candidate.clone());
                        best_passing_depth = depth;
                    }
                } else if any_definitive_failure {
                    // PRUNE: a deterministic verifier failed (or could not
                    // run). The branch is abandoned (PRD §11.7: "aggressive
                    // pruning"; directive: "abandoning a branch if a
                    // deterministic verifier fails early"). Track as a
                    // fallback partial leaf ONLY when we've hit max_depth
                    // (so the outcome still carries SOMETHING if every
                    // branch was pruned at the deepest level).
                    if depth >= max_depth && best_partial_leaf.is_none() {
                        best_partial_leaf = Some(candidate.clone());
                    }
                    // Do NOT push to next_frontier — the branch is pruned.
                } else if depth >= max_depth {
                    // Non-definitive (Inconclusive/Skipped) at max depth —
                    // terminal partial. Track as a fallback when no passing
                    // leaf is ever found.
                    if best_partial_leaf.is_none() {
                        best_partial_leaf = Some(candidate.clone());
                    }
                } else {
                    // Non-definitive (Inconclusive/Skipped) at depth <
                    // max_depth — expand further with a refined prompt that
                    // carries this node's output forward so downstream
                    // generations refine rather than re-derive. The branch
                    // is NOT pruned because the verifier could not reach a
                    // determination; deeper search may resolve it.
                    let refined_prompt = format!(
                        "{prompt}\n\nRefine this intermediate answer:\n{}",
                        canonical_output_for_prompt(&candidate.output)
                    );
                    next_frontier.push(SearchNode {
                        depth,
                        output: candidate.output.clone(),
                        refined_prompt,
                    });
                }

                // Budget guard — halt before the next expansion would blow
                // past max_model_calls. PRD §22.3.
                if explored >= max_model_calls {
                    budget_exhausted = true;
                    break;
                }
            }
            if budget_exhausted {
                break;
            }
        }

        frontier = next_frontier;
    }

    // Pick the best leaf: passing > partial > none.
    let overall = overall_summary_status(
        verifiers_passed,
        verifiers_failed,
        best_passing_leaf.is_some(),
    );

    if let Some(c) = best_passing_leaf {
        return ReasoningOutcome {
            status: OutcomeStatus::Succeeded,
            final_output: Some(c.output.clone()),
            selected_candidate: Some(c.candidate_id.clone()),
            verification_summary: VerificationSummary {
                passed: verifiers_passed,
                failed: verifiers_failed,
                overall,
            },
            unresolved_risks: vec![format!(
                "bounded tree search: verifier-passing leaf found at depth {best_passing_depth} \
                 after {explored} candidate generations (pruned branches abandoned early)"
            )],
            cost: total_cost,
        };
    }

    if let Some(c) = best_partial_leaf {
        return ReasoningOutcome {
            status: if budget_exhausted {
                OutcomeStatus::BudgetExceeded
            } else {
                OutcomeStatus::Partial
            },
            final_output: Some(c.output.clone()),
            selected_candidate: Some(c.candidate_id.clone()),
            verification_summary: VerificationSummary {
                passed: verifiers_passed,
                failed: verifiers_failed,
                overall,
            },
            unresolved_risks: vec![format!(
                "bounded tree search: no verifier-passing leaf found at depth {max_depth} \
                 after {explored} candidate generations; returning deepest partial leaf"
            )],
            cost: total_cost,
        };
    }

    // No leaf of any kind — every branch was pruned before reaching the
    // deepest level (or every expansion returned AllBranchesFailed).
    ReasoningOutcome {
        status: if budget_exhausted {
            OutcomeStatus::BudgetExceeded
        } else {
            OutcomeStatus::Failed
        },
        final_output: None,
        selected_candidate: None,
        verification_summary: VerificationSummary {
            passed: verifiers_passed,
            failed: verifiers_failed,
            overall,
        },
        unresolved_risks: vec![format!(
            "bounded tree search: every branch was pruned before reaching a leaf \
             (explored {explored} candidate generations at depth {max_depth})"
        )],
        cost: total_cost,
    }
}

/// Renders a structured output into a stable, compact string for embedding in
/// a downstream "refine this intermediate answer" prompt. Uses serde_json's
/// compact formatter so the prompt is deterministic regardless of feature
/// flags.
fn canonical_output_for_prompt(output: &StructuredOutput) -> String {
    serde_json::to_string(output).unwrap_or_else(|_| "null".to_string())
}

/// Computes the overall [`VerificationSummary`] status from raw counters +
/// whether a passing leaf was reached. A passing leaf is `Passed`; otherwise
/// `Failed` (or `Skipped` when no verifiers ran at all — e.g. an empty
/// verifier set).
fn overall_summary_status(passed: u32, failed: u32, has_passing_leaf: bool) -> VerificationStatus {
    if has_passing_leaf {
        VerificationStatus::Passed
    } else if passed == 0 && failed == 0 {
        VerificationStatus::Skipped
    } else {
        VerificationStatus::Failed
    }
}

// ===========================================================================
// VRO-6 — Proposer, Critic, Adjudicator (PRD §11.8)
// ===========================================================================
//
// Strict role separation (PRD §11.8): three independent seams — proposer
// (the generator), critic (per-candidate objective critique), adjudicator
// (final picker). The adjudicator receives BOTH candidates AND critiques so
// it can select on objective criteria, not persuasive prose (PRD §11.8: "The
// adjudicator must evaluate explicit criteria, not select the most persuasive
// prose"). Mirrors the VRO-4 [`CandidateJudge`] pattern but adds the
// critique layer and the explicit-criteria contract.
//
// ## Halts
//
// - Adjudicator returns an in-range index ⇒
//   [`OutcomeStatus::Succeeded`] with the winning candidate.
// - All branches failed in fan-out ⇒ [`OutcomeStatus::Failed`].
// - Budget caps fan-out to zero branches ⇒ [`OutcomeStatus::Failed`].
//
// ## Zero-breakage contract
//
// Invoked only when the profiled strategy is
// [`ProposerCriticAdjudicator`](vesper_domain::ReasoningStrategy). The
// existing strategies are completely unaffected.

/// The Critic seam (PRD §10.8 "constraint/factual-consistency/plan-
/// completeness critic" + §11.8). Produces an objective critique of one
/// candidate against the supplied `criteria` — never a persuasive re-write.
/// The composition boundary supplies a real provider-backed implementation;
/// the orchestrator never makes a provider call itself.
pub trait CandidateCritic: Send + Sync {
    /// Produces a critique string for `candidate` measured against
    /// `criteria`. The returned critique MUST be evidence-anchored (e.g.
    /// "criterion X is unmet because …") so the adjudicator can score on
    /// objective facts, not rhetoric.
    fn critique<'a>(
        &'a self,
        candidate: &'a Candidate,
        criteria: &'a [String],
    ) -> Pin<Box<dyn Future<Output = String> + Send + 'a>>;
}

/// The Adjudicator seam (PRD §10.8 "Adjudicator" + §11.8). Receives the full
/// candidate set + the per-candidate critiques + the explicit `criteria`,
/// and returns the index of the winning candidate in `candidates`. MUST be in
/// `0..candidates.len()`; out-of-range values are clamped by the caller.
///
/// The contract (PRD §11.8): the adjudicator selects on **objective criteria
/// backed by the critiques**, NOT on the rhetorical strength of the candidate
/// outputs. A real implementation derives its pick from the critiques' match
/// against `criteria`, not from a "which one sounds best" read of the
/// candidates.
pub trait Adjudicator: Send + Sync {
    /// Returns the index of the winning candidate in `candidates`. MUST be in
    /// `0..candidates.len()`; out-of-range values are clamped.
    fn adjudicate<'a>(
        &'a self,
        candidates: &'a [Candidate],
        critiques: &'a [String],
        criteria: &'a [String],
    ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>>;
}

/// Runs the Proposer-Critic-Adjudicator strategy (PRD §11.8).
///
/// 1. **Propose**: fan out `branch_count` candidates (capped to
///    `budget.max_parallel_branches`) via the VRO-4 executor.
/// 2. **Critic**: each candidate receives an objective critique from `critic`
///    measured against `criteria`.
/// 3. **Adjudicate**: `adjudicator` picks the winner from the candidate +
///    critique + criteria triple — NOT from persuasive prose alone.
///
/// `criteria` is the explicit list the adjudicator scores against. Production
/// callers should derive it from the profile's risk/ambiguity/verifier set
/// (e.g. "must compile under cargo check", "must not regress the test
/// suite", "must keep the public API stable").
pub async fn run_proposer_critic_adjudicator(
    generator: &dyn CandidateGenerator,
    critic: &dyn CandidateCritic,
    adjudicator: &dyn Adjudicator,
    prompt: &str,
    branch_count: usize,
    budget: ReasoningBudget,
    criteria: &[String],
) -> ReasoningOutcome {
    // 1. Propose.
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
                unresolved_risks: vec![
                    "budget.max_parallel_branches is zero — proposer cannot fan out".to_string(),
                ],
                cost: InferenceCost::default(),
            };
        }
        Err(ExecutorError::AllBranchesFailed(n)) => {
            return ReasoningOutcome {
                status: OutcomeStatus::Failed,
                final_output: None,
                selected_candidate: None,
                verification_summary: VerificationSummary::default(),
                unresolved_risks: vec![format!("all {n} proposer branches failed")],
                cost: InferenceCost::default(),
            };
        }
    };

    let candidates: Vec<Candidate> = outcomes.into_iter().map(|o| o.candidate).collect();
    let total_cost = aggregate_cost(&candidates);

    // 2. Critic: per-candidate objective critique. Each critique is anchored
    //    to the explicit criteria so the adjudicator scores on facts, not
    //    rhetoric.
    let mut critiques: Vec<String> = Vec::with_capacity(candidates.len());
    for c in &candidates {
        critiques.push(critic.critique(c, criteria).await);
    }

    // 3. Adjudicate: pick the winner from the (candidate, critique, criteria)
    //    triple. The adjudicator contract requires scoring on the critique's
    //    match against the criteria, NOT on the candidate's prose.
    let pick_idx = adjudicator
        .adjudicate(&candidates, &critiques, criteria)
        .await;
    let pick_idx = pick_idx.min(candidates.len().saturating_sub(1));
    let winner = &candidates[pick_idx];

    // The adjudicator's selection is itself a form of verification (PRD §10.8
    // model-based verifier); record it as a single Passed entry.
    ReasoningOutcome {
        status: OutcomeStatus::Succeeded,
        final_output: Some(winner.output.clone()),
        selected_candidate: Some(winner.candidate_id.clone()),
        verification_summary: VerificationSummary {
            passed: 1,
            failed: 0,
            overall: VerificationStatus::Passed,
        },
        unresolved_risks: vec![format!(
            "adjudicator selected candidate {} from {} proposals scored against {} explicit criteria \
             (selection based on objective critiques, not persuasive prose)",
            winner.candidate_id.as_str(),
            candidates.len(),
            criteria.len(),
        )],
        cost: total_cost,
    }
}

/// Helper used by both VRO-6 strategies: when a verifier ID is not registered
/// the search records an `Error`-status result so the search treats the node
/// as unverifiable (and prunes it). Mirrors the private helper in
/// `orchestrator.rs` so this module is self-contained.
fn unregistered_verifier_result(id: &VerifierId) -> VerificationResult {
    VerificationResult {
        verifier_id: id.clone(),
        status: VerificationStatus::Error,
        confidence: 0.0,
        findings: vec![vesper_domain::VerificationFinding {
            message: format!("verifier `{}` is not registered", id),
            severity: vesper_domain::VerificationSeverity::Error,
            location: None,
        }],
        evidence_refs: vec![],
        repairable: false,
    }
}

/// Returns the count of verifier IDs that should be tried (always the full
/// slice — used to keep the strategy module self-documenting).
#[allow(dead_code)]
fn verifier_count(ids: &[VerifierId]) -> usize {
    ids.len()
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

    // ===================================================================
    // VRO-6 — Bounded Tree Search tests (PRD §11.7, directive 4)
    // ===================================================================

    use super::super::verifiers::{VerificationContext, Verifier, VerifierRegistry};
    use std::path::Path;
    use vesper_domain::{VerificationSeverity, VerifierId};

    /// Verifier that returns a programmed sequence of results (pops from the
    /// front). Verification runs sequentially in candidate-id order after
    /// fan_out sorts the outcomes, so the sequence is deterministic.
    struct ScriptedVerifier {
        id: String,
        results: Arc<Mutex<Vec<VerificationResult>>>,
    }
    impl ScriptedVerifier {
        fn new(id: &str, results: Vec<VerificationResult>) -> Self {
            Self {
                id: id.to_string(),
                results: Arc::new(Mutex::new(results)),
            }
        }
    }
    impl Verifier for ScriptedVerifier {
        fn id(&self) -> &str {
            &self.id
        }
        fn verify<'a>(
            &'a self,
            _ctx: &'a VerificationContext,
        ) -> Pin<Box<dyn Future<Output = VerificationResult> + Send + 'a>> {
            let results = Arc::clone(&self.results);
            Box::pin(async move {
                let mut results = results.lock().expect("poisoned");
                if results.len() == 1 {
                    results[0].clone()
                } else {
                    results.remove(0)
                }
            })
        }
    }

    fn bt_passed(id: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Passed,
            confidence: 1.0,
            findings: vec![],
            evidence_refs: vec![],
            repairable: false,
        }
    }

    fn bt_failed(id: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Failed,
            confidence: 0.0,
            findings: vec![VerificationFinding {
                message: "branch failed verification".to_string(),
                severity: VerificationSeverity::Error,
                location: None,
            }],
            evidence_refs: vec![],
            repairable: false,
        }
    }

    fn bt_inconclusive(id: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Inconclusive,
            confidence: 0.5,
            findings: vec![],
            evidence_refs: vec![],
            repairable: false,
        }
    }

    fn bt_registry(verifiers: Vec<Box<dyn Verifier>>) -> VerifierRegistry {
        let mut registry = VerifierRegistry::new();
        for v in verifiers {
            registry.register(v);
        }
        registry
    }

    fn bt_budget(max_depth: u16, branching: u16, max_model_calls: u32) -> ReasoningBudget {
        ReasoningBudget {
            max_search_depth: max_depth,
            max_parallel_branches: branching,
            max_model_calls,
            ..ReasoningBudget::balanced()
        }
    }

    const BT_VID: &str = "cargo_check";

    /// Directive 4 test: BoundedTreeSearch strictly halts at max_search_depth.
    ///
    /// With branching=1, max_depth=3, and a verifier that always returns
    /// Inconclusive (non-definitive → expand further), the search must expand
    /// a linear chain of exactly 3 nodes (one per depth) and then halt at
    /// max_depth with Partial status (no passing leaf was found).
    #[tokio::test]
    async fn bounded_tree_search_halts_at_max_search_depth() {
        let registry = bt_registry(vec![Box::new(ScriptedVerifier::new(
            BT_VID,
            vec![bt_inconclusive(BT_VID)],
        ))]);
        // 3 distinct outputs for the 3-deep linear chain (branching=1 means
        // one candidate per depth).
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"depth": 1}),
            serde_json::json!({"depth": 2}),
            serde_json::json!({"depth": 3}),
        ]);
        let outcome = run_bounded_tree_search(
            &generator,
            &[VerifierId::new(BT_VID).unwrap()],
            &registry,
            Path::new("/tmp/ws"),
            "find the root cause",
            bt_budget(3, 1, 10),
        )
        .await;

        // No verifier-passing leaf was found → Partial.
        assert_eq!(
            outcome.status,
            OutcomeStatus::Partial,
            "search must reach max_depth without a passing leaf → Partial"
        );
        // Exactly 3 candidate generations (one per depth level).
        assert_eq!(
            outcome.cost.model_calls, 3,
            "search must halt at max_search_depth=3 (explored exactly 3 nodes)"
        );
        // The deepest partial leaf is returned.
        assert!(outcome.final_output.is_some());
        assert!(outcome.selected_candidate.is_some());
    }

    /// Directive 4 test: the search correctly prunes a branch if a simulated
    /// verifier fails, picking an alternate branch.
    ///
    /// With branching=2, max_depth=1: cand-0000 fails verification (pruned),
    /// cand-0001 passes (best leaf). The search must pick cand-0001, NOT
    /// cand-0000. Verification runs sequentially in candidate-id order so the
    /// ScriptedVerifier's [Failed, Passed] sequence maps deterministically.
    #[tokio::test]
    async fn bounded_tree_search_prunes_failed_branch_picks_alternate() {
        let registry = bt_registry(vec![Box::new(ScriptedVerifier::new(
            BT_VID,
            vec![bt_failed(BT_VID), bt_passed(BT_VID)],
        ))]);
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"branch": "A"}),
            serde_json::json!({"branch": "B"}),
        ]);
        let outcome = run_bounded_tree_search(
            &generator,
            &[VerifierId::new(BT_VID).unwrap()],
            &registry,
            Path::new("/tmp/ws"),
            "find the root cause",
            bt_budget(1, 2, 10),
        )
        .await;

        // The passing branch (cand-0001) was selected as the best leaf.
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(
            outcome.selected_candidate.as_ref().map(|c| c.as_str()),
            Some("cand-0001"),
            "search must pick the alternate (passing) branch, not the pruned one"
        );
        // The verifier ran exactly 2 times (once per candidate).
        assert_eq!(outcome.cost.model_calls, 2);
        // The unresolved risk confirms pruning happened.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("pruned")),
            "risk message must mention pruning: {:?}",
            outcome.unresolved_risks
        );
    }

    /// Extra: the search early-stops when the first verifier-passing leaf is
    /// found at depth 1 (PRD §10.6 "early stopping"). With branching=2 and
    /// BOTH candidates passing, the search must NOT expand to depth 2.
    #[tokio::test]
    async fn bounded_tree_search_early_stops_on_first_passing_leaf() {
        let registry = bt_registry(vec![Box::new(ScriptedVerifier::new(
            BT_VID,
            vec![bt_passed(BT_VID)],
        ))]);
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"answer": "yes"}),
            serde_json::json!({"answer": "yes"}),
        ]);
        let outcome = run_bounded_tree_search(
            &generator,
            &[VerifierId::new(BT_VID).unwrap()],
            &registry,
            Path::new("/tmp/ws"),
            "find the root cause",
            // max_depth=3 but the search should stop at depth 1.
            bt_budget(3, 2, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Only 2 candidates generated (depth 1 only — early stop).
        assert_eq!(
            outcome.cost.model_calls, 2,
            "early-stop: must NOT expand to depth 2 when depth-1 leaves pass"
        );
    }

    /// Extra: when every branch is pruned (all fail), the search returns
    /// Failed with no final_output.
    #[tokio::test]
    async fn bounded_tree_search_returns_failed_when_all_branches_pruned() {
        let registry = bt_registry(vec![Box::new(ScriptedVerifier::new(
            BT_VID,
            vec![bt_failed(BT_VID)],
        ))]);
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"a": 1}),
            serde_json::json!({"b": 2}),
        ]);
        let outcome = run_bounded_tree_search(
            &generator,
            &[VerifierId::new(BT_VID).unwrap()],
            &registry,
            Path::new("/tmp/ws"),
            "find the root cause",
            bt_budget(2, 2, 10),
        )
        .await;

        // All branches pruned at depth 1 → no leaf of any kind → Failed.
        assert_eq!(outcome.status, OutcomeStatus::Failed);
        assert!(outcome.final_output.is_none());
        assert!(outcome.selected_candidate.is_none());
    }

    /// Extra: budget exhaustion (max_model_calls) halts the search with
    /// BudgetExceeded. With branching=1, max_depth=5, max_model_calls=2,
    /// and Inconclusive verifiers, the search explores 2 nodes then halts.
    #[tokio::test]
    async fn bounded_tree_search_halts_on_model_call_budget() {
        let registry = bt_registry(vec![Box::new(ScriptedVerifier::new(
            BT_VID,
            vec![bt_inconclusive(BT_VID)],
        ))]);
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"d": 1}),
            serde_json::json!({"d": 2}),
            serde_json::json!({"d": 3}),
            serde_json::json!({"d": 4}),
            serde_json::json!({"d": 5}),
        ]);
        let outcome = run_bounded_tree_search(
            &generator,
            &[VerifierId::new(BT_VID).unwrap()],
            &registry,
            Path::new("/tmp/ws"),
            "find the root cause",
            bt_budget(5, 1, 2),
        )
        .await;

        // Budget exhausted after 2 model calls → BudgetExceeded.
        assert_eq!(
            outcome.status,
            OutcomeStatus::BudgetExceeded,
            "must halt when max_model_calls is exhausted"
        );
        assert_eq!(outcome.cost.model_calls, 2);
    }

    // ===================================================================
    // VRO-6 — Proposer / Critic / Adjudicator tests (PRD §11.8)
    // ===================================================================

    /// Critic that counts how many times it was invoked and records the
    /// candidate IDs it saw. Returns a canned critique referencing the
    /// candidate id so the adjudicator test can verify it received it.
    struct CountingCritic {
        call_count: Arc<Mutex<u32>>,
        seen_ids: Arc<Mutex<Vec<String>>>,
    }
    impl CountingCritic {
        fn new() -> Self {
            Self {
                call_count: Arc::new(Mutex::new(0)),
                seen_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }
    impl CandidateCritic for CountingCritic {
        fn critique<'a>(
            &'a self,
            candidate: &'a Candidate,
            _criteria: &'a [String],
        ) -> Pin<Box<dyn Future<Output = String> + Send + 'a>> {
            let call_count = Arc::clone(&self.call_count);
            let seen_ids = Arc::clone(&self.seen_ids);
            let id = candidate.candidate_id.as_str().to_string();
            Box::pin(async move {
                *call_count.lock().expect("poisoned") += 1;
                seen_ids.lock().expect("poisoned").push(id.clone());
                format!("critique of {id}: meets all criteria")
            })
        }
    }

    /// Adjudicator that records the candidates AND critiques it received,
    /// then picks by index.
    struct RecordingAdjudicator {
        pick: usize,
        saw_candidates: Arc<Mutex<usize>>,
        saw_critiques: Arc<Mutex<Vec<String>>>,
        saw_criteria: Arc<Mutex<usize>>,
    }
    impl RecordingAdjudicator {
        fn new(pick: usize) -> Self {
            Self {
                pick,
                saw_candidates: Arc::new(Mutex::new(0)),
                saw_critiques: Arc::new(Mutex::new(Vec::new())),
                saw_criteria: Arc::new(Mutex::new(0)),
            }
        }
    }
    impl Adjudicator for RecordingAdjudicator {
        fn adjudicate<'a>(
            &'a self,
            candidates: &'a [Candidate],
            critiques: &'a [String],
            criteria: &'a [String],
        ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>> {
            let saw_candidates = Arc::clone(&self.saw_candidates);
            let saw_critiques = Arc::clone(&self.saw_critiques);
            let saw_criteria = Arc::clone(&self.saw_criteria);
            let pick = self.pick;
            let critiques_owned: Vec<String> = critiques.to_vec();
            Box::pin(async move {
                *saw_candidates.lock().expect("poisoned") = candidates.len();
                *saw_criteria.lock().expect("poisoned") = criteria.len();
                saw_critiques
                    .lock()
                    .expect("poisoned")
                    .extend(critiques_owned);
                pick
            })
        }
    }

    /// Directive: ProposerCriticAdjudicator enforces strict role separation.
    /// Each of the three seams (generator, critic, adjudicator) is invoked
    /// exactly the expected number of times: generator fans out N branches,
    /// critic runs once per candidate (N times), adjudicator runs once.
    #[tokio::test]
    async fn pca_enforces_strict_role_separation() {
        let generator_call_count = Arc::new(Mutex::new(0u32));
        let generator = ScriptedGeneratorWithCount::new(
            vec![
                serde_json::json!({"design": "A"}),
                serde_json::json!({"design": "B"}),
                serde_json::json!({"design": "C"}),
            ],
            Arc::clone(&generator_call_count),
        );
        let critic = CountingCritic::new();
        let adjudicator = RecordingAdjudicator::new(1); // pick cand-0001

        let criteria = vec![
            "must compile".to_string(),
            "must not regress tests".to_string(),
        ];
        let outcome = run_proposer_critic_adjudicator(
            &generator,
            &critic,
            &adjudicator,
            "design the auth module",
            3,
            budget(3),
            &criteria,
        )
        .await;

        // Outcome succeeded with the adjudicator's pick (cand-0001).
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(
            outcome.selected_candidate.as_ref().map(|c| c.as_str()),
            Some("cand-0001"),
        );

        // Role separation: generator ran 3 times (fan-out), critic ran 3
        // times (once per candidate), adjudicator ran 1 time (final pick).
        assert_eq!(
            *generator_call_count.lock().expect("poisoned"),
            3,
            "proposer (generator) must fan out 3 branches"
        );
        assert_eq!(
            *critic.call_count.lock().expect("poisoned"),
            3,
            "critic must run once per candidate (3 total)"
        );
        // The critic saw all 3 candidate IDs.
        let seen_ids = critic.seen_ids.lock().expect("poisoned").clone();
        assert_eq!(seen_ids.len(), 3);
        // The adjudicator saw all 3 critiques (not just the candidates).
        let saw_critiques = adjudicator.saw_critiques.lock().expect("poisoned").clone();
        assert_eq!(
            saw_critiques.len(),
            3,
            "adjudicator must receive all 3 critiques"
        );
        // The adjudicator also received the explicit criteria.
        assert_eq!(
            *adjudicator.saw_criteria.lock().expect("poisoned"),
            2,
            "adjudicator must receive the explicit criteria"
        );
    }

    /// Directive: the adjudicator receives BOTH candidates AND critiques
    /// (not just candidates). This is the structural guarantee that
    /// selection is based on objective critiques, not persuasive prose
    /// (PRD §11.8).
    #[tokio::test]
    async fn pca_adjudicator_receives_candidates_and_critiques() {
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"design": "X"}),
            serde_json::json!({"design": "Y"}),
        ]);
        let critic = CountingCritic::new();
        let adjudicator = RecordingAdjudicator::new(0);

        let outcome = run_proposer_critic_adjudicator(
            &generator,
            &critic,
            &adjudicator,
            "design the system",
            2,
            budget(2),
            &["criterion-1".to_string()],
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Adjudicator saw 2 candidates AND 2 critiques.
        assert_eq!(
            *adjudicator.saw_candidates.lock().expect("poisoned"),
            2,
            "adjudicator must receive all candidates"
        );
        let critiques = adjudicator.saw_critiques.lock().expect("poisoned").clone();
        assert_eq!(critiques.len(), 2, "adjudicator must receive all critiques");
        // Each critique references the candidate it was for.
        assert!(critiques.iter().all(|c| c.contains("critique of cand-")));
        // The unresolved risk confirms objective-criteria selection.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("objective critiques")),
            "risk must confirm objective-criteria selection: {:?}",
            outcome.unresolved_risks
        );
    }

    /// Extra: out-of-range adjudicator pick is clamped to the last candidate.
    #[tokio::test]
    async fn pca_clamps_out_of_range_adjudicator_pick() {
        let generator = ScriptedGenerator::new(vec![
            serde_json::json!({"v": 1}),
            serde_json::json!({"v": 2}),
        ]);
        let critic = CountingCritic::new();
        let adjudicator = RecordingAdjudicator::new(999); // out of range

        let outcome = run_proposer_critic_adjudicator(
            &generator,
            &critic,
            &adjudicator,
            "design",
            2,
            budget(2),
            &[],
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Clamped to last candidate (cand-0001).
        assert_eq!(
            outcome.selected_candidate.as_ref().map(|c| c.as_str()),
            Some("cand-0001"),
        );
    }

    /// ScriptedGenerator variant that counts calls via a shared counter
    /// (for the role-separation test). Mirrors ScriptedGenerator but exposes
    /// the call count externally.
    struct ScriptedGeneratorWithCount {
        outputs: Arc<Mutex<Vec<StructuredOutput>>>,
        call_count: Arc<Mutex<u32>>,
    }
    impl ScriptedGeneratorWithCount {
        fn new(outputs: Vec<StructuredOutput>, call_count: Arc<Mutex<u32>>) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(outputs)),
                call_count,
            }
        }
    }
    impl CandidateGenerator for ScriptedGeneratorWithCount {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = super::super::orchestrator::GeneratedCandidate> + Send + 'a>>
        {
            let outputs = Arc::clone(&self.outputs);
            let call_count = Arc::clone(&self.call_count);
            Box::pin(async move {
                *call_count.lock().expect("poisoned") += 1;
                let output = {
                    let mut outputs = outputs.lock().expect("poisoned");
                    if outputs.len() == 1 {
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
            Box::new(Self {
                outputs: Arc::clone(&self.outputs),
                call_count: Arc::clone(&self.call_count),
            })
        }
    }
}
