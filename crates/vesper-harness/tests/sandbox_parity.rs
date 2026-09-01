//! VRO-13 PR-4 — sandbox route holder parity tests.
//!
//! These prove the composed contract at the layer that owns it
//! (`vesper-harness`, the composition adapter both hosts share):
//!
//! 1. One process, one route: `install_from_env()` is
//!    first-resolution-wins and `shared()`/`route_id()` are stable across
//!    repeated installs — the same structural property `vesper-policy`'s
//!    firewall holder test pins (`vesper-agent/tests/firewall.rs`).
//! 2. Off-path structural identity: with no active `[sandbox]` demand the
//!    holder stays `None` — the executor path is byte-identical to PR-3.
//! 3. A demand the resolved backend cannot satisfy yields an unsatisfied
//!    `satisfies_demand()` → the executor refuses honestly (fail-closed),
//!    never a silent fallback to unsandboxed execution.
//!
//! The test binary has no host boot step; tests resolve the holder here to
//! exercise the real process-global state rather than an empty global.
//! These tests never execute a backend; they pin holder/route contracts.

use std::sync::Arc;

use vesper_agent::sandbox_route::{
    CapabilityStatus, IsolationRequirement, SandboxBackendChoice, SandboxBackendPort,
    SandboxCapabilities, SandboxDemand, SandboxOutcome, SandboxRoute, SandboxRunError,
    SecurityStrength,
};
use vesper_harness::sandbox_backend::holder;

/// Backend stub with no capabilities at all.
struct NoCaps;

impl SandboxBackendPort for NoCaps {
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "no-caps stub".to_owned(),
            process_tree: CapabilityStatus::Unavailable,
            filesystem: CapabilityStatus::Unavailable,
            network: CapabilityStatus::Unavailable,
            strength: SecurityStrength::None,
        }
    }

    fn run_command(
        &self,
        _command: &str,
        _cwd: &std::path::Path,
        _timeout_seconds: u64,
        _cancellation: &Arc<dyn vesper_agent::CancellationSignal>,
    ) -> Result<SandboxOutcome, SandboxRunError> {
        Err(SandboxRunError::Backend("unavailable".to_string()))
    }
}

/// One process, one route: first-resolution-wins, stable identity.
#[test]
fn shared_instance_is_process_global_and_first_wins() {
    let _boot = holder::install_from_env();
    let before = holder::route_id();
    let first = holder::install_from_env();
    let second = holder::install_from_env();
    let after = holder::route_id();
    assert_eq!(
        before, after,
        "install cannot flip an installed route identity"
    );
    assert_eq!(
        first.is_some(),
        second.is_some(),
        "repeated installs resolve identically"
    );
    if let Some(route) = &first {
        assert_ne!(route.instance_id(), 0, "pointer identity is nonzero");
    }
}

/// With no `[sandbox]` demand in scope and no env override, the holder
/// stays `None`: the legacy executor path is structurally unchanged.
#[test]
fn inactive_demand_leaves_holder_empty() {
    // Precondition: this binary's CWD has no active sandbox scope.
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Ok(scope) = vesper_config::read_sandbox_scope(&cwd) {
        assert!(!scope.is_active(), "test precondition: no active scope");
    }
    let _boot = holder::install_from_env();
    assert!(
        holder::shared().is_none(),
        "no demand → no route → executor stays on the legacy path"
    );
    assert_eq!(holder::route_id(), 0);
}

/// Fails closed when the backend cannot satisfy the demand: the executor
/// sees `satisfies_demand() == false` and refuses before provisioning.
#[test]
fn unsatisfiable_demand_fails_closed() {
    let demand = SandboxDemand {
        requirement: IsolationRequirement::Filesystem,
        ..SandboxDemand::none()
    };
    let route = SandboxRoute::new(demand, SandboxBackendChoice::Default, Arc::new(NoCaps));
    assert!(
        !route.satisfies_demand(),
        "unsatisfiable demand must fail closed, not run unsandboxed"
    );
}
