//! Scope-demanded sandbox routing (VRO-13 PR-4).
//!
//! [`SandboxRoute`] is the single seam between the hosts' resolved
//! `[sandbox]` config and the executor's sandboxed `run_command` path. It
//! exists so host-parity is structural, not conventional: TUI and ACP
//! construct the **same** route value from the **same** config parser, and
//! [`SandboxRoute::instance_id`] makes "both hosts share one instance" a
//! machine-checkable property (mirroring the PR-2 firewall holder).
//!
//! **Dependency shape.** This module is pure data plus an injected port. It
//! deliberately does *not* depend on `vesper-sandbox` (the architecture
//! allowlist keeps `vesper-agent` on domain/provider/runtime/testkit/policy
//! only): the backend, its probe, and its construction all live at the
//! composition boundary (the host), exactly like provider auth and the
//! firewall ruleset Arc. The executor consults the *route's* injected
//! capability report — never a backend directly — so the fail-closed gate
//! (`vesper-security`'s `SandboxCapabilities::satisfies`) is exercised in
//! both hosts through one shared type.
//!
//! Zero-cost opt-in: with no `[sandbox]` demand, hosts leave
//! `ToolContext.sandbox` as `None` and the executor path is byte-identical
//! to PR-3 — no probe, no allocation, no branch beyond one `Option` check.

use std::sync::Arc;

/// The capability report the route's fail-closed gate consults. Re-exported
/// under its real name so hosts implement `SandboxBackendPort` without a
/// direct `vesper-security` dependency; the gate stays one shared type.
pub use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};

/// The opt-in isolation demand a project scope declared for shell-class
/// tools, parsed from `.agent-vesper/config.toml` `[sandbox]` (or supplied
/// per-call by a tool that explicitly demands isolation).
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxDemand {
    /// Isolation class required for `run_command`-class executors.
    pub requirement: IsolationRequirement,
    /// Whether the scope explicitly granted network access inside the
    /// sandbox. Demanding `IsolationRequirement::Network` or stronger is
    /// the *isolation* demand; this flag is the *egress grant*. A demand
    /// with `allow_network = false` provisions with no network at all —
    /// there is no default grant.
    pub allow_network: bool,
    /// Optional CPU quota (Docker `--cpus`) demanded by the scope.
    pub cpu_limit: Option<f64>,
    /// Optional memory ceiling in bytes (Docker `--memory`).
    pub memory_limit_bytes: Option<u64>,
}

impl SandboxDemand {
    /// No demand — the structural zero-cost default.
    #[must_use]
    pub fn none() -> Self {
        Self {
            requirement: IsolationRequirement::None,
            allow_network: false,
            cpu_limit: None,
            memory_limit_bytes: None,
        }
    }

    /// Whether this demand actually routes through a backend.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self.requirement, IsolationRequirement::None)
    }
}

impl Default for SandboxDemand {
    fn default() -> Self {
        Self::none()
    }
}

/// Port through which the executor consults the sandbox route's backend.
///
/// Hosts implement this over the real backend (`vesper-sandbox`, Linux
/// namespaces or feature-gated Docker); tests implement it over honest
/// stubs. The executor never sees a concrete backend, which keeps the
/// architecture allowlist clean and the executor testable without Docker.
pub trait SandboxBackendPort: Send + Sync {
    /// Honest, probed capability report. Never claims what was not verified.
    fn capabilities(&self) -> SandboxCapabilities;
    /// Executes one shell command through the host's provisioned sandbox
    /// (provision → run → teardown), returning bounded combined output.
    ///
    /// The route validated the capability demand before this is called;
    /// implementations still own their own resource bounds and must fold
    /// output into the same bounded shape the unsandboxed path produces.
    fn run_command(
        &self,
        command: &str,
        cwd: &std::path::Path,
        timeout_seconds: u64,
        cancellation: &std::sync::Arc<dyn vesper_provider::CancellationSignal>,
    ) -> Result<SandboxOutcome, SandboxRunError>;
}

/// Bounded output of one sandboxed command run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutcome {
    /// Combined stdout+stderr, already capped, matching `run_bounded`'s shape.
    pub output: String,
    /// Whether the run hit its wall-clock bound and was terminated.
    pub timed_out: bool,
}

/// Why a sandboxed run failed before producing output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SandboxRunError {
    /// Provisioning or teardown failed (backend unavailable mid-run).
    #[error("{0}")]
    Backend(String),
    /// The caller cancelled the run.
    #[error("command cancelled")]
    Cancelled,
}

/// Resolved backend choice recorded at host boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxBackendChoice {
    /// Platform default: Linux namespaces (or the honest stub elsewhere).
    Default,
    /// The feature-gated Docker backend (`vesper-sandbox/docker` feature).
    Docker,
}

/// One host-resolved sandbox route, shared by both hosts and consulted by
/// the shell executor when a scope demand is active.
pub struct SandboxRoute {
    demand: SandboxDemand,
    /// Backend selection recorded at host boot.
    choice: SandboxBackendChoice,
    /// Host-injected backend consultation port. The route never constructs
    /// a backend; it only carries the one the host built at boot.
    port: Arc<dyn SandboxBackendPort>,
}

impl std::fmt::Debug for SandboxRoute {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SandboxRoute")
            .field("demand", &self.demand)
            .field("choice", &self.choice)
            .field("port", &"<sandbox-backend-port>")
            .finish()
    }
}

impl SandboxRoute {
    /// Builds a route from a resolved demand, backend choice, and the
    /// host-constructed backend port.
    #[must_use]
    pub fn new(
        demand: SandboxDemand,
        choice: SandboxBackendChoice,
        port: Arc<dyn SandboxBackendPort>,
    ) -> Self {
        Self {
            demand,
            choice,
            port,
        }
    }

    /// The resolved demand this route carries.
    #[must_use]
    pub fn demand(&self) -> &SandboxDemand {
        &self.demand
    }

    /// The backend selection recorded at host boot.
    #[must_use]
    pub fn choice(&self) -> &SandboxBackendChoice {
        &self.choice
    }

    /// The host-injected backend port (cloned Arc, so callers can move it
    /// into blocking tasks without borrowing the route).
    #[must_use]
    pub fn port(&self) -> std::sync::Arc<dyn SandboxBackendPort> {
        std::sync::Arc::clone(&self.port)
    }

    /// Honest capability report, delegated to the injected port.
    #[must_use]
    pub fn capabilities(&self) -> SandboxCapabilities {
        self.port.capabilities()
    }

    /// Stable instance identity (address-based, mirroring the firewall
    /// holder's `instance_id`). Host-parity tests assert TUI and ACP see
    /// the same id when they share one route Arc.
    #[must_use]
    pub fn instance_id(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    /// Whether the routed backend satisfies this route's own demand.
    ///
    /// This is the fail-closed gate the executor consults before
    /// provisioning: a Docker daemon that is down, or a host that forbids
    /// namespaces, yields `false` and the executor refuses honestly
    /// instead of running unsandboxed.
    #[must_use]
    pub fn satisfies_demand(&self) -> bool {
        self.capabilities().satisfies(self.demand.requirement)
    }

    /// Model-facing refusal text when the backend cannot satisfy the demand.
    /// Port of qm's `CapabilityUnsupportedError` refusal shape: fail fast,
    /// never hang the turn, never silently run unsandboxed.
    #[must_use]
    pub fn refusal_text(&self) -> String {
        let caps = self.capabilities();
        format!(
            "sandbox unavailable: backend {:?} cannot satisfy {:?} \
             (process_tree={:?}, filesystem={:?}, network={:?}) — \
             the operation needs isolation; refusing to run unsandboxed",
            caps.backend, self.demand.requirement, caps.process_tree, caps.filesystem, caps.network
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_security::{CapabilityStatus, SecurityStrength};

    struct FixedCaps(SandboxCapabilities);

    impl SandboxBackendPort for FixedCaps {
        fn capabilities(&self) -> SandboxCapabilities {
            self.0.clone()
        }
        fn run_command(
            &self,
            _command: &str,
            _cwd: &std::path::Path,
            _timeout_seconds: u64,
            _cancellation: &std::sync::Arc<dyn vesper_provider::CancellationSignal>,
        ) -> Result<SandboxOutcome, SandboxRunError> {
            // The route tests exercise the capability gate only; no host
            // executor is wired in unit tests.
            Err(SandboxRunError::Backend(
                "test stub does not execute".into(),
            ))
        }
    }

    fn full_caps() -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "test-full".into(),
            process_tree: CapabilityStatus::Available,
            filesystem: CapabilityStatus::Available,
            network: CapabilityStatus::Available,
            strength: SecurityStrength::Full,
        }
    }

    fn down_caps() -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "test-down".into(),
            process_tree: CapabilityStatus::Unavailable,
            filesystem: CapabilityStatus::Unavailable,
            network: CapabilityStatus::Unavailable,
            strength: SecurityStrength::None,
        }
    }

    #[test]
    fn none_demand_is_not_active() {
        assert!(!SandboxDemand::none().is_active());
    }

    #[test]
    fn full_backend_satisfies_full_demand() {
        let route = SandboxRoute::new(
            SandboxDemand {
                requirement: IsolationRequirement::Full,
                ..SandboxDemand::none()
            },
            SandboxBackendChoice::Default,
            Arc::new(FixedCaps(full_caps())),
        );
        assert!(route.satisfies_demand());
    }

    #[test]
    fn down_backend_fails_closed_for_any_active_demand() {
        let route = SandboxRoute::new(
            SandboxDemand {
                requirement: IsolationRequirement::ProcessTree,
                ..SandboxDemand::none()
            },
            SandboxBackendChoice::Docker,
            Arc::new(FixedCaps(down_caps())),
        );
        assert!(!route.satisfies_demand());
        let text = route.refusal_text();
        assert!(text.contains("sandbox unavailable"), "{text}");
        assert!(text.contains("refusing to run unsandboxed"), "{text}");
        assert!(text.contains("test-down"), "{text}");
    }

    #[test]
    fn filesystem_demand_requires_filesystem_capability() {
        // process-tree-only backend must fail a Filesystem demand.
        let caps = SandboxCapabilities {
            backend: "test-process-only".into(),
            process_tree: CapabilityStatus::Available,
            filesystem: CapabilityStatus::Unavailable,
            network: CapabilityStatus::Unavailable,
            strength: SecurityStrength::Process,
        };
        let route = SandboxRoute::new(
            SandboxDemand {
                requirement: IsolationRequirement::Filesystem,
                ..SandboxDemand::none()
            },
            SandboxBackendChoice::Default,
            Arc::new(FixedCaps(caps)),
        );
        assert!(!route.satisfies_demand());
    }

    #[test]
    fn inactive_demand_needs_no_capability() {
        let route = SandboxRoute::new(
            SandboxDemand::none(),
            SandboxBackendChoice::Default,
            Arc::new(FixedCaps(down_caps())),
        );
        assert!(!route.demand().is_active());
        assert!(route.capabilities().satisfies(IsolationRequirement::None));
    }
}
