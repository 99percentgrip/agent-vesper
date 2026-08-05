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

pub mod orchestrator;
pub mod profiler;
pub mod verifiers;

pub use orchestrator::{CandidateGenerator, GeneratedCandidate, run_generate_verify_repair};
pub use profiler::TaskProfiler;
use std::path::Path;
use std::sync::Arc;
pub use verifiers::{
    CargoCheckVerifier, CargoTestVerifier, VerificationContext, Verifier, VerifierRegistry,
};
use vesper_domain::{
    ReasoningBudget, ReasoningConfig, ReasoningMode, ReasoningOutcome, ReasoningRequest,
    ReasoningStrategy, TaskProfile,
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
    /// preset), and dispatches to the strategy executor. VRO-2.3 implements
    /// the [`GenerateVerifyRepair`](ReasoningStrategy) loop; other non-`Direct`
    /// strategies fall back to a single generate-and-verify pass
    /// (`max_repairs == 0`) until their dedicated executors land.
    ///
    /// `generator` is the provider seam: the host supplies a real
    /// provider-backed [`CandidateGenerator`]; the orchestrator never makes a
    /// provider call itself.
    pub async fn execute(
        &self,
        request: &ReasoningRequest,
        generator: &dyn CandidateGenerator,
        workspace_root: &Path,
    ) -> ReasoningOutcome {
        let profile = self.profiler.profile_request(request);
        let budget = request
            .budget_override
            .unwrap_or_else(|| self.config.preset_for(request.mode));
        // Repair budget: full for GenerateVerifyRepair; zero (single pass) for
        // the other non-Direct strategies VRO-2.3 does not yet specialize.
        let repair_budget =
            if profile.recommended_strategy == ReasoningStrategy::GenerateVerifyRepair {
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
            workspace_root,
            repair_budget,
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
}
