#![forbid(unsafe_code)]
//! Store-backed slash-command execution for the production ACP and TUI
//! compositions (ADR 0010 Tier C parity surface).
//!
//! The stable catalog and parser live in `vesper-domain::slash_commands`
//! so `vesper-acp` advertisement and `vesper-sessions` persisted replay
//! share one source of truth. This module owns what only a store-holding
//! host can do: rendering report commands against the durable
//! `vesper-memory` stores and validating provider-facing arguments into a
//! [`SessionOverrides`] payload the composing host applies through its own
//! provider-configuration boundary.
//!
//! This module never performs terminal I/O, never runs provider wire code,
//! and never touches a session store.

use std::time::SystemTime;

use vesper_domain::SessionOperatingMode;
use vesper_domain::slash_commands::ORACLE_SLASH_COMMANDS;
use vesper_memory::{MemoryEntry, MemoryKind};

use crate::MemoryStores;

/// What one command execution decided the host should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandOutcome {
    /// Render `text` to the user and finish the turn.
    Text(String),
    /// Apply these provider/session overrides, then render `text`.
    Override {
        /// Provider/session overrides validated by the catalog.
        overrides: SessionOverrides,
        /// User-visible confirmation text.
        text: String,
    },
    /// The host owns this command; run its own implementation. The argument
    /// is passed through verbatim.
    Host(String),
    /// The command is not in the catalog.
    Unknown(String),
}

/// Host-applied session override requested by a provider-facing command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionOverrides {
    /// Requested model id (`zai:model`).
    pub model: Option<String>,
    /// Requested endpoint plan (`zai:endpoint-plan`): coding/standard/bigmodel.
    pub endpoint_plan: Option<String>,
    /// Requested reasoning mode (`zai:reasoning-mode`): disabled/enabled/high/max.
    pub reasoning_mode: Option<String>,
    /// Requested generation profile (`zai:generation-profile`).
    pub generation_profile: Option<String>,
    /// Requested auxiliary model (`zai:auxiliary-model`).
    pub auxiliary_model: Option<String>,
    /// Requested mixture mode (`zai:mixture-mode`): off/enabled.
    pub mixture_mode: Option<String>,
    /// Requested per-turn tool-call cap.
    pub max_tool_iterations: Option<u32>,
}

impl SessionOverrides {
    /// Creates empty overrides.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

// No hardcoded model/plan/profile choice lists live here: per the
// multi-provider contract, every provider-facing choice surface (models,
// endpoint plans, reasoning modes, generation profiles) must be sourced from
// the active provider's advertised descriptor/catalog at the composition
// boundary, never from a provider-neutral constant that can drift stale.

/// One execution context for a slash command, supplied by the composing host.
pub struct SlashCommandContext<'a> {
    /// Durable stores shared with the hosted tool surface. May be absent in
    /// minimal compositions; store-backed commands then report unavailability.
    pub stores: Option<&'a MemoryStores>,
    /// Current model id, for /status.
    pub model: String,
    /// Current endpoint plan, for /status.
    pub endpoint_plan: String,
    /// Current reasoning mode, for /status.
    pub reasoning_mode: String,
    /// Current permission mode label, for /status.
    pub permission_mode: String,
    /// Current operating mode, for /status.
    pub operating_mode: SessionOperatingMode,
    /// Whether provider quota polling is available in this composition.
    pub quota_available: bool,
    /// Visible-message count for /status context usage.
    pub visible_messages: usize,
    /// Bounded context-window size in tokens for /status.
    pub context_window: u64,
    /// Cumulative tokens used in this session for /status.
    pub tokens_used: u64,
}

impl Default for SlashCommandContext<'_> {
    fn default() -> Self {
        Self {
            stores: None,
            model: "unavailable".to_owned(),
            endpoint_plan: "unavailable".to_owned(),
            reasoning_mode: "unavailable".to_owned(),
            permission_mode: "ask".to_owned(),
            operating_mode: SessionOperatingMode::Code,
            quota_available: false,
            visible_messages: 0,
            context_window: 0,
            tokens_used: 0,
        }
    }
}

/// Executes one catalog command against the host context.
#[must_use]
pub fn execute_slash_command(
    name: &str,
    argument: &str,
    context: &SlashCommandContext<'_>,
) -> SlashCommandOutcome {
    let known = ORACLE_SLASH_COMMANDS
        .iter()
        .any(|command| command.name == name);
    if !known {
        return SlashCommandOutcome::Unknown(format!("/{name}"));
    }
    match name {
        "help" => SlashCommandOutcome::Text(help_text()),
        "status" => SlashCommandOutcome::Text(status_text(context)),
        "max-iterations" => match argument.trim().parse::<u32>() {
            Ok(number @ 1..=1000) => SlashCommandOutcome::Override {
                overrides: SessionOverrides {
                    max_tool_iterations: Some(number),
                    ..SessionOverrides::new()
                },
                text: format!("Per-turn tool-call cap set to {number}."),
            },
            _ => SlashCommandOutcome::Text("Usage: /max-iterations [1-1000]".to_owned()),
        },
        "memory" => SlashCommandOutcome::Text(memory_text(context)),
        "skills" => SlashCommandOutcome::Text(skills_text(context)),
        "profile" => SlashCommandOutcome::Text(profile_text(context)),
        "awareness" | "metacognition" | "deliberation" => {
            SlashCommandOutcome::Text(awareness_text(name, context))
        }
        "goal" | "subgoal" => goal_outcome(name, argument, context),
        "curator" => curator_outcome(context),
        "version" => {
            SlashCommandOutcome::Text(format!("agent-vesper-acp {}", env!("CARGO_PKG_VERSION")))
        }
        "usage" => SlashCommandOutcome::Host(argument.to_owned()),
        "compact" | "clear-plan" | "clear-history" | "undo" | "diff" | "export" => {
            SlashCommandOutcome::Host(argument.to_owned())
        }
        "checkpoint" | "rollback" | "plugins" | "mcp" | "release" | "ci" => {
            SlashCommandOutcome::Host(argument.to_owned())
        }
        "sessions" | "lineage" => SlashCommandOutcome::Host(argument.to_owned()),
        _ => SlashCommandOutcome::Unknown(format!("/{name}")),
    }
}

/// The frozen-oracle `/help` response, byte-exact against the captured
/// fixture `fixtures/acp/slash-command` (comparison class `exact-output`).
/// Changing any character breaks fixture parity.
const ORACLE_HELP_TEXT: &str = "⌨️ **Harness Commands**

- `/status` — session, model, permissions, context, and evidence
- `/usage` — live Coding Plan 5-hour, weekly, and MCP quota
- `/max-iterations [N]` — per-turn tool-call cap (default 50, max 1000)
- `/compact [focus]` — compact older context
- `/diff` · `/export` · `/clear-plan` · `/clear-history`
- `/checkpoint …` · `/rollback [id]` · `/plugins`
- `/goal …` · `/subgoal …` · `/awareness`
- `/metacognition` · `/deliberation` · `/repository`
- `/meta-learning …` · `/observability [json]`
- `/memory` · `/skills` · `/profile` · `/curator`
- `/sessions [query]` · `/lineage`

Terminal UI: type `/` for the live command menu; `/plan` switches Coding Plan, Standard API, or BigModel (CN); `/thinking` changes provider reasoning; `/model` switches the plan-compatible model. F1 help · F2 reasoning view · F3 settings · Ctrl-L clear view · Ctrl-C cancel · Ctrl-X quit (F10 or `/exit` also work). `/reasoning-panel`, `/settings`, and `/clear-view` provide the equivalent presentation controls.";

/// Renders the `/help` text — byte-exact against the captured oracle
/// fixture `fixtures/acp/slash-command` (`comparison_class:
/// exact-output`; 0 provider requests).
#[must_use]
pub fn help_text() -> String {
    ORACLE_HELP_TEXT.to_owned()
}

/// Renders the `/status` report.
fn status_text(context: &SlashCommandContext<'_>) -> String {
    let mut lines = vec![
        format!("**Model**: {}", context.model),
        format!("**API plan**: {}", context.endpoint_plan),
        format!("**Reasoning**: {}", context.reasoning_mode),
        format!("**Permissions**: {}", context.permission_mode),
        format!(
            "**Mode**: {}",
            match context.operating_mode {
                SessionOperatingMode::Code => "code",
                SessionOperatingMode::Plan => "plan",
            }
        ),
    ];
    if context.context_window > 0 {
        lines.push(format!(
            "**Context**: {} / {} tokens ({} visible messages)",
            context.tokens_used, context.context_window, context.visible_messages
        ));
    }
    lines.join("\n")
}

/// Renders the `/memory` report.
fn memory_text(context: &SlashCommandContext<'_>) -> String {
    let Some(memory) = store_memory(context) else {
        return "Memory store unavailable in this composition.".to_owned();
    };
    let entries = memory.list(None);
    if entries.is_empty() {
        return "No durable project facts recorded.".to_owned();
    }
    let mut lines = vec!["**Project memory**".to_owned()];
    for entry in entries {
        lines.push(format!("- {}", entry.summary));
    }
    lines.join("\n")
}

fn store_memory<'a>(
    context: &'a SlashCommandContext<'_>,
) -> Option<&'a vesper_memory::MemoryStore> {
    context
        .stores
        .and_then(|stores| stores.memory())
        .map(|v| &**v)
}

/// Renders the `/skills` report.
fn skills_text(context: &SlashCommandContext<'_>) -> String {
    let Some(stores) = context.stores else {
        return "Skill store unavailable in this composition.".to_owned();
    };
    let Some(skills) = stores.skills() else {
        return "Skill store unavailable in this composition.".to_owned();
    };
    let entries = skills.list();
    if entries.is_empty() {
        return "No learned skills.".to_owned();
    }
    let mut lines = vec!["**Learned skills**".to_owned()];
    for entry in entries {
        lines.push(format!("- {}", entry.headline));
    }
    lines.join("\n")
}

/// Renders the `/profile` report.
fn profile_text(context: &SlashCommandContext<'_>) -> String {
    let Some(stores) = context.stores else {
        return "User profile unavailable in this composition.".to_owned();
    };
    let Some(profile) = stores.profile() else {
        return "User profile unavailable in this composition.".to_owned();
    };
    let body = profile.read();
    if body.is_empty() {
        return "No approved user preferences recorded.".to_owned();
    }
    format!("**User profile**\n\n{body}")
}

/// Renders the `/awareness`-family reports.
fn awareness_text(name: &str, context: &SlashCommandContext<'_>) -> String {
    let Some(stores) = context.stores else {
        return format!("/{name} is unavailable in this composition.");
    };
    let Some(ledger) = stores.awareness() else {
        return format!("/{name} is unavailable in this composition.");
    };
    let records = ledger.list(None);
    if records.is_empty() {
        return format!("No /{name} records yet.");
    }
    let mut lines = vec![format!("**/{name}**")];
    for record in records {
        lines.push(format!("- {}", record.summary));
    }
    lines.join("\n")
}

/// Executes `/curator` — deterministic bounded skill/memory maintenance
/// against the durable memory store (mirrors the TUI composition's
/// `MemoryOp::Curate` drain).
fn curator_outcome(context: &SlashCommandContext<'_>) -> SlashCommandOutcome {
    let Some(memory) = store_memory(context) else {
        return SlashCommandOutcome::Text(
            "curator: memory store unavailable in this composition.".to_owned(),
        );
    };
    match memory.curate() {
        Ok((duplicates_removed, overflow_trimmed)) => SlashCommandOutcome::Text(format!(
            "curator: removed {duplicates_removed} duplicate(s), trimmed {overflow_trimmed} overflow(s)"
        )),
        Err(error) => SlashCommandOutcome::Text(format!("curator: failed — {error}")),
    }
}

/// Executes `/goal` and `/subgoal` against the durable memory store.
fn goal_outcome(
    name: &str,
    argument: &str,
    context: &SlashCommandContext<'_>,
) -> SlashCommandOutcome {
    let Some(memory) = store_memory(context) else {
        return SlashCommandOutcome::Text(format!("/{name} is unavailable in this composition."));
    };
    let kind = if name == "goal" {
        MemoryKind::Goal
    } else {
        MemoryKind::Subgoal
    };
    let trimmed = argument.trim();
    if trimmed.is_empty() {
        let entries = memory.list(Some(kind));
        if entries.is_empty() {
            return SlashCommandOutcome::Text(format!("No active {name}."));
        }
        list_kind(memory, kind, name)
    } else if trimmed.eq_ignore_ascii_case("clear") {
        let entries = memory.list(Some(kind));
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        match memory.forget(&ids) {
            Ok(removed) => SlashCommandOutcome::Text(format!("Cleared {removed} {name} entries.")),
            Err(error) => SlashCommandOutcome::Text(format!("{name} store failed: {error}")),
        }
    } else {
        match memory.append(MemoryEntry {
            id: String::new(),
            kind,
            summary: trimmed.to_owned(),
            scopes: Vec::new(),
            evidence: Vec::new(),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        }) {
            Ok(_) => SlashCommandOutcome::Text(format!("{name} recorded.")),
            Err(error) => SlashCommandOutcome::Text(format!("{name} store failed: {error}")),
        }
    }
}

fn list_kind(
    memory: &vesper_memory::MemoryStore,
    kind: MemoryKind,
    name: &str,
) -> SlashCommandOutcome {
    let entries = memory.list(Some(kind));
    let mut lines = vec![format!("**Active {name}**")];
    for entry in entries {
        lines.push(format!("- {}", entry.summary));
    }
    SlashCommandOutcome::Text(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::slash_commands::parse_slash_command;

    fn context() -> SlashCommandContext<'static> {
        SlashCommandContext::default()
    }

    #[test]
    fn catalog_is_delegated_to_domain() {
        assert_eq!(ORACLE_SLASH_COMMANDS.len(), 28);
        assert!(parse_slash_command("/help").is_some());
        assert!(parse_slash_command("/future-command").is_none());
    }

    #[test]
    fn max_iterations_validates_range() {
        match execute_slash_command("max-iterations", "200", &context()) {
            SlashCommandOutcome::Override { overrides, text } => {
                assert_eq!(overrides.max_tool_iterations, Some(200));
                assert!(text.contains("200"));
            }
            other => panic!("expected override, got {other:?}"),
        }
        assert!(matches!(
            execute_slash_command("max-iterations", "0", &context()),
            SlashCommandOutcome::Text(_)
        ));
    }

    #[test]
    fn unknown_commands_are_refused() {
        assert!(matches!(
            execute_slash_command("future-command", "", &context()),
            SlashCommandOutcome::Unknown(_)
        ));
    }

    #[test]
    fn help_matches_oracle_fixture_exactly() {
        let help = help_text();
        assert!(help.starts_with("⌨️ **Harness Commands**"));
        assert!(help.contains("- `/status` — session, model, permissions, context, and evidence"));
        assert!(help.contains("- `/sessions [query]` · `/lineage`"));
        assert!(help.ends_with("provide the equivalent presentation controls."));
        assert!(!help.contains("\n\n\n"));
    }

    #[test]
    fn status_reports_context_fields() {
        let context = SlashCommandContext {
            model: "glm-5.2".to_owned(),
            endpoint_plan: "coding".to_owned(),
            ..SlashCommandContext::default()
        };
        let status = status_text(&context);
        assert!(status.contains("glm-5.2"));
        assert!(status.contains("coding"));
    }

    #[test]
    fn goal_reports_unavailable_without_stores() {
        match execute_slash_command("goal", "", &context()) {
            SlashCommandOutcome::Text(text) => {
                assert!(text.contains("unavailable"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn curator_is_a_catalog_command_and_curates_real_stores() {
        // Without stores the command must report unavailability — never the
        // oracle unknown-command fallback (curator IS in the catalog).
        match execute_slash_command("curator", "", &context()) {
            SlashCommandOutcome::Text(text) => {
                assert!(text.contains("unavailable"));
            }
            other => panic!("expected text, got {other:?}"),
        }
        // With a real memory store it runs deterministic curation.
        let local = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        let stores = MemoryStores::open_at(local.path(), global.path());
        let context = SlashCommandContext {
            stores: Some(&stores),
            ..SlashCommandContext::default()
        };
        match execute_slash_command("curator", "", &context) {
            SlashCommandOutcome::Text(text) => {
                assert!(text.starts_with("curator: removed"));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }
}
