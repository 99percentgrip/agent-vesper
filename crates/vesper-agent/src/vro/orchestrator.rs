//! Generate-Verify-Repair execution loop (VRO-2.3, PRD §11.3 + §10.9).
//!
//! This module wires the [`TaskProfiler`](super::TaskProfiler) output and the
//! [`VerifierRegistry`](super::VerifierRegistry) into the bounded repair loop
//! that implements the [`GenerateVerifyRepair`](vesper_domain::ReasoningStrategy)
//! strategy. The orchestrator drives the loop; the composition boundary supplies
//! a real provider-backed [`CandidateGenerator`]. The loop performs **no** direct
//! provider calls and never touches [`crate::AgentLoop`]/`agent_loop.rs` — when
//! routing resolves to [`Direct`](vesper_domain::ReasoningStrategy::Direct) the
//! host keeps using the unchanged direct execution path.
//!
//! ## Loop (PRD §11.3)
//!
//! ```text
//! generate -> verify(all mandatory verifiers)
//!   -> all pass        -> Succeeded
//!   -> any Error       -> Inconclusive (could not verify)
//!   -> repairable fail -> append findings as corrections -> re-generate
//!   -> non-repairable  -> Failed
//! ```
//!
//! ## Halt conditions (directive)
//!
//! - All mandatory verifiers pass ⇒ [`OutcomeStatus::Succeeded`].
//! - Any verifier returns [`VerificationStatus::Error`] ⇒
//!   [`OutcomeStatus::Inconclusive`] (PRD §10.8: a verifier that cannot run is
//!   distinct from one that found problems).
//! - `max_repairs` exhausted while still failing ⇒ [`OutcomeStatus::Failed`].
//! - `max_model_calls` exhausted (safety bound, PRD §10.9: "never enter an
//!   unrestricted review loop") ⇒ [`OutcomeStatus::BudgetExceeded`].
//!
//! The richer §10.9 Repair-Controller heuristics (repair the *smallest* scope,
//! avoid repeating an *identical* failed attempt, escalate strategy) are
//! intentionally deferred: VRO-2.3 ships the bounded core loop per the
//! directive; the dedicated Repair Controller is its own later component.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use vesper_domain::{
    InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningOutcome, StructuredOutput,
    VerificationFinding, VerificationResult, VerificationStatus, VerificationSummary, VerifierId,
};

use super::rate_limit::RateLimitTracker;
use super::repair::RepairController;
use super::verifiers::{VerificationContext, VerifierRegistry};

// ---------------------------------------------------------------------------
// Generation seam
// ---------------------------------------------------------------------------

/// One model-generated candidate awaiting verification.
#[derive(Debug, Clone)]
pub struct GeneratedCandidate {
    /// The structured output payload (placeholder `serde_json::Value`).
    pub output: StructuredOutput,
    /// Cost consumed producing this candidate.
    pub cost: InferenceCost,
}

/// The provider-facing generation seam.
///
/// The orchestrator drives the Generate-Verify-Repair loop; the composition
/// boundary (TUI / ACP host) supplies a real provider-backed implementation.
/// `corrections` carries the accumulated verifier findings from prior failed
/// attempts so the generator can produce a *targeted* repair (PRD §10.9:
/// "include exact failure evidence"). The trait is async + object-safe via a
/// boxed `Send` future (the workspace has no `async_trait` dependency).
///
/// `boxed_clone` is required because VRO-4's parallel executor (PRD §10.6)
/// spawns each branch on its own `tokio::task::spawn`, which needs an owned
/// `'static` generator handle. Concrete impls that hold non-`Clone` resources
/// (sockets, etc.) should wrap them in `Arc` so `Clone` is cheap.
pub trait CandidateGenerator: Send + Sync {
    /// Produces a candidate for `prompt`, optionally informed by prior
    /// verifier `corrections`.
    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        corrections: &'a [VerificationFinding],
    ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>>;

    /// Boxes a clone of this generator. Used by the VRO-4 parallel executor
    /// to give each `tokio::task::spawn` branch an owned `'static` handle.
    fn boxed_clone(&self) -> Box<dyn CandidateGenerator>;
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

/// Runs the Generate-Verify-Repair loop (PRD §11.3, §10.9).
///
/// `verifier_ids` are the mandatory verifiers (typically the profile's
/// `available_verifiers`); **all** must pass for success. Each repair attempt
/// consumes one unit of `budget.max_repairs` and feeds the failed verifiers'
/// findings back to `generator` as corrections.
///
/// This entry point uses an untracked rate-limit tracker and a fresh
/// [`RepairController`]; behavior is byte-identical to VRO-9. Use
/// [`run_generate_verify_repair_with_rate_limit`] to wire a real
/// [`RateLimitTracker`] (VRO-10 §10.4) and a custom [`RepairController`]
/// (VRO-10 §10.9).
///
/// See the [module docs](self) for the full halt-condition table.
pub async fn run_generate_verify_repair(
    prompt: &str,
    verifier_ids: &[VerifierId],
    registry: &VerifierRegistry,
    generator: &dyn CandidateGenerator,
    workspace_root: &Path,
    budget: ReasoningBudget,
) -> ReasoningOutcome {
    run_generate_verify_repair_with_rate_limit(
        prompt,
        verifier_ids,
        registry,
        generator,
        workspace_root,
        budget,
        Arc::new(RateLimitTracker::untracked()),
        RepairController::new(),
    )
    .await
}

/// VRO-10 entry point: like [`run_generate_verify_repair`] but accepts a
/// shared [`RateLimitTracker`] (PRD §10.4) and a caller-owned
/// [`RepairController`] (PRD §10.9).
///
/// The composition boundary typically constructs one `RateLimitTracker` per
/// session, shares it via `Arc` between the provider adapter (which calls
/// [`RateLimitTracker::record_429`] on HTTP 429) and the orchestrator (which
/// consults [`RateLimitTracker::status`] before every Generate). When the
/// tracker is blocked, the loop halts immediately with
/// [`OutcomeStatus::RateLimitExceeded`] and a risk note carrying the
/// remaining backoff window.
///
/// The [`RepairController`] augments the corrections fed to the next
/// Generate with class-specific hints (JSON syntax / file-not-found / schema
/// mismatch / compilation / test / constraint) and detects repeated
/// identical attempts (PRD §10.9: "Avoid repeating an identical failed
/// attempt").
#[allow(clippy::too_many_arguments)]
pub async fn run_generate_verify_repair_with_rate_limit(
    prompt: &str,
    verifier_ids: &[VerifierId],
    registry: &VerifierRegistry,
    generator: &dyn CandidateGenerator,
    workspace_root: &Path,
    budget: ReasoningBudget,
    rate_limit: Arc<RateLimitTracker>,
    mut repair: RepairController,
) -> ReasoningOutcome {
    let mut remaining_repairs = u32::from(budget.max_repairs);
    let mut corrections: Vec<VerificationFinding> = Vec::new();
    let mut cost = InferenceCost::default();
    let mut last_output: Option<StructuredOutput> = None;
    let mut attempts = 0u32;
    // PRD §10.4 ("Budget Manager") — wall-clock enforcement. Captured at
    // loop entry so the FIRST iteration's elapsed time is the baseline; the
    // check before every Generate ensures we never start a fresh model call
    // past the soft ceiling.
    let started_at = std::time::Instant::now();

    loop {
        // --- Halt (pre-Generate): provider rate-limit exhausted (VRO-10,
        // PRD §10.4: "account for provider rate limits"). The composition
        // boundary's provider adapter called `rate_limit.record_429(...)`
        // when an HTTP 429 was observed; we now halt with a specific
        // RateLimitExceeded outcome instead of crashing on the next call.
        let status = rate_limit.status();
        if status.is_blocked() {
            let retry_note = match status.retry_after_ms() {
                Some(ms) => format!("provider rate-limited (HTTP 429): retry after {ms} ms"),
                None => "provider rate-limited (HTTP 429): no Retry-After hint; \
                         clear the tracker to retry"
                    .to_string(),
            };
            return build_outcome(
                OutcomeStatus::RateLimitExceeded,
                last_output.clone(),
                &[],
                cost,
                &[retry_note],
            );
        }

        // --- Halt (pre-Generate): wall-clock budget exhausted (PRD §10.4) ---
        // Checked BEFORE each Generate so a long-running repair cannot silently
        // exceed the user-facing latency ceiling.
        let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        if elapsed_ms >= budget.max_wall_time_ms {
            return build_outcome(
                OutcomeStatus::BudgetExceeded,
                last_output.clone(),
                &[],
                cost,
                &["max_wall_time_ms exhausted before next generate".to_string()],
            );
        }

        // --- Generate ---
        attempts += 1;
        let candidate = generator.generate(prompt, &corrections).await;
        cost.model_calls += 1;
        cost.total_tokens = cost
            .total_tokens
            .saturating_add(candidate.cost.total_tokens);
        last_output = Some(candidate.output.clone());

        // --- Halt (post-Generate): total-token budget exhausted (PRD §10.4) ---
        // PRD §10.4: "Prevent retries from silently exceeding user limits."
        // Triggered after the cumulative token count crosses the configured
        // ceiling so a runaway repair loop cannot quietly overspend.
        if cost.total_tokens >= budget.max_total_output_tokens {
            return build_outcome(
                OutcomeStatus::BudgetExceeded,
                last_output,
                &[],
                cost,
                &["max_total_output_tokens exhausted before convergence".to_string()],
            );
        }

        // --- Verify all mandatory verifiers ---
        let ctx = VerificationContext::new(workspace_root.to_path_buf());
        let mut results: Vec<VerificationResult> = Vec::with_capacity(verifier_ids.len());
        let mut all_passed = true;
        let mut had_error = false;
        for vid in verifier_ids {
            match registry.run(vid.as_str(), &ctx).await {
                Some(result) => {
                    match result.status {
                        VerificationStatus::Passed => {}
                        VerificationStatus::Error => {
                            had_error = true;
                            all_passed = false;
                        }
                        VerificationStatus::Failed
                        | VerificationStatus::Skipped
                        | VerificationStatus::Inconclusive => {
                            all_passed = false;
                        }
                    }
                    results.push(result);
                }
                None => {
                    // A mandatory verifier is not registered — cannot verify.
                    had_error = true;
                    all_passed = false;
                    results.push(unregistered_verifier_result(vid));
                }
            }
        }

        // --- Halt: all mandatory verifiers pass ---
        if all_passed {
            return build_outcome(OutcomeStatus::Succeeded, last_output, &results, cost, &[]);
        }
        // --- Halt: a verifier could not run ---
        if had_error {
            return build_outcome(
                OutcomeStatus::Inconclusive,
                last_output,
                &results,
                cost,
                &["a verifier could not run (VerificationStatus::Error)".to_string()],
            );
        }
        // --- Halt: model-call safety budget exhausted (PRD §10.9) ---
        if attempts >= budget.max_model_calls {
            return build_outcome(
                OutcomeStatus::BudgetExceeded,
                last_output,
                &results,
                cost,
                &["max_model_calls exhausted before convergence".to_string()],
            );
        }

        // --- Repair? ---
        let any_repairable = results
            .iter()
            .any(|r| r.status == VerificationStatus::Failed && r.repairable);
        if !any_repairable {
            return build_outcome(
                OutcomeStatus::Failed,
                last_output,
                &results,
                cost,
                &["non-repairable verifier failure".to_string()],
            );
        }
        if remaining_repairs == 0 {
            return build_outcome(
                OutcomeStatus::Failed,
                last_output,
                &results,
                cost,
                &["max_repairs exhausted while verifiers still failing".to_string()],
            );
        }

        // --- Collect this attempt's failed findings (PRD §10.9: include
        // exact failure evidence) ---
        let attempt_findings: Vec<VerificationFinding> = results
            .iter()
            .filter(|r| r.status != VerificationStatus::Passed)
            .flat_map(|r| r.findings.iter().cloned())
            .collect();

        // --- VRO-10 §10.9: repeated-attempt guard. If the failing
        // findings are byte-identical to the previous attempt's, escalate
        // to Failed with a clear risk note rather than re-issuing the same
        // prompt (PRD §10.9: "Avoid repeating an identical failed attempt").
        if repair.is_repeated_attempt(&attempt_findings) && attempts >= 2 {
            return build_outcome(
                OutcomeStatus::Failed,
                last_output,
                &results,
                cost,
                &["repeated identical verifier failure: escalating to avoid \
                     an unbounded review loop (PRD §10.9)"
                    .to_string()],
            );
        }

        // Consume one repair unit and feed every failed verifier's findings
        // back to the generator as corrections (PRD §10.9: exact failure
        // evidence).
        remaining_repairs -= 1;
        corrections.extend(attempt_findings.iter().cloned());

        // --- VRO-10 §10.9: Repair Controller heuristics. For each failed
        // finding, classify it (JSON parse / file-not-found / schema / etc.)
        // and append a class-specific correction hint to the corrections
        // vector. Generic findings inject no hint, preserving VRO-9 behavior
        // for unclassifiable failures.
        let _classes = repair.augment_corrections(&mut corrections, &attempt_findings);

        // Loop back to Generate.
    }
}

// ---------------------------------------------------------------------------
// Outcome builders
// ---------------------------------------------------------------------------

fn build_outcome(
    status: OutcomeStatus,
    output: Option<StructuredOutput>,
    results: &[VerificationResult],
    cost: InferenceCost,
    unresolved_risks: &[String],
) -> ReasoningOutcome {
    let passed = results
        .iter()
        .filter(|r| r.status == VerificationStatus::Passed)
        .count() as u32;
    let failed = results
        .iter()
        .filter(|r| r.status == VerificationStatus::Failed)
        .count() as u32;
    let overall = overall_status(results);
    ReasoningOutcome {
        status,
        final_output: output,
        selected_candidate: None,
        verification_summary: VerificationSummary {
            passed,
            failed,
            overall,
        },
        unresolved_risks: unresolved_risks.to_vec(),
        cost,
    }
}

/// Rolls a slice of verifier results into one overall status: all-pass ⇒
/// `Passed`; any `Error` ⇒ `Error`; otherwise `Failed`. An empty verifier set
/// is treated as `Passed` (nothing to check).
fn overall_status(results: &[VerificationResult]) -> VerificationStatus {
    if results.is_empty() {
        return VerificationStatus::Passed;
    }
    if results
        .iter()
        .all(|r| r.status == VerificationStatus::Passed)
    {
        return VerificationStatus::Passed;
    }
    if results
        .iter()
        .any(|r| r.status == VerificationStatus::Error)
    {
        return VerificationStatus::Error;
    }
    VerificationStatus::Failed
}

fn unregistered_verifier_result(id: &VerifierId) -> VerificationResult {
    VerificationResult {
        verifier_id: id.clone(),
        status: VerificationStatus::Error,
        confidence: 0.0,
        findings: vec![VerificationFinding {
            message: format!("verifier `{}` is not registered", id),
            severity: vesper_domain::VerificationSeverity::Error,
            location: None,
        }],
        evidence_refs: vec![],
        repairable: false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vro::Verifier;
    use std::sync::Mutex;
    use vesper_domain::VerificationSeverity;

    // --- Fakes ---

    /// Generates from a programmed sequence of outputs; records how many calls
    /// were made and the corrections seen on each.
    struct FakeGenerator {
        outputs: Mutex<Vec<StructuredOutput>>,
        call_count: Mutex<u32>,
        corrections_seen: Mutex<Vec<Vec<VerificationFinding>>>,
    }

    impl FakeGenerator {
        fn new(outputs: Vec<StructuredOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs),
                call_count: Mutex::new(0),
                corrections_seen: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> u32 {
            *self.call_count.lock().expect("poisoned")
        }

        fn corrections_seen(&self) -> Vec<Vec<VerificationFinding>> {
            self.corrections_seen.lock().expect("poisoned").clone()
        }
    }

    impl CandidateGenerator for FakeGenerator {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            corrections: &'a [VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            Box::pin(async move {
                {
                    let mut count = self.call_count.lock().expect("poisoned");
                    *count += 1;
                }
                self.corrections_seen
                    .lock()
                    .expect("poisoned")
                    .push(corrections.to_vec());
                let output = {
                    let mut outputs = self.outputs.lock().expect("poisoned");
                    if outputs.len() == 1 {
                        outputs[0].clone()
                    } else {
                        outputs.remove(0)
                    }
                };
                GeneratedCandidate {
                    output,
                    cost: InferenceCost {
                        model_calls: 1,
                        total_tokens: 100,
                    },
                }
            })
        }

        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            // FakeGenerator holds `Mutex`-wrapped state, so cloning shares the
            // same underlying observations. This is fine for VRO-2.3 tests
            // which run a single generator sequentially; VRO-4's parallel
            // executor uses its own ScriptedGenerator-style fakes that DO
            // produce independent clones.
            Box::new(Self {
                outputs: Mutex::new(self.outputs.lock().expect("poisoned").clone()),
                call_count: Mutex::new(*self.call_count.lock().expect("poisoned")),
                corrections_seen: Mutex::new(
                    self.corrections_seen.lock().expect("poisoned").clone(),
                ),
            })
        }
    }

    /// Verifier that returns a programmed sequence of results. A single entry
    /// repeats forever ("always X"); multiple entries are consumed in order and
    /// then the last repeats.
    struct FakeVerifier {
        id: String,
        results: Mutex<Vec<VerificationResult>>,
    }

    impl FakeVerifier {
        fn new(id: &str, results: Vec<VerificationResult>) -> Self {
            Self {
                id: id.to_string(),
                results: Mutex::new(results),
            }
        }
    }

    impl Verifier for FakeVerifier {
        fn id(&self) -> &str {
            &self.id
        }

        fn verify<'a>(
            &'a self,
            _ctx: &'a VerificationContext,
        ) -> Pin<Box<dyn Future<Output = VerificationResult> + Send + 'a>> {
            Box::pin(async move {
                let mut results = self.results.lock().expect("poisoned");
                if results.len() == 1 {
                    results[0].clone()
                } else {
                    results.remove(0)
                }
            })
        }
    }

    fn passed(id: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Passed,
            confidence: 1.0,
            findings: vec![],
            evidence_refs: vec![],
            repairable: false,
        }
    }

    fn failed_repairable(id: &str, message: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Failed,
            confidence: 0.0,
            findings: vec![VerificationFinding {
                message: message.to_string(),
                severity: VerificationSeverity::Error,
                location: Some("src/lib.rs:1".to_string()),
            }],
            evidence_refs: vec![],
            repairable: true,
        }
    }

    fn failed_non_repairable(id: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Failed,
            confidence: 0.0,
            findings: vec![VerificationFinding {
                message: "schema mismatch".to_string(),
                severity: VerificationSeverity::Critical,
                location: None,
            }],
            evidence_refs: vec![],
            repairable: false,
        }
    }

    fn error_result(id: &str) -> VerificationResult {
        VerificationResult {
            verifier_id: VerifierId::new(id).unwrap(),
            status: VerificationStatus::Error,
            confidence: 0.0,
            findings: vec![VerificationFinding {
                message: "cargo not found".to_string(),
                severity: VerificationSeverity::Error,
                location: None,
            }],
            evidence_refs: vec![],
            repairable: false,
        }
    }

    fn registry_with(verifiers: Vec<Box<dyn Verifier>>) -> VerifierRegistry {
        let mut registry = VerifierRegistry::new();
        for v in verifiers {
            registry.register(v);
        }
        registry
    }

    fn budget(max_repairs: u16, max_model_calls: u32) -> ReasoningBudget {
        ReasoningBudget {
            max_repairs,
            max_model_calls,
            ..ReasoningBudget::balanced()
        }
    }

    const VID: &str = "cargo_check";

    // --- Directive test 1: passes on the first try ---

    #[tokio::test]
    async fn loop_halts_with_success_when_verifier_passes_first_try() {
        let registry = registry_with(vec![Box::new(FakeVerifier::new(VID, vec![passed(VID)]))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "v1"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(2, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Exactly one generation; no repair attempted.
        assert_eq!(generator.call_count(), 1);
        assert_eq!(outcome.verification_summary.passed, 1);
        assert_eq!(outcome.verification_summary.failed, 0);
        assert_eq!(
            outcome.verification_summary.overall,
            VerificationStatus::Passed
        );
        assert!(outcome.unresolved_risks.is_empty());
        assert_eq!(outcome.cost.model_calls, 1);
    }

    // --- Directive test 2: fails once, repairs, passes on the second try ---

    #[tokio::test]
    async fn loop_repairs_and_succeeds_when_verifier_fails_once_then_passes() {
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_repairable(VID, "missing semicolon"), passed(VID)],
        ))]);
        let generator = FakeGenerator::new(vec![
            serde_json::json!({"answer": "broken"}),
            serde_json::json!({"answer": "fixed"}),
        ]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(2, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Two generations: the initial attempt plus one repair.
        assert_eq!(generator.call_count(), 2);
        // The repair feedback reached the generator on the second call.
        // VRO-10 §10.9: the second call sees the raw finding PLUS a
        // class-specific correction hint (the "missing semicolon" finding
        // has location `src/lib.rs:1`, severity Error → classified as
        // CompilationError → injects a cargo-error hint). So the second
        // call sees ≥ 2 corrections.
        let corrections_seen = generator.corrections_seen();
        assert_eq!(corrections_seen.len(), 2);
        assert!(
            corrections_seen[0].is_empty(),
            "first call has no prior failure"
        );
        assert!(
            corrections_seen[1]
                .iter()
                .any(|c| c.message == "missing semicolon"),
            "second call receives the failed verifier's finding as a correction: {:?}",
            corrections_seen[1]
        );
        assert_eq!(outcome.cost.model_calls, 2);
    }

    // --- Directive test 3: max_repairs exhausted -> Failed ---

    #[tokio::test]
    async fn loop_halts_failed_when_max_repairs_exhausted() {
        // Always-fails repairable verifier with DISTINCT findings per call
        // so the VRO-10 §10.9 repetition guard does not pre-empt the
        // max_repairs halt. With max_repairs=2 the loop runs:
        //   generate -> fail (msg #1) -> repair(1)
        //   generate -> fail (msg #2) -> repair(2)
        //   generate -> fail (msg #3) -> exhausted -> Failed.
        // That is 3 generations. (If the messages were identical, the §10.9
        // repetition guard would correctly halt on iteration 2 instead —
        // that behavior is covered separately by
        // `repair_controller_escalates_on_repeated_identical_failure`.)
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![
                failed_repairable(VID, "still broken attempt 1"),
                failed_repairable(VID, "still broken attempt 2"),
                failed_repairable(VID, "still broken attempt 3"),
            ],
        ))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "x"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(2, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Failed);
        // 1 initial + 2 repairs = 3 generations, then the budget halts.
        assert_eq!(generator.call_count(), 3);
        assert_eq!(outcome.verification_summary.failed, 1);
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("max_repairs exhausted"))
        );
        assert_eq!(outcome.cost.model_calls, 3);
    }

    // --- Extra halt-condition coverage ---

    #[tokio::test]
    async fn loop_halts_inconclusive_when_verifier_errors() {
        // A verifier that cannot run halts immediately with Inconclusive
        // (distinct from Failed).
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![error_result(VID)],
        ))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "v1"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(3, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Inconclusive);
        assert_eq!(generator.call_count(), 1);
    }

    #[tokio::test]
    async fn loop_halts_failed_on_non_repairable_failure() {
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_non_repairable(VID)],
        ))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "v1"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(3, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Failed);
        assert_eq!(
            generator.call_count(),
            1,
            "non-repairable failures do not retry"
        );
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("non-repairable"))
        );
    }

    #[tokio::test]
    async fn loop_with_no_verifiers_succeeds_on_first_generate() {
        // No mandatory verifiers => nothing to check => accept the first
        // generated output. (Used as the fallback for strategies that have no
        // deterministic verifier.)
        let registry = VerifierRegistry::new();
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "v1"})]);

        let outcome = run_generate_verify_repair(
            "explain recursion",
            &[],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(0, 10),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(generator.call_count(), 1);
    }

    #[tokio::test]
    async fn loop_halts_budget_exceeded_when_model_calls_cap_hits_before_repair_converges() {
        // max_model_calls=2 but the verifier always fails: after 2 generations
        // the safety bound trips before max_repairs would.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_repairable(VID, "broken")],
        ))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "x"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            budget(10, 2),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
        assert_eq!(generator.call_count(), 2);
    }

    // ======================================================================
    // Directive 2 — strict budget enforcement (VRO-9, PRD §10.4
    // "Budget Manager")
    //
    // The GVR loop must enforce ALL THREE budget ceilings — model-call,
    // total-token, wall-clock — and trigger OutcomeStatus::BudgetExceeded on
    // breach. The existing tests above cover model-call; the two below cover
    // total-token and wall-clock.
    // ======================================================================

    /// Generator that emits a fixed large `total_tokens` per call so a tight
    /// `max_total_output_tokens` budget trips on the second iteration.
    struct HeavyTokenGenerator {
        outputs: Mutex<Vec<StructuredOutput>>,
        call_count: Mutex<u32>,
    }
    impl HeavyTokenGenerator {
        fn new(outputs: Vec<StructuredOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs),
                call_count: Mutex::new(0),
            }
        }
        fn call_count(&self) -> u32 {
            *self.call_count.lock().expect("poisoned")
        }
    }
    impl CandidateGenerator for HeavyTokenGenerator {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            Box::pin(async move {
                *self.call_count.lock().expect("poisoned") += 1;
                let output = {
                    let mut outputs = self.outputs.lock().expect("poisoned");
                    if outputs.len() == 1 {
                        outputs[0].clone()
                    } else {
                        outputs.remove(0)
                    }
                };
                GeneratedCandidate {
                    output,
                    cost: InferenceCost {
                        model_calls: 1,
                        // Each call burns 1_000 tokens. With
                        // max_total_output_tokens = 1_500, the loop trips on
                        // the SECOND iteration (cumulative 2_000 > 1_500)
                        // BEFORE max_model_calls=10 would.
                        total_tokens: 1_000,
                    },
                }
            })
        }
        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            Box::new(Self {
                outputs: Mutex::new(self.outputs.lock().expect("poisoned").clone()),
                call_count: Mutex::new(*self.call_count.lock().expect("poisoned")),
            })
        }
    }

    #[tokio::test]
    async fn loop_halts_budget_exceeded_when_max_total_output_tokens_exhausted() {
        // The verifier always fails repairable; the generator emits 1_000
        // tokens per call. With max_total_output_tokens=1_500, the loop
        // runs: generate (1_000) -> fail -> generate (2_000 > 1_500) ->
        // HALT BudgetExceeded. So exactly 2 generations, even though
        // max_model_calls=10 and max_repairs=10 would allow more.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_repairable(VID, "broken")],
        ))]);
        let generator = HeavyTokenGenerator::new(vec![serde_json::json!({"answer": "x"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget {
                max_repairs: 10,
                max_model_calls: 10,
                max_total_output_tokens: 1_500,
                // Generous wall-clock so only the token ceiling trips here.
                max_wall_time_ms: 60_000,
                ..ReasoningBudget::balanced()
            },
        )
        .await;

        assert_eq!(
            outcome.status,
            OutcomeStatus::BudgetExceeded,
            "max_total_output_tokens breach must trigger BudgetExceeded"
        );
        // Two generations: 1_000 + 1_000 = 2_000 > 1_500 ceiling.
        assert_eq!(generator.call_count(), 2);
        // The risk message identifies the breached ceiling.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("max_total_output_tokens")),
            "risk must name the breached budget: {:?}",
            outcome.unresolved_risks
        );
        // Cost reflects the overspend: cumulative tokens crossed 1_500.
        assert!(outcome.cost.total_tokens >= 1_500);
    }

    #[tokio::test]
    async fn loop_halts_budget_exceeded_when_max_wall_time_ms_exhausted() {
        // max_wall_time_ms = 0 — the strict-edge posture: ANY elapsed time
        // (even 0 ns on the first pre-Generate check) trips the ceiling. The
        // wall-clock check fires BEFORE the first Generate, returning
        // BudgetExceeded with zero model calls. The generator must never be
        // invoked.
        //
        // Note: `started_at` is captured INSIDE run_generate_verify_repair,
        // so a pre-call sleep in the test body does not advance the elapsed
        // clock — only a zero-or-negative ceiling reliably trips on the very
        // first iteration. This test deliberately exercises the strict-edge
        // posture (max_wall_time_ms = 0); production presets ship ≥ 30 s.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_repairable(VID, "broken")],
        ))]);
        let generator = HeavyTokenGenerator::new(vec![serde_json::json!({"answer": "x"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget {
                max_repairs: 10,
                max_model_calls: 10,
                max_total_output_tokens: 100_000,
                // Strict-edge wall-clock so only this ceiling trips here.
                max_wall_time_ms: 0,
                ..ReasoningBudget::balanced()
            },
        )
        .await;

        assert_eq!(
            outcome.status,
            OutcomeStatus::BudgetExceeded,
            "max_wall_time_ms breach must trigger BudgetExceeded"
        );
        // The generator was never called — the pre-Generate wall-clock
        // check tripped first.
        assert_eq!(
            generator.call_count(),
            0,
            "wall-clock halt must fire before any Generate"
        );
        // The risk message identifies the breached ceiling.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("max_wall_time_ms")),
            "risk must name the breached budget: {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn loop_with_high_token_budget_does_not_trip_on_small_workload() {
        // Sanity: a normal workload (one short successful generation) under
        // the calibrated Balanced preset (24_576 tokens / 180 s wall) must
        // NOT trip the new budget enforcement. This is the regression guard:
        // the calibrated defaults must absorb ordinary turns.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(VID, vec![passed(VID)]))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "ok"})]);

        let outcome = run_generate_verify_repair(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget::balanced(),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(generator.call_count(), 1);
        // The FakeGenerator emits 100 tokens — well under 24_576.
        assert_eq!(outcome.cost.total_tokens, 100);
    }

    // ======================================================================
    // VRO-10 — provider rate-limit accounting (§10.4) and Repair Controller
    // heuristics (§10.9). The rate-limit tracker + repair controller are
    // wired through `run_generate_verify_repair_with_rate_limit`.
    // ======================================================================

    #[tokio::test]
    async fn loop_halts_rate_limit_exceeded_when_tracker_blocks_before_first_generate() {
        // The composition boundary observed an HTTP 429 and recorded it on
        // the tracker. Before the FIRST Generate the loop consults the
        // tracker and halts immediately with RateLimitExceeded — the
        // generator is never invoked.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(VID, vec![passed(VID)]))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "ok"})]);
        let tracker = std::sync::Arc::new(super::super::rate_limit::RateLimitTracker::untracked());
        tracker.record_429(Some(60_000)); // 60s backoff

        let outcome = run_generate_verify_repair_with_rate_limit(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget::balanced(),
            std::sync::Arc::clone(&tracker),
            super::super::repair::RepairController::new(),
        )
        .await;

        assert_eq!(
            outcome.status,
            OutcomeStatus::RateLimitExceeded,
            "blocked tracker must halt with RateLimitExceeded"
        );
        assert_eq!(
            generator.call_count(),
            0,
            "generator must not be called when tracker blocks"
        );
        // The risk note names the rate-limit condition and the backoff window.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("rate-limited") && r.contains("retry after")),
            "risk must name the rate-limit condition: {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn loop_halts_rate_limit_exceeded_with_no_retry_after_hint() {
        // A 429 without Retry-After blocks indefinitely. The risk note must
        // surface the absence so the operator knows to clear the tracker.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(VID, vec![passed(VID)]))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "ok"})]);
        let tracker = std::sync::Arc::new(super::super::rate_limit::RateLimitTracker::untracked());
        tracker.record_429(None);

        let outcome = run_generate_verify_repair_with_rate_limit(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget::balanced(),
            tracker,
            super::super::repair::RepairController::new(),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::RateLimitExceeded);
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("no Retry-After") && r.contains("clear the tracker")),
            "risk must surface missing Retry-After: {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn loop_with_unblocked_tracker_behaves_identically_to_vro_9() {
        // Regression guard: the default untracked tracker must not change
        // behavior. A first-pass-success run completes with Succeeded and
        // one model call, identical to run_generate_verify_repair.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(VID, vec![passed(VID)]))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "ok"})]);

        let outcome = run_generate_verify_repair_with_rate_limit(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget::balanced(),
            std::sync::Arc::new(super::super::rate_limit::RateLimitTracker::untracked()),
            super::super::repair::RepairController::new(),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(generator.call_count(), 1);
    }

    #[tokio::test]
    async fn repair_controller_injects_class_specific_correction_hint_for_json_parse() {
        // The first verifier failure is a JSON-parse error. The Repair
        // Controller must classify it (RepairHeuristic::JsonParse) and inject
        // a JSON-syntax correction hint into the corrections vector fed to
        // the second Generate.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![
                failed_repairable(VID, "invalid JSON: unexpected token"),
                passed(VID),
            ],
        ))]);
        let generator = FakeGenerator::new(vec![
            serde_json::json!({"answer": "broken"}),
            serde_json::json!({"answer": "fixed"}),
        ]);

        let outcome = run_generate_verify_repair_with_rate_limit(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget::balanced(),
            std::sync::Arc::new(super::super::rate_limit::RateLimitTracker::untracked()),
            super::super::repair::RepairController::new(),
        )
        .await;

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Two generations: initial + one repair.
        assert_eq!(generator.call_count(), 2);
        // The corrections seen on the second Generate include the original
        // finding PLUS the injected JSON-syntax hint.
        let corrections_seen = generator.corrections_seen();
        assert_eq!(corrections_seen.len(), 2);
        let second = &corrections_seen[1];
        // At least two corrections: the raw finding + the JSON hint.
        assert!(
            second.len() >= 2,
            "second Generate must see the raw finding + the JSON-syntax hint, got {second:?}"
        );
        assert!(
            second.iter().any(|c| c.message.contains("JSON")),
            "corrections must include the JSON-syntax hint: {second:?}"
        );
    }

    #[tokio::test]
    async fn repair_controller_escalates_on_repeated_identical_failure() {
        // PRD §10.9: "Avoid repeating an identical failed attempt." When the
        // same finding repeats on two consecutive repairs, the loop must
        // halt with Failed (not loop forever burning the entire max_repairs
        // budget) and surface an escalation risk note.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_repairable(VID, "schema mismatch: missing field")],
        ))]);
        let generator = FakeGenerator::new(vec![serde_json::json!({"answer": "x"})]);

        let outcome = run_generate_verify_repair_with_rate_limit(
            "fix the bug",
            &[VerifierId::new(VID).unwrap()],
            &registry,
            &generator,
            Path::new("/tmp/workspace"),
            ReasoningBudget {
                max_repairs: 10, // generous: must trip the repetition guard FIRST
                max_model_calls: 10,
                ..ReasoningBudget::balanced()
            },
            std::sync::Arc::new(super::super::rate_limit::RateLimitTracker::untracked()),
            super::super::repair::RepairController::new(),
        )
        .await;

        assert_eq!(
            outcome.status,
            OutcomeStatus::Failed,
            "repeated identical failure must escalate to Failed, not loop"
        );
        // The risk note names the repetition escalation.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("repeated identical verifier failure")),
            "risk must surface the escalation: {:?}",
            outcome.unresolved_risks
        );
        // Two generations max (initial + first repair). The second repair's
        // identical findings trigger the guard before a third Generate.
        assert!(
            generator.call_count() <= 3,
            "repetition guard must halt before unbounded retries; got {}",
            generator.call_count()
        );
    }
}
