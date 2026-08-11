//! Parallel Candidate Executor (VRO-4, PRD §10.6 + §11.4 / §11.5).
//!
//! The executor fans a [`ReasoningRequest`] out into N independent inference
//! branches that run concurrently on `tokio::task::spawn`. Each branch
//! receives a **deeply cloned, isolated** [`BranchContext`] so intermediate
//! tool outputs (or scratch notes a branch records while reasoning) cannot
//! leak into a sibling branch. Results are aggregated deterministically:
//! branches are assigned monotonic [`CandidateId`]s and the final vector is
//! sorted by that ID before being returned to the strategy layer.
//!
//! ## Budget enforcement (PRD §10.4)
//!
//! The requested `branches` count is **capped** at `budget.max_parallel_branches`
//! (PRD §10.4: the budget manager must "stop low-value branches" and "respect
//! cancellation"). The cap is a hard upper bound; passing `branches = 5` with
//! `max_parallel_branches = 2` runs exactly 2 concurrent branches.
//!
//! ## Zero-breakage contract
//!
//! This module is only invoked by the [`ParallelCandidatesConsensus`] and
//! [`ParallelCandidatesJudge`] strategy handlers (PRD §11.4 / §11.5). It does
//! not touch [`crate::AgentLoop`], `agent_loop.rs`, the tool registry, or the
//! permission gate. The existing [`Direct`](vesper_domain::ReasoningStrategy)
//! and [`GenerateVerifyRepair`](vesper_domain::ReasoningStrategy) paths are
//! completely unaffected.

use tokio::task::JoinHandle;
use vesper_domain::{Candidate, CandidateId, ReasoningBudget};

use super::orchestrator::{CandidateGenerator, GeneratedCandidate};

// ---------------------------------------------------------------------------
// Branch isolation
// ---------------------------------------------------------------------------

/// Per-branch isolated scratch context.
///
/// Each parallel branch receives its OWN deep clone of this value (the
/// executor calls [`Self::clone_for_branch`] when spawning). Because the
/// clone allocates a fresh `Vec`, any notes a branch records during reasoning
/// (intermediate tool outputs, scratch reasoning, side-channel discoveries)
/// stay local to that branch. The Consensus and Judge strategies never read
/// the per-branch scratch — only the candidate `output` and `evidence` are
/// consumed — so isolation is preserved even when a branch attempts to write
/// into the shared request.
///
/// This is the structural answer to PRD §10.6: "Preservation of evidence and
/// tool results" must NOT cross branch boundaries.
#[derive(Debug, Clone, Default)]
pub struct BranchContext {
    /// Per-branch mutable scratch notes. Cloned per branch so writes here
    /// never leak to siblings.
    notes: Vec<String>,
    /// Index of this branch in the deterministic fan-out order (0-based).
    branch_index: usize,
}

impl BranchContext {
    /// Builds a fresh, empty context for the given branch index.
    #[must_use]
    pub fn new(branch_index: usize) -> Self {
        Self {
            notes: Vec::new(),
            branch_index,
        }
    }

    /// Deep-clones this context for one branch. The returned value has its
    /// own `Vec` allocation so mutations in the branch never affect the
    /// original or any sibling branch.
    #[must_use]
    pub fn clone_for_branch(&self) -> Self {
        Self {
            notes: self.notes.clone(),
            branch_index: self.branch_index,
        }
    }

    /// Records a per-branch scratch note. Mutates only this branch's clone.
    pub fn record_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Snapshot of the branch-local notes (used in tests to prove isolation).
    #[must_use]
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// The branch index assigned at fan-out time.
    #[must_use]
    pub fn branch_index(&self) -> usize {
        self.branch_index
    }
}

// ---------------------------------------------------------------------------
// Deterministic candidate IDs
// ---------------------------------------------------------------------------

/// Builds a deterministic, monotonic [`CandidateId`] for branch `index`.
///
/// The IDs are stable strings (`cand-0000`, `cand-0001`, …) so the
/// aggregated vector sorts lexicographically in the same order the branches
/// were spawned. This is the determinism contract: callers can compare two
/// fan-out runs byte-for-byte when the generator is deterministic.
fn candidate_id_for(index: usize) -> CandidateId {
    // Four-digit zero-padded prefix so lexical sort matches numeric sort up
    // to 9999 branches (well above any sane `max_parallel_branches`).
    CandidateId::new(format!("cand-{index:04}")).expect("cand-NNNN is a valid CandidateId")
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// Outcome of one parallel branch.
#[derive(Debug, Clone)]
pub struct BranchOutcome {
    /// The candidate (assigned its deterministic ID).
    pub candidate: Candidate,
    /// The branch-local scratch context snapshot, for tests / telemetry.
    pub context: BranchContext,
}

/// Errors returned by the executor. None are currently fatal — branches that
/// panic are reported as a single `AllBranchesFailed` outcome so the strategy
/// layer can degrade gracefully (PRD §18 failure modes).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecutorError {
    /// Every spawned branch panicked or returned an internal error.
    #[error("all {0} parallel branches failed")]
    AllBranchesFailed(usize),
    /// The budget caps the branch count to zero (misconfigured request).
    #[error("budget.max_parallel_branches is zero — cannot fan out")]
    ZeroParallelBranches,
}

/// The parallel candidate executor (PRD §10.6).
///
/// Stateless and cheap to construct. Holds no provider handles and mutates no
/// shared session state. Each call to [`fan_out`](Self::fan_out) produces an
/// isolated batch of branches; the executor does not retain any per-branch
/// state between calls.
#[derive(Debug, Default, Clone)]
pub struct CandidateExecutor;

impl CandidateExecutor {
    /// Constructs a new executor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Caps a requested branch count to the budget's `max_parallel_branches`.
    ///
    /// Returns `Err(ZeroParallelBranches)` when the budget explicitly forbids
    /// any parallel work. Returns `Ok(n)` with `n ∈ [1, max_parallel_branches]`
    /// otherwise — `n` is at least 1 because a single branch is always useful
    /// (the strategy can still converge on the lone candidate).
    pub fn capped_branch_count(
        &self,
        requested: usize,
        budget: &ReasoningBudget,
    ) -> Result<usize, ExecutorError> {
        let cap = usize::from(budget.max_parallel_branches);
        if cap == 0 {
            return Err(ExecutorError::ZeroParallelBranches);
        }
        Ok(requested.clamp(1, cap))
    }

    /// Fans the request out into `requested` parallel branches (capped to
    /// `budget.max_parallel_branches`) and returns the aggregated candidates
    /// sorted by their deterministic [`CandidateId`].
    ///
    /// Each branch runs on its own `tokio::task::spawn` and receives a deep
    /// clone of the per-branch [`BranchContext`]. Branches that panic are
    /// dropped (the strategy still proceeds with whatever survived); if every
    /// branch fails the function returns
    /// [`ExecutorError::AllBranchesFailed`].
    ///
    /// `prompt` is the user message text (or a strategy-augmented variant)
    /// every branch starts from. The `generator` is the provider seam — the
    /// orchestrator never makes a provider call directly.
    pub async fn fan_out(
        &self,
        generator: &dyn CandidateGenerator,
        prompt: &str,
        requested: usize,
        budget: ReasoningBudget,
    ) -> Result<Vec<BranchOutcome>, ExecutorError> {
        let branch_count = self.capped_branch_count(requested, &budget)?;

        // Build one isolated context per branch up front. Each branch task
        // receives its own clone_of-clone so even if the task mutates its
        // context, no sibling sees the change.
        let contexts: Vec<BranchContext> = (0..branch_count).map(BranchContext::new).collect();

        // Spawn each branch on its own task. Each task owns its own cloned
        // BranchContext; the original `contexts[i]` stays untouched so we
        // can later prove (in tests) that no branch leaked into the parent
        // snapshot. The generator gets a per-branch `boxed_clone` so the
        // spawned future has an owned `'static` handle (tokio::task::spawn
        // requires `'static + Send`).
        let mut handles: Vec<JoinHandle<Option<BranchOutcome>>> = Vec::new();
        for (index, context) in contexts.iter().enumerate() {
            // Deep-clone the context FOR THIS BRANCH. The branch's mutation
            // surface is `&mut BranchContext` on its own allocation.
            let mut branch_context = context.clone_for_branch();
            let prompt_owned = prompt.to_string();
            // Strategy variant tag: lets a future VRO phase diversify prompts
            // per branch (PRD §10.6 "diverse sampling"). For VRO-4 every
            // branch uses the same prompt; the variant tag is still recorded
            // so the candidate is self-describing.
            let strategy_variant = format!("parallel-branch-{index}");
            let generator_clone = generator.boxed_clone();
            let handle = tokio::spawn(async move {
                let candidate = generator_clone
                    .generate(&prompt_owned, &[])
                    .await
                    .into_candidate(candidate_id_for(index), &strategy_variant);
                // Record a scratch note in the branch-local context so tests
                // can assert that sibling branches do NOT see this note.
                branch_context.record_note(format!(
                    "branch-{index}-completed (output-len={})",
                    candidate.output.to_string().len()
                ));
                Some(BranchOutcome {
                    candidate,
                    context: branch_context,
                })
            });
            handles.push(handle);
        }

        // Await every branch. Branches that panicked return `Err` from
        // `JoinHandle::await`; we drop those and proceed with the survivors
        // (PRD §18: degrade gracefully instead of hard-failing one bad
        // branch).
        let mut outcomes: Vec<BranchOutcome> = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(Some(outcome)) => outcomes.push(outcome),
                Ok(None) => {} // generator returned a poison sentinel (none today)
                Err(_join_err) => {} // branch task panicked — skip
            }
        }

        if outcomes.is_empty() {
            return Err(ExecutorError::AllBranchesFailed(branch_count));
        }

        // Deterministic ordering: sort by candidate_id. The IDs are monotonic
        // and zero-padded so lexical sort matches spawn order.
        outcomes.sort_by(|a, b| {
            a.candidate
                .candidate_id
                .as_str()
                .cmp(b.candidate.candidate_id.as_str())
        });
        Ok(outcomes)
    }
}

// ---------------------------------------------------------------------------
// GeneratedCandidate → Candidate bridge
// ---------------------------------------------------------------------------

impl GeneratedCandidate {
    /// Promotes a raw generated candidate into a full [`Candidate`] tagged
    /// with its deterministic [`CandidateId`] and strategy variant.
    #[must_use]
    pub fn into_candidate(self, id: CandidateId, strategy_variant: &str) -> Candidate {
        Candidate {
            candidate_id: id,
            strategy_variant: strategy_variant.to_string(),
            output: self.output,
            evidence: Vec::new(),
            verification: Vec::new(),
            cost: self.cost,
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic shuffle (XorShift8) — for the Judge strategy
// ---------------------------------------------------------------------------

/// Tiny deterministic seedable PRNG (xorshift32). Used by the Judge strategy
/// to shuffle candidate order without introducing a `rand` dependency on
/// `vesper-agent`. Two `XorShiftRng` instances created with the same seed
/// produce identical shuffle sequences — this is what makes the Judge
/// strategy reproducible in tests.
///
/// NOT cryptographically secure. Used only for shuffling candidate order to
/// reduce position bias (PRD §11.5); never used for any security-relevant
/// decision.
#[derive(Debug, Clone)]
pub struct XorShiftRng {
    state: u32,
}

impl XorShiftRng {
    /// Creates a new RNG. A seed of `0` is remapped to a non-zero constant
    /// (xorshift would otherwise stay at 0 forever).
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0xDEAD_BEEF } else { seed as u32 },
        }
    }

    /// Returns the next pseudo-random `u32`.
    fn next_u32(&mut self) -> u32 {
        // Marsaglia xorshift32. Cheap, deterministic, sufficient for
        // shuffle position randomization.
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// In-place Fisher–Yates shuffle of `slice`. Deterministic for a given
    /// seed. Used by the Judge strategy (PRD §11.5: "candidates in randomized
    /// order to reduce position bias").
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        if slice.len() < 2 {
            return;
        }
        // Fisher–Yates: for i from last down to 1, swap with a random index
        // in [0, i].
        for i in (1..slice.len()).rev() {
            let j = (self.next_u32() as usize) % (i + 1);
            slice.swap(i, j);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use vesper_domain::{InferenceCost, ReasoningMode, ReasoningRequest, StructuredOutput};

    // --- Fakes ---

    /// Generator that counts concurrent invocations and records the maximum
    /// observed concurrency (for the budget-cap test). Each call records its
    /// own BranchContext note (so isolation can be asserted).
    struct ConcurrencyCountingGenerator {
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        outputs: Mutex<Vec<StructuredOutput>>,
    }

    impl ConcurrencyCountingGenerator {
        fn new(outputs: Vec<StructuredOutput>) -> Self {
            Self {
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
                outputs: Mutex::new(outputs),
            }
        }

        fn max_concurrency(&self) -> usize {
            self.max_active.load(Ordering::SeqCst)
        }
    }

    impl CandidateGenerator for ConcurrencyCountingGenerator {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [vesper_domain::VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            Box::pin(async move {
                let cur = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                let mut max = self.max_active.load(Ordering::SeqCst);
                while cur > max {
                    match self.max_active.compare_exchange(
                        max,
                        cur,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(observed) => max = observed,
                    }
                }
                // Yield once so multiple branches overlap in time. The
                // budget-cap test asserts max_concurrency <= cap.
                tokio::task::yield_now().await;
                let output = {
                    let mut outputs = self.outputs.lock().expect("poisoned");
                    if outputs.len() == 1 {
                        outputs[0].clone()
                    } else {
                        outputs.remove(0)
                    }
                };
                self.active.fetch_sub(1, Ordering::SeqCst);
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
            // Share the active/max counters across clones (the budget-cap
            // test asserts observed concurrency across all branches); each
            // clone gets its own outputs Vec so the programmed sequence can
            // be replayed per branch.
            Box::new(Self {
                active: Arc::clone(&self.active),
                max_active: Arc::clone(&self.max_active),
                outputs: Mutex::new(self.outputs.lock().expect("poisoned").clone()),
            })
        }
    }

    fn budget(max_parallel: u16) -> ReasoningBudget {
        ReasoningBudget {
            max_parallel_branches: max_parallel,
            ..ReasoningBudget::balanced()
        }
    }

    fn request_with(message: &str) -> ReasoningRequest {
        ReasoningRequest {
            request_id: vesper_domain::RequestId::new("req-test").unwrap(),
            session_id: vesper_domain::SessionId::new("sess-test").unwrap(),
            user_message: message.to_string(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: None,
            privacy_mode: vesper_domain::PrivacyMode::Private,
        }
    }

    // --- Directive 4 test 1: requesting 5 branches with cap=2 runs only 2 ---

    #[tokio::test]
    async fn budget_cap_limits_concurrent_branches_to_max_parallel_branches() {
        // Request 5 branches; budget caps at 2. The executor MUST run only 2.
        let generator = ConcurrencyCountingGenerator::new(vec![
            serde_json::json!({"answer": "a"}),
            serde_json::json!({"answer": "b"}),
        ]);
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out(&generator, "compute", 5, budget(2))
            .await
            .expect("fan_out must succeed");

        // Only 2 candidates returned (the cap was respected).
        assert_eq!(
            outcomes.len(),
            2,
            "requesting 5 branches with max_parallel_branches=2 must yield 2"
        );
        // The max observed concurrency never exceeded the cap.
        assert!(
            generator.max_concurrency() <= 2,
            "max_concurrency {} must be <= cap 2",
            generator.max_concurrency()
        );
        // IDs are deterministic and monotonic.
        assert_eq!(
            outcomes[0].candidate.candidate_id.as_str(),
            "cand-0000",
            "first outcome is branch 0"
        );
        assert_eq!(
            outcomes[1].candidate.candidate_id.as_str(),
            "cand-0001",
            "second outcome is branch 1"
        );
    }

    // --- Directive 4 test 2: cross-branch state isolation ---

    #[tokio::test]
    async fn branch_context_isolation_no_cross_branch_leakage() {
        // Each branch records a note into its own context. The aggregated
        // outcomes must each contain ONLY their own note — proof that no
        // branch leaked its scratch into a sibling.
        let generator = ConcurrencyCountingGenerator::new(vec![
            serde_json::json!({"answer": "a"}),
            serde_json::json!({"answer": "b"}),
            serde_json::json!({"answer": "c"}),
        ]);
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out(&generator, "compute", 3, budget(3))
            .await
            .expect("fan_out must succeed");

        assert_eq!(outcomes.len(), 3);
        for (i, outcome) in outcomes.iter().enumerate() {
            // Each branch saw exactly ONE note: its own. No sibling note
            // leaked in. (Branch index 0 → "branch-0-completed", etc.)
            assert_eq!(
                outcome.context.notes().len(),
                1,
                "branch {i} must have exactly one note (its own)"
            );
            assert!(
                outcome.context.notes()[0].contains(&format!("branch-{i}-completed")),
                "branch {i} must contain its own note, got: {:?}",
                outcome.context.notes()
            );
        }
    }

    // --- Directive 4 test 3: deterministic ordering + IDs ---

    #[tokio::test]
    async fn fan_out_returns_monotonic_candidate_ids_in_order() {
        let generator = ConcurrencyCountingGenerator::new(vec![
            serde_json::json!({"answer": "x"}),
            serde_json::json!({"answer": "y"}),
            serde_json::json!({"answer": "z"}),
        ]);
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out(&generator, "compute", 3, budget(3))
            .await
            .expect("fan_out must succeed");

        let ids: Vec<&str> = outcomes
            .iter()
            .map(|o| o.candidate.candidate_id.as_str())
            .collect();
        assert_eq!(ids, vec!["cand-0000", "cand-0001", "cand-0002"]);
        // Strategy variants encode the spawn order too.
        assert_eq!(outcomes[0].candidate.strategy_variant, "parallel-branch-0");
    }

    // --- Directive 4 test 4: zero-cap budget errors clearly ---

    #[tokio::test]
    async fn zero_cap_budget_returns_zero_parallel_branches_error() {
        let generator = ConcurrencyCountingGenerator::new(vec![serde_json::json!({"a": 1})]);
        let executor = CandidateExecutor::new();
        let err = executor
            .fan_out(&generator, "compute", 1, budget(0))
            .await
            .expect_err("zero cap must error");
        assert_eq!(err, ExecutorError::ZeroParallelBranches);
    }

    // --- XorShiftRng shuffle determinism + reversibility ---

    #[test]
    fn xor_shift_rng_shuffle_is_deterministic_for_same_seed() {
        let mut a = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut b = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut rng_a = XorShiftRng::new(42);
        let mut rng_b = XorShiftRng::new(42);
        rng_a.shuffle(&mut a);
        rng_b.shuffle(&mut b);
        assert_eq!(a, b, "same seed must produce identical shuffle");
    }

    #[test]
    fn xor_shift_rng_different_seeds_usually_differ() {
        // Different seeds usually produce different orders for a long enough
        // slice (this is a sanity check, not a probabilistic guarantee).
        let mut a = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut b = a.clone();
        XorShiftRng::new(1).shuffle(&mut a);
        XorShiftRng::new(2).shuffle(&mut b);
        assert_ne!(a, b, "different seeds should differ");
    }

    #[test]
    fn xor_shift_rng_shuffle_preserves_element_set() {
        let mut a = vec!["cand-0000", "cand-0001", "cand-0002", "cand-0003"];
        let original: Vec<&str> = a.clone();
        XorShiftRng::new(7).shuffle(&mut a);
        // Shuffle is a permutation — same multiset.
        let mut original_sorted = original.clone();
        let mut a_sorted = a.clone();
        original_sorted.sort();
        a_sorted.sort();
        assert_eq!(original_sorted, a_sorted);
    }

    #[test]
    fn xor_shift_rng_seed_zero_is_remapped_not_stuck() {
        // Seed 0 must be remapped so the RNG does not stay at 0 forever.
        let mut rng = XorShiftRng::new(0);
        let a = rng.next_u32_inner();
        let b = rng.next_u32_inner();
        assert_ne!(a, b, "RNG must advance from seed 0");
    }

    impl XorShiftRng {
        fn next_u32_inner(&mut self) -> u32 {
            self.next_u32()
        }
    }

    // --- BranchContext isolation unit (no async needed) ---

    #[test]
    fn branch_context_clone_does_not_share_state() {
        let mut parent = BranchContext::new(0);
        parent.record_note("parent");
        let mut child = parent.clone_for_branch();
        child.record_note("child");
        // Parent does NOT see the child's note.
        assert_eq!(parent.notes(), &["parent"]);
        // Child sees both (it inherited the parent's note at clone time and
        // then added its own — that's correct deep-clone semantics).
        assert_eq!(child.notes(), &["parent", "child"]);
    }

    // --- Sanity: a fan_out round trips through ReasoningRequest prompt ---

    #[tokio::test]
    async fn fan_out_uses_the_request_prompt_text_as_generator_input() {
        // The executor takes the prompt string directly (not the full
        // ReasoningRequest), so the request construction is host-side. This
        // test confirms the prompt is forwarded unchanged. The seen-state
        // MUST be shared across the boxed clones (Arc<Mutex>) so the
        // original instance observes prompts recorded by spawned branches.
        struct EchoGenerator {
            seen: Arc<Mutex<Vec<String>>>,
        }
        impl CandidateGenerator for EchoGenerator {
            fn generate<'a>(
                &'a self,
                prompt: &'a str,
                _corrections: &'a [vesper_domain::VerificationFinding],
            ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
                let seen = Arc::clone(&self.seen);
                Box::pin(async move {
                    seen.lock().expect("poisoned").push(prompt.to_string());
                    GeneratedCandidate {
                        output: serde_json::json!({"echo": prompt}),
                        cost: InferenceCost::default(),
                    }
                })
            }

            fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
                Box::new(Self {
                    seen: Arc::clone(&self.seen),
                })
            }
        }
        let generator = EchoGenerator {
            seen: Arc::new(Mutex::new(Vec::new())),
        };
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out(&generator, "the-prompt-text", 2, budget(2))
            .await
            .expect("fan_out must succeed");
        assert_eq!(outcomes.len(), 2);
        let seen = generator.seen.lock().expect("poisoned").clone();
        assert_eq!(seen, vec!["the-prompt-text".to_string(); 2]);
        // Silence unused-warning when request_with is referenced elsewhere.
        let _ = request_with("unused");
    }
}
