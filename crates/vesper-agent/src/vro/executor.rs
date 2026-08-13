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

use std::future::Future;
use std::pin::Pin;
use tokio::task::JoinHandle;
use vesper_domain::{Candidate, CandidateId, ReasoningBudget, VerificationFinding};

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

/// Per-branch prompt diversification (VRO-10, PRD §10.6
/// "Candidate-specific prompts").
///
/// The VRO-4 executor originally fed the **identical** prompt to every
/// parallel branch. PRD §10.6 demands "separate inference calls with
/// controlled variation in strategy, assumptions, or decoding" — diversity
/// must NOT be simulated by asking for "three alternatives" in one
/// completion. This enum captures the diversification strategy the executor
/// applies to each branch's prompt.
///
/// Because the [`CandidateGenerator`](super::orchestrator::CandidateGenerator)
/// trait's `generate(prompt, corrections)` signature has no temperature
/// parameter, diversification is expressed as a per-branch **prompt prefix**
/// that nudges the model toward a distinct reasoning stance. The prefix is
/// prepended verbatim to the branch's prompt before `generate` is called.
#[derive(Debug, Clone, Default)]
pub enum BranchDiversification {
    /// No diversification — every branch receives the identical prompt
    /// (VRO-4 behavior). The default; the original `fan_out` /
    /// `fan_out_with_early_stop` API preserves this.
    #[default]
    None,
    /// Per-branch system-prompt variants. Branch `i` receives variant
    /// `variants[i % variants.len()]` prepended to its prompt. The PRD
    /// §10.6 directive's canonical example: `["Be conservative",
    /// "Be balanced", "Be creative", "Be highly creative"]`.
    SystemPromptVariants(Vec<String>),
}

impl BranchDiversification {
    /// The PRD §10.6 directive's canonical 4-variant default: conservative →
    /// balanced → creative → highly creative. Branches past index 3 wrap
    /// around modulo 4.
    ///
    /// Used by [`CandidateExecutor::fan_out_diverse`] when no explicit
    /// diversification is supplied.
    #[must_use]
    pub fn diverse_branches() -> Self {
        Self::SystemPromptVariants(vec![
            "Be conservative: prefer the smallest, safest, most conventional solution that satisfies the constraints."
                .to_string(),
            "Be balanced: weigh trade-offs explicitly and pick the solution with the best cost/benefit ratio."
                .to_string(),
            "Be creative: explore a non-obvious approach, but justify why it is sound."
                .to_string(),
            "Be highly creative: propose the most divergent viable approach, even if unconventional."
                .to_string(),
        ])
    }
    /// Returns the prompt prefix for branch `index`, or `None` when no
    /// diversification is configured. The prefix is `"\n\n"`-separated from
    /// the user prompt when applied.
    #[must_use]
    pub fn prompt_prefix_for(&self, index: usize) -> Option<&str> {
        match self {
            Self::None => None,
            Self::SystemPromptVariants(variants) => {
                if variants.is_empty() {
                    return None;
                }
                Some(&variants[index % variants.len()])
            }
        }
    }
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
    /// Equivalent to [`fan_out_with_early_stop`](Self::fan_out_with_early_stop)
    /// with a predicate that never stops early (every branch is awaited). Used
    /// by the [`ParallelCandidatesConsensus`] /
    /// [`ParallelCandidatesJudge`] strategies where the verifier only runs
    /// AFTER every candidate is in hand (PRD §11.4 / §11.5: "compare N
    /// candidates").
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
        self.fan_out_with_early_stop(generator, prompt, requested, budget, |_| false)
            .await
    }

    /// Race-aware fan-out (PRD §10.6 "Branch cancellation" + "Early stopping
    /// when a verifier establishes success").
    ///
    /// Identical spawn/isolation/ordering contract as
    /// [`fan_out`](Self::fan_out), with one addition: branches are polled
    /// concurrently with `tokio::select!` over their `JoinHandle`s. As soon
    /// as a branch completes, `early_stop(&outcome)` is consulted. If it
    /// returns `true` the executor **immediately** [`abort`](JoinHandle::abort)s
    /// every still-pending sibling and returns the outcomes collected so far
    /// (including the triggering outcome). PRD §10.6: "Respect cancellation
    /// immediately"; PRD §10.4: "Stop low-value branches".
    ///
    /// `early_stop` is the verifier hook: the caller supplies a closure that
    /// returns `true` when the candidate is **definitively** verified-success
    /// (e.g. a structured-output field matches the expected schema AND the
    /// associated verifier passed). The executor never interprets candidate
    /// contents — that decision is the strategy layer's responsibility.
    ///
    /// **Determinism:** when no early-stop fires, behavior is byte-identical
    /// to [`fan_out`](Self::fan_out). When early-stop fires, the returned
    /// vector is still sorted by `CandidateId` (lexical = spawn order), so
    /// the deterministic-ordering contract holds for the subset that ran.
    ///
    /// **Zero-breakage:** existing callers use [`fan_out`](Self::fan_out),
    /// which delegates here with `|_| false` (never stop early). The new
    /// surface is opt-in.
    pub async fn fan_out_with_early_stop<F>(
        &self,
        generator: &dyn CandidateGenerator,
        prompt: &str,
        requested: usize,
        budget: ReasoningBudget,
        early_stop: F,
    ) -> Result<Vec<BranchOutcome>, ExecutorError>
    where
        F: Fn(&BranchOutcome) -> bool + Send + Sync,
    {
        // VRO-4 behavior: no diversification. Each branch receives the
        // identical prompt.
        self.fan_out_impl(
            generator,
            prompt,
            requested,
            budget,
            std::sync::Arc::new(early_stop),
            BranchDiversification::None,
        )
        .await
    }

    /// VRO-10 (PRD §10.6 "Candidate-specific prompts") — fan-out where each
    /// branch receives a **mathematically distinct prompt prefix** so the
    /// parallel candidates follow heterogeneous reasoning paths instead of
    /// collapsing onto the same answer.
    ///
    /// Behavior is identical to [`fan_out_with_early_stop`](Self::fan_out_with_early_stop)
    /// except that `diversification.prompt_prefix_for(branch_index)` is
    /// prepended to the per-branch prompt before `generate` is invoked. When
    /// `diversification == BranchDiversification::None` this method is
    /// byte-identical to `fan_out_with_early_stop` with a never-stop
    /// predicate.
    ///
    /// PRD §10.6: "Candidate diversity must not be simulated merely by
    /// asking for 'three alternatives' in one completion."
    pub async fn fan_out_diverse<F>(
        &self,
        generator: &dyn CandidateGenerator,
        prompt: &str,
        requested: usize,
        budget: ReasoningBudget,
        diversification: BranchDiversification,
        early_stop: F,
    ) -> Result<Vec<BranchOutcome>, ExecutorError>
    where
        F: Fn(&BranchOutcome) -> bool + Send + Sync,
    {
        self.fan_out_impl(
            generator,
            prompt,
            requested,
            budget,
            std::sync::Arc::new(early_stop),
            diversification,
        )
        .await
    }

    /// Shared fan-out core used by [`fan_out`], [`fan_out_with_early_stop`],
    /// and [`fan_out_diverse`]. The `diversification` parameter controls
    /// per-branch prompt prefixes (VRO-10 §10.6); `BranchDiversification::None`
    /// preserves the VRO-4 single-prompt behavior.
    async fn fan_out_impl<F>(
        &self,
        generator: &dyn CandidateGenerator,
        prompt: &str,
        requested: usize,
        budget: ReasoningBudget,
        early_stop: std::sync::Arc<F>,
        diversification: BranchDiversification,
    ) -> Result<Vec<BranchOutcome>, ExecutorError>
    where
        F: Fn(&BranchOutcome) -> bool + Send + Sync,
    {
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
            let mut branch_context = context.clone_for_branch();
            // VRO-10 §10.6: prepend the per-branch diversification prefix
            // (system-prompt variant) so each branch follows a distinct
            // reasoning stance. When no diversification is configured, the
            // prefix is empty and the prompt is byte-identical across
            // branches (VRO-4 behavior).
            let branch_prompt = match diversification.prompt_prefix_for(index) {
                Some(prefix) => format!("{prefix}\n\n{prompt}"),
                None => prompt.to_string(),
            };
            let strategy_variant = format!("parallel-branch-{index}");
            let generator_clone = generator.boxed_clone();
            let handle = tokio::spawn(async move {
                let candidate = generator_clone
                    .generate(&branch_prompt, &[])
                    .await
                    .into_candidate(candidate_id_for(index), &strategy_variant);
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

        // Race-aware aggregation: poll every handle concurrently. As soon as
        // one completes, check the early-stop predicate; if it fires, abort
        // the remaining siblings and return what we have.
        let mut outcomes: Vec<BranchOutcome> = Vec::with_capacity(handles.len());
        while !handles.is_empty() {
            // Poll all pending handles concurrently; resolve one per loop
            // iteration. tokio::select! over a slice requires a manual loop
            // (no variadic macro on dynamic counts), so we use futures::join
            // semantics via `tokio::select!` on each handle's await in turn.
            // Cheaper approach: `tokio::task::yield_now` then check which
            // handles have completed via `is_finished()` (no I/O blocking).
            //
            // We instead use a portable pattern: poll handles in order with
            // `tokio::select!` race over a single batched future built from
            // the live handles. To keep the implementation allocation-free
            // and dependency-light (no futures::stream), we drive each handle
            // with `tokio::select!` against a `tokio::time::sleep(0)` poll
            // tick; this is O(N) per resolution and bounded by branch_count
            // (≤ max_parallel_branches ≤ max_global_parallel_branches = 4).
            let mut resolved_index = None;
            for (slot, handle) in handles.iter_mut().enumerate() {
                // tokio::select! races `handle.await` against an instant
                // wakeup; whichever is ready first wins. The instant branch
                // never wins unless `handle.await` is pending AND nothing
                // else wakes the task — making this effectively a non-
                // blocking poll. When the handle is ready, we capture its
                // slot and break.
                tokio::select! {
                    biased;
                    joined = handle => {
                        resolved_index = Some((slot, joined));
                        break;
                    }
                    _ = tokio::task::yield_now() => {}
                }
            }

            let Some((slot, joined)) = resolved_index else {
                // No handle resolved this pass; yield once and retry. This
                // cannot loop forever because at least one spawned task will
                // eventually complete (every generator future resolves).
                tokio::task::yield_now().await;
                continue;
            };

            // Remove the resolved handle from the live set.
            handles.remove(slot);
            match joined {
                Ok(Some(outcome)) => {
                    if early_stop.as_ref()(&outcome) {
                        // Cancellation: abort every still-pending sibling.
                        for pending in &handles {
                            pending.abort();
                        }
                        // Drain remaining (aborted) handles so JoinHandle
                        // resources are reclaimed and panics are silenced.
                        for pending in handles.drain(..) {
                            // JoinError from abort is expected; ignore it.
                            let _ = pending.await;
                        }
                        outcomes.push(outcome);
                        break;
                    }
                    outcomes.push(outcome);
                }
                Ok(None) => {} // generator returned a poison sentinel (none today)
                Err(_join_err) => {} // branch task panicked — skip
            }
        }

        if outcomes.is_empty() {
            return Err(ExecutorError::AllBranchesFailed(branch_count));
        }

        // Deterministic ordering: sort by candidate_id. The IDs are monotonic
        // and zero-padded so lexical sort matches spawn order. Early-stop
        // outcomes are a strict subset of all outcomes, but the sort still
        // applies (and is stable w.r.t. spawn order).
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
// Multi-model candidate generator (PRD §10.6 "Cross-model candidates")
// ---------------------------------------------------------------------------

/// A [`CandidateGenerator`] that fans out a single request across **multiple
/// heterogeneous providers** (PRD §10.6: "Cross-model candidates").
///
/// Each branch index N is routed to the provider at slot `N % providers.len()`
/// (round-robin diversity). This ensures reasoning diversity even when every
/// provider receives the identical prompt — the directive's "separate inference
/// calls with controlled variation in … model" (PRD §10.6: "Candidate diversity
/// must not be simulated merely by asking for 'three alternatives' in one
/// completion").
///
/// `boxed_clone` returns a deep clone holding one `boxed_clone` of every
/// underlying provider — so the VRO-4 executor can give each spawned branch
/// an owned `'static` handle while preserving the round-robin routing.
///
/// Construction is fallible: an empty provider list is rejected (no branches
/// could ever run).
pub struct MultiModelCandidateGenerator {
    /// Round-robin provider pool. The first call uses index 0, the second
    /// index 1, etc. Wraps modulo `providers.len()`.
    providers: Vec<Box<dyn CandidateGenerator>>,
    /// Monotonic call counter (atomic so concurrent `boxed_clone`s and
    /// invocations route deterministically under spawn).
    counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl std::fmt::Debug for MultiModelCandidateGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiModelCandidateGenerator")
            .field("provider_count", &self.providers.len())
            .field(
                "next_call_index",
                &self.counter.load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish()
    }
}

impl MultiModelCandidateGenerator {
    /// Builds a multi-model generator over `providers`. Returns `Err` if the
    /// pool is empty (no branches could ever run — a misconfiguration the
    /// composition boundary should catch).
    pub fn new(providers: Vec<Box<dyn CandidateGenerator>>) -> Result<Self, MultiModelError> {
        if providers.is_empty() {
            return Err(MultiModelError::EmptyProviderPool);
        }
        Ok(Self {
            providers,
            counter: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    }

    /// Number of providers in the round-robin pool.
    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Picks the provider for call index `n` (round-robin).
    fn provider_for_index(&self, n: usize) -> &dyn CandidateGenerator {
        self.providers[n % self.providers.len()].as_ref()
    }
}

/// Errors raised by [`MultiModelCandidateGenerator`] construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MultiModelError {
    /// `MultiModelCandidateGenerator::new` was called with an empty pool.
    #[error("multi-model generator requires at least one provider")]
    EmptyProviderPool,
}

impl CandidateGenerator for MultiModelCandidateGenerator {
    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        corrections: &'a [VerificationFinding],
    ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
        // Allocate the call index UP FRONT so each invocation gets a
        // monotonically increasing, deterministic routing slot.
        let call_index = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let provider = self.provider_for_index(call_index);
        provider.generate(prompt, corrections)
    }

    fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
        // Deep-clone every underlying provider so the VRO-4 executor can
        // hand each spawned branch an owned, fully-isolated `'static`
        // handle. The counter is shared (Arc) so all clones observe the
        // same monotonically increasing call stream.
        let cloned_providers: Vec<Box<dyn CandidateGenerator>> =
            self.providers.iter().map(|p| p.boxed_clone()).collect();
        Box::new(Self {
            providers: cloned_providers,
            counter: std::sync::Arc::clone(&self.counter),
        })
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

    // ======================================================================
    // Directive 1a — race-aware fan-out with early-stop + cancellation
    // (VRO-9, PRD §10.6 "Branch cancellation" + "Early stopping")
    // ======================================================================

    /// Generator that yields N times then returns. Used to prove the
    /// executor's `tokio::select!` race actually cancels still-pending
    /// siblings when early-stop fires.
    struct CountingSleepyGenerator {
        label: String,
        yields: usize,
        output: StructuredOutput,
        ran_to_completion: Arc<AtomicUsize>,
    }

    impl CountingSleepyGenerator {
        fn new(label: impl Into<String>, yields: usize, output: StructuredOutput) -> Self {
            Self {
                label: label.into(),
                yields,
                output,
                ran_to_completion: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl CandidateGenerator for CountingSleepyGenerator {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [vesper_domain::VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            let output = self.output.clone();
            let label = self.label.clone();
            let yields = self.yields;
            let counter = Arc::clone(&self.ran_to_completion);
            Box::pin(async move {
                // Yield several times so the executor races branches
                // concurrently (otherwise a no-op generator resolves before
                // any sibling is even spawned).
                for _ in 0..yields {
                    tokio::task::yield_now().await;
                }
                counter.fetch_add(1, Ordering::SeqCst);
                GeneratedCandidate {
                    output: serde_json::json!({
                        "label": label,
                        "wrapped": output,
                    }),
                    cost: InferenceCost {
                        model_calls: 1,
                        total_tokens: 10,
                    },
                }
            })
        }

        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            Box::new(Self {
                label: self.label.clone(),
                yields: self.yields,
                output: self.output.clone(),
                ran_to_completion: Arc::clone(&self.ran_to_completion),
            })
        }
    }

    #[tokio::test]
    async fn early_stop_aborts_pending_sibling_branches() {
        // Two branches. Branch 0 completes first (0 yields) and matches the
        // early-stop predicate. Branch 1 would have completed later (many
        // yields) but MUST be aborted and never reach its post-yield
        // counter increment.
        let fast = CountingSleepyGenerator::new("fast", 0, serde_json::json!({"v": 1}));
        let slow = CountingSleepyGenerator::new("slow", 50, serde_json::json!({"v": 2}));
        let counter_slow = Arc::clone(&slow.ran_to_completion);

        let multi = MultiModelCandidateGenerator::new(vec![Box::new(fast), Box::new(slow)])
            .expect("two providers");
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out_with_early_stop(
                &multi,
                "compute",
                2,
                ReasoningBudget::balanced(),
                |outcome| {
                    // Early-stop fires when the candidate's JSON `label`
                    // equals "fast" (the branch we expect to finish first).
                    outcome
                        .candidate
                        .output
                        .get("label")
                        .and_then(|v| v.as_str())
                        == Some("fast")
                },
            )
            .await
            .expect("early-stop fan_out must succeed");

        // We got back at least one outcome (the fast branch).
        assert!(!outcomes.is_empty());
        // The slow branch was aborted BEFORE its generator finished — the
        // post-yield counter increment must NOT have fired.
        assert_eq!(
            counter_slow.load(Ordering::SeqCst),
            0,
            "aborted sibling must NOT reach its completion counter"
        );
        // At least one outcome carries the fast label (the early-stop trigger).
        assert!(
            outcomes.iter().any(|o| {
                o.candidate.output.get("label").and_then(|v| v.as_str()) == Some("fast")
            }),
            "early-stop outcomes must include the triggering branch"
        );
    }

    #[tokio::test]
    async fn early_stop_predicate_never_true_behaves_like_fan_out() {
        // With an always-false predicate, fan_out_with_early_stop must
        // produce identical results to fan_out: every branch completes, the
        // deterministic ordering holds, and the count matches.
        let generator = ConcurrencyCountingGenerator::new(vec![
            serde_json::json!({"a": 1}),
            serde_json::json!({"a": 2}),
            serde_json::json!({"a": 3}),
        ]);
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out_with_early_stop(
                &generator,
                "compute",
                3,
                budget(3),
                |_| false, // never early-stop
            )
            .await
            .expect("never-stop fan_out must succeed");
        assert_eq!(outcomes.len(), 3, "no early-stop must await all 3 branches");
        let ids: Vec<&str> = outcomes
            .iter()
            .map(|o| o.candidate.candidate_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["cand-0000", "cand-0001", "cand-0002"],
            "deterministic ordering must hold when no early-stop fires"
        );
    }

    #[tokio::test]
    async fn fan_out_delegates_to_early_stop_with_never_predicate() {
        // fan_out is a thin shim over fan_out_with_early_stop; this sanity
        // check proves the delegation is wired correctly.
        let generator = ConcurrencyCountingGenerator::new(vec![serde_json::json!({"v": 1})]);
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out(&generator, "compute", 1, budget(1))
            .await
            .expect("fan_out must succeed");
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].candidate.candidate_id.as_str(), "cand-0000");
    }

    // ======================================================================
    // Directive 1b — MultiModelCandidateGenerator (VRO-9, PRD §10.6
    // "Cross-model candidates")
    // ======================================================================

    /// Records which provider handled each call so tests can assert
    /// round-robin routing. Each clone gets its own call log shared via
    /// Arc<Mutex> across boxed_clones so the parent instance observes
    /// calls made by spawned branches.
    struct LabeledProvider {
        label: String,
        calls: Arc<Mutex<Vec<String>>>,
    }
    impl LabeledProvider {
        fn new(label: impl Into<String>) -> Self {
            Self {
                label: label.into(),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
        fn observed_labels(&self) -> Vec<String> {
            self.calls.lock().expect("poisoned").clone()
        }
    }
    impl CandidateGenerator for LabeledProvider {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [vesper_domain::VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            let label = self.label.clone();
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                calls.lock().expect("poisoned").push(label.clone());
                GeneratedCandidate {
                    output: serde_json::json!({"provider": label}),
                    cost: InferenceCost {
                        model_calls: 1,
                        total_tokens: 5,
                    },
                }
            })
        }
        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            Box::new(Self {
                label: self.label.clone(),
                calls: Arc::clone(&self.calls),
            })
        }
    }

    #[tokio::test]
    async fn multi_model_generator_round_robins_across_providers() {
        // Three providers; fan out 6 branches. Each branch's call should
        // route round-robin: branch 0 -> provider 0, branch 1 -> provider 1,
        // branch 2 -> provider 2, branch 3 -> provider 0, …
        let p0 = Arc::new(LabeledProvider::new("lmstudio"));
        let p1 = Arc::new(LabeledProvider::new("openai-compat"));
        let p2 = Arc::new(LabeledProvider::new("remote-api"));
        let multi = MultiModelCandidateGenerator::new(vec![
            Box::new(LabeledProviderProxy(Arc::clone(&p0))),
            Box::new(LabeledProviderProxy(Arc::clone(&p1))),
            Box::new(LabeledProviderProxy(Arc::clone(&p2))),
        ])
        .expect("three providers");
        assert_eq!(multi.provider_count(), 3);

        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out(&multi, "compute", 6, budget(6))
            .await
            .expect("fan_out must succeed");
        assert_eq!(outcomes.len(), 6);

        // Collect the per-branch provider label.
        let routed: Vec<&str> = outcomes
            .iter()
            .map(|o| {
                o.candidate
                    .output
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
            })
            .collect();
        // Each provider handled exactly 2 calls (6 / 3 round-robin).
        // The exact interleaving order is not deterministic across
        // `tokio::spawn` finish order, so we assert the COUNT per provider
        // rather than the precise sequence. Sorted by branch index inside
        // the executor's deterministic ordering, the assignment of branch
        // indices to providers IS round-robin: branch_index % 3.
        let mut lmstudio = 0;
        let mut openai = 0;
        let mut remote = 0;
        for label in &routed {
            match *label {
                "lmstudio" => lmstudio += 1,
                "openai-compat" => openai += 1,
                "remote-api" => remote += 1,
                _ => {}
            }
        }
        assert_eq!(lmstation_label_call_count(&p0, &p1, &p2), (2, 2, 2));
        assert_eq!((lmstudio, openai, remote), (2, 2, 2));

        // Round-robin routing evidence: branch 0 routed to lmstudio, branch
        // 1 to openai-compat, branch 2 to remote-api, branch 3 back to
        // lmstudio, etc. We can assert this by inspecting the per-branch
        // candidate output in spawn-order (which the executor sorts back
        // into by candidate_id).
        assert_eq!(routed[0], "lmstudio");
        assert_eq!(routed[1], "openai-compat");
        assert_eq!(routed[2], "remote-api");
        assert_eq!(routed[3], "lmstudio");
        assert_eq!(routed[4], "openai-compat");
        assert_eq!(routed[5], "remote-api");
    }

    /// Helper: returns (lmstudio, openai, remote) call counts.
    fn lmstation_label_call_count(
        p0: &Arc<LabeledProvider>,
        p1: &Arc<LabeledProvider>,
        p2: &Arc<LabeledProvider>,
    ) -> (usize, usize, usize) {
        (
            p0.observed_labels().len(),
            p1.observed_labels().len(),
            p2.observed_labels().len(),
        )
    }

    /// Thin proxy so we can construct `Box<dyn CandidateGenerator>` from an
    /// `Arc<LabeledProvider>` (the trait impl is on LabeledProvider directly;
    /// this lets the test share observation state across the multi-model
    /// generator and the test assertions).
    struct LabeledProviderProxy(Arc<LabeledProvider>);
    impl CandidateGenerator for LabeledProviderProxy {
        fn generate<'a>(
            &'a self,
            prompt: &'a str,
            corrections: &'a [vesper_domain::VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            self.0.generate(prompt, corrections)
        }
        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            Box::new(LabeledProviderProxy(Arc::clone(&self.0)))
        }
    }

    #[test]
    fn multi_model_generator_rejects_empty_pool() {
        let err = MultiModelCandidateGenerator::new(Vec::new()).expect_err("empty pool must error");
        assert_eq!(err, MultiModelError::EmptyProviderPool);
    }

    #[tokio::test]
    async fn multi_model_generator_distributes_calls_deterministically_under_clone() {
        // boxed_clone must preserve the round-robin counter so two clones
        // observing independent calls see globally-monotonic routing.
        let p0 = Arc::new(LabeledProvider::new("a"));
        let p1 = Arc::new(LabeledProvider::new("b"));
        let multi = MultiModelCandidateGenerator::new(vec![
            Box::new(LabeledProviderProxy(Arc::clone(&p0))),
            Box::new(LabeledProviderProxy(Arc::clone(&p1))),
        ])
        .expect("two providers");

        let clone = multi.boxed_clone();
        // Two calls on the original, then two on the clone — counter is
        // shared, so the routing is a:0, b:1, a:2, b:3 across both.
        let _ = multi.generate("p", &[]).await;
        let _ = multi.generate("p", &[]).await;
        let _ = clone.generate("p", &[]).await;
        let _ = clone.generate("p", &[]).await;

        // Provider "a" handled calls 0 and 2; provider "b" handled 1 and 3.
        assert_eq!(p0.observed_labels().len(), 2);
        assert_eq!(p1.observed_labels().len(), 2);
    }

    // ======================================================================
    // VRO-10 — candidate-specific branch prompts (§10.6).
    //
    // The directive: each parallel branch must receive a distinct prompt
    // prefix so the candidates follow heterogeneous reasoning paths. The
    // `BranchDiversification` enum captures the strategy;
    // `fan_out_diverse` applies it.
    // ======================================================================

    #[test]
    fn branch_diversification_default_is_none() {
        let d = BranchDiversification::default();
        assert!(matches!(d, BranchDiversification::None));
        assert_eq!(d.prompt_prefix_for(0), None);
        assert_eq!(d.prompt_prefix_for(7), None);
    }

    #[test]
    fn branch_diversification_diverse_branches_has_four_variants() {
        let d = BranchDiversification::diverse_branches();
        // Each of the four canonical stances (conservative / balanced /
        // creative / highly creative) is present.
        let p0 = d.prompt_prefix_for(0).expect("branch 0 has a prefix");
        let p1 = d.prompt_prefix_for(1).expect("branch 1 has a prefix");
        let p2 = d.prompt_prefix_for(2).expect("branch 2 has a prefix");
        let p3 = d.prompt_prefix_for(3).expect("branch 3 has a prefix");
        assert!(p0.to_ascii_lowercase().contains("conservative"));
        assert!(p1.to_ascii_lowercase().contains("balanced"));
        assert!(p2.to_ascii_lowercase().contains("creative"));
        assert!(p3.to_ascii_lowercase().contains("highly creative"));
        // Index ≥ 4 wraps modulo 4 — branch 4 == branch 0's prefix.
        assert_eq!(d.prompt_prefix_for(4), Some(p0));
        assert_eq!(d.prompt_prefix_for(5), Some(p1));
    }

    #[test]
    fn branch_diversification_empty_variants_returns_none() {
        let d = BranchDiversification::SystemPromptVariants(Vec::new());
        assert_eq!(d.prompt_prefix_for(0), None);
    }

    #[tokio::test]
    async fn fan_out_diverse_injects_distinct_prompt_prefix_per_branch() {
        // Each branch records the prompt it received into the shared seen
        // log. fan_out_diverse with 4-variant diversification must produce
        // 4 distinct prompts (one per branch), each prefixed with the
        // corresponding variant.
        struct RecordingGenerator {
            seen: Arc<Mutex<Vec<String>>>,
        }
        impl CandidateGenerator for RecordingGenerator {
            fn generate<'a>(
                &'a self,
                prompt: &'a str,
                _corrections: &'a [vesper_domain::VerificationFinding],
            ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
                let seen = Arc::clone(&self.seen);
                Box::pin(async move {
                    seen.lock().expect("poisoned").push(prompt.to_string());
                    GeneratedCandidate {
                        output: serde_json::json!({"v": 1}),
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

        let generator = RecordingGenerator {
            seen: Arc::new(Mutex::new(Vec::new())),
        };
        let executor = CandidateExecutor::new();
        let outcomes = executor
            .fan_out_diverse(
                &generator,
                "the-base-prompt",
                4,
                ReasoningBudget {
                    max_parallel_branches: 4,
                    ..ReasoningBudget::balanced()
                },
                BranchDiversification::diverse_branches(),
                |_| false,
            )
            .await
            .expect("fan_out_diverse must succeed");

        assert_eq!(outcomes.len(), 4);
        let seen = generator.seen.lock().expect("poisoned").clone();
        assert_eq!(seen.len(), 4, "exactly 4 prompts recorded");
        // Every seen prompt contains the base prompt.
        assert!(
            seen.iter().all(|p| p.contains("the-base-prompt")),
            "every prompt must include the base: {seen:?}"
        );
        // The four prompts are pairwise distinct (each carries a different
        // variant prefix).
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            4,
            "all 4 branch prompts must be pairwise distinct: {seen:?}"
        );
        // Each variant prefix appears in exactly one branch's prompt.
        let prefixes = ["conservative", "balanced", "creative", "highly creative"];
        for prefix in prefixes {
            assert!(
                seen.iter().any(|p| p.to_ascii_lowercase().contains(prefix)),
                "no branch received the `{prefix}` prefix: {seen:?}"
            );
        }
    }

    #[tokio::test]
    async fn fan_out_diverse_with_no_diversification_is_byte_identical_to_fan_out() {
        // Regression guard: when diversification is None, fan_out_diverse
        // produces the same prompts as fan_out — every branch sees the
        // identical base prompt.
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
                        output: serde_json::json!({"v": 1}),
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
        let _ = executor
            .fan_out_diverse(
                &generator,
                "the-base-prompt",
                3,
                ReasoningBudget {
                    max_parallel_branches: 3,
                    ..ReasoningBudget::balanced()
                },
                BranchDiversification::None,
                |_| false,
            )
            .await
            .expect("fan_out_diverse must succeed");
        let seen = generator.seen.lock().expect("poisoned").clone();
        // Every branch saw the identical base prompt — no prefix injected.
        assert!(
            seen.iter().all(|p| p == "the-base-prompt"),
            "with no diversification every prompt must equal the base: {seen:?}"
        );
    }
}
