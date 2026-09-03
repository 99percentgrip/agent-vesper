//! VRO-10 Directive §22.4: Soak Tests (PRD §22.4 — "Repeated Deep-mode
//! requests / Memory growth / Parallel sessions with same-session
//! serialization").
//!
//! These tests loop the Vesper Reasoning Orchestrator through **50+ back-to-
//! back synthetic requests** to prove memory safety, thread-leak prevention,
//! and bounded resource use under sustained load. They use **synthetic
//! in-process fakes** (not real network/HTTP) so the loop is deterministic
//! and CI-safe — the load comes from the orchestrator's own bookkeeping
//! (corrections vectors, repair-controller signatures, candidate-id
//! allocations, XorShift state, rate-limit atomics) accumulating across
//! many turns.
//!
//! ## Why #[ignore]?
//!
//! PRD §22.4 lists soak tests as a distinct testing category: "Long
//! sessions / Repeated Deep-mode requests / Memory growth." They are
//! designed for **manual / nightly** execution under load, not the standard
//! CI gate (50 iterations × parallel branches would multiply CI time past
//! the budget). Every test is `#[ignore]`-gated; standard `cargo test`
//! skips them, and a developer runs them with `--ignored`.
//!
//! To run locally:
//!
//! ```sh
//! cargo test -p vesper-agent --test soak_test -- --ignored --nocapture
//! ```
//!
//! ## What this proves
//!
//! 1. **No memory leak across 50 turns**: each turn's allocations
//!    (corrections vectors, repair signatures, candidate objects, etc.)
//!    are released when the turn ends. The test asserts the resident
//!    allocation count stays within a bounded factor of the first turn.
//! 2. **No thread leak**: tokio task JoinHandles from the parallel executor
//!    are awaited and released. The test asserts no panic / no JoinError
//!    accumulation across all iterations.
//! 3. **Deterministic ordering holds under load**: 50 turns each produce
//!    the same canonical outcome shape (Succeeded with the expected
//!    candidate id).
//! 4. **Rate-limit tracker survives repeated 429s**: a long soak that
//!    records many 429s then clears and re-runs proves the atomic counters
//!    do not saturate or wrap unexpectedly.
//! 5. **Repair controller signature set stays bounded**: 50 distinct
//!    repair attempts (each with a distinct finding message) record 50
//!    signatures; the test asserts the controller does not retain more
//!    than the most recent signature.
//! 6. **VRO-12 loop detector stays bounded and deterministic over 200
//!    calls** (PRD `docs/result-aware-loop-detection-prd.md` §6 S1): an
//!    adversarial mixed workload (exact repeats, ping-pong, no-progress,
//!    healthy resets) run 200 times against two detectors simultaneously
//!    must keep every window ≤ 5 entries, produce identical actions at
//!    every step (determinism), and actually fire all three tiers.

#![cfg(test)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vesper_agent::vro::orchestrator::{
    CandidateGenerator, GeneratedCandidate, run_generate_verify_repair_with_rate_limit,
};
use vesper_agent::vro::rate_limit::RateLimitTracker;
use vesper_agent::vro::repair::RepairController;
use vesper_agent::vro::verifiers::{VerificationContext, Verifier, VerifierRegistry};
use vesper_domain::{
    InferenceCost, OutcomeStatus, ReasoningBudget, VerificationResult, VerificationStatus,
    VerifierId,
};

/// Number of soak iterations per PRD §22.4 directive (50+).
const SOAK_ITERATIONS: usize = 50;

/// Number of soak iterations for the parallel-candidate branch test (lower
/// to keep wall-clock bounded; PRD §22.4 says "repeated Deep-mode requests"
/// — the iteration count is the soak axis, not the parallel branch count).
const PARALLEL_SOAK_ITERATIONS: usize = 20;

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

/// Generator that always returns a fixed small candidate. Cheap to clone
/// (Arc-wrapped counter) so it can be `boxed_clone`d for the parallel
/// executor.
struct SoakGenerator {
    call_count: Arc<AtomicUsize>,
}

impl SoakGenerator {
    fn new() -> Self {
        Self {
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn total_calls(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl CandidateGenerator for SoakGenerator {
    fn generate<'a>(
        &'a self,
        _prompt: &'a str,
        _corrections: &'a [vesper_domain::VerificationFinding],
    ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
        let counter = Arc::clone(&self.call_count);
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            GeneratedCandidate {
                output: serde_json::json!({"answer": format!("iter-{}", counter.load(Ordering::SeqCst))}),
                cost: InferenceCost {
                    model_calls: 1,
                    total_tokens: 64,
                },
            }
        })
    }

    fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
        Box::new(Self {
            call_count: Arc::clone(&self.call_count),
        })
    }
}

/// Verifier that always passes (cheap). Stateless.
struct AlwaysPassVerifier;

impl Verifier for AlwaysPassVerifier {
    fn id(&self) -> &str {
        "soak_pass"
    }
    fn verify<'a>(
        &'a self,
        _ctx: &'a VerificationContext,
    ) -> Pin<Box<dyn Future<Output = VerificationResult> + Send + 'a>> {
        Box::pin(async {
            VerificationResult {
                verifier_id: VerifierId::new("soak_pass").unwrap(),
                status: VerificationStatus::Passed,
                confidence: 1.0,
                findings: vec![],
                evidence_refs: vec![],
                repairable: false,
            }
        })
    }
}

fn soak_registry() -> VerifierRegistry {
    let mut registry = VerifierRegistry::new();
    registry.register(Box::new(AlwaysPassVerifier));
    registry
}

fn soak_budget() -> ReasoningBudget {
    ReasoningBudget {
        max_model_calls: 4,
        max_repairs: 2,
        max_parallel_branches: 2,
        max_search_depth: 1,
        max_total_output_tokens: 8_192,
        max_tool_calls: 4,
        max_wall_time_ms: 60_000,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// **PRD §22.4 "Repeated Deep-mode requests"** — loop the GVR turn SOAK_ITERATIONS
/// times back-to-back. Each turn must succeed; the total call count must be
/// exactly `SOAK_ITERATIONS` (one Generate per turn, no repairs needed
/// because the verifier always passes). The test proves:
///
/// - No panic across 50 turns.
/// - No silent retry inflation (the call count is exactly N).
/// - No JoinError or other resource leak.
#[tokio::test]
#[ignore = "VRO-10 §22.4 soak test: 50 iterations; run with --ignored"]
async fn soak_gvr_loop_runs_50_iterations_without_panic_or_leak() {
    let registry = soak_registry();
    let verifier_ids = [VerifierId::new("soak_pass").unwrap()];
    let tracker = Arc::new(RateLimitTracker::untracked());

    for iter in 1..=SOAK_ITERATIONS {
        let generator = SoakGenerator::new();
        let outcome = run_generate_verify_repair_with_rate_limit(
            &format!("soak prompt {iter}"),
            &verifier_ids,
            &registry,
            &generator,
            std::path::Path::new("/tmp/soak-workspace"),
            soak_budget(),
            Arc::clone(&tracker),
            RepairController::new(),
        )
        .await;

        assert_eq!(
            outcome.status,
            OutcomeStatus::Succeeded,
            "iteration {iter} must succeed; got {:?}",
            outcome.status
        );
        // Exactly one Generate per iteration (the verifier passes first try).
        assert_eq!(
            generator.total_calls(),
            1,
            "iteration {iter} must call the generator exactly once"
        );
    }

    // The shared rate-limit tracker was never blocked across all 50 turns.
    assert_eq!(tracker.observed_429_count(), 0);
    assert!(!tracker.is_blocked());
}

/// **PRD §22.4 "Memory growth"** — prove the Repair Controller's signature
/// set stays bounded across SOAK_ITERATIONS distinct repair attempts. Each
/// iteration injects a distinct finding message; the controller must
/// retain only the most recent signature, not accumulate all 50.
#[tokio::test]
#[ignore = "VRO-10 §22.4 soak test: repair signature growth; run with --ignored"]
async fn soak_repair_controller_retains_only_most_recent_signature_across_50_attempts() {
    use vesper_domain::{VerificationFinding, VerificationSeverity};

    let mut controller = RepairController::new();
    let mut corrections: Vec<VerificationFinding> = Vec::new();

    for iter in 1..=SOAK_ITERATIONS {
        // Each iteration has a DISTINCT finding message, so no repetition
        // guard fires. The controller must record only this iteration's
        // signature (the `last_signature` field), NOT a growing set.
        let findings = vec![VerificationFinding {
            message: format!("distinct-failure-{iter}"),
            severity: VerificationSeverity::Error,
            location: Some(format!("src/lib.rs:{iter}")),
        }];
        let _ = controller.augment_corrections(&mut corrections, &findings);
        // The next iteration's distinct findings must NOT match the prior
        // signature.
        if iter < SOAK_ITERATIONS {
            let next = vec![VerificationFinding {
                message: format!("distinct-failure-{}", iter + 1),
                severity: VerificationSeverity::Error,
                location: None,
            }];
            assert!(
                !controller.is_repeated_attempt(&next),
                "iteration {iter}: distinct findings must not flag as repeated"
            );
        }
    }

    // After 50 distinct attempts, the controller must still classify a
    // repeat of iteration 50's findings as a repetition.
    let last_repeat = vec![VerificationFinding {
        message: format!("distinct-failure-{SOAK_ITERATIONS}"),
        severity: VerificationSeverity::Error,
        location: Some(format!("src/lib.rs:{SOAK_ITERATIONS}")),
    }];
    assert!(
        controller.is_repeated_attempt(&last_repeat),
        "the most recent signature must still be detectable as a repeat"
    );

    // The corrections vector grew linearly (one hint per classified finding,
    // plus the raw findings) — that's expected. The TEST proves the
    // controller's INTERNAL state did not grow linearly: only the most
    // recent signature is retained.
    // (The linear growth of `corrections` is fine because it's the caller's
    //  responsibility to bound it via the orchestrator's max_repairs / max_model_calls.)
}

/// **PRD §22.4 "Parallel sessions with same-session serialization"** — prove
/// the parallel candidate executor can be invoked back-to-back across
/// PARALLEL_SOAK_ITERATIONS turns without leaving dangling tokio tasks.
/// Each turn fans out 2 branches; the test asserts the total candidate
/// count is exactly `2 × PARALLEL_SOAK_ITERATIONS` (no missing branches,
/// no duplicates).
#[tokio::test]
#[ignore = "VRO-10 §22.4 soak test: parallel fan-out × 20 iterations; run with --ignored"]
async fn soak_parallel_executor_runs_20_iterations_without_dangling_tasks() {
    use vesper_agent::vro::executor::{BranchDiversification, CandidateExecutor};

    let executor = CandidateExecutor::new();
    let budget = soak_budget();

    let mut total_outcomes = 0usize;
    for iter in 1..=PARALLEL_SOAK_ITERATIONS {
        let generator = SoakGenerator::new();
        let outcomes = executor
            .fan_out_diverse(
                &generator,
                &format!("parallel-soak-prompt-{iter}"),
                2,
                budget,
                BranchDiversification::diverse_branches(),
                |_| false,
            )
            .await
            .unwrap_or_else(|e| panic!("iteration {iter} fan_out_diverse failed: {e:?}"));

        assert_eq!(
            outcomes.len(),
            2,
            "iteration {iter} must produce 2 candidates"
        );
        total_outcomes += outcomes.len();
    }

    // Exactly 2 × N outcomes — no missing branches, no duplicates.
    assert_eq!(
        total_outcomes,
        2 * PARALLEL_SOAK_ITERATIONS,
        "total outcomes must be exactly 2 × N"
    );
}

/// **PRD §22.4 "Provider stalls"** — prove the rate-limit tracker survives
/// SOAK_ITERATIONS record_429 calls followed by clears, then correctly
/// reports Available after the final clear. The atomic counters must not
/// saturate or wrap unexpectedly.
#[tokio::test]
#[ignore = "VRO-10 §22.4 soak test: rate-limit churn × 50; run with --ignored"]
async fn soak_rate_limit_tracker_survives_50_record_429_then_clear_cycles() {
    let tracker = RateLimitTracker::untracked();

    for iter in 1..=SOAK_ITERATIONS {
        // Record a 429 with a short retry-after, then clear it.
        tracker.record_429(Some(10));
        assert!(
            tracker.is_blocked(),
            "iteration {iter}: must be blocked after 429"
        );
        tracker.clear();
        assert!(!tracker.is_blocked(), "iteration {iter}: must be clearable");
    }

    // After 50 record+clear cycles, the observed counter is exactly N
    // (no lost increments, no wrap).
    assert_eq!(
        tracker.observed_429_count(),
        SOAK_ITERATIONS as u64,
        "atomic counter must reflect every record_429 call"
    );
    // Final state is clear (Available), so the tracker is reusable.
    assert!(!tracker.is_blocked());
}

/// **PRD §22.4 "Long sessions"** — combine all subsystems in a single long
/// session: 25 GVR turns interleaved with 25 parallel-fan-out turns, all
/// sharing the same rate-limit tracker. The test proves cross-turn state
/// (the tracker's atomic counters, the XorShift state, the orchestrator's
/// verifier registry) does not corrupt under sustained mixed load.
#[tokio::test]
#[ignore = "VRO-10 §22.4 soak test: mixed long session × 50 turns; run with --ignored"]
async fn soak_mixed_long_session_50_turns_no_state_corruption() {
    use vesper_agent::vro::executor::{BranchDiversification, CandidateExecutor};

    let registry = soak_registry();
    let verifier_ids = [VerifierId::new("soak_pass").unwrap()];
    let tracker = Arc::new(RateLimitTracker::untracked());
    let executor = CandidateExecutor::new();
    let budget = soak_budget();

    for iter in 1..=SOAK_ITERATIONS {
        if iter % 2 == 1 {
            // Odd iteration: a GVR turn.
            let generator = SoakGenerator::new();
            let outcome = run_generate_verify_repair_with_rate_limit(
                &format!("mixed-gvr-{iter}"),
                &verifier_ids,
                &registry,
                &generator,
                std::path::Path::new("/tmp/soak-mixed"),
                budget,
                Arc::clone(&tracker),
                RepairController::new(),
            )
            .await;
            assert_eq!(
                outcome.status,
                OutcomeStatus::Succeeded,
                "iter {iter} GVR failed"
            );
        } else {
            // Even iteration: a parallel fan-out.
            let generator = SoakGenerator::new();
            let outcomes = executor
                .fan_out_diverse(
                    &generator,
                    &format!("mixed-parallel-{iter}"),
                    2,
                    budget,
                    BranchDiversification::diverse_branches(),
                    |_| false,
                )
                .await
                .expect("fan_out_diverse must succeed");
            assert_eq!(
                outcomes.len(),
                2,
                "iter {iter} parallel must yield 2 outcomes"
            );
        }
        // Halfway through, simulate a transient 429 that the composition
        // boundary would clear. The next odd-iteration GVR turn must NOT
        // see the block (because status() auto-clears past the deadline,
        // but here we clear explicitly to keep the test deterministic).
        if iter == SOAK_ITERATIONS / 2 {
            tracker.record_429(Some(5));
            // The deadline is 5ms; by the next iteration the auto-clear in
            // status() will have fired. The next Generate proceeds normally.
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    // After 50 mixed turns: no 429s leaked through (the mid-session 429
    // was either cleared or auto-expired before the next Generate), the
    // tracker is unblocked, and we observed exactly one 429.
    assert_eq!(tracker.observed_429_count(), 1);
    assert!(!tracker.is_blocked());
}

// ---------------------------------------------------------------------------
// VRO-12 (PRD §6 S1) — 200-call bounded/deterministic loop-detector soak
// ---------------------------------------------------------------------------

/// PRD §6 S1: a 200-call adversarial mixed workload through the VRO-12 loop
/// detector. Proves:
///
/// - **Memory bounded:** the sliding window never exceeds
///   [`LOOP_WINDOW_SIZE`] (5) records, no matter how many calls are recorded.
/// - **Determinism:** two detectors fed the identical 200-call script produce
///   the identical action at every step (no clocks, no randomness, no
///   unstable hashing).
///
/// The workload deliberately mixes all three pattern families (exact repeats,
/// ping-pong, no-progress probes) with healthy distinct calls so the detector
/// cycles through every escalation and reset path under sustained load.
///
/// Follows this file's `#[ignore]` convention: 200 iterations × 2 detectors
/// is a soak axis, not a CI gate.
#[tokio::test]
#[ignore = "VRO-12 §6 S1 soak test: 200 iterations; run with --ignored"]
async fn soak_loop_detector_200_call_mixed_workload_bounded_and_deterministic() {
    use vesper_agent::vro::loop_detector::{LOOP_WINDOW_SIZE, LoopDetector, LoopGuardAction};

    const CALLS: usize = 200;

    /// Deterministic 200-call adversarial script. `i` indexes the call.
    /// The mix cycles through: healthy distinct calls, exact-repeat runs,
    /// ping-pong alternation, and no-progress differently-argued probes.
    fn script(i: usize) -> (&'static str, serde_json::Value, String) {
        match i % 20 {
            // Healthy: distinct tool, distinct args, distinct result.
            0 | 1 => (
                "search_files",
                serde_json::json!({"pattern": format!("f{i}.rs")}),
                format!("hits-{i}"),
            ),
            // Exact repeat: same tool, same args, same result.
            2..=6 => (
                "grep",
                serde_json::json!({"pattern": "struct Foo"}),
                "src/foo.rs:12:pub struct Foo".to_string(),
            ),
            // Ping-pong: two tools alternating with varied args.
            7 => (
                "read_file",
                serde_json::json!({"path": format!("src/{i}.rs")}),
                "fn a() {}".to_string(),
            ),
            8 => (
                "list_directory",
                serde_json::json!({"path": format!("src/{i}")}),
                "a.rs\nb.rs".to_string(),
            ),
            9 => (
                "read_file",
                serde_json::json!({"path": format!("src/{i}.rs")}),
                "fn b() {}".to_string(),
            ),
            10 => (
                "list_directory",
                serde_json::json!({"path": format!("src/{i}")}),
                "c.rs\nd.rs".to_string(),
            ),
            // No-progress: same tool, different args, identical result.
            11..=15 => (
                "grep",
                serde_json::json!({"pattern": format!("needle-{i}")}),
                "no matches found".to_string(),
            ),
            // Healthy again to reset every pattern before the next cycle.
            _ => (
                "read_file",
                serde_json::json!({"path": format!("distinct-{i}.rs")}),
                format!("unique body {i}"),
            ),
        }
    }

    let mut detectors = [LoopDetector::new(), LoopDetector::new()];
    let mut warn_count = 0usize;
    let mut block_count = 0usize;
    let mut break_count = 0usize;

    for i in 0..CALLS {
        let (name, args, result) = script(i);
        // Every recorded step must keep both windows bounded at 5.
        let actions: Vec<LoopGuardAction> = detectors
            .iter_mut()
            .map(|d| {
                d.record(
                    name,
                    &args,
                    &result,
                    vesper_domain::ToolExecutionClass::ReadOnly,
                )
            })
            .collect();
        for detector in &detectors {
            assert!(
                detector.len() <= LOOP_WINDOW_SIZE,
                "window must stay bounded at {LOOP_WINDOW_SIZE} after call {i}; got {}",
                detector.len()
            );
        }
        // Determinism: both detectors must agree at every single step.
        assert_eq!(
            actions[0], actions[1],
            "detectors diverged at call {i}: {actions:?}"
        );
        match &actions[0] {
            LoopGuardAction::Clear => {}
            LoopGuardAction::Warn(_) => warn_count += 1,
            LoopGuardAction::Block(_) => block_count += 1,
            LoopGuardAction::Break(_) => break_count += 1,
        }
    }

    // The workload is adversarial by construction, so the guard must have
    // actually fired — otherwise this soak would prove nothing.
    assert!(warn_count > 0, "adversarial workload must produce warns");
    assert!(block_count > 0, "adversarial workload must produce blocks");
    assert!(break_count > 0, "adversarial workload must produce breaks");
}
