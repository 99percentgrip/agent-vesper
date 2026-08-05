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

use vesper_domain::{
    InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningOutcome, StructuredOutput,
    VerificationFinding, VerificationResult, VerificationStatus, VerificationSummary, VerifierId,
};

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
pub trait CandidateGenerator: Send + Sync {
    /// Produces a candidate for `prompt`, optionally informed by prior
    /// verifier `corrections`.
    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        corrections: &'a [VerificationFinding],
    ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>>;
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
/// See the [module docs](self) for the full halt-condition table.
pub async fn run_generate_verify_repair(
    prompt: &str,
    verifier_ids: &[VerifierId],
    registry: &VerifierRegistry,
    generator: &dyn CandidateGenerator,
    workspace_root: &Path,
    budget: ReasoningBudget,
) -> ReasoningOutcome {
    let mut remaining_repairs = u32::from(budget.max_repairs);
    let mut corrections: Vec<VerificationFinding> = Vec::new();
    let mut cost = InferenceCost::default();
    let mut last_output: Option<StructuredOutput>;
    let mut attempts = 0u32;

    loop {
        // --- Generate ---
        attempts += 1;
        let candidate = generator.generate(prompt, &corrections).await;
        cost.model_calls += 1;
        cost.total_tokens = cost
            .total_tokens
            .saturating_add(candidate.cost.total_tokens);
        last_output = Some(candidate.output.clone());

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
        // Consume one repair unit and feed every failed verifier's findings
        // back to the generator as corrections (PRD §10.9: exact failure
        // evidence).
        remaining_repairs -= 1;
        for result in &results {
            if result.status != VerificationStatus::Passed {
                corrections.extend(result.findings.iter().cloned());
            }
        }
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
        let corrections_seen = generator.corrections_seen();
        assert_eq!(corrections_seen.len(), 2);
        assert!(
            corrections_seen[0].is_empty(),
            "first call has no prior failure"
        );
        assert_eq!(
            corrections_seen[1].len(),
            1,
            "second call receives the failed verifier's finding as a correction"
        );
        assert_eq!(
            corrections_seen[1][0].message, "missing semicolon",
            "the exact failure evidence is fed back (PRD §10.9)"
        );
        assert_eq!(outcome.cost.model_calls, 2);
    }

    // --- Directive test 3: max_repairs exhausted -> Failed ---

    #[tokio::test]
    async fn loop_halts_failed_when_max_repairs_exhausted() {
        // Always-fails repairable verifier. With max_repairs=2 the loop runs:
        // generate -> fail -> repair(1) -> generate -> fail -> repair(2) ->
        // generate -> fail -> exhausted -> Failed. That is 3 generations.
        let registry = registry_with(vec![Box::new(FakeVerifier::new(
            VID,
            vec![failed_repairable(VID, "still broken")],
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
}
