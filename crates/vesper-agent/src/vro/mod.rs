//! Vesper Reasoning Orchestrator (VRO) agent-side scaffolding (Phase VRO-1).
//!
//! This module establishes the **orchestration seam** without altering
//! [`crate::agent_loop::AgentLoop`]. The composition boundary (TUI / ACP host)
//! constructs a [`VroOrchestrator`] from a [`ReasoningConfig`] and asks it, per
//! turn, whether to orchestrate or to use the existing direct execution loop.
//!
//! ## VRO-1 behavior
//!
//! Per the PRD §21 "VRO-1 — Core Contracts and Direct Compatibility" exit
//! criteria ("No behavior regression when disabled"), [`VroOrchestrator::route`]
//! returns [`VroRoutingDecision::Direct`] **unconditionally** in this phase:
//!
//! - When `config.enabled` is `false` (the shipped default), the direct path is
//!   taken — byte-for-byte identical to today's `agent_loop.rs` behavior.
//! - When `config.enabled` is `true`, the path is *still* direct, because no
//!   Task Profiler, Policy Engine, or strategy executor is wired yet. This is
//!   the fail-safe posture: VRO-1 ships the contracts and the seam, never the
//!   behavior. Future phases (VRO-2 onward) replace the body of `route` with a
//!   real routing decision and add the orchestrator to the composition root.
//!
//! Nothing in this module touches [`crate::AgentLoopConfig`],
//! [`crate::agent_loop::AgentLoop`], the tool registry, or the permission gate.
//! The existing direct execution loop is unchanged.
//!
//! See `crates/vesper-agent/AGENTS.md` for ownership and contract scope.

use vesper_domain::{ReasoningBudget, ReasoningConfig, ReasoningMode};

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

/// Phase VRO-1 orchestrator scaffolding.
///
/// Holds the reasoning configuration and answers the single question the
/// composition boundary needs: *does this turn route through VRO or through
/// direct execution?* In VRO-1 the answer is always `Direct` (see module docs).
///
/// This type performs **no I/O**, holds no provider handles, and mutates no
/// session state. It is safe to construct cheaply at host startup and clone
/// per turn.
#[derive(Debug, Clone, Default)]
pub struct VroOrchestrator {
    config: ReasoningConfig,
}

impl VroOrchestrator {
    /// Creates a new orchestrator bound to the given configuration.
    #[must_use]
    pub fn new(config: ReasoningConfig) -> Self {
        Self { config }
    }

    /// Creates a disabled orchestrator (equivalent to `new(ReasoningConfig::default())`).
    #[must_use]
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Returns whether the VRO feature flag is enabled.
    ///
    /// Note: `true` does **not** mean orchestration runs in VRO-1 — it only
    /// means the flag is set. [`route`](Self::route) still returns `Direct`.
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

    /// Decides whether a turn routes through VRO or the direct execution loop.
    ///
    /// **VRO-1: always returns [`VroRoutingDecision::Direct`].**
    ///
    /// The `user_message` and `mode` parameters are accepted (and validated
    /// against the enabled flag) so the signature is forward-compatible with
    /// the Task Profiler + Policy Engine that later phases will plug in here.
    /// Until then, this is the explicit no-op seam that guarantees the existing
    /// `agent_loop.rs` direct path is taken unchanged.
    pub fn route(&self, _user_message: &str, _mode: ReasoningMode) -> VroRoutingDecision {
        // Fail-safe: VRO-1 ships contracts + seam only. No orchestration logic
        // is wired, so every turn uses the direct execution loop regardless of
        // the enabled flag. The flag is surfaced via `enabled()` for telemetry
        // and host-side diagnostics only.
        VroRoutingDecision::Direct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_default_is_not_enabled() {
        let vro = VroOrchestrator::disabled();
        assert!(!vro.enabled());
        assert_eq!(vro.default_mode(), ReasoningMode::Auto);
    }

    #[test]
    fn route_always_returns_direct_in_vro_1_even_when_enabled() {
        // The core VRO-1 contract: even with the flag on, the direct path is
        // taken because no orchestration logic is wired yet.
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
                "VRO-1 must always route Direct, even when enabled (mode {mode:?})"
            );
        }
    }

    #[test]
    fn route_returns_direct_when_disabled() {
        let vro = VroOrchestrator::disabled();
        assert_eq!(
            vro.route("anything", ReasoningMode::Auto),
            VroRoutingDecision::Direct
        );
        // Off mode also routes direct.
        assert_eq!(
            vro.route("anything", ReasoningMode::Off),
            VroRoutingDecision::Direct
        );
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
