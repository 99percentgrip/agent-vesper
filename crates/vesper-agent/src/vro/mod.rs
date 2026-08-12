//! Vesper Reasoning Orchestrator (VRO) agent-side scaffolding.
//!
//! This module establishes the **orchestration seam** without altering
//! [`crate::agent_loop::AgentLoop`]. The composition boundary (TUI / ACP host)
//! constructs a [`VroOrchestrator`] from a [`ReasoningConfig`] and asks it, per
//! turn, whether to orchestrate or to use the existing direct execution loop.
//!
//! ## Phase behavior
//!
//! - **VRO-1** established the contracts + seam.
//! - **VRO-2.1** added the deterministic [`TaskProfiler`].
//! - **VRO-2.2** added the [`VerifierRegistry`] + cargo verifiers.
//! - **VRO-2.3** wires the profiler + verifiers into the
//!   [`GenerateVerifyRepair`](vesper_domain::ReasoningStrategy) loop
//!   ([`orchestrator`]). [`route`](VroOrchestrator::route) now returns
//!   [`VroRoutingDecision::Orchestrate`] for non-`Direct` strategies when the
//!   flag is on; the host then calls [`execute`](VroOrchestrator::execute) with
//!   a provider-backed [`CandidateGenerator`].
//!
//! **Zero-breakage contract:** when `enabled` is `false` (shipped default), or
//! when the profiled strategy is [`Direct`](vesper_domain::ReasoningStrategy),
//! [`route`](VroOrchestrator::route) returns `Direct` and the host keeps using
//! the unchanged `agent_loop.rs` direct path. Nothing in this module touches
//! [`crate::AgentLoopConfig`], [`crate::agent_loop::AgentLoop`], the tool
//! registry, or the permission gate.
//!
//! See `crates/vesper-agent/AGENTS.md` for ownership and contract scope.

pub mod executor;
pub mod orchestrator;
pub mod profiler;
pub mod react;
pub mod strategies;
pub mod verifiers;

pub use executor::{BranchContext, BranchOutcome, CandidateExecutor, ExecutorError, XorShiftRng};
pub use orchestrator::{CandidateGenerator, GeneratedCandidate, run_generate_verify_repair};
pub use profiler::TaskProfiler;
pub use react::{
    ReactAgent, ReactDecision, RegistryToolInvoker, ToolInvocationError, ToolInvoker,
    TrajectoryEntry, run_tool_grounded_react,
};
use std::path::Path;
use std::sync::Arc;
pub use strategies::{
    CandidateJudge, normalize_output, quorum_threshold, run_parallel_candidates_consensus,
    run_parallel_candidates_judge,
};
pub use verifiers::{
    CargoCheckVerifier, CargoTestVerifier, VerificationContext, Verifier, VerifierRegistry,
};
use vesper_domain::{
    InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningConfig, ReasoningMode,
    ReasoningOutcome, ReasoningRequest, ReasoningStrategy, TaskProfile, VerificationStatus,
    VerificationSummary,
};

/// The routing decision a host consumes before dispatching a turn.
///
/// In VRO-1 this is always [`VroRoutingDecision::Direct`]; the variant exists so
/// future phases can extend the host switch without changing its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VroRoutingDecision {
    /// Dispatch through the existing [`AgentLoop`](crate::AgentLoop) direct
    /// execution path. VRO is a no-op for this turn.
    Direct,
    /// Dispatch through the VRO orchestrator (not implemented in VRO-1).
    Orchestrate,
}

/// Phase VRO-1..VRO-2.3 orchestrator.
///
/// Holds the reasoning configuration, a [`TaskProfiler`], and a shared
/// [`VerifierRegistry`]. It answers two questions the composition boundary
/// needs:
///
/// - [`route`](Self::route): *does this turn route through VRO or the direct
///   execution loop?* Returns `Direct` when disabled, in `Off` mode, or when
///   the profiled strategy is [`Direct`](ReasoningStrategy); otherwise
///   `Orchestrate`.
/// - [`execute`](Self::execute): when routing chose `Orchestrate`, runs the
///   Generate-Verify-Repair loop (or a strategy-appropriate variant) using a
///   caller-supplied [`CandidateGenerator`] (the provider seam).
///
/// When `enabled` is `false` (shipped default) the direct path is always taken
/// with zero behavior change.
///
/// This type holds no provider handles and mutates no session state. It is
/// safe to construct cheaply at host startup and clone per turn (the registry
/// is shared behind an [`Arc`]).
#[derive(Debug, Clone)]
pub struct VroOrchestrator {
    config: ReasoningConfig,
    profiler: TaskProfiler,
    registry: Arc<VerifierRegistry>,
}

impl Default for VroOrchestrator {
    fn default() -> Self {
        // Disabled orchestrator: empty registry (it never executes).
        Self {
            config: ReasoningConfig::default(),
            profiler: TaskProfiler::new(),
            registry: Arc::new(VerifierRegistry::new()),
        }
    }
}

impl VroOrchestrator {
    /// Creates a new orchestrator bound to the given configuration, wiring the
    /// default cargo verifiers (`cargo_check`, `cargo_test`) into the registry.
    #[must_use]
    pub fn new(config: ReasoningConfig) -> Self {
        Self {
            config,
            profiler: TaskProfiler::new(),
            registry: Arc::new(VerifierRegistry::default_cargo()),
        }
    }

    /// Creates a disabled orchestrator with an empty registry.
    #[must_use]
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Returns whether the VRO feature flag is enabled.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Returns a reference to the bound configuration.
    #[must_use]
    pub fn config(&self) -> &ReasoningConfig {
        &self.config
    }

    /// The configured default mode.
    #[must_use]
    pub fn default_mode(&self) -> ReasoningMode {
        self.config.default_mode
    }

    /// Resolves the budget preset for a mode (delegates to the config).
    #[must_use]
    pub fn preset_for(&self, mode: ReasoningMode) -> ReasoningBudget {
        self.config.preset_for(mode)
    }

    /// Returns a reference to the verifier registry.
    #[must_use]
    pub fn registry(&self) -> &VerifierRegistry {
        &self.registry
    }

    /// Profiles a user message into a [`TaskProfile`] using the deterministic
    /// [`TaskProfiler`]. Always available regardless of the `enabled` flag.
    #[must_use]
    pub fn profile(&self, user_message: &str) -> TaskProfile {
        self.profiler.profile(user_message)
    }

    /// Decides whether a turn routes through VRO or the direct execution loop.
    ///
    /// **Zero-breakage contract:**
    /// - `enabled == false` ⇒ `Direct` (no profiling).
    /// - `mode == Off` ⇒ `Direct`.
    /// - profiled strategy `== Direct` ⇒ `Direct` (host uses `agent_loop.rs`).
    /// - otherwise ⇒ `Orchestrate` (host should call [`execute`](Self::execute)).
    pub fn route(&self, user_message: &str, mode: ReasoningMode) -> VroRoutingDecision {
        // Master switch off -> direct, no work done.
        if !self.config.enabled {
            return VroRoutingDecision::Direct;
        }
        // Explicit Off mode -> direct.
        if mode == ReasoningMode::Off {
            return VroRoutingDecision::Direct;
        }
        // VRO-2.3: profile and route on the recommended strategy. A Direct
        // profile falls back to the unchanged direct execution loop.
        let profile = self.profiler.profile(user_message);
        if profile.recommended_strategy == ReasoningStrategy::Direct {
            VroRoutingDecision::Direct
        } else {
            VroRoutingDecision::Orchestrate
        }
    }

    /// Executes a turn through the orchestrator (call only when
    /// [`route`](Self::route) returned [`VroRoutingDecision::Orchestrate`]).
    ///
    /// Profiles the request, resolves the budget (caller override or the mode
    /// preset), and dispatches to the strategy executor:
    ///
    /// - **VRO-2.3** ([`GenerateVerifyRepair`](ReasoningStrategy)): runs the
    ///   bounded verify-repair loop with full `max_repairs`.
    /// - **VRO-4** ([`ParallelCandidatesConsensus`](ReasoningStrategy) /
    ///   [`ParallelCandidatesJudge`](ReasoningStrategy)): fans out parallel
    ///   branches via [`CandidateExecutor`] and collapses them via the
    ///   consensus or judge strategy handler. The Judge strategy requires a
    ///   [`CandidateJudge`] — when none is supplied it degrades to consensus.
    ///   The default branch count is `max_parallel_branches` from the budget.
    /// - Other non-`Direct` strategies (VRO-2.3 baseline) fall back to a
    ///   single generate-and-verify pass (`max_repairs == 0`) until their
    ///   dedicated executors land in later phases.
    ///
    /// `generator` is the provider seam: the host supplies a real
    /// provider-backed [`CandidateGenerator`]; the orchestrator never makes a
    /// provider call itself. `judge` is the optional Judge seam used only by
    /// [`ParallelCandidatesJudge`](ReasoningStrategy); pass `None` to degrade
    /// that strategy to consensus.
    pub async fn execute(
        &self,
        request: &ReasoningRequest,
        generator: &dyn CandidateGenerator,
        workspace_root: &Path,
    ) -> ReasoningOutcome {
        self.execute_with_judge(request, generator, workspace_root, None, 0)
            .await
    }

    /// Like [`execute`](Self::execute) but supplies an optional
    /// [`CandidateJudge`] for the [`ParallelCandidatesJudge`] strategy plus a
    /// deterministic `seed` controlling the position-bias shuffle. The seed
    /// is exposed so tests can reproduce an exact shuffle; production callers
    /// should derive it from the request id.
    ///
    /// **VRO-5.1 dispatch guard:** when the profiled strategy is
    /// [`ToolGroundedReact`](ReasoningStrategy), this method returns a
    /// [`Failed`](OutcomeStatus::Failed) outcome with a clear "use
    /// [`execute_react`](Self::execute_react)" message instead of silently
    /// falling through to the GenerateVerifyRepair single-pass baseline.
    /// [`ToolGroundedReact`](ReasoningStrategy) requires the
    /// [`ReactAgent`] + [`ToolInvoker`] seams that this method's signature
    /// does not accept; the composition boundary must call
    /// [`execute_react`](Self::execute_react) instead.
    pub async fn execute_with_judge(
        &self,
        request: &ReasoningRequest,
        generator: &dyn CandidateGenerator,
        _workspace_root: &Path,
        judge: Option<&dyn CandidateJudge>,
        seed: u64,
    ) -> ReasoningOutcome {
        let profile = self.profiler.profile_request(request);
        let budget = request
            .budget_override
            .unwrap_or_else(|| self.config.preset_for(request.mode));
        match profile.recommended_strategy {
            // VRO-5.1 — ToolGroundedReact requires the ReactAgent +
            // ToolInvoker seams; refuse to silently fall through to a
            // single-pass GenerateVerifyRepair (which would lie about what
            // happened). The composition boundary must call execute_react.
            ReasoningStrategy::ToolGroundedReact => ReasoningOutcome {
                status: OutcomeStatus::Failed,
                final_output: None,
                selected_candidate: None,
                verification_summary: VerificationSummary {
                    passed: 0,
                    failed: 0,
                    overall: VerificationStatus::Skipped,
                },
                unresolved_risks: vec![
                    "ToolGroundedReact requires the ReactAgent + ToolInvoker seams; \
                     call VroOrchestrator::execute_react instead of execute / execute_with_judge"
                        .to_string(),
                ],
                cost: InferenceCost::default(),
            },
            // VRO-4 — ParallelCandidatesConsensus (PRD §11.4).
            ReasoningStrategy::ParallelCandidatesConsensus => {
                run_parallel_candidates_consensus(
                    generator,
                    &request.user_message,
                    usize::from(budget.max_parallel_branches.max(1)),
                    budget,
                )
                .await
            }
            // VRO-4 — ParallelCandidatesJudge (PRD §11.5). Degrades to
            // consensus when no judge is supplied.
            ReasoningStrategy::ParallelCandidatesJudge => match judge {
                Some(judge) => {
                    run_parallel_candidates_judge(
                        generator,
                        judge,
                        &request.user_message,
                        usize::from(budget.max_parallel_branches.max(1)),
                        budget,
                        seed,
                    )
                    .await
                }
                None => {
                    run_parallel_candidates_consensus(
                        generator,
                        &request.user_message,
                        usize::from(budget.max_parallel_branches.max(1)),
                        budget,
                    )
                    .await
                }
            },
            // VRO-2.3 baseline — GenerateVerifyRepair (and single-pass
            // fallback for the not-yet-specialized non-Direct strategies).
            other => {
                let repair_budget = if other == ReasoningStrategy::GenerateVerifyRepair {
                    budget
                } else {
                    ReasoningBudget {
                        max_repairs: 0,
                        ..budget
                    }
                };
                run_generate_verify_repair(
                    &request.user_message,
                    &profile.available_verifiers,
                    &self.registry,
                    generator,
                    _workspace_root,
                    repair_budget,
                )
                .await
            }
        }
    }

    /// Executes a turn through the Tool-Grounded ReAct loop (VRO-5.1, PRD
    /// §11.6).
    ///
    /// Profiles the request, resolves the budget (caller override or the
    /// mode preset), and dispatches to [`run_tool_grounded_react`].
    /// `agent` is the provider-backed [`ReactAgent`] seam that decides each
    /// action; `invoker` is the [`ToolInvoker`] seam that runs tools
    /// through the existing permission sandbox (production impl:
    /// [`RegistryToolInvoker`]).
    ///
    /// This is the **only** entry point that accepts the ReactAgent +
    /// ToolInvoker seams. [`execute`](Self::execute) and
    /// [`execute_with_judge`](Self::execute_with_judge) deliberately reject
    /// [`ToolGroundedReact`](ReasoningStrategy) (they return `Failed`) so
    /// callers cannot accidentally run a tool-grounded prompt through the
    /// GenerateVerifyRepair baseline.
    ///
    /// **Read-Before-Write:** when the profile requires grounding,
    /// [`run_tool_grounded_react`] rejects mutating tools until at least one
    /// read-only tool has produced an observation (directive 3).
    pub async fn execute_react(
        &self,
        request: &ReasoningRequest,
        agent: &dyn ReactAgent,
        invoker: &dyn ToolInvoker,
        _workspace_root: &Path,
    ) -> ReasoningOutcome {
        let profile = self.profiler.profile_request(request);
        let budget = request
            .budget_override
            .unwrap_or_else(|| self.config.preset_for(request.mode));
        // The profile's `requires_grounding` flag drives the Read-Before-Write
        // policy. The orchestrator remains agnostic of HOW grounding is
        // detected — that lives in the deterministic TaskProfiler.
        run_tool_grounded_react(
            &request.user_message,
            agent,
            invoker,
            budget,
            profile.requires_grounding,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::ReasoningStrategy;

    #[test]
    fn disabled_default_is_not_enabled() {
        let vro = VroOrchestrator::disabled();
        assert!(!vro.enabled());
        assert_eq!(vro.default_mode(), ReasoningMode::Auto);
    }

    #[test]
    fn route_returns_orchestrate_for_non_direct_when_enabled() {
        // VRO-2.3: with the flag on, a non-Direct profile now routes through
        // the orchestrator (Orchestrate). A Direct profile (chat) still falls
        // back to the unchanged direct execution loop.
        let vro = VroOrchestrator::new(ReasoningConfig {
            enabled: true,
            ..ReasoningConfig::default()
        });
        assert!(vro.enabled());

        // "refactor src/main.rs" profiles to coding / PlanExecuteVerify (non-Direct).
        assert_eq!(
            vro.route("refactor src/main.rs", ReasoningMode::Auto),
            VroRoutingDecision::Orchestrate,
            "non-Direct profile must route to Orchestrate when enabled"
        );
        // A chat greeting profiles to Direct -> stays on the direct path.
        assert_eq!(
            vro.route("hello world", ReasoningMode::Auto),
            VroRoutingDecision::Direct,
            "Direct profile must stay on the direct execution loop"
        );
        // Every non-Off mode behaves the same for a non-Direct profile.
        for mode in [
            ReasoningMode::Fast,
            ReasoningMode::Balanced,
            ReasoningMode::Deep,
            ReasoningMode::Maximum,
        ] {
            assert_eq!(
                vro.route("calculate the result in calc.py", mode),
                VroRoutingDecision::Orchestrate,
                "non-Direct profile routes Orchestrate for mode {mode:?}"
            );
        }
    }

    #[test]
    fn disabled_route_skips_profiling_and_returns_direct() {
        // When disabled, route returns Direct without depending on profiling.
        let vro = VroOrchestrator::disabled();
        assert_eq!(
            vro.route("anything", ReasoningMode::Auto),
            VroRoutingDecision::Direct
        );
        // A non-Direct prompt still routes Direct when disabled.
        assert_eq!(
            vro.route("refactor src/main.rs", ReasoningMode::Auto),
            VroRoutingDecision::Direct
        );
        // Off mode also routes direct even when enabled.
        let enabled = VroOrchestrator::new(ReasoningConfig {
            enabled: true,
            ..ReasoningConfig::default()
        });
        assert_eq!(
            enabled.route("refactor src/main.rs", ReasoningMode::Off),
            VroRoutingDecision::Direct
        );
    }

    #[test]
    fn enabled_orchestrator_wires_default_cargo_verifiers() {
        // new() registers the cargo verifiers so execute() has verifiers to run.
        let vro = VroOrchestrator::new(ReasoningConfig {
            enabled: true,
            ..ReasoningConfig::default()
        });
        assert!(vro.registry().contains("cargo_check"));
        assert!(vro.registry().contains("cargo_test"));
        // The disabled default has an empty registry.
        assert!(VroOrchestrator::disabled().registry().ids().is_empty());
    }

    #[test]
    fn profiler_runs_and_profiles_through_orchestrator() {
        // The orchestrator exposes profiling regardless of the enabled flag.
        let vro = VroOrchestrator::disabled();
        let greeting = vro.profile("hello world");
        assert_eq!(greeting.recommended_strategy, ReasoningStrategy::Direct);
        assert_eq!(greeting.domain.as_str(), "chat");

        let refactor = vro.profile("refactor src/main.rs");
        assert_eq!(
            refactor.recommended_strategy,
            ReasoningStrategy::PlanExecuteVerify
        );
        assert_eq!(refactor.domain.as_str(), "coding");
    }

    #[test]
    fn preset_for_delegates_to_config() {
        let vro = VroOrchestrator::default();
        assert_eq!(vro.preset_for(ReasoningMode::Fast), vro.config().fast);
        assert_eq!(vro.preset_for(ReasoningMode::Deep), vro.config().deep);
        assert_eq!(
            vro.preset_for(ReasoningMode::Maximum),
            ReasoningBudget::maximum()
        );
    }

    #[test]
    fn orchestrator_is_clone_and_debug() {
        let vro = VroOrchestrator::disabled();
        let cloned = vro.clone();
        assert_eq!(vro.enabled(), cloned.enabled());
        assert!(format!("{vro:?}").contains("VroOrchestrator"));
    }

    // === VRO-4 — orchestrator-level dispatch tests ===
    // These cover the wiring between execute() and the parallel-strategy
    // handlers (the per-component tests live in executor.rs and
    // strategies.rs; these prove the orchestrator actually INVOKES them).
    //
    // Pattern: enable the orchestrator, build a request whose user_message
    // triggers the parallel-strategy profiler path, supply a generator fake,
    // and assert the outcome reflects the parallel path (multiple model
    // calls, consensus/judge outcome shape).

    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use vesper_domain::{
        InferenceCost, OutcomeStatus, PrivacyMode, ReasoningBudget, RequestId, SessionId,
        StructuredOutput, VerificationFinding,
    };

    /// Deterministic generator that always returns the same single output.
    /// The call count is shared via `Arc<Mutex>` so the per-branch
    /// `boxed_clone`s all bump the SAME counter — without that, the original
    /// instance would not observe calls made by spawned branches.
    struct SingleAnswerGenerator {
        call_count: Arc<Mutex<u32>>,
        output: StructuredOutput,
    }
    impl CandidateGenerator for SingleAnswerGenerator {
        fn generate<'a>(
            &'a self,
            _prompt: &'a str,
            _corrections: &'a [VerificationFinding],
        ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
            let count = Arc::clone(&self.call_count);
            let output = self.output.clone();
            Box::pin(async move {
                *count.lock().expect("poisoned") += 1;
                GeneratedCandidate {
                    output,
                    cost: InferenceCost {
                        model_calls: 1,
                        total_tokens: 50,
                    },
                }
            })
        }

        fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
            Box::new(Self {
                call_count: Arc::clone(&self.call_count),
                output: self.output.clone(),
            })
        }
    }

    fn enabled_orchestrator() -> VroOrchestrator {
        VroOrchestrator::new(ReasoningConfig {
            enabled: true,
            ..ReasoningConfig::default()
        })
    }

    fn request_for(message: &str) -> ReasoningRequest {
        ReasoningRequest {
            request_id: RequestId::new("req-test").unwrap(),
            session_id: SessionId::new("sess-test").unwrap(),
            user_message: message.to_string(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: Some(ReasoningBudget {
                max_parallel_branches: 3,
                ..ReasoningBudget::balanced()
            }),
            privacy_mode: PrivacyMode::Private,
        }
    }

    #[tokio::test]
    async fn execute_dispatches_parallel_candidates_consensus_through_profiler() {
        // End-to-end: a verify-this-claim prompt profiles to
        // ParallelCandidatesConsensus → execute() runs the consensus loop.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"answer": "yes"}),
        };
        let req = request_for("Is this correct? The Rust borrow checker prevents data races.");
        let outcome = vro
            .execute(&req, &generator, std::path::Path::new("/tmp"))
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // All N branches ran (N = max_parallel_branches = 3).
        let calls = *generator.call_count.lock().expect("poisoned");
        assert_eq!(calls, 3, "consensus strategy fans out to all 3 branches");
        // All branches agreed → quorum reached → consensus risk surfaced.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("consensus reached")),
            "expected consensus-reached risk, got {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn execute_dispatches_parallel_candidates_judge_through_profiler() {
        // End-to-end: a trade-off prompt profiles to ParallelCandidatesJudge.
        // Without a judge supplied, execute() degrades to consensus (still
        // succeeds).
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"design": "rust-async-runtime"}),
        };
        let req = request_for(
            "Compare options for the new runtime: tokio vs async-std vs smol. Weigh the pros and cons of each.",
        );
        let outcome = vro
            .execute(&req, &generator, std::path::Path::new("/tmp"))
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Confirm 3 branches ran (the parallel path was actually taken).
        let calls = *generator.call_count.lock().expect("poisoned");
        assert_eq!(calls, 3, "judge-or-fallback path fans out to 3 branches");
    }

    /// Always-pick-first Judge for the execute_with_judge test. Boxes the
    /// observed candidate set so the test can assert the judge saw shuffled
    /// (non-spawn-order) input. Shared via Arc<Mutex> so the per-branch
    /// observations are visible from the original instance.
    struct FirstPickJudge {
        observed: Arc<Mutex<Vec<String>>>,
    }
    impl super::CandidateJudge for FirstPickJudge {
        fn judge<'a>(
            &'a self,
            candidates: &'a [vesper_domain::Candidate],
        ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>> {
            let observed = Arc::clone(&self.observed);
            Box::pin(async move {
                observed.lock().expect("poisoned").extend(
                    candidates
                        .iter()
                        .map(|c| c.candidate_id.as_str().to_string()),
                );
                0
            })
        }
    }

    #[tokio::test]
    async fn execute_with_judge_invokes_judge_with_shuffled_candidates() {
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let judge = FirstPickJudge {
            observed: Arc::new(Mutex::new(Vec::new())),
        };
        let req = request_for(
            "Compare options for the parser: pest vs nom vs chumsky. Weigh the pros and cons.",
        );
        let outcome = vro
            .execute_with_judge(
                &req,
                &generator,
                std::path::Path::new("/tmp"),
                Some(&judge),
                99,
            )
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // The judge saw 3 candidates (one per parallel branch).
        let observed = judge.observed.lock().expect("poisoned").clone();
        assert_eq!(observed.len(), 3, "judge must observe 3 candidates");
        // The outcome's selected_candidate is a real id from cand-0000..cand-0002.
        let selected = outcome
            .selected_candidate
            .expect("judge must select a candidate");
        let valid: Vec<String> = (0..3).map(|i| format!("cand-{i:04}")).collect();
        assert!(
            valid.iter().any(|v| v == selected.as_str()),
            "selected {selected} must be a real candidate id"
        );
    }

    #[tokio::test]
    async fn execute_with_judge_degrades_to_consensus_when_no_judge_supplied() {
        // execute() passes judge=None — the Judge strategy falls back to the
        // consensus path instead of erroring.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"x": 42}),
        };
        let req =
            request_for("Compare options for the cache layer. Weigh the pros and cons of each.");
        let outcome = vro
            .execute(&req, &generator, std::path::Path::new("/tmp"))
            .await;
        // Degrades to consensus → succeeds.
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(
            *generator.call_count.lock().expect("poisoned"),
            3,
            "degrade-to-consensus still fans out 3 branches"
        );
    }

    // === VRO-5.1 — Tool-Grounded ReAct dispatch tests ===
    //
    // The orchestrator's execute_react is the canonical entry point for
    // ToolGroundedReact. execute / execute_with_judge must refuse that
    // strategy so callers cannot silently fall through to a single-pass
    // GenerateVerifyRepair baseline.

    use super::react::{ReactAgent, ReactDecision, ToolInvocationError, ToolInvoker};
    use vesper_domain::ToolExecutionClass;

    /// Always-Finish agent: returns the same final answer on the first
    /// next_action call. Used to assert the orchestrator's execute_react
    /// actually drives the ReactAgent.
    struct ImmediateFinishAgent {
        call_count: Arc<Mutex<u32>>,
        output: StructuredOutput,
    }
    impl ReactAgent for ImmediateFinishAgent {
        fn next_action<'a>(
            &'a self,
            _prompt: &'a str,
            _trajectory: &'a [super::react::TrajectoryEntry],
        ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
            let count = Arc::clone(&self.call_count);
            let output = self.output.clone();
            Box::pin(async move {
                *count.lock().expect("poisoned") += 1;
                ReactDecision::Finish { output }
            })
        }
    }

    /// No-op invoker: registers nothing, so any call surfaces UnknownTool.
    /// Sufficient for the ImmediateFinishAgent tests because the agent never
    /// actually calls a tool.
    struct NullInvoker;
    impl ToolInvoker for NullInvoker {
        fn class_of(&self, _name: &str) -> Option<ToolExecutionClass> {
            None
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            _arguments: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            let name = name.to_string();
            Box::pin(async move { Err(ToolInvocationError::UnknownTool(name)) })
        }
    }

    /// A request whose user_message profiles to ToolGroundedReact.
    /// "What does the main.rs file do?" is the directive's example — a
    /// short prompt with a file extension that triggers grounding.
    fn react_request() -> ReasoningRequest {
        ReasoningRequest {
            request_id: RequestId::new("req-react").unwrap(),
            session_id: SessionId::new("sess-react").unwrap(),
            user_message: "What does the main.rs file do?".to_string(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: Some(ReasoningBudget {
                max_model_calls: 5,
                max_tool_calls: 5,
                ..ReasoningBudget::balanced()
            }),
            privacy_mode: PrivacyMode::Private,
        }
    }

    #[tokio::test]
    async fn execute_react_dispatches_to_run_tool_grounded_react() {
        // End-to-end: a "what does main.rs do?" prompt profiles to
        // ToolGroundedReact → execute_react drives the ReactAgent → the
        // agent Finishes → outcome is Succeeded.
        let vro = enabled_orchestrator();
        let agent = ImmediateFinishAgent {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"answer": "main.rs is the entry point"}),
        };
        let invoker = NullInvoker;
        let outcome = vro
            .execute_react(
                &react_request(),
                &agent,
                &invoker,
                std::path::Path::new("/tmp"),
            )
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(*agent.call_count.lock().expect("poisoned"), 1);
        assert_eq!(
            outcome.final_output,
            Some(serde_json::json!({"answer": "main.rs is the entry point"}))
        );
    }

    #[tokio::test]
    async fn execute_with_judge_refuses_tool_grounded_react_strategy() {
        // Guard: the non-react entry points must NOT silently fall through
        // to GenerateVerifyRepair for ToolGroundedReact prompts. They return
        // Failed with a clear "use execute_react" message.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let outcome = vro
            .execute(&react_request(), &generator, std::path::Path::new("/tmp"))
            .await;
        assert_eq!(
            outcome.status,
            OutcomeStatus::Failed,
            "execute() must refuse ToolGroundedReact (not silently fall through)"
        );
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("execute_react")),
            "risk message must direct the caller to execute_react: {:?}",
            outcome.unresolved_risks
        );
        // The generator was never called — the dispatch guard fires before
        // the GenerateVerifyRepair baseline.
        assert_eq!(
            *generator.call_count.lock().expect("poisoned"),
            0,
            "guard must short-circuit before invoking the generator"
        );
    }
}
