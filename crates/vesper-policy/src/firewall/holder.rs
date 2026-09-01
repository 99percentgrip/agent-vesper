//! Process-global firewall holder (VRO-13 PR-2).
//!
//! One [`Arc<CommandFirewall>`] per process, resolved once at host boot from
//! `AGENT_VESPER_FIREWALL`. Both hosts (TUI and ACP) and the agent loop read
//! the same Arc, which makes "same instance id" a structural property
//! instead of a convention. The holder is a plain global slot: hosts set it
//! exactly once during startup; nothing re-sets it later (a change requires
//! a restart, which is exactly what the `/firewall` panel advertises).
//!
//! Envelope:
//! - `AGENT_VESPER_FIREWALL=off` (or `0`/`false`, ASCII case-insensitive)
//!   → [`FirewallState::Disabled`]: the holder keeps `None`, executors never
//!   see a firewall, and the off path is structurally identical to the
//!   pre-VRO-13 executor (one `Option` check).
//! - unset, `on`, `1`, `true`, or any other value → enabled with the
//!   default ruleset. Failing to read the variable also enables: the safe
//!   default for a single-user native harness is ON.
//!
//! This module performs no I/O beyond `std::env::var` and never panics.

use std::sync::{Arc, OnceLock};

use crate::firewall::rules::{CommandFirewall, DEFAULT_RULES};

/// Runtime envelope for the process-global firewall.
#[derive(Debug, Clone)]
pub enum FirewallState {
    /// The firewall is enabled with the compiled ruleset shown.
    Enabled {
        /// Shared ruleset instance (one per process).
        rules: Arc<CommandFirewall>,
        /// Environment value that produced this state, when present.
        source: Option<String>,
    },
    /// The firewall is off (`AGENT_VESPER_FIREWALL=off`).
    Disabled {
        /// The exact environment value that disabled it.
        source: String,
    },
}

static HOLDER: OnceLock<Option<Arc<CommandFirewall>>> = OnceLock::new();

/// Disabling values for `AGENT_VESPER_FIREWALL` (ASCII case-insensitive).
const OFF_VALUES: [&str; 3] = ["off", "0", "false"];

/// Reads `AGENT_VESPER_FIREWALL` once and installs the process-global
/// firewall. Later calls are no-ops: the first resolution wins, so a host
/// cannot flip the firewall mid-process.
///
/// Returns the resolved state, including the shared Arc when enabled.
#[must_use]
pub fn install_from_env() -> FirewallState {
    let raw = std::env::var("AGENT_VESPER_FIREWALL").ok();
    let cell = HOLDER.get_or_init(|| {
        let disabled = raw
            .as_deref()
            .is_some_and(|value| OFF_VALUES.contains(&value.to_ascii_lowercase().as_str()));
        if disabled {
            None
        } else {
            // Compile the default ruleset once into the process-global Arc.
            // Both hosts and the agent loop share this exact instance.
            Some(Arc::new(
                CommandFirewall::compile(DEFAULT_RULES).expect("default ruleset compiles"),
            ))
        }
    });
    match (cell, raw) {
        (Some(rules), source) => FirewallState::Enabled {
            rules: Arc::clone(rules),
            source,
        },
        (None, source) => FirewallState::Disabled {
            source: source.unwrap_or_default(),
        },
    }
}

/// Returns the shared firewall when enabled, or `None` when disabled or not
/// yet installed. Executors consult this only when `ToolContext::firewall`
/// was not set by the host; hosts that inject explicitly always win.
#[must_use]
pub fn shared() -> Option<Arc<CommandFirewall>> {
    HOLDER.get().and_then(|slot| slot.as_ref().map(Arc::clone))
}

/// Whether the process-global firewall is enabled. `false` before
/// [`install_from_env`] runs (hosts must call it at boot).
#[must_use]
pub fn is_enabled() -> bool {
    HOLDER.get().is_some_and(|slot| slot.is_some())
}

/// Stable identity of the installed ruleset for cross-host parity tests:
/// the Arc pointer address. Two hosts in one process share one id; distinct
/// processes do not (that is the point of the shared-instance contract).
#[must_use]
pub fn instance_id() -> Option<usize> {
    shared().map(|rules| Arc::as_ptr(&rules) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_first_resolution_wins() {
        // Installing twice cannot flip the state: the OnceLock cell wins.
        let first = install_from_env();
        let second = install_from_env();
        let id_first = instance_id();
        let id_second = instance_id();
        if matches!(first, FirewallState::Disabled { .. }) {
            assert!(matches!(second, FirewallState::Disabled { .. }));
        } else {
            assert!(matches!(second, FirewallState::Enabled { .. }));
            assert_eq!(id_first, id_second);
        }
    }

    #[test]
    fn instance_id_is_stable_across_calls() {
        let a = instance_id();
        let b = instance_id();
        assert_eq!(a, b);
        if is_enabled() {
            assert!(a.is_some());
        }
    }
}
