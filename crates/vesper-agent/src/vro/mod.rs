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
//! - **VRO-2.1** adds the deterministic [`TaskProfiler`]. When the feature
//!   flag is **on**, [`route`](VroOrchestrator::route) now runs the profiler to
//!   build a [`TaskProfile`] (available via [`profile`](VroOrchestrator::profile)
//!   for telemetry/inspection). The routing decision is still
//!   [`VroRoutingDecision::Direct`] because **no strategy executor is wired
//!   yet** — VRO-2.2+ will dispatch the selected strategy. When the flag is
//!   **off** (the shipped default), `route` returns `Direct` immediately
//!   without profiling, preserving byte-for-byte direct execution.
//!
//! Nothing in this module touches [`crate::AgentLoopConfig`],
//! [`crate::agent_loop::AgentLoop`], the tool registry, or the permission gate.
//! The existing direct execution loop is unchanged.
//!
//! See `crates/vesper-agent/AGENTS.md` for ownership and contract scope.

pub mod profiler;

pub use profiler::TaskProfiler;
use vesper_domain::{ReasoningBudget, ReasoningConfig, ReasoningMode, TaskProfile};

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

/// Phase VRO-1/VRO-2.1 orchestrator scaffolding.
///
/// Holds the reasoning configuration and a [`TaskProfiler`]. It answers the
/// single question the composition boundary needs: *does this turn route
/// through VRO or through direct execution?*
///
/// - When `enabled` is `false` (shipped default), `route` returns `Direct`
///   immediately — no profiling, zero behavior change.
/// - When `enabled` is `true`, `route` runs the profiler to build a
///   [`TaskProfile`] but **still returns `Direct`**, because no strategy
///   executor is wired yet. The profile is surfaced via [`Self::profile`] for
///   telemetry and validation.
///
/// This type performs **no I/O**, holds no provider handles, and mutates no
/// session state. It is safe to construct cheaply at host startup and clone
/// per turn.
#[derive(Debug, Clone, Default)]
pub struct VroOrchestrator {
    config: ReasoningConfig,
    profiler: TaskProfiler,
}

impl VroOrchestrator {
    /// Creates a new orchestrator bound to the given configuration.
    #[must_use]
    pub fn new(config: ReasoningConfig) -> Self {
        Self {
            config,
            profiler: TaskProfiler::new(),
        }
    }

    /// Creates a disabled orchestrator (equivalent to `new(ReasoningConfig::default())`).
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

    /// Profiles a user message into a [`TaskProfile`] using the deterministic
    /// [`TaskProfiler`]. Always available regardless of the `enabled` flag so
    /// hosts and tests can inspect profiling without enabling orchestration.
    #[must_use]
    pub fn profile(&self, user_message: &str) -> TaskProfile {
        self.profiler.profile(user_message)
    }

    /// Decides whether a turn routes through VRO or the direct execution loop.
    ///
    /// **Zero-breakage contract (VRO-1/VRO-2.1):**
    /// - When `enabled` is `false`, returns [`VroRoutingDecision::Direct`]
    ///   immediately (no profiling).
    /// - When `enabled` is `true`, runs the [`TaskProfiler`] to build a profile
    ///   (for telemetry), but **still returns `Direct`** because no strategy
    ///   executor is wired yet. VRO-2.2+ will return `Orchestrate` based on the
    ///   profiled strategy.
    pub fn route(&self, user_message: &str, mode: ReasoningMode) -> VroRoutingDecision {
        // Master switch off -> direct, no work done.
        if !self.config.enabled {
            return VroRoutingDecision::Direct;
        }
        // Explicit Off mode -> direct.
        if mode == ReasoningMode::Off {
            return VroRoutingDecision::Direct;
        }
        // VRO-2.1: profile the request so the composition boundary can observe
        // the TaskProfile. Execution stays on the direct path until a strategy
        // executor lands (VRO-2.2+); the profile is discarded here.
        let _profile = self.profiler.profile(user_message);
        VroRoutingDecision::Direct
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
    fn route_still_returns_direct_in_vro_2_1_even_when_enabled() {
        // VRO-2.1: even with the flag on, the profiler now RUNS but the routing
        // decision stays Direct because no strategy executor is wired yet.
        // (This preserves the VRO-1 zero-breakage contract.) VRO-2.2+ will
        // return Orchestrate based on the profiled strategy.
        let enabled_cfg = ReasoningConfig {
            enabled: true,
            ..ReasoningConfig::default()
        };
        let vro = VroOrchestrator::new(enabled_cfg);
        assert!(vro.enabled());
        for mode in [
            ReasoningMode::Auto,
            ReasoningMode::Fast,
            ReasoningMode::Balanced,
            ReasoningMode::Deep,
            ReasoningMode::Maximum,
        ] {
            assert_eq!(
                vro.route("refactor the session writer", mode),
                VroRoutingDecision::Direct,
                "VRO-2.1 must still route Direct even when enabled (mode {mode:?})"
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
        // Off mode also routes direct even when enabled.
        let enabled = VroOrchestrator::new(ReasoningConfig {
            enabled: true,
            ..ReasoningConfig::default()
        });
        assert_eq!(
            enabled.route("anything", ReasoningMode::Off),
            VroRoutingDecision::Direct
        );
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
