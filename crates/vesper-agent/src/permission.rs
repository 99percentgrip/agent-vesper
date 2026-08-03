//! Permission gate (ADR 0010, Tier C Phase 1).
//!
//! Pure decisions over `(operating mode × permission mode × execution class)`.
//! No I/O, no async — the agent loop calls [`check_tool_permission`] before
//! routing a tool call to its executor. This mirrors the Python oracle's
//! `DESTRUCTIVE_TOOLS` frozenset (`config.py:639`) and mode-based eligibility
//! (`agent.py:2843-2872`).
//!
//! The static gate never treats `Ask` as authorization. It returns an
//! intermediate request decision that the agent loop resolves through its
//! injected [`PermissionPort`]. Hosts that do not provide a channel fail
//! closed, matching the oracle's non-interactive behavior.

use crate::executor::{ToolContext, ToolFuture};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use vesper_domain::{SessionOperatingMode, SessionPermissionMode, ToolExecutionClass};
use vesper_domain::{ToolCall, ToolDefinition};

/// Outcome of the permission gate for one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// The tool may execute.
    Allow,
    /// The host must obtain one-time approval before execution.
    Ask(String),
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

/// Host-provided asynchronous approval boundary.
///
/// The agent crate does not know whether approval comes from ACP, a terminal
/// prompt, or another frontend. The port returns a one-turn decision and is
/// never allowed to grant authority to a statically denied operation.
pub trait PermissionPort: Send + Sync {
    /// Resolves one pending `Ask` decision.
    fn authorize<'a>(
        &'a self,
        call: &'a ToolCall,
        definition: &'a ToolDefinition,
        context: &'a ToolContext,
    ) -> ToolFuture<'a, PermissionDecision>;
}

/// Fail-closed approval port used when a host has not wired an interaction
/// channel. This keeps library and non-interactive callers safe by default.
#[derive(Debug, Default)]
pub struct DenyPermissionPort;

impl PermissionPort for DenyPermissionPort {
    fn authorize<'a>(
        &'a self,
        _call: &'a ToolCall,
        _definition: &'a ToolDefinition,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, PermissionDecision> {
        Box::pin(async {
            PermissionDecision::Deny("interactive approval channel unavailable".into())
        })
    }
}

/// A host-visible one-time approval request.
///
/// The request intentionally exposes only the tool id, JSON arguments, and a
/// bounded reason. It does not expose the full conversation or cancellation
/// capability to a UI. Dropping a request closes the responder and therefore
/// fails the waiting tool call closed.
pub struct PermissionRequest {
    /// Tool awaiting approval.
    pub tool: String,
    /// Arguments the model supplied for the tool.
    pub arguments: serde_json::Value,
    /// Human-readable static gate reason.
    pub reason: String,
    responder: oneshot::Sender<PermissionDecision>,
}

impl std::fmt::Debug for PermissionRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PermissionRequest")
            .field("tool", &self.tool)
            .field("arguments", &"<redacted-from-debug>")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

impl PermissionRequest {
    /// Approves this request exactly once.
    pub fn approve(self) {
        let _ = self.responder.send(PermissionDecision::Allow);
    }

    /// Rejects this request exactly once.
    pub fn reject(self, reason: impl Into<String>) {
        let _ = self.responder.send(PermissionDecision::Deny(reason.into()));
    }
}

/// Interactive one-time approval broker for terminal or ACP hosts.
///
/// The agent loop blocks only the current tool-call future while the host
/// resolves the request. No request is auto-approved, and a closed receiver
/// produces a denial.
#[derive(Clone)]
pub struct ApprovalBroker {
    sender: mpsc::UnboundedSender<PermissionRequest>,
}

impl std::fmt::Debug for ApprovalBroker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApprovalBroker(..)")
    }
}

impl ApprovalBroker {
    /// Creates a broker and the host-side request receiver.
    pub fn channel() -> (Arc<Self>, mpsc::UnboundedReceiver<PermissionRequest>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Arc::new(Self { sender }), receiver)
    }
}

impl PermissionPort for ApprovalBroker {
    fn authorize<'a>(
        &'a self,
        call: &'a ToolCall,
        _definition: &'a ToolDefinition,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, PermissionDecision> {
        let sender = self.sender.clone();
        let tool = call.tool_id.as_str().to_owned();
        let arguments = call.arguments.clone();
        Box::pin(async move {
            let (responder, receiver) = oneshot::channel();
            let request = PermissionRequest {
                tool,
                arguments,
                reason: "one-time approval required".into(),
                responder,
            };
            if sender.send(request).is_err() {
                return PermissionDecision::Deny("approval channel unavailable".into());
            }
            receiver
                .await
                .unwrap_or_else(|_| PermissionDecision::Deny("approval request closed".into()))
        })
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
    if matches!(permission_mode, SessionPermissionMode::Ask) {
        return PermissionDecision::Ask(format!("{class:?} tool requires one-time approval"));
    }
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
            // Mutating: Code + Bypass, or Code + Ask with an injected port.
            (code, ask, Class::Mutating, false),
            (code, bypass, Class::Mutating, true),
            (code, read, Class::Mutating, false),
            (plan, bypass, Class::Mutating, false),
            // Shell: same rule as Mutating.
            (code, ask, Class::Shell, false),
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
            PermissionDecision::Ask(reason) => {
                panic!("Plan mode must deny before asking for approval: {reason}");
            }
            PermissionDecision::Allow => panic!("shell must be denied in Plan mode"),
        }
    }

    #[test]
    fn ask_returns_an_approval_request_instead_of_authorizing() {
        assert!(matches!(
            check_tool_permission(
                SessionOperatingMode::Code,
                SessionPermissionMode::Ask,
                ToolExecutionClass::Mutating,
            ),
            PermissionDecision::Ask(_)
        ));
    }
}
