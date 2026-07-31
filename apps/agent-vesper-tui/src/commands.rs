//! Slash-command registry (Stage 11b).
//!
//! Implements the slash-command surface required by the approved TUI
//! architecture (`/plan`, `/effort`, `/thinking`, `/model`). The registry is
//! pure: it parses user input into a [`CommandIntent`] and the surrounding
//! event loop dispatches the intent to the appropriate handler (Plan Mode
//! state machine, superpowers surface, or runtime).
//!
//! The registry does not import any concrete provider adapter; superpower
//! command targets (`effort`, `thinking`, `model`) are resolved dynamically
//! against the descriptors advertised by the active provider at startup. This
//! keeps the TUI provider-neutral and lets the same `/effort` command target
//! `zai:effort` for GLM or `synthetic:reply` for the synthetic provider.

use vesper_domain::ProviderId;
use vesper_provider::{SuperpowerDescriptor, SuperpowerValue};

use crate::plan_mode::PlanState;

/// Parsed user intent, in priority order: empty input, slash command, then
/// free-text prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandIntent {
    /// Empty input — the event loop should ignore the submission.
    Empty,
    /// Slash command and (optional) trimmed argument.
    Slash {
        /// Canonical command name without the leading slash (e.g. `plan`).
        name: String,
        /// Argument portion after the command, trimmed. Empty when no arg.
        argument: String,
    },
    /// Free-text prompt to send to the runtime.
    Prompt(String),
}

impl CommandIntent {
    /// Parses one raw input line into a [`CommandIntent`].
    ///
    /// Leading and trailing whitespace is trimmed. A leading slash denotes a
    /// command; the remainder is split into command name and argument on the
    /// first whitespace run. Empty input is `Empty`.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::Empty;
        }
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, argument) = match rest.split_once(char::is_whitespace) {
                Some((name, argument)) => (name.to_ascii_lowercase(), argument.trim().to_string()),
                None => (rest.to_ascii_lowercase(), String::new()),
            };
            return Self::Slash { name, argument };
        }
        Self::Prompt(trimmed.to_string())
    }
}

/// Outcome of resolving a slash command against the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Free-text prompt — pass straight to the runtime.
    Prompt(String),
    /// Plan Mode was invoked with a PRD.
    Plan { prd: String },
    /// Plan Mode gesture that did not require text (`approve`, `cancel`).
    PlanGesture(PlanGesture),
    /// `/review <body>` — the model produced a plan body; advance PLANNING to
    /// REVIEW so the driver can interrogate or `/approve` it.
    FinalizePlan {
        /// Bounded plan body to surface for human approval.
        body: String,
    },
    /// A superpower command targeted one descriptor.
    Superpower {
        /// Provider that owns the resolved descriptor.
        provider_id: ProviderId,
        /// Descriptor targeted by the command.
        descriptor: SuperpowerDescriptor,
        /// Parsed argument expressed as a superpower value.
        value: SuperpowerValue,
    },
    /// Help/usage text to display.
    Help(String),
    /// Quit/exit requested.
    Quit,
    /// Unknown command or invalid argument; the message is shown to the user.
    Error(String),
}

/// Plan-related slash gestures that take no argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanGesture {
    /// `/approve` — finalize the in-review plan.
    Approve,
    /// `/cancel` — abort any in-flight plan.
    Cancel,
}

/// Static, provider-neutral registry that maps command names to handlers.
///
/// The registry does not own any concrete provider state. Superpower commands
/// (`effort`, `thinking`, `model`) are resolved against the *currently
/// advertised* descriptors at dispatch time, which lets the same command
/// dispatch to whichever provider the composition boundary has selected.
#[derive(Debug, Default, Clone)]
pub struct CommandRegistry {
    /// Known top-level command names in stable registration order.
    names: Vec<String>,
}

impl CommandRegistry {
    /// Creates a registry populated with the canonical Stage 11b command set.
    #[must_use]
    pub fn stage_11b() -> Self {
        // ADR 0009: `/effort` is retired — the GLM reasoning dial collapsed to
        // the single `/thinking` control. `low`/`medium` are no longer valid.
        let names = [
            "plan", "review", "approve", "cancel", "thinking", "model", "help", "quit",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        Self { names }
    }

    /// Returns the registered command names in registration order.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Returns true when `name` matches a registered command.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|candidate| candidate == name)
    }

    /// Resolves a [`CommandIntent`] into a [`CommandOutcome`].
    ///
    /// `plan_state` is consulted to validate `/approve` and `/cancel`.
    /// `superpowers` is consulted to resolve `/effort`, `/thinking`, and
    /// `/model` against the active provider's advertised descriptors.
    pub fn resolve(
        &self,
        intent: &CommandIntent,
        plan_state: &PlanState,
        active_provider: &ProviderId,
        superpowers: &[SuperpowerDescriptor],
    ) -> CommandOutcome {
        match intent {
            CommandIntent::Empty => CommandOutcome::Error("Empty input.".into()),
            CommandIntent::Prompt(text) => CommandOutcome::Prompt(text.clone()),
            CommandIntent::Slash { name, argument } => match name.as_str() {
                "plan" => {
                    if argument.is_empty() {
                        CommandOutcome::Error("Usage: /plan <your requirements as a PRD>".into())
                    } else {
                        CommandOutcome::Plan {
                            prd: argument.clone(),
                        }
                    }
                }
                "review" => {
                    if argument.is_empty() {
                        CommandOutcome::Error(
                            "Usage: /review <plan body to surface for approval>".into(),
                        )
                    } else {
                        CommandOutcome::FinalizePlan {
                            body: argument.clone(),
                        }
                    }
                }
                "approve" => {
                    if plan_state.phase() != crate::plan_mode::PlanPhase::Review {
                        CommandOutcome::Error(
                            "/approve only works while a plan is under review.".into(),
                        )
                    } else {
                        CommandOutcome::PlanGesture(PlanGesture::Approve)
                    }
                }
                "cancel" => CommandOutcome::PlanGesture(PlanGesture::Cancel),
                "thinking" | "model" => {
                    self.resolve_superpower(name, argument, active_provider, superpowers)
                }
                "help" => CommandOutcome::Help(self.help_text()),
                "quit" | "exit" => CommandOutcome::Quit,
                other => CommandOutcome::Error(format!("Unknown command: /{other}")),
            },
        }
    }

    fn resolve_superpower(
        &self,
        command: &str,
        argument: &str,
        active_provider: &ProviderId,
        superpowers: &[SuperpowerDescriptor],
    ) -> CommandOutcome {
        // Find the descriptor whose `command_alias` matches the slash command
        // AND that belongs to the currently active provider.
        let descriptor = superpowers.iter().find(|descriptor| {
            descriptor.provider_id == *active_provider
                && descriptor
                    .command_alias
                    .as_ref()
                    .map(|alias| alias.as_str())
                    .is_some_and(|alias| alias == command)
        });
        let descriptor = match descriptor {
            Some(descriptor) => descriptor.clone(),
            None => {
                return CommandOutcome::Error(format!(
                    "/{command} is not advertised by the active provider \
                     (did you select a different provider via the runtime?)."
                ));
            }
        };

        if argument.is_empty() {
            return CommandOutcome::Error(format!(
                "Usage: /{command} <value>. Allowed: {}",
                self.format_allowed(&descriptor)
            ));
        }

        match superpower_value_for_argument(&descriptor, argument) {
            Ok(value) => CommandOutcome::Superpower {
                provider_id: descriptor.provider_id.clone(),
                descriptor,
                value,
            },
            Err(message) => CommandOutcome::Error(message),
        }
    }

    fn format_allowed(&self, descriptor: &SuperpowerDescriptor) -> String {
        if descriptor.allowed_values.is_empty() {
            return "(free-form value)".into();
        }
        descriptor
            .allowed_values
            .iter()
            .map(|value| match value {
                SuperpowerValue::Choice { value } => value.as_str().to_string(),
                SuperpowerValue::Flag { value } => value.to_string(),
                SuperpowerValue::Number { value } => value.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn help_text(&self) -> String {
        let mut buffer = String::new();
        buffer.push_str("Vesper TUI commands\n");
        buffer.push_str("  /plan <PRD>      — enter Plan Mode and interrogate the requirements\n");
        buffer.push_str(
            "  /review <body>   — surface the generated plan body and wait for /approve\n",
        );
        buffer.push_str("  /approve         — finalize the reviewed plan and start execution\n");
        buffer.push_str("  /cancel          — abort the in-flight plan\n");
        buffer.push_str("  /thinking <lvl>  — session reasoning (disabled/enabled/high/max)\n");
        buffer.push_str("  /model <name>    — switch the active model\n");
        buffer.push_str("  /help            — show this help\n");
        buffer.push_str("  /quit            — exit the TUI\n");
        buffer
    }
}

/// Coerces a free-form argument into the value shape a descriptor expects.
fn superpower_value_for_argument(
    descriptor: &SuperpowerDescriptor,
    argument: &str,
) -> Result<SuperpowerValue, String> {
    use vesper_provider::SuperpowerKind;
    match descriptor.kind {
        SuperpowerKind::Choice => {
            if !descriptor.allowed_values.is_empty() {
                let allowed = descriptor
                    .allowed_values
                    .iter()
                    .filter_map(|value| match value {
                        SuperpowerValue::Choice { value } => Some(value.as_str()),
                        _ => None,
                    })
                    .any(|allowed| allowed == argument);
                if !allowed {
                    return Err(format!(
                        "/{} does not allow {argument:?}.",
                        descriptor
                            .command_alias
                            .as_ref()
                            .map(|alias| alias.as_str())
                            .unwrap_or(descriptor.id.as_str())
                    ));
                }
            }
            vesper_domain::BoundedString::new(argument)
                .map(|value| SuperpowerValue::Choice { value })
                .map_err(|error| error.to_string())
        }
        SuperpowerKind::Toggle => {
            let parsed = match argument.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" | "yes" => true,
                "off" | "false" | "0" | "no" => false,
                _ => return Err("Toggle expects on/off, true/false, 1/0, or yes/no.".into()),
            };
            Ok(SuperpowerValue::Flag { value: parsed })
        }
        SuperpowerKind::Numeric => argument
            .parse::<i64>()
            .map(|value| SuperpowerValue::Number { value })
            .map_err(|_| format!("{argument:?} is not a valid integer")),
    }
}

#[cfg(test)]
mod tests {
    //! Command parsing, registry resolution, and value coercion.

    use super::*;
    use crate::plan_mode::PlanPhase;
    use vesper_domain::BoundedString;
    use vesper_provider::{SuperpowerKind, SuperpowerScope};

    fn provider() -> ProviderId {
        ProviderId::new("test").unwrap()
    }

    fn choice_descriptor(alias: &str, allowed: &[&str]) -> SuperpowerDescriptor {
        SuperpowerDescriptor {
            id: BoundedString::new("test:effort").unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new("Effort").unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Request,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("high").unwrap(),
            },
            allowed_values: allowed
                .iter()
                .map(|raw| SuperpowerValue::Choice {
                    value: BoundedString::new(*raw).unwrap(),
                })
                .collect(),
            command_alias: Some(BoundedString::new(alias).unwrap()),
            help: Some(BoundedString::new("Per-request effort.").unwrap()),
        }
    }

    fn toggle_descriptor(alias: &str) -> SuperpowerDescriptor {
        SuperpowerDescriptor {
            id: BoundedString::new("test:thinking").unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new("Thinking").unwrap(),
            kind: SuperpowerKind::Toggle,
            scope: SuperpowerScope::Both,
            default_value: SuperpowerValue::Flag { value: true },
            allowed_values: Vec::new(),
            command_alias: Some(BoundedString::new(alias).unwrap()),
            help: Some(BoundedString::new("Toggle thinking.").unwrap()),
        }
    }

    #[test]
    fn parse_handles_empty_command_and_prompt() {
        assert_eq!(CommandIntent::parse(""), CommandIntent::Empty);
        assert_eq!(CommandIntent::parse("   "), CommandIntent::Empty);
        assert_eq!(
            CommandIntent::parse("hello world"),
            CommandIntent::Prompt("hello world".into())
        );
        assert_eq!(
            CommandIntent::parse("/plan"),
            CommandIntent::Slash {
                name: "plan".into(),
                argument: "".into()
            }
        );
        assert_eq!(
            CommandIntent::parse("  /THINKING  MAX  "),
            CommandIntent::Slash {
                name: "thinking".into(),
                argument: "MAX".into()
            }
        );
    }

    #[test]
    fn resolve_plan_requires_a_prd() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "plan".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));

        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "plan".into(),
                argument: "ship the matrix".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(
            outcome,
            CommandOutcome::Plan {
                prd: "ship the matrix".into()
            }
        );
    }

    #[test]
    fn resolve_approve_requires_review_phase() {
        let registry = CommandRegistry::stage_11b();
        let provider = provider();
        let mut plan_state = PlanState::default();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "approve".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));

        plan_state.start("prd").unwrap();
        plan_state.finalize("body").unwrap();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "approve".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::PlanGesture(PlanGesture::Approve));
    }

    #[test]
    fn resolve_superpower_targets_active_provider_only() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let active = provider();
        let other = ProviderId::new("other").unwrap();

        // Descriptor belongs to a different provider; command must error.
        let mut foreign = choice_descriptor("thinking", &["disabled", "high"]);
        foreign.provider_id = other;
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "thinking".into(),
                argument: "high".into(),
            },
            &plan_state,
            &active,
            &[foreign],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));

        // Descriptor belongs to the active provider; resolves to a value.
        let descriptor = choice_descriptor("thinking", &["disabled", "high"]);
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "thinking".into(),
                argument: "high".into(),
            },
            &plan_state,
            &active,
            std::slice::from_ref(&descriptor),
        );
        match outcome {
            CommandOutcome::Superpower {
                provider_id,
                descriptor: resolved,
                value,
            } => {
                assert_eq!(provider_id, active);
                assert_eq!(resolved.id, descriptor.id);
                assert!(matches!(value, SuperpowerValue::Choice { .. }));
            }
            other => panic!("expected Superpower outcome, got {other:?}"),
        }
    }

    #[test]
    fn choice_value_rejects_unknown_options() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let descriptor = choice_descriptor("thinking", &["disabled", "high"]);
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "thinking".into(),
                argument: "ludicrous".into(),
            },
            &plan_state,
            &provider,
            &[descriptor],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));
    }

    #[test]
    fn toggle_value_accepts_canonical_forms() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let descriptor = toggle_descriptor("thinking");

        for (raw, expected) in [("on", true), ("OFF", false), ("yes", true), ("0", false)] {
            let outcome = registry.resolve(
                &CommandIntent::Slash {
                    name: "thinking".into(),
                    argument: raw.into(),
                },
                &plan_state,
                &provider,
                std::slice::from_ref(&descriptor),
            );
            match outcome {
                CommandOutcome::Superpower {
                    value: SuperpowerValue::Flag { value },
                    ..
                } => assert_eq!(value, expected),
                other => panic!("expected Flag for {raw:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn help_and_quit_dispatch() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let help = registry.resolve(
            &CommandIntent::Slash {
                name: "help".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(help, CommandOutcome::Help(_)));
        let quit = registry.resolve(
            &CommandIntent::Slash {
                name: "quit".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(quit, CommandOutcome::Quit);
    }

    #[test]
    fn unknown_command_is_an_error() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "frobnicate".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));
    }

    #[test]
    fn registry_knows_its_commands() {
        let registry = CommandRegistry::stage_11b();
        assert!(registry.contains("plan"));
        assert!(registry.contains("approve"));
        assert!(registry.contains("thinking"));
        assert!(!registry.contains("effort"), "ADR 0009 retires /effort");
        assert!(!registry.contains("frobnicate"));
        assert_eq!(
            registry.names(),
            &[
                "plan".to_string(),
                "review".into(),
                "approve".into(),
                "cancel".into(),
                "thinking".into(),
                "model".into(),
                "help".into(),
                "quit".into(),
            ]
        );
    }

    #[test]
    fn resolve_review_requires_a_body() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let empty = registry.resolve(
            &CommandIntent::Slash {
                name: "review".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(empty, CommandOutcome::Error(_)));

        let with_body = registry.resolve(
            &CommandIntent::Slash {
                name: "review".into(),
                argument: "1. do thing\n2. ship".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(
            with_body,
            CommandOutcome::FinalizePlan {
                body: "1. do thing\n2. ship".into()
            }
        );
    }

    #[test]
    fn cancel_is_always_available() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "cancel".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::PlanGesture(PlanGesture::Cancel));
    }

    // Compile-time guard so the test module name does not get pruned.
    const _: PlanPhase = PlanPhase::Normal;
}
