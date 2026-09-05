#![forbid(unsafe_code)]
//! Stable provider-neutral slash-command catalog (ADR 0010 Tier C surface).
//!
//! The frozen Python oracle advertises 28 slash commands over ACP through
//! `session/update` (`available_commands_update`) and executes them from
//! prompt text that begins with `/`. This module owns the stable catalog
//! (exact oracle names + descriptions) and the pure parser so every layer —
//! `vesper-acp` advertisement, `vesper-harness` execution, and
//! `vesper-sessions` persisted replay — shares one source of truth.
//!
//! The catalog is plain data: no stores, no provider calls, no host state.
//! Store-backed execution lives in `vesper-harness` (`slash_commands`
//! module); front ends keep their own UX.

/// One slash command from the oracle catalog: stable name (without the
/// leading slash) and the exact description the frozen oracle advertises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandDescriptor {
    /// Stable command name, e.g. `help`.
    pub name: &'static str,
    /// Exact oracle description advertised through `available_commands_update`.
    pub description: &'static str,
}

/// The complete frozen-oracle ACP command catalog, in oracle registration
/// order. Names and descriptions are byte-stable: the ACP adapter emits them
/// verbatim in `available_commands_update`, and the process-transcript tests
/// assert the exact 28-entry list.
pub const ORACLE_SLASH_COMMANDS: [SlashCommandDescriptor; 28] = [
    SlashCommandDescriptor {
        name: "help",
        description: "Show available harness slash commands",
    },
    SlashCommandDescriptor {
        name: "compact",
        description: "Manually trigger context compaction — summarize older messages; optionally add a focus after /compact",
    },
    SlashCommandDescriptor {
        name: "clear-plan",
        description: "Clear the current task plan / todo list",
    },
    SlashCommandDescriptor {
        name: "clear-history",
        description: "Clear all conversation history and start fresh (keeps current model/plan settings)",
    },
    SlashCommandDescriptor {
        name: "diff",
        description: "Show a git diff of all changes made during this session",
    },
    SlashCommandDescriptor {
        name: "export",
        description: "Export the conversation as a Markdown file",
    },
    SlashCommandDescriptor {
        name: "undo",
        description: "Take back the last N user turns (default 1) and prefill the composer with the most recent removed user message",
    },
    SlashCommandDescriptor {
        name: "status",
        description: "Show current model, plan, API endpoint, permission mode, and context usage",
    },
    SlashCommandDescriptor {
        name: "usage",
        description: "Refresh Z.ai Coding Plan 5-hour, weekly, and MCP quota limits",
    },
    SlashCommandDescriptor {
        name: "max-iterations",
        description: "Show or set the per-turn tool-call iteration cap (default 50, max 1000; e.g. /max-iterations 200)",
    },
    SlashCommandDescriptor {
        name: "awareness",
        description: "Show current evidence, uncertainty, contradictions, stale support, capability limits, and completion coverage",
    },
    SlashCommandDescriptor {
        name: "metacognition",
        description: "Show uncertainty classes, adaptive execution mode, risk, and the matching empirical capability profile",
    },
    SlashCommandDescriptor {
        name: "deliberation",
        description: "Show falsifiable hypotheses, evidence-backed test status, the evidence-only critic, and value-of-information action ranking",
    },
    SlashCommandDescriptor {
        name: "memory",
        description: "Show durable project facts learned with permission",
    },
    SlashCommandDescriptor {
        name: "skills",
        description: "List reusable project skills learned after verification",
    },
    SlashCommandDescriptor {
        name: "profile",
        description: "Show approved private preferences shared across projects",
    },
    SlashCommandDescriptor {
        name: "curator",
        description: "Show learned-skill lifecycle and usage status",
    },
    SlashCommandDescriptor {
        name: "sessions",
        description: "Browse recent sessions or search with /sessions <words>",
    },
    SlashCommandDescriptor {
        name: "lineage",
        description: "Show this session's parent, branch root, and direct child sessions",
    },
    SlashCommandDescriptor {
        name: "goal",
        description: "Set, show, pause, resume, or clear a persistent coding goal",
    },
    SlashCommandDescriptor {
        name: "subgoal",
        description: "Add, list, remove, or clear persistent acceptance criteria",
    },
    SlashCommandDescriptor {
        name: "checkpoint",
        description: "Create/list checkpoints, manage deduplicated storage, toggle auto, or set limits",
    },
    SlashCommandDescriptor {
        name: "rollback",
        description: "Conflict-aware rollback to the latest or selected checkpoint",
    },
    SlashCommandDescriptor {
        name: "plugins",
        description: "List and verify installed hash-pinned declarative plugins",
    },
    SlashCommandDescriptor {
        name: "version",
        description: "Show the current project version",
    },
    SlashCommandDescriptor {
        name: "release",
        description: "Cut a release: /release patch|minor|major — bumps version, verifies, commits, tags, pushes",
    },
    SlashCommandDescriptor {
        name: "ci",
        description: "Show recent CI run status via gh CLI",
    },
    SlashCommandDescriptor {
        name: "mcp",
        description: "Manage MCP servers: /mcp, /mcp add <name> <url>, /mcp remove <name>, /mcp tools <name>, /mcp test <name>",
    },
];

/// Host-neutral Vesper extensions implemented by both production hosts.
/// Kept separate from the frozen oracle catalog so compatibility fixtures
/// remain byte-stable while ACP clients can discover the real added surface.
pub const HOST_PARITY_SLASH_COMMANDS: [SlashCommandDescriptor; 17] = [
    SlashCommandDescriptor {
        name: "remember",
        description: "Save a fact to cognitive memory",
    },
    SlashCommandDescriptor {
        name: "recall",
        description: "Search cognitive memory",
    },
    SlashCommandDescriptor {
        name: "forget",
        description: "Delete a cognitive memory by id prefix",
    },
    SlashCommandDescriptor {
        name: "memories",
        description: "Audit project and global cognitive memories",
    },
    SlashCommandDescriptor {
        name: "promote",
        description: "Copy a project memory to global scope",
    },
    SlashCommandDescriptor {
        name: "demote",
        description: "Move a global memory to project scope",
    },
    SlashCommandDescriptor {
        name: "embedding",
        description: "Show, set, or clear cognitive embedding configuration",
    },
    SlashCommandDescriptor {
        name: "reasoning",
        description: "Show or set the reasoning orchestration mode",
    },
    SlashCommandDescriptor {
        name: "repository",
        description: "Show repository-intelligence metadata",
    },
    SlashCommandDescriptor {
        name: "meta-learning",
        description: "Show metacognitive-learning candidates",
    },
    SlashCommandDescriptor {
        name: "observability",
        description: "Show secret-safe local reliability metrics",
    },
    SlashCommandDescriptor {
        name: "journey",
        description: "Show the durable memory and skill timeline",
    },
    SlashCommandDescriptor {
        name: "skill",
        description: "Explicitly run one skill or bundle: /skill <name|bundle:name> [task]",
    },
    SlashCommandDescriptor {
        name: "firewall",
        description: "Show the VRO-13 command firewall status (view/disable-with-restart only)",
    },
    SlashCommandDescriptor {
        name: "sandbox",
        description: "Show the VRO-13 command sandbox status (view only; set AGENT_VESPER_SANDBOX and restart)",
    },
    SlashCommandDescriptor {
        name: "daemon",
        description: "Show the headless daemon's lock state and watcher sweep health (read-only)",
    },
    SlashCommandDescriptor {
        name: "watch",
        description: "Register, list, remove, or probe file-tail watchers (daemon fires bounded turns)",
    },
];

/// Parses raw prompt text into a catalog command name + argument, or `None`
/// when the text does not start with a catalog command. Case-insensitive on
/// the command name; the argument is passed through verbatim (trimmed).
#[must_use]
pub fn parse_slash_command(text: &str) -> Option<(&'static str, &str)> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix('/')?;
    let (name, argument) = match rest.split_once(char::is_whitespace) {
        Some((name, argument)) => (name, argument.trim()),
        None => (rest, ""),
    };
    let lowered = name.to_ascii_lowercase();
    ORACLE_SLASH_COMMANDS
        .iter()
        .find(|command| command.name == lowered)
        .map(|command| (command.name, argument))
}

/// Converts `/skill` arguments into the ordinary prompt consumed by the
/// shared skill router. Both production hosts call this function so explicit
/// skill and bundle syntax cannot drift.
pub fn skill_workflow_prompt(argument: &str) -> Result<String, &'static str> {
    let argument = argument.trim();
    if argument.is_empty() {
        return Err("Usage: /skill <name|bundle:name> [task description]");
    }
    let (selection, task) = argument
        .split_once(char::is_whitespace)
        .map_or((argument, ""), |(selection, task)| (selection, task.trim()));
    let prompt = if let Some(bundle) = selection.strip_prefix("bundle:") {
        if bundle.is_empty() {
            return Err("Usage: /skill bundle:<name> [task description]");
        }
        format!("Use bundle {bundle}. {task}")
    } else {
        format!("Use skill {selection}. {task}")
    };
    Ok(prompt.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_28_oracle_commands_in_order() {
        assert_eq!(ORACLE_SLASH_COMMANDS.len(), 28);
        let names: Vec<_> = ORACLE_SLASH_COMMANDS.iter().map(|c| c.name).collect();
        assert_eq!(
            names[..6],
            [
                "help",
                "compact",
                "clear-plan",
                "clear-history",
                "diff",
                "export"
            ]
        );
        assert_eq!(names[26], "ci");
        assert_eq!(names[27], "mcp");
    }

    #[test]
    fn parse_accepts_known_commands_with_arguments() {
        let (name, argument) = parse_slash_command("/max-iterations 200").unwrap();
        assert_eq!(name, "max-iterations");
        assert_eq!(argument, "200");
        let (name, argument) = parse_slash_command("/STATUS").unwrap();
        assert_eq!(name, "status");
        assert_eq!(argument, "");
        let (name, argument) = parse_slash_command("/compact   focus on tests").unwrap();
        assert_eq!(name, "compact");
        assert_eq!(argument, "focus on tests");
    }

    #[test]
    fn parse_rejects_non_catalog_commands() {
        assert!(parse_slash_command("/future-command").is_none());
        assert!(parse_slash_command("plain prompt").is_none());
        assert!(parse_slash_command("").is_none());
    }

    #[test]
    fn skill_workflow_parser_handles_single_skills_and_bundles() {
        assert_eq!(
            skill_workflow_prompt("xlsx build a forecast").unwrap(),
            "Use skill xlsx. build a forecast"
        );
        assert_eq!(
            skill_workflow_prompt("bundle:evidence investigate").unwrap(),
            "Use bundle evidence. investigate"
        );
        assert!(skill_workflow_prompt("").is_err());
        assert!(skill_workflow_prompt("bundle:").is_err());
    }
}
