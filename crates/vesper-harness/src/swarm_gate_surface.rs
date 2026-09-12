//! Shared governance gate rendering and command parsing for both hosts
//! (VRO-16 PR-1). One implementation; TUI and ACP both delegate — the
//! cross-host parity contract requires identical `HostCommand` semantics
//! and countdown rendering from a single source.

use vesper_swarm::hive::governance::{AuditEvent, FallbackAction, GateView, HostCommand};

/// Render an open-gate line with its derived countdown. Shared by the TUI
/// status surface and the ACP gate listing; formatting is identical.
#[must_use]
pub fn render_gate(view: &GateView) -> String {
    let seconds = view.remaining_ms / 1000;
    let minutes = seconds / 60;
    let remainder = seconds % 60;
    let fallback = match view.fallback {
        FallbackAction::FailTask => "fail-task",
    };
    format!(
        "Gate {} (task {}): {} — {}m {:02}s remaining, fallback {}",
        view.gate_id, view.task_id, view.reason, minutes, remainder, fallback
    )
}

/// Render the governance audit tail as text (both hosts).
#[must_use]
pub fn render_audit(events: &[AuditEvent]) -> String {
    if events.is_empty() {
        return String::from("No governance events.");
    }
    events
        .iter()
        .map(|event| match event {
            AuditEvent::GateOpened {
                gate_id,
                task_id,
                opened_at_ms,
                timeout_ms,
                ..
            } => format!(
                "opened  {gate_id} task {task_id} at {opened_at_ms}ms window {timeout_ms}ms"
            ),
            AuditEvent::GateResolved { gate_id, at_ms, .. } => {
                format!("resolved {gate_id} at {at_ms}ms")
            }
            AuditEvent::GateExpired { gate_id, at_ms, .. } => {
                format!("expired {gate_id} at {at_ms}ms")
            }
            AuditEvent::DecisionIssued {
                goal_id,
                verdict,
                refine_iterations,
                pivot_iterations,
                at_ms,
                ..
            } => {
                let verdict_text = match verdict {
                    vesper_swarm::hive::DecisionVerdictPayload::Proceed => {
                        String::from("proceed")
                    }
                    vesper_swarm::hive::DecisionVerdictPayload::ProceedWithFailure { reason } => {
                        format!("proceed-with-failure ({reason})")
                    }
                    vesper_swarm::hive::DecisionVerdictPayload::Refine { amended, iteration } => {
                        format!("refine {} task(s), iteration {iteration}", amended.len())
                    }
                    vesper_swarm::hive::DecisionVerdictPayload::Pivot { tasks } => {
                        format!("pivot with {} task(s)", tasks.len())
                    }
                };
                format!(
                    "decision {goal_id}: {verdict_text} (refines {refine_iterations}, pivots {pivot_iterations}) at {at_ms}ms"
                )
            }
            AuditEvent::VerificationVerdict {
                goal_id,
                check,
                failure,
                ..
            } => {
                let check_text = match check {
                    vesper_swarm::hive::VerificationCheck::Digest => "digest",
                    vesper_swarm::hive::VerificationCheck::Trace => "trace",
                };
                match failure {
                    Some(failure) => {
                        format!("verification {goal_id}: {check_text} FAILED — {failure}")
                    }
                    None => format!("verification {goal_id}: {check_text} passed"),
                }
            }
            AuditEvent::BudgetThreshold {
                goal_id,
                level,
                consumed_tokens,
                ceiling_tokens,
                at_ms,
            } => format!(
                "budget {goal_id}: {level}% ({consumed_tokens}/{ceiling_tokens} tokens) at {at_ms}ms"
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a host gate command into a [`HostCommand`]. Both hosts accept the
/// same verb set; unknown verbs are refused with the shared usage text.
///
/// # Errors
///
/// Unknown verb or an empty redirect/fail payload.
pub fn parse_gate_command(argument: &str) -> Result<(String, HostCommand), String> {
    let argument = argument.trim();
    let (task_id, rest) = argument
        .split_once(char::is_whitespace)
        .ok_or("Usage: gate <task-id> resume | redirect <directive> | fail <reason> | cancel")?;
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Err(
            "Usage: gate <task-id> resume | redirect <directive> | fail <reason> | cancel".into(),
        );
    }
    let rest = rest.trim();
    let command = if rest == "resume" {
        HostCommand::Resume
    } else if let Some(directive) = rest.strip_prefix("redirect ") {
        let directive = directive.trim();
        if directive.is_empty() {
            return Err("redirect requires a directive.".into());
        }
        HostCommand::Redirect {
            directive: directive.to_owned(),
        }
    } else if let Some(reason) = rest.strip_prefix("fail ") {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("fail requires a reason.".into());
        }
        HostCommand::Fail {
            reason: reason.to_owned(),
        }
    } else if rest == "cancel" {
        HostCommand::Cancel
    } else {
        return Err(
            "Usage: gate <task-id> resume | redirect <directive> | fail <reason> | cancel".into(),
        );
    };
    Ok((task_id.to_owned(), command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_and_parsing_are_shared_and_bounded() {
        assert_eq!(
            parse_gate_command("goal-task-0 resume"),
            Ok((String::from("goal-task-0"), HostCommand::Resume))
        );
        assert_eq!(
            parse_gate_command("goal-task-0 redirect use column two"),
            Ok((
                String::from("goal-task-0"),
                HostCommand::Redirect {
                    directive: String::from("use column two")
                }
            ))
        );
        assert_eq!(
            parse_gate_command("goal-task-0 fail bad data"),
            Ok((
                String::from("goal-task-0"),
                HostCommand::Fail {
                    reason: String::from("bad data")
                }
            ))
        );
        assert_eq!(
            parse_gate_command("goal-task-0 cancel"),
            Ok((String::from("goal-task-0"), HostCommand::Cancel))
        );
        assert!(parse_gate_command("goal-task-0 redirect ").is_err());
        assert!(parse_gate_command("goal-task-0 explode").is_err());
        assert!(parse_gate_command("").is_err());
        // The verb surface is exactly the ratified set, in stable order.
        assert_eq!(
            HostCommand::verbs(),
            &["resume", "redirect", "fail", "cancel"]
        );
    }

    #[test]
    fn gate_rendering_shows_derived_countdown() {
        let view = GateView {
            gate_id: String::from("g-task-0-gate"),
            task_id: String::from("g-task-0"),
            reason: String::from("repeated failed turns"),
            remaining_ms: 205_000,
            fallback: FallbackAction::FailTask,
        };
        let line = render_gate(&view);
        assert!(line.contains("3m 25s remaining"));
        assert!(line.contains("fail-task"));
    }
}
