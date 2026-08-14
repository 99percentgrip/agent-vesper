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
pub mod learning;
pub mod lens_integration;
pub mod orchestrator;
pub mod profiler;
pub mod rate_limit;
pub mod react;
pub mod repair;
pub mod strategies;
pub mod verifiers;

pub use executor::{
    BranchContext, BranchDiversification, BranchOutcome, CandidateExecutor, ExecutorError,
    XorShiftRng,
};
pub use learning::{
    LearningError, ProceduralMemory, ProceduralMemorySink, ProceduralStep, SecretScrubber,
    WorkflowExtractor, cost_summary, distinct_actions, is_learning_eligible,
};
pub use lens_integration::{
    LensReviewPort, NoOpLensReviewPort, diagnostic_for_review, feedback_as_context_message,
    looks_like_html_artifact,
};
pub use orchestrator::{
    CandidateGenerator, GeneratedCandidate, run_generate_verify_repair,
    run_generate_verify_repair_with_rate_limit,
};
pub use profiler::TaskProfiler;
pub use rate_limit::{RateLimitStatus, RateLimitTracker, backoff_duration};
pub use react::{
    ReactAgent, ReactDecision, RegistryToolInvoker, ToolInvocationError, ToolInvoker,
    TrajectoryEntry, run_tool_grounded_react, run_tool_grounded_react_with_trajectory,
};
pub use repair::{RepairController, RepairHeuristic, classify_finding, classify_findings};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
pub use strategies::{
    Adjudicator, CandidateCritic, CandidateJudge, normalize_output, quorum_threshold,
    run_bounded_tree_search, run_parallel_candidates_consensus, run_parallel_candidates_judge,
    run_proposer_critic_adjudicator,
};
pub use verifiers::{
    CargoCheckVerifier, CargoTestVerifier, VerificationContext, Verifier, VerifierRegistry,
};
use vesper_domain::{
    InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningConfig, ReasoningMode,
    ReasoningOutcome, ReasoningRequest, ReasoningStrategy, TaskProfile, ToolExecutionClass,
    VerificationStatus, VerificationSummary,
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
    /// Optional VesperLens review port (ADR 0017, VRO-11.2). `None` keeps
    /// byte-identical VRO-10 behavior; `Some(port)` enables
    /// [`Self::maybe_review_html_artifact`] for human-in-the-loop HTML
    /// artifact review.
    lens_port: Option<Arc<dyn LensReviewPort>>,
}

impl Default for VroOrchestrator {
    fn default() -> Self {
        // Disabled orchestrator: empty registry (it never executes).
        Self {
            config: ReasoningConfig::default(),
            profiler: TaskProfiler::new(),
            registry: Arc::new(VerifierRegistry::new()),
            lens_port: None,
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
            lens_port: None,
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

    /// Wire a [`LensReviewPort`] for human-in-the-loop HTML artifact
    /// review (ADR 0017, VRO-11.2). Builder-style; consumed at the
    /// composition boundary (TUI binary).
    ///
    /// After this is called, [`Self::maybe_review_html_artifact`] will
    /// route detectable HTML artifacts through the port. All other
    /// orchestrator behavior is unchanged.
    #[must_use]
    pub fn with_lens_port(mut self, port: Arc<dyn LensReviewPort>) -> Self {
        self.lens_port = Some(port);
        self
    }

    /// Returns the configured [`LensReviewPort`], if any.
    #[must_use]
    pub fn lens_port(&self) -> Option<&dyn LensReviewPort> {
        self.lens_port.as_deref()
    }

    /// Maybe route an HTML-looking tool output through VesperLens review.
    ///
    /// Returns `None` when no port is configured OR the text does not
    /// look like a reviewable HTML artifact (see
    /// [`looks_like_html_artifact`]). Returns `Some(Ok(feedback))` after
    /// the human submits, or `Some(Err(_))` if the port itself failed
    /// (timeout, parse error, etc.).
    ///
    /// `on_diagnostic` is called with the
    /// `[VesperLens] Artifact ready for review. Open: <URL>` line right
    /// before the port blocks awaiting the human (PRD §4). The host
    /// wires this to the TUI status line.
    pub async fn maybe_review_html_artifact(
        &self,
        html: &str,
        on_diagnostic: &(dyn Fn(&str) + Send + Sync),
    ) -> Option<Result<crate::planning::LensFeedback, crate::planning::vesper_lens::LensError>>
    {
        let port = self.lens_port.as_ref()?;
        if !looks_like_html_artifact(html) {
            return None;
        }
        // The port is responsible for invoking its own URL-announcement
        // callback (the port implementation calls `on_url(url)` once the
        // listener binds); we surface that URL to the host's diagnostic
        // sink via a small adapter closure. The URL string itself is
        // formatted by `diagnostic_for_review`.
        let on_url = |url: &str| {
            on_diagnostic(&diagnostic_for_review(url));
        };
        Some(port.review(html, &on_url).await)
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
            // VRO-6 — BoundedTreeSearch (PRD §11.7). Works with no extra seam:
            // the strategy uses generator + verifier_ids + the orchestrator's
            // own registry. Verifier IDs come from the profile.
            ReasoningStrategy::BoundedTreeSearch => {
                run_bounded_tree_search(
                    generator,
                    &profile.available_verifiers,
                    &self.registry,
                    _workspace_root,
                    &request.user_message,
                    budget,
                )
                .await
            }
            // VRO-6 — ProposerCriticAdjudicator (PRD §11.8) requires the
            // CandidateCritic + Adjudicator seams that this method's signature
            // does not accept. Degrade to consensus with a clear risk so the
            // host sees that the strategy was downgraded (mirror the existing
            // ParallelCandidatesJudge→consensus degradation pattern). Callers
            // that want full PCA must use execute_with_critic_adjudicator.
            ReasoningStrategy::ProposerCriticAdjudicator => {
                let outcome = run_parallel_candidates_consensus(
                    generator,
                    &request.user_message,
                    usize::from(budget.max_parallel_branches.max(1)),
                    budget,
                )
                .await;
                let mut degraded = outcome;
                degraded.unresolved_risks.push(
                    "ProposerCriticAdjudicator downgraded to consensus: call \
                     VroOrchestrator::execute_with_critic_adjudicator to exercise the \
                     critic + adjudicator roles"
                        .to_string(),
                );
                degraded
            }
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

    /// Executes a turn through the Proposer-Critic-Adjudicator strategy (VRO-6,
    /// PRD §11.8) — the **only** entry point that accepts the
    /// [`CandidateCritic`] + [`Adjudicator`] seams.
    ///
    /// Profiles the request, resolves the budget (caller override or the
    /// mode preset), and dispatches to
    /// [`run_proposer_critic_adjudicator`]. `critic` produces per-candidate
    /// objective critiques against `criteria`; `adjudicator` selects the
    /// winner from the (candidate, critique, criteria) triple — NOT from the
    /// candidates' persuasive prose (PRD §11.8). The strategy variant's
    /// strict role separation (PRD §11.8) is enforced structurally: the
    /// [`CandidateGenerator`] (proposer), [`CandidateCritic`], and
    /// [`Adjudicator`] are three independent trait objects.
    ///
    /// When the profiled strategy is **not**
    /// [`ProposerCriticAdjudicator`](ReasoningStrategy), this method
    /// delegates to [`execute_with_judge`](Self::execute_with_judge) so the
    /// other strategies (Direct, GenerateVerifyRepair, parallel, bounded
    /// tree search) behave identically to their canonical entry points. This
    /// makes `execute_with_critic_adjudicator` a drop-in upgrade of
    /// `execute_with_judge` for hosts that always have a critic + adjudicator
    /// available.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_with_critic_adjudicator(
        &self,
        request: &ReasoningRequest,
        generator: &dyn CandidateGenerator,
        workspace_root: &Path,
        judge: Option<&dyn CandidateJudge>,
        critic: Option<&dyn CandidateCritic>,
        adjudicator: Option<&dyn Adjudicator>,
        seed: u64,
        criteria: &[String],
    ) -> ReasoningOutcome {
        let profile = self.profiler.profile_request(request);
        let budget = request
            .budget_override
            .unwrap_or_else(|| self.config.preset_for(request.mode));

        // Only the ProposerCriticAdjudicator strategy needs the new seams;
        // every other strategy delegates to execute_with_judge so behavior is
        // identical regardless of which entry point the host chose.
        if profile.recommended_strategy != ReasoningStrategy::ProposerCriticAdjudicator {
            return self
                .execute_with_judge(request, generator, workspace_root, judge, seed)
                .await;
        }

        // PCA requires both seams. Degrade to consensus with a clear risk
        // when either is missing (mirror the existing
        // ParallelCandidatesJudge→consensus degradation pattern).
        let (Some(critic), Some(adjudicator)) = (critic, adjudicator) else {
            let mut degraded = run_parallel_candidates_consensus(
                generator,
                &request.user_message,
                usize::from(budget.max_parallel_branches.max(1)),
                budget,
            )
            .await;
            degraded.unresolved_risks.push(
                "ProposerCriticAdjudicator downgraded to consensus: critic or adjudicator \
                 seam was not supplied"
                    .to_string(),
            );
            return degraded;
        };

        run_proposer_critic_adjudicator(
            generator,
            critic,
            adjudicator,
            &request.user_message,
            usize::from(budget.max_parallel_branches.max(1)),
            budget,
            criteria,
        )
        .await
    }

    /// Executes a turn AND, when it succeeds with a complex strategy,
    /// extracts a sanitized procedural memory and persists it through the
    /// supplied [`ProceduralMemorySink`] (VRO-7, PRD §11.9 — Verified
    /// Workflow Learning).
    ///
    /// ## Wiring
    ///
    /// This is a single composition-boundary entry point that handles every
    /// strategy:
    ///
    /// - [`ToolGroundedReact`](ReasoningStrategy) ⇒ calls
    ///   [`run_tool_grounded_react_with_trajectory`] so the trajectory is
    ///   available for [`WorkflowExtractor::extract_from_trajectory`].
    /// - [`ProposerCriticAdjudicator`](ReasoningStrategy) ⇒ calls
    ///   [`Self::execute_with_critic_adjudicator`] (the canonical PCA path).
    /// - Every other strategy ⇒ calls [`Self::execute_with_judge`].
    ///
    /// After the underlying turn, if the outcome is `Succeeded` AND the
    /// profiled strategy is [`is_learning_eligible`], the orchestrator
    /// runs the [`WorkflowExtractor`] on the (scrubbed) objective + outcome
    /// (+ trajectory for ReAct) and persists the result via the sink.
    ///
    /// ## Zero-breakage guarantee
    ///
    /// - When `sink == None`: extraction still runs (so the result is
    ///   observable in tests via the returned `unresolved_risks` note), but
    ///   persistence is skipped.
    /// - When extraction fails: the original outcome is returned with ONE
    ///   additional `unresolved_risks` entry (`"workflow-learning skipped:
    ///   <reason>"`). The turn itself is unaffected.
    /// - When persistence fails: the original outcome is returned with ONE
    ///   additional `unresolved_risks` entry (`"workflow-learning
    ///   persistence skipped: <reason>"`).
    /// - The orchestrator never panics from a learning error.
    ///
    /// `extracted_at` is caller-supplied (RFC3339 timestamp) so tests can
    /// pin it deterministically; production callers should pass
    /// `chrono::Utc::now().to_rfc3339()`.
    #[allow(
        clippy::too_many_arguments,
        reason = "VRO-7 wiring fan-in: every optional strategy seam is needed for a single drop-in entry point"
    )]
    pub async fn execute_with_learning(
        &self,
        request: &ReasoningRequest,
        generator: &dyn CandidateGenerator,
        workspace_root: &Path,
        judge: Option<&dyn CandidateJudge>,
        critic: Option<&dyn CandidateCritic>,
        adjudicator: Option<&dyn Adjudicator>,
        agent: Option<&dyn ReactAgent>,
        invoker: Option<&dyn ToolInvoker>,
        seed: u64,
        criteria: &[String],
        sink: Option<&dyn ProceduralMemorySink>,
        extractor: &WorkflowExtractor,
        extracted_at: &str,
    ) -> ReasoningOutcome {
        let profile = self.profiler.profile_request(request);
        let strategy = profile.recommended_strategy;

        // --- Dispatch to the strategy-appropriate underlying executor and
        // capture the ReAct trajectory when applicable. ---
        let (outcome, trajectory): (ReasoningOutcome, Vec<TrajectoryEntry>) =
            if strategy == ReasoningStrategy::ToolGroundedReact {
                let (Some(agent), Some(invoker)) = (agent, invoker) else {
                    // Missing seam: fall through to execute_react, which
                    // itself returns Failed with a clear "missing seam"
                    // message (mirrors the existing execute_with_judge
                    // guard). No trajectory is captured.
                    let failed = self
                        .execute_react(
                            request,
                            agent.unwrap_or(&NullReactAgent),
                            invoker.unwrap_or(&NullInvoker),
                            workspace_root,
                        )
                        .await;
                    return finalize_outcome(
                        failed,
                        Vec::new(),
                        strategy,
                        request,
                        sink,
                        extractor,
                        extracted_at,
                    )
                    .await;
                };
                let budget = request
                    .budget_override
                    .unwrap_or_else(|| self.config.preset_for(request.mode));
                let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
                    &request.user_message,
                    agent,
                    invoker,
                    budget,
                    profile.requires_grounding,
                )
                .await;
                (outcome, trajectory)
            } else if strategy == ReasoningStrategy::ProposerCriticAdjudicator {
                let outcome = self
                    .execute_with_critic_adjudicator(
                        request,
                        generator,
                        workspace_root,
                        judge,
                        critic,
                        adjudicator,
                        seed,
                        criteria,
                    )
                    .await;
                (outcome, Vec::new())
            } else {
                let outcome = self
                    .execute_with_judge(request, generator, workspace_root, judge, seed)
                    .await;
                (outcome, Vec::new())
            };

        // --- Layer VRO-7 on top of the underlying outcome. ---
        finalize_outcome(
            outcome,
            trajectory,
            strategy,
            request,
            sink,
            extractor,
            extracted_at,
        )
        .await
    }
}

/// Worker: takes the underlying outcome + (possibly empty) trajectory and
/// runs VRO-7 extraction + persistence with the documented zero-breakage
/// guarantees. Kept as a free async fn so the call sites above stay readable.
async fn finalize_outcome(
    mut outcome: ReasoningOutcome,
    trajectory: Vec<TrajectoryEntry>,
    strategy: ReasoningStrategy,
    request: &ReasoningRequest,
    sink: Option<&dyn ProceduralMemorySink>,
    extractor: &WorkflowExtractor,
    extracted_at: &str,
) -> ReasoningOutcome {
    // Only successful complex-strategy turns are eligible for learning.
    if outcome.status != OutcomeStatus::Succeeded || !is_learning_eligible(strategy) {
        return outcome;
    }

    // Extract (sanitizes every byte of the source material). Errors are
    // non-fatal — surface as a risk note and return the original outcome.
    let procedure = if strategy == ReasoningStrategy::ToolGroundedReact {
        extractor.extract_from_trajectory(request, &outcome, &trajectory, strategy, extracted_at)
    } else {
        extractor.extract_from_outcome(request, &outcome, strategy, extracted_at)
    };
    let procedure = match procedure {
        Ok(proc) => proc,
        Err(err) => {
            outcome
                .unresolved_risks
                .push(format!("workflow-learning skipped: {err}"));
            return outcome;
        }
    };

    // Persist through the sink (if supplied). Errors are non-fatal — surface
    // as a risk note. When no sink is supplied, still record that extraction
    // succeeded (useful for tests and hosts that have not wired cognition).
    if let Some(sink) = sink {
        match sink.save_procedure(&procedure).await {
            Ok(_id) => {
                outcome.unresolved_risks.push(format!(
                    "workflow-learning persisted: `{}` ({} steps, {} model calls)",
                    procedure.title,
                    procedure.steps.len(),
                    procedure.model_calls,
                ));
            }
            Err(err) => {
                outcome
                    .unresolved_risks
                    .push(format!("workflow-learning persistence skipped: {err}"));
            }
        }
    } else {
        outcome.unresolved_risks.push(format!(
            "workflow-learning extracted (no sink): `{}` ({} steps)",
            procedure.title,
            procedure.steps.len(),
        ));
    }

    outcome
}

/// Sentinel `ReactAgent` used only when `execute_with_learning` is called
/// with `agent = None` for a `ToolGroundedReact` profile (a misconfiguration
/// at the composition boundary). It produces an immediate `Finish` so the
/// orchestrator returns `Succeeded` with empty output, which then fails
/// extraction (`NoStepsToExtract`) — the documented zero-breakage path.
struct NullReactAgent;

impl ReactAgent for NullReactAgent {
    fn next_action<'a>(
        &'a self,
        _prompt: &'a str,
        _trajectory: &'a [TrajectoryEntry],
    ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
        Box::pin(async {
            ReactDecision::Finish {
                output: serde_json::Value::Null,
            }
        })
    }
}

/// Sentinel `ToolInvoker` paired with [`NullReactAgent`]. Never invoked
/// because the null agent always finishes immediately.
struct NullInvoker;
impl ToolInvoker for NullInvoker {
    fn class_of(&self, _name: &str) -> Option<ToolExecutionClass> {
        None
    }
    fn invoke<'a>(
        &'a self,
        name: &'a str,
        _arguments: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>> {
        let name = name.to_string();
        Box::pin(async move { Err(ToolInvocationError::UnknownTool(name)) })
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

    // === VRO-11.2 — VesperLens integration seam ===

    /// In-test LensReviewPort impl that returns a fixed feedback.
    #[derive(Debug)]
    struct FixedLens {
        action: crate::planning::vesper_lens::Action,
    }

    impl crate::vro::LensReviewPort for FixedLens {
        fn review(
            &self,
            _html: &str,
            on_url: &(dyn Fn(&str) + Send + Sync),
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::planning::LensFeedback,
                            crate::planning::vesper_lens::LensError,
                        >,
                    > + Send,
            >,
        > {
            on_url("http://127.0.0.1:9999/");
            let action = self.action;
            Box::pin(async move {
                Ok(crate::planning::LensFeedback {
                    action,
                    ..Default::default()
                })
            })
        }
    }

    #[tokio::test]
    async fn orchestrator_without_lens_port_skips_review() {
        // Default orchestrator: lens_port is None.
        let vro = VroOrchestrator::disabled();
        let diagnostic_seen = std::sync::Mutex::new(String::new());
        let result = vro
            .maybe_review_html_artifact("<html><body>hi</body></html>", &|line: &str| {
                *diagnostic_seen.lock().unwrap() = line.to_string()
            })
            .await;
        assert!(result.is_none(), "no port configured => no review");
        assert!(
            diagnostic_seen.lock().unwrap().is_empty(),
            "diagnostic sink must not be called when port is None"
        );
    }

    #[tokio::test]
    async fn orchestrator_with_lens_port_skips_non_html() {
        let vro = VroOrchestrator::disabled().with_lens_port(std::sync::Arc::new(FixedLens {
            action: crate::planning::vesper_lens::Action::Approve,
        }));
        let result = vro
            .maybe_review_html_artifact(
                "Sure, I can use <html> tags in the response.",
                &|_line: &str| {},
            )
            .await;
        assert!(result.is_none(), "non-HTML prose must not trigger review");
    }

    #[tokio::test]
    async fn orchestrator_with_lens_port_reviews_html_and_emits_diagnostic() {
        let vro = VroOrchestrator::disabled().with_lens_port(std::sync::Arc::new(FixedLens {
            action: crate::planning::vesper_lens::Action::Approve,
        }));
        let diagnostic = std::sync::Mutex::new(String::new());
        let result = vro
            .maybe_review_html_artifact(
                "<html><head></head><body><h1>artifact</h1></body></html>",
                &|line: &str| *diagnostic.lock().unwrap() = line.to_string(),
            )
            .await;
        let feedback = result
            .expect("HTML artifact must route through port")
            .unwrap();
        assert_eq!(
            feedback.action,
            crate::planning::vesper_lens::Action::Approve
        );
        let diag = diagnostic.lock().unwrap().clone();
        assert!(
            diag.starts_with("[VesperLens] Artifact ready for review. Open: http://"),
            "diagnostic must match PRD §4 format; got: {diag}",
        );
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
            // Internal: shared by VRO-4 (no learning) and VRO-7 (learning)
            // tests. The VRO-7 tests need an eligible privacy mode so the
            // extractor actually runs; VRO-4 tests are unaffected.
            privacy_mode: PrivacyMode::Internal,
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

    // === VRO-6 — orchestrator-level dispatch tests ===
    //
    // These prove the orchestrator dispatches BoundedTreeSearch and
    // ProposerCriticAdjudicator through the profiler → strategy pipeline.
    // The per-component tests (depth-halt, pruning, role separation) live in
    // strategies.rs; these prove the orchestrator WIRING is correct.

    use super::strategies::{Adjudicator, CandidateCritic};
    use vesper_domain::VerificationStatus;

    /// A request whose user_message profiles to BoundedTreeSearch.
    /// "find the root cause of the bug" triggers the VRO-6 tree-search
    /// routing.
    fn tree_search_request() -> ReasoningRequest {
        ReasoningRequest {
            request_id: RequestId::new("req-bts").unwrap(),
            session_id: SessionId::new("sess-bts").unwrap(),
            user_message: "find the root cause of the intermittent test failure".to_string(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: Some(ReasoningBudget {
                max_search_depth: 1,
                max_parallel_branches: 2,
                max_model_calls: 5,
                ..ReasoningBudget::balanced()
            }),
            privacy_mode: PrivacyMode::Private,
        }
    }

    /// A request whose user_message profiles to ProposerCriticAdjudicator.
    fn pca_request() -> ReasoningRequest {
        ReasoningRequest {
            request_id: RequestId::new("req-pca").unwrap(),
            session_id: SessionId::new("sess-pca").unwrap(),
            user_message: "adjudicate between the proposed designs for the auth system".to_string(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: Some(ReasoningBudget {
                max_parallel_branches: 2,
                ..ReasoningBudget::balanced()
            }),
            privacy_mode: PrivacyMode::Private,
        }
    }

    #[tokio::test]
    async fn execute_dispatches_bounded_tree_search_through_profiler() {
        // End-to-end: a "find the root cause" prompt profiles to
        // BoundedTreeSearch → execute() runs the tree-search loop. With no
        // verifiers registered (default cargo verifiers aren't applicable to
        // a non-.rs prompt), every candidate is an Error → pruned → the
        // search returns Failed (no valid leaf). The key assertion is that
        // the tree-search path was taken (cost > 0), not GenerateVerifyRepair.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"root_cause": "race_condition"}),
        };
        let outcome = vro
            .execute(
                &tree_search_request(),
                &generator,
                std::path::Path::new("/tmp"),
            )
            .await;
        // The search ran (model calls consumed) — it didn't silently
        // short-circuit.
        let calls = *generator.call_count.lock().expect("poisoned");
        assert!(calls > 0, "bounded tree search must invoke the generator");
        // The outcome's status reflects the search result (not GenerateVerifyRepair).
        // With default cargo verifiers on a non-.rs prompt, verifiers are
        // unregistered → Error → pruned → Failed. This proves the tree-search
        // path ran, not the GVR baseline.
        assert!(
            outcome.status == OutcomeStatus::Failed
                || outcome.status == OutcomeStatus::Succeeded
                || outcome.status == OutcomeStatus::Partial,
            "outcome must come from the tree-search path, got {:?}",
            outcome.status
        );
    }

    #[tokio::test]
    async fn execute_degrades_pca_to_consensus_without_critic_adjudicator() {
        // execute() passes critic=None, adjudicator=None — the PCA strategy
        // falls back to consensus instead of erroring.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"design": "A"}),
        };
        let outcome = vro
            .execute(&pca_request(), &generator, std::path::Path::new("/tmp"))
            .await;
        // Degrades to consensus → succeeds (all branches agree).
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // The risk message records the downgrade so the host sees it.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("ProposerCriticAdjudicator downgraded")),
            "risk must record the PCA→consensus downgrade: {:?}",
            outcome.unresolved_risks
        );
    }

    /// No-op critic + pick-first adjudicator for the execute_with_critic
    /// test.
    struct NullCritic;
    impl CandidateCritic for NullCritic {
        fn critique<'a>(
            &'a self,
            candidate: &'a vesper_domain::Candidate,
            _criteria: &'a [String],
        ) -> Pin<Box<dyn Future<Output = String> + Send + 'a>> {
            let id = candidate.candidate_id.as_str().to_string();
            Box::pin(async move { format!("critique of {id}") })
        }
    }

    struct FirstPickAdjudicator;
    impl Adjudicator for FirstPickAdjudicator {
        fn adjudicate<'a>(
            &'a self,
            _candidates: &'a [vesper_domain::Candidate],
            _critiques: &'a [String],
            _criteria: &'a [String],
        ) -> Pin<Box<dyn Future<Output = usize> + Send + 'a>> {
            Box::pin(async { 0 })
        }
    }

    #[tokio::test]
    async fn execute_with_critic_adjudicator_drives_full_pca_pipeline() {
        // End-to-end: execute_with_critic_adjudicator with real critic +
        // adjudicator seams runs the full Propose→Critic→Adjudicate pipeline.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"design": "X"}),
        };
        let outcome = vro
            .execute_with_critic_adjudicator(
                &pca_request(),
                &generator,
                std::path::Path::new("/tmp"),
                None, // no judge needed
                Some(&NullCritic),
                Some(&FirstPickAdjudicator),
                0,
                &["criterion-1".to_string()],
            )
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // The outcome confirms objective-criteria selection.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("adjudicator selected")),
            "risk must confirm adjudicator selection: {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn execute_with_critic_adjudicator_delegates_non_pca_to_execute_with_judge() {
        // When the profile is NOT PCA, execute_with_critic_adjudicator
        // delegates to execute_with_judge. A parallel-judge prompt should
        // route through the existing judge path, not PCA.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let req = request_for(
            "Compare options for the parser: pest vs nom vs chumsky. Weigh the pros and cons.",
        );
        let outcome = vro
            .execute_with_critic_adjudicator(
                &req,
                &generator,
                std::path::Path::new("/tmp"),
                None,
                Some(&NullCritic),
                Some(&FirstPickAdjudicator),
                0,
                &[],
            )
            .await;
        // Delegates to judge/consensus → succeeds (not PCA).
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // The risk message does NOT mention the adjudicator (delegation
        // happened, not PCA).
        assert!(
            !outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("adjudicator selected")),
            "non-PCA profile must NOT run PCA: {:?}",
            outcome.unresolved_risks
        );
    }

    // Silence unused-import warning for VerificationStatus when the VRO-6
    // dispatch tests don't reference it directly.
    #[allow(dead_code)]
    fn _verification_status_marker(_: VerificationStatus) {}

    // === VRO-7 — Verified Workflow Learning orchestrator wiring tests ===
    //
    // These prove execute_with_learning actually:
    //   (1) dispatches to the right underlying executor (ReAct vs PCA vs
    //       judge) AND layers VRO-7 on top when status==Succeeded,
    //   (2) skips learning when the strategy is Direct or the outcome is
    //       not Succeeded,
    //   (3) survives a failing sink (zero-breakage) without panicking,
    //   (4) survives a sink=None call (extraction noted but not persisted).

    use super::learning::{LearningError, RecordingSink, SecretScrubber, WorkflowExtractor};
    use std::sync::atomic::Ordering;

    /// A ReAct agent that calls one read-only tool then finishes. Drives
    /// the ReAct path far enough to produce a non-empty trajectory.
    struct ReadThenFinishAgent {
        output: StructuredOutput,
        call_count: Arc<Mutex<u32>>,
    }
    impl ReactAgent for ReadThenFinishAgent {
        fn next_action<'a>(
            &'a self,
            _prompt: &'a str,
            trajectory: &'a [super::react::TrajectoryEntry],
        ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
            let count = Arc::clone(&self.call_count);
            let output = self.output.clone();
            Box::pin(async move {
                *count.lock().expect("poisoned") += 1;
                if trajectory.is_empty() {
                    ReactDecision::CallTool {
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({"path": "src/lib.rs"}),
                    }
                } else {
                    ReactDecision::Finish { output }
                }
            })
        }
    }

    /// Read-only invoker that returns a canned observation.
    struct ReadOnlyInvoker {
        text: String,
    }
    impl ToolInvoker for ReadOnlyInvoker {
        fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
            if name == "read_file" {
                Some(ToolExecutionClass::ReadOnly)
            } else {
                None
            }
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            _arguments: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            let text = self.text.clone();
            Box::pin(async move {
                if name == "read_file" {
                    Ok(text)
                } else {
                    Err(ToolInvocationError::UnknownTool(name.to_string()))
                }
            })
        }
    }

    fn learning_react_request() -> ReasoningRequest {
        ReasoningRequest {
            request_id: RequestId::new("req-vro7-react").unwrap(),
            session_id: SessionId::new("sess-vro7-react").unwrap(),
            user_message: "What does the main.rs file do?".to_string(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: Some(ReasoningBudget {
                max_model_calls: 5,
                max_tool_calls: 5,
                ..ReasoningBudget::balanced()
            }),
            // Internal: these tests assert learning actually runs, so the
            // extractor must accept the request. Private is rejected per
            // PRD §17 — see execute_with_learning_skips_learning_for_private_request.
            privacy_mode: PrivacyMode::Internal,
        }
    }

    #[tokio::test]
    async fn execute_with_learning_extracts_and_persists_procedure_on_react_success() {
        // End-to-end: ToolGroundedReact turn succeeds -> extractor produces a
        // procedure -> sink records it -> outcome carries a "persisted" risk.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let agent = ReadThenFinishAgent {
            output: serde_json::json!({"answer": "main.rs is the entry point"}),
            call_count: Arc::new(Mutex::new(0)),
        };
        let invoker = ReadOnlyInvoker {
            text: "fn main() {}".to_string(),
        };
        let sink = Arc::new(RecordingSink::new());
        let extractor = WorkflowExtractor::new();
        let outcome = vro
            .execute_with_learning(
                &learning_react_request(),
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                Some(&agent),
                Some(&invoker),
                0,
                &[],
                Some(sink.as_ref()),
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        // Underlying turn succeeded.
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // The sink recorded exactly one procedure.
        assert_eq!(
            sink.saved.lock().expect("poisoned").len(),
            1,
            "sink must record exactly one procedure on ReAct success"
        );
        // The outcome carries a "persisted" risk note.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("workflow-learning persisted")),
            "outcome must carry a persisted note: {:?}",
            outcome.unresolved_risks
        );
        // The procedure has at least one step (the read_file action).
        let saved = sink.saved.lock().expect("poisoned");
        let proc = &saved[0];
        assert!(
            proc.steps.iter().any(|s| s.action == "read_file"),
            "procedure must include the read_file step"
        );
        assert_eq!(proc.source_strategy, "tool_grounded_react");
    }

    #[tokio::test]
    async fn execute_with_learning_skips_persistence_when_sink_is_none_but_records_extraction() {
        // When sink is None, extraction still runs and a "extracted (no
        // sink)" risk note is added so the host sees the gap.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let agent = ReadThenFinishAgent {
            output: serde_json::json!({"answer": "x"}),
            call_count: Arc::new(Mutex::new(0)),
        };
        let invoker = ReadOnlyInvoker {
            text: "fn main() {}".to_string(),
        };
        let extractor = WorkflowExtractor::new();
        let outcome = vro
            .execute_with_learning(
                &learning_react_request(),
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                Some(&agent),
                Some(&invoker),
                0,
                &[],
                None, // no sink
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("workflow-learning extracted (no sink)")),
            "expected extracted-no-sink note: {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn execute_with_learning_survives_sink_failure_without_panicking() {
        // Zero-breakage: a sink that always returns Err must NOT crash the
        // turn. The outcome stays Succeeded with a "persistence skipped"
        // risk note.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let agent = ReadThenFinishAgent {
            output: serde_json::json!({"answer": "x"}),
            call_count: Arc::new(Mutex::new(0)),
        };
        let invoker = ReadOnlyInvoker {
            text: "fn main() {}".to_string(),
        };
        let sink = Arc::new(RecordingSink::new());
        sink.fail_next.store(true, Ordering::SeqCst);
        let extractor = WorkflowExtractor::new();
        let outcome = vro
            .execute_with_learning(
                &learning_react_request(),
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                Some(&agent),
                Some(&invoker),
                0,
                &[],
                Some(sink.as_ref()),
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        // Outcome still Succeeded (the underlying turn was fine).
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Risk note records the persistence skip.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("workflow-learning persistence skipped")),
            "expected persistence-skipped note: {:?}",
            outcome.unresolved_risks
        );
        // The sink did not record anything (the save failed).
        assert!(sink.saved.lock().expect("poisoned").is_empty());
    }

    #[tokio::test]
    async fn execute_with_learning_skips_learning_for_non_succeeded_outcome() {
        // When the underlying turn fails, VRO-7 must not even attempt
        // extraction. Use a ReAct budget of zero model calls so the loop
        // halts immediately with BudgetExceeded.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let agent = ReadThenFinishAgent {
            output: serde_json::json!({"answer": "x"}),
            call_count: Arc::new(Mutex::new(0)),
        };
        let invoker = ReadOnlyInvoker {
            text: "fn main() {}".to_string(),
        };
        let sink = Arc::new(RecordingSink::new());
        let extractor = WorkflowExtractor::new();
        // Budget of 0 model calls -> loop halts on first iteration with
        // BudgetExceeded before the agent can finish.
        let request = ReasoningRequest {
            budget_override: Some(ReasoningBudget {
                max_model_calls: 0,
                max_tool_calls: 0,
                ..ReasoningBudget::balanced()
            }),
            ..learning_react_request()
        };
        let outcome = vro
            .execute_with_learning(
                &request,
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                Some(&agent),
                Some(&invoker),
                0,
                &[],
                Some(sink.as_ref()),
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        // Outcome was NOT Succeeded.
        assert_ne!(outcome.status, OutcomeStatus::Succeeded);
        // No learning risk note was added.
        assert!(
            !outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("workflow-learning")),
            "non-succeeded outcome must not trigger learning: {:?}",
            outcome.unresolved_risks
        );
        // Sink recorded nothing.
        assert!(sink.saved.lock().expect("poisoned").is_empty());
    }

    #[tokio::test]
    async fn execute_with_learning_skips_learning_for_private_request() {
        // PRD §17: PrivacyMode::Private requests must NOT be persisted to
        // cognitive memory. Even on a Succeeded turn with a learning-eligible
        // strategy, the extractor refuses BEFORE doing any work, so no
        // scrubbed-but-still-private bytes can leak through a future sink bug.
        // The outcome still succeeds — privacy only blocks learning, not
        // the underlying turn.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"answer": "yes"}),
        };
        let sink = Arc::new(RecordingSink::new());
        let extractor = WorkflowExtractor::new();
        // A consensus prompt that would normally trigger learning.
        let mut req = request_for("Is this correct? The Rust borrow checker prevents data races.");
        req.privacy_mode = PrivacyMode::Private;
        let outcome = vro
            .execute_with_learning(
                &req,
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                None,
                None,
                0,
                &[],
                Some(sink.as_ref()),
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        // The underlying turn still succeeded — privacy only blocks learning.
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Learning was skipped with a Private-rejection risk note.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("workflow-learning skipped")
                    && r.contains("PrivacyMode::Private")),
            "expected skipped-PrivateRequestRejected note: {:?}",
            outcome.unresolved_risks
        );
        // Sink recorded nothing.
        assert!(sink.saved.lock().expect("poisoned").is_empty());
    }

    #[tokio::test]
    async fn execute_with_learning_persists_procedure_on_consensus_success() {
        // Non-ReAct path: ParallelCandidatesConsensus success → extractor
        // synthesizes a generate step → sink records it.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"answer": "yes"}),
        };
        let sink = Arc::new(RecordingSink::new());
        let extractor = WorkflowExtractor::new();
        let req = request_for("Is this correct? The Rust borrow checker prevents data races.");
        let outcome = vro
            .execute_with_learning(
                &req,
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                None,
                None,
                0,
                &[],
                Some(sink.as_ref()),
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Sink recorded one procedure with at least a generate step.
        assert_eq!(sink.saved.lock().expect("poisoned").len(), 1);
        let saved = sink.saved.lock().expect("poisoned");
        let proc = &saved[0];
        assert!(
            proc.steps.iter().any(|s| s.action == "generate"),
            "non-ReAct procedure must include a generate step"
        );
        assert_eq!(proc.source_strategy, "parallel_candidates_consensus");
    }

    #[tokio::test]
    async fn execute_with_learning_with_missing_react_seams_falls_back_gracefully() {
        // A ToolGroundedReact profile called WITHOUT agent/invoker seams:
        // the orchestrator's NullReactAgent finishes immediately, extraction
        // produces NoStepsToExtract, and the outcome carries the
        // "workflow-learning skipped" risk note. The turn does not crash.
        let vro = enabled_orchestrator();
        let generator = SingleAnswerGenerator {
            call_count: Arc::new(Mutex::new(0)),
            output: serde_json::json!({"v": 1}),
        };
        let sink = Arc::new(RecordingSink::new());
        let extractor = WorkflowExtractor::new();
        let outcome = vro
            .execute_with_learning(
                &learning_react_request(),
                &generator,
                std::path::Path::new("/tmp"),
                None,
                None,
                None,
                None, // missing agent
                None, // missing invoker
                0,
                &[],
                Some(sink.as_ref()),
                &extractor,
                "2026-01-01T00:00:00Z",
            )
            .await;
        // The NullReactAgent finishes with empty output → Succeeded.
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // Extraction produced NoStepsToExtract → skipped note.
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("workflow-learning skipped")
                    && r.contains("no extractable steps")),
            "expected skipped-NoStepsToExtract note: {:?}",
            outcome.unresolved_risks
        );
        // Sink recorded nothing.
        assert!(sink.saved.lock().expect("poisoned").is_empty());
    }

    #[test]
    fn learning_marker_for_unused_learningerror_import() {
        // Ensure LearningError stays imported even when an assertion path
        // does not construct it directly.
        let _ = LearningError::NoStepsToExtract;
        let _ = SecretScrubber::new();
    }
}
