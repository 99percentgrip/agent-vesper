#![forbid(unsafe_code)]
//! Pure policy and permission decisions. Transport/UI approval lives elsewhere.

/// VRO-13 PR-1 — hard-denial firewall over shell command text.
pub mod firewall;

use serde::{Deserialize, Serialize};
use vesper_domain::{SessionOperatingMode, SessionPermissionMode};
use vesper_security::{IsolationRequirement, SandboxCapabilities};

/// Workspace policy effect after ordered rule evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyEffect {
    /// Policy permits evaluation to continue.
    Allow,
    /// Policy requires interactive approval unless Bypass applies.
    Ask,
    /// Absolute denial.
    Deny,
}

/// Authority class of a tool/workflow operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationClass {
    /// Repository/session read.
    Read,
    /// Explicit plan-artifact write.
    PlanArtifactWrite,
    /// Generic MCP discovery/call.
    Mcp,
    /// Filesystem mutation.
    Write,
    /// Direct argv process.
    Process,
    /// Explicit shell-language execution.
    Shell,
    /// Other external side effect.
    ExternalSideEffect,
}

impl OperationClass {
    /// Whether normal Ask mode needs explicit approval.
    #[must_use]
    pub const fn requires_approval(self) -> bool {
        !matches!(self, Self::Read)
    }

    /// Whether Read Only must reject it.
    #[must_use]
    pub const fn prohibited_in_read_only(self) -> bool {
        !matches!(self, Self::Read)
    }

    /// Whether Plan Mode permits it at source parity.
    #[must_use]
    pub const fn permitted_in_plan(self) -> bool {
        matches!(self, Self::Read | Self::PlanArtifactWrite | Self::Mcp)
    }
}

/// Approval channel state supplied to the pure evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalChannelResult {
    /// Approval has not yet been requested.
    NotRequested,
    /// User allowed once.
    Granted,
    /// User rejected.
    Rejected,
    /// Channel failed or disappeared.
    Failure,
}

/// Provider-powered review result. It is evidence, never authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SmartReviewEvidence {
    /// Reviewer judged the request safe.
    Safe,
    /// Reviewer raised concern.
    Unsafe,
    /// Reviewer could not decide.
    Unknown,
    /// Reviewer was unavailable/timed out.
    Unavailable,
}

/// Final evaluator outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionKind {
    /// Operation may proceed.
    Allow,
    /// Frontend approval is required.
    Ask,
    /// Operation must not proceed.
    Deny,
}

/// Stable reason suitable for tests and frontend localization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionReason {
    /// Policy allowed and no approval was required.
    Allowed,
    /// Bypass skipped approval after policy.
    AllowedBypass,
    /// Source-compatible Plan Mode allowance.
    AllowedPlanMode,
    /// Absolute policy denial.
    PolicyDenied,
    /// Plan Mode prohibits the operation.
    PlanModeRestricted,
    /// Read Only prohibits the operation.
    ReadOnlyRestricted,
    /// Required sandbox capability is unavailable.
    IsolationUnavailable,
    /// User approval required.
    ApprovalRequired,
    /// User rejected.
    ApprovalRejected,
    /// Approval channel failure.
    ApprovalChannelFailure,
    /// Nested workflow contains a denial.
    NestedStepDenied,
}

/// Decision plus advisory evidence that did not change authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Allow/ask/deny.
    pub kind: DecisionKind,
    /// Stable reason.
    pub reason: DecisionReason,
    /// Optional smart-review evidence.
    pub smart_review: Option<SmartReviewEvidence>,
    /// Denied workflow step index, when applicable.
    pub nested_step: Option<usize>,
}

/// Complete pure input for one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyInput<'a> {
    /// Ordered workspace policy outcome.
    pub policy: PolicyEffect,
    /// Ask/Bypass/Read Only.
    pub permission_mode: SessionPermissionMode,
    /// Code/Plan.
    pub operating_mode: SessionOperatingMode,
    /// Operation authority class.
    pub operation: OperationClass,
    /// Required process isolation.
    pub required_isolation: IsolationRequirement,
    /// Discovered backend capability, if applicable.
    pub sandbox: Option<&'a SandboxCapabilities>,
    /// Approval result.
    pub approval: ApprovalChannelResult,
    /// Advisory provider review.
    pub smart_review: Option<SmartReviewEvidence>,
}

/// Pure evaluator enforcing precedence.
#[derive(Debug, Clone, Copy, Default)]
pub struct PolicyEvaluator;

impl PolicyEvaluator {
    /// Evaluates one operation.
    #[must_use]
    pub fn evaluate(input: &PolicyInput<'_>) -> PolicyDecision {
        let decision = |kind, reason| PolicyDecision {
            kind,
            reason,
            smart_review: input.smart_review,
            nested_step: None,
        };

        if input.policy == PolicyEffect::Deny {
            return decision(DecisionKind::Deny, DecisionReason::PolicyDenied);
        }
        if input.operating_mode == SessionOperatingMode::Plan
            && !input.operation.permitted_in_plan()
        {
            return decision(DecisionKind::Deny, DecisionReason::PlanModeRestricted);
        }
        if input.permission_mode == SessionPermissionMode::ReadOnly
            && input.operation.prohibited_in_read_only()
        {
            return decision(DecisionKind::Deny, DecisionReason::ReadOnlyRestricted);
        }
        if !matches!(input.required_isolation, IsolationRequirement::None)
            && !input
                .sandbox
                .is_some_and(|sandbox| sandbox.satisfies(input.required_isolation))
        {
            return decision(DecisionKind::Deny, DecisionReason::IsolationUnavailable);
        }
        if input.permission_mode == SessionPermissionMode::Bypass {
            return decision(DecisionKind::Allow, DecisionReason::AllowedBypass);
        }
        if input.operating_mode == SessionOperatingMode::Plan
            && input.operation.permitted_in_plan()
            && input.policy == PolicyEffect::Allow
        {
            return decision(DecisionKind::Allow, DecisionReason::AllowedPlanMode);
        }

        let needs_approval =
            input.policy == PolicyEffect::Ask || input.operation.requires_approval();
        if !needs_approval {
            return decision(DecisionKind::Allow, DecisionReason::Allowed);
        }
        match input.approval {
            ApprovalChannelResult::NotRequested => {
                decision(DecisionKind::Ask, DecisionReason::ApprovalRequired)
            }
            ApprovalChannelResult::Granted => {
                decision(DecisionKind::Allow, DecisionReason::Allowed)
            }
            ApprovalChannelResult::Rejected => {
                decision(DecisionKind::Deny, DecisionReason::ApprovalRejected)
            }
            ApprovalChannelResult::Failure => {
                decision(DecisionKind::Deny, DecisionReason::ApprovalChannelFailure)
            }
        }
    }

    /// Evaluates each nested workflow step; wrappers cannot launder authority.
    #[must_use]
    pub fn evaluate_workflow(steps: &[PolicyInput<'_>]) -> PolicyDecision {
        let mut first_ask = None;
        for (index, step) in steps.iter().enumerate() {
            let decision = Self::evaluate(step);
            match decision.kind {
                DecisionKind::Deny => {
                    return PolicyDecision {
                        kind: DecisionKind::Deny,
                        reason: DecisionReason::NestedStepDenied,
                        smart_review: decision.smart_review,
                        nested_step: Some(index),
                    };
                }
                DecisionKind::Ask if first_ask.is_none() => first_ask = Some((index, decision)),
                DecisionKind::Allow | DecisionKind::Ask => {}
            }
        }
        if let Some((index, decision)) = first_ask {
            return PolicyDecision {
                nested_step: Some(index),
                ..decision
            };
        }
        PolicyDecision {
            kind: DecisionKind::Allow,
            reason: DecisionReason::Allowed,
            smart_review: None,
            nested_step: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        policy: PolicyEffect,
        permission_mode: SessionPermissionMode,
        operating_mode: SessionOperatingMode,
        operation: OperationClass,
        approval: ApprovalChannelResult,
    ) -> PolicyInput<'static> {
        PolicyInput {
            policy,
            permission_mode,
            operating_mode,
            operation,
            required_isolation: IsolationRequirement::None,
            sandbox: None,
            approval,
            smart_review: None,
        }
    }

    #[test]
    fn fixture_bypass_plus_deny_is_absolute() {
        let result = PolicyEvaluator::evaluate(&input(
            PolicyEffect::Deny,
            SessionPermissionMode::Bypass,
            SessionOperatingMode::Code,
            OperationClass::Write,
            ApprovalChannelResult::Granted,
        ));
        assert_eq!(result.kind, DecisionKind::Deny);
        assert_eq!(result.reason, DecisionReason::PolicyDenied);
    }

    #[test]
    fn fixture_precedence_matrix_matches_all_six_reference_cases() {
        let cases = [
            (
                SessionPermissionMode::Ask,
                OperationClass::Read,
                ApprovalChannelResult::NotRequested,
                DecisionKind::Allow,
            ),
            (
                SessionPermissionMode::Ask,
                OperationClass::Write,
                ApprovalChannelResult::Granted,
                DecisionKind::Allow,
            ),
            (
                SessionPermissionMode::ReadOnly,
                OperationClass::Read,
                ApprovalChannelResult::NotRequested,
                DecisionKind::Allow,
            ),
            (
                SessionPermissionMode::ReadOnly,
                OperationClass::Write,
                ApprovalChannelResult::Granted,
                DecisionKind::Deny,
            ),
            (
                SessionPermissionMode::Bypass,
                OperationClass::Read,
                ApprovalChannelResult::NotRequested,
                DecisionKind::Allow,
            ),
            (
                SessionPermissionMode::Bypass,
                OperationClass::Write,
                ApprovalChannelResult::NotRequested,
                DecisionKind::Allow,
            ),
        ];
        for (mode, operation, approval, expected) in cases {
            let result = PolicyEvaluator::evaluate(&input(
                PolicyEffect::Allow,
                mode,
                SessionOperatingMode::Code,
                operation,
                approval,
            ));
            assert_eq!(
                result.kind, expected,
                "mode={mode:?}, operation={operation:?}"
            );
        }
    }

    #[test]
    fn fixture_read_only_plus_destructive_denies() {
        let result = PolicyEvaluator::evaluate(&input(
            PolicyEffect::Allow,
            SessionPermissionMode::ReadOnly,
            SessionOperatingMode::Code,
            OperationClass::Write,
            ApprovalChannelResult::Granted,
        ));
        assert_eq!(result.reason, DecisionReason::ReadOnlyRestricted);
    }

    #[test]
    fn fixture_plan_mode_generic_mcp_is_preserved() {
        let result = PolicyEvaluator::evaluate(&input(
            PolicyEffect::Allow,
            SessionPermissionMode::Ask,
            SessionOperatingMode::Plan,
            OperationClass::Mcp,
            ApprovalChannelResult::NotRequested,
        ));
        assert_eq!(result.kind, DecisionKind::Allow);
        assert_eq!(result.reason, DecisionReason::AllowedPlanMode);
    }

    #[test]
    fn fixture_approval_channel_failure_denies() {
        let result = PolicyEvaluator::evaluate(&input(
            PolicyEffect::Allow,
            SessionPermissionMode::Ask,
            SessionOperatingMode::Code,
            OperationClass::Write,
            ApprovalChannelResult::Failure,
        ));
        assert_eq!(result.reason, DecisionReason::ApprovalChannelFailure);
    }

    #[test]
    fn nested_workflow_denial_closes_over_wrapper() {
        let steps = [
            input(
                PolicyEffect::Allow,
                SessionPermissionMode::Bypass,
                SessionOperatingMode::Code,
                OperationClass::Read,
                ApprovalChannelResult::NotRequested,
            ),
            input(
                PolicyEffect::Deny,
                SessionPermissionMode::Bypass,
                SessionOperatingMode::Code,
                OperationClass::Write,
                ApprovalChannelResult::NotRequested,
            ),
        ];
        let result = PolicyEvaluator::evaluate_workflow(&steps);
        assert_eq!(result.kind, DecisionKind::Deny);
        assert_eq!(result.nested_step, Some(1));
    }

    #[test]
    fn smart_review_cannot_independently_expand_authority() {
        let mut request = input(
            PolicyEffect::Ask,
            SessionPermissionMode::Ask,
            SessionOperatingMode::Code,
            OperationClass::Write,
            ApprovalChannelResult::NotRequested,
        );
        request.smart_review = Some(SmartReviewEvidence::Safe);
        let result = PolicyEvaluator::evaluate(&request);
        assert_eq!(result.kind, DecisionKind::Ask);
        assert_eq!(result.smart_review, Some(SmartReviewEvidence::Safe));
    }
}
