//! Permission gate (ADR 0010, Tier C Phase 1).
//!
//! Pure decisions over `(operating mode × permission mode × execution class)`.
//! No I/O, no async — the agent loop calls [`check_tool_permission`] before
//! routing a tool call to its executor. This mirrors the Python oracle's
//! `DESTRUCTIVE_TOOLS` frozenset (`config.py:639`) and mode-based eligibility
//! (`agent.py:2843-2872`).
//!
//! Phase 1 policy:
//! - `ReadOnly` tools are always allowed (read-only reconnaissance in any mode).
//! - `Mutating`/`Shell`/`Process`/`NestedWorkflow` require `Code` mode AND a
//!   permission mode other than `ReadOnly`. `Ask` mode in `Code` is allowed at
//!   the gate in Phase 1 (stub executors do no real I/O); the full
//!   interactive-deferral channel (`PermissionOutcome`) is a later phase.

use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolExecutionClass};

/// Outcome of the permission gate for one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// The tool may execute.
    Allow,
    /// The tool is blocked; the reason is fed back to the model.
    Deny(String),
}

impl PermissionDecision {
    /// Whether the decision permits execution.
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Decides whether a tool of `class` may run under `(mode, permission)`.
#[must_use]
pub fn check_tool_permission(
    operating_mode: SessionOperatingMode,
    permission_mode: SessionPermissionMode,
    class: ToolExecutionClass,
) -> PermissionDecision {
    if matches!(class, ToolExecutionClass::ReadOnly) {
        return PermissionDecision::Allow;
    }
    // Mutating / Shell / Process / NestedWorkflow.
    if !matches!(operating_mode, SessionOperatingMode::Code) {
        return PermissionDecision::Deny(format!(
            "{class:?} tools require Code mode (active mode: {operating_mode:?})"
        ));
    }
    if matches!(permission_mode, SessionPermissionMode::ReadOnly) {
        return PermissionDecision::Deny(
            "read-only permission blocks mutating/shell/process tools".into(),
        );
    }
    // Code mode + Ask/Bypass: allowed at the gate (Phase 1). The full Ask-mode
    // interactive deferral (PermissionOutcome::AllowOnce/RejectOnce) lands in a
    // later phase when executors perform real I/O.
    PermissionDecision::Allow
}

#[cfg(test)]
mod tests {
    //! The full (mode × permission × class) decision matrix.

    use super::*;

    fn matrix() -> Vec<(
        SessionOperatingMode,
        SessionPermissionMode,
        ToolExecutionClass,
        bool,
    )> {
        use SessionOperatingMode as Mode;
        use SessionPermissionMode as Perm;
        use ToolExecutionClass as Class;
        let code = Mode::Code;
        let plan = Mode::Plan;
        let ask = Perm::Ask;
        let bypass = Perm::Bypass;
        let read = Perm::ReadOnly;
        vec![
            // ReadOnly: always allowed regardless of mode/permission.
            (code, ask, Class::ReadOnly, true),
            (plan, read, Class::ReadOnly, true),
            (plan, bypass, Class::ReadOnly, true),
            // Mutating: only Code + non-ReadOnly.
            (code, ask, Class::Mutating, true),
            (code, bypass, Class::Mutating, true),
            (code, read, Class::Mutating, false),
            (plan, bypass, Class::Mutating, false),
            // Shell: same rule as Mutating.
            (code, ask, Class::Shell, true),
            (code, read, Class::Shell, false),
            (plan, ask, Class::Shell, false),
            // Process / NestedWorkflow: same rule.
            (code, bypass, Class::Process, true),
            (plan, ask, Class::NestedWorkflow, false),
        ]
    }

    #[test]
    fn decision_matrix_matches_policy() {
        for (mode, perm, class, allowed) in matrix() {
            let decision = check_tool_permission(mode, perm, class);
            assert_eq!(
                decision.is_allowed(),
                allowed,
                "({mode:?}, {perm:?}, {class:?}) should be allowed={allowed}, got {decision:?}"
            );
        }
    }

    #[test]
    fn deny_carries_a_safe_reason_string() {
        let decision = check_tool_permission(
            SessionOperatingMode::Plan,
            SessionPermissionMode::Ask,
            ToolExecutionClass::Shell,
        );
        match decision {
            PermissionDecision::Deny(reason) => {
                assert!(
                    reason.contains("Code mode"),
                    "reason should explain the gate: {reason}"
                );
            }
            PermissionDecision::Allow => panic!("shell must be denied in Plan mode"),
        }
    }
}
