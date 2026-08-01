//! Slash-command registry (Stage 11b + Tier C Phase 7 — ADR 0010).
//!
//! Implements the full command surface of the Python oracle's
//! `LOCAL_COMMANDS` (`glm_acp/tui.py:86`). Every one of the 80 oracle
//! commands is accounted for here:
//!
//! - **Plan Mode** (`/plan`, `/approve`, `/cancel`, `/planmode` alias)
//! - **Superpowers** (`/thinking`, `/model`, `/reasoning` alias)
//! - **Context mutations** (`/clear-plan`, `/clear-history`, `/compact`)
//! - **Context views** (`/recap`, `/context`, `/status`, `/tasks`,
//!   `/max-iterations`, `/usage`)
//! - **Workflow prompts** that construct a prompt and trigger a background
//!   `AgentLoop` turn (`/security-review`, `/smart`, `/release`, `/insights`,
//!   `/diff`)
//! - **Deferred commands** that depend on a subsystem Vesper has not built
//!   yet (mobile, voice, MCP, plugins, worktrees, checkpoints, awareness
//!   views, image rendering, etc.) — these resolve to a clear
//!   [`CommandOutcome::Deferred`] notice rather than silently erroring as
//!   "Unknown command", so the migration surface is auditable.
//!
//! The registry is pure: it parses user input into a [`CommandIntent`] and
//! resolves it to a [`CommandOutcome`] without inspecting session state
//! (state inspection happens in [`crate::dispatch::apply_outcome`]).
//! Superpower targets are resolved dynamically against the descriptors
//! advertised by the active provider, keeping the TUI provider-neutral.

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
    /// `/version` — surface the agent binary's version.
    Version,
    /// `/clear-view` — clear only the visible transcript (not Plan Mode state).
    ClearView,

    // === Tier C Phase 7 (ADR 0010) — context mutations ===
    /// `/clear-plan`, `/clear-history` — clear Plan Mode back to NORMAL.
    ClearPlan,
    /// `/compact [N]` — drop all but the last `keep` transcript lines.
    Compact { keep: usize },

    // === Tier C Phase 7 (ADR 0010) — context views ===
    /// Read-only view of session state. The dispatch surface inspects
    /// [`crate::dispatch::SessionState`] to produce the view text.
    ContextView(ViewKind),

    // === Tier C Phase 7 (ADR 0010) — workflow prompts ===
    /// A workflow command built a prompt that should drive a background
    /// `AgentLoop` turn. `display` is shown in the transcript; `prompt` is
    /// what the binary feeds to the loop (drained via
    /// [`crate::dispatch::SessionState::pending_prompt`]).
    Workflow { display: String, prompt: String },

    // === Tier C Phase 7 (ADR 0010) — deferred subsystem commands ===
    /// The command is recognized but depends on a subsystem Vesper has not
    /// built yet. The dispatch surface pushes a clear, actionable warning
    /// rather than erroring as "Unknown command", so the migration surface
    /// stays auditable.
    Deferred {
        /// Canonical command name (without the leading slash).
        command: String,
        /// The subsystem / stage that owns the future implementation.
        reason: String,
    },

    /// Quit/exit requested.
    Quit,
    /// Unknown command or invalid argument; the message is shown to the user.
    Error(String),
}

/// Read-only context view flavours (Tier C Phase 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// `/recap` — one-line summary of the session so far.
    Recap,
    /// `/context` — context-window usage by segment.
    Context,
    /// `/status` — session, model, permissions, context summary.
    Status,
    /// `/tasks` — session dashboard (turn state, queue, tokens, model).
    Tasks,
    /// `/max-iterations` — show the per-turn tool-call iteration cap.
    MaxIterations,
    /// `/usage` — live quota / API-plan usage.
    Usage,
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
///
/// Tier C Phase 7 (ADR 0010): the registry now covers the **entire** Python
/// oracle `LOCAL_COMMANDS` surface (80 commands). Each command is either
/// implemented (Plan Mode, superpowers, context, workflow) or deferred with a
/// clear [`CommandOutcome::Deferred`] reason naming the subsystem/stage that
/// owns the future work. No oracle command falls through to "Unknown command".
#[derive(Debug, Default, Clone)]
pub struct CommandRegistry {
    /// Known command names in stable registration order. The order matches
    /// the oracle's `LOCAL_COMMANDS` so the migration matrix is auditable.
    names: Vec<String>,
}

impl CommandRegistry {
    /// Creates a registry populated with the **complete** Python oracle
    /// command surface (Tier C Phase 7, ADR 0010).
    ///
    /// Every entry in `glm_acp/tui.py:LOCAL_COMMANDS` is registered here.
    /// Implemented commands resolve to their handler; deferred commands
    /// resolve to [`CommandOutcome::Deferred`] with the owning subsystem.
    ///
    /// ADR 0009: `/effort` is retired — the GLM reasoning dial collapsed to
    /// the single `/thinking` control.
    /// ADR 0010 (Tier C Phase 5): `/review` is retired — the model drives
    /// PLANNING → REVIEW via the `update_plan` tool.
    #[must_use]
    pub fn stage_11b() -> Self {
        let names = ORACLE_COMMAND_SURFACE
            .iter()
            .map(|entry| entry.name.to_string())
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
            CommandIntent::Slash { name, argument } => {
                // First gate: is this command part of the oracle surface at
                // all? If not, it's a genuine unknown — not a deferred one.
                if !self.contains(name) {
                    return CommandOutcome::Error(format!("Unknown command: /{name}"));
                }
                self.resolve_known(name, argument, plan_state, active_provider, superpowers)
            }
        }
    }

    /// Resolves a command that is known to be in the oracle surface.
    fn resolve_known(
        &self,
        name: &str,
        argument: &str,
        plan_state: &PlanState,
        active_provider: &ProviderId,
        superpowers: &[SuperpowerDescriptor],
    ) -> CommandOutcome {
        match name {
            // === Plan Mode ===
            "plan" | "planmode" | "api-plan" | "endpoint" => {
                if argument.is_empty() {
                    CommandOutcome::Error("Usage: /plan <your requirements as a PRD>".into())
                } else {
                    CommandOutcome::Plan {
                        prd: argument.to_string(),
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

            // === Superpowers (resolved dynamically against the active provider) ===
            "thinking" | "reasoning" | "model" => self.resolve_superpower(
                superpower_alias(name),
                argument,
                active_provider,
                superpowers,
            ),

            // === Meta ===
            "help" => CommandOutcome::Help(self.help_text()),
            "version" => CommandOutcome::Version,
            "clear-view" => CommandOutcome::ClearView,
            "quit" | "exit" => CommandOutcome::Quit,

            // === Phase 7 — context mutations ===
            "clear-plan" | "clear-history" => CommandOutcome::ClearPlan,
            "compact" => {
                let keep = parse_compact_keep(argument);
                CommandOutcome::Compact { keep }
            }

            // === Phase 7 — context views ===
            "recap" => CommandOutcome::ContextView(ViewKind::Recap),
            "context" => CommandOutcome::ContextView(ViewKind::Context),
            "status" => CommandOutcome::ContextView(ViewKind::Status),
            "tasks" => CommandOutcome::ContextView(ViewKind::Tasks),
            "max-iterations" => CommandOutcome::ContextView(ViewKind::MaxIterations),
            "usage" => CommandOutcome::ContextView(ViewKind::Usage),

            // === Phase 7 — workflow prompts (construct text + trigger AgentLoop) ===
            "security-review" => CommandOutcome::Workflow {
                display: "security-review: scanning the working-tree diff for vulnerabilities."
                    .into(),
                prompt: "Perform a security review of the uncommitted changes in the working \
                         tree. Run `git diff` to see the changes, then identify any security \
                         vulnerabilities, unsafe patterns, secret leaks, injection risks, or \
                         authorization weaknesses. Report each finding with severity, location, \
                         and a recommended fix."
                    .into(),
            },
            "smart" => resolve_smart(argument),
            "release" => CommandOutcome::Workflow {
                display: format!(
                    "release: cutting a {} release from the workspace.",
                    release_bump(argument)
                ),
                prompt: format!(
                    "Cut a {} release from this workspace. Bump the version, update the \
                     changelog, run the full verification gate, commit, tag, and push.",
                    release_bump(argument)
                ),
            },
            "insights" => CommandOutcome::Workflow {
                display: "insights: analyzing the session for friction and improvements.".into(),
                prompt: "Analyze the current session and the recent working-tree changes for \
                         friction points, repeated mistakes, missing tests, architectural \
                         drift, and improvement opportunities. Report concrete, actionable \
                         suggestions ranked by impact."
                    .into(),
            },
            "diff" => CommandOutcome::Workflow {
                display: "diff: showing the working-tree diff via the agent loop.".into(),
                prompt: "Run `git diff` (and `git diff --staged` if there are staged changes) \
                         and summarize the working-tree changes: files touched, lines added / \
                         removed, and a one-paragraph summary of what the changes do."
                    .into(),
            },

            // === Phase 7 — deferred subsystem commands ===
            // Each deferred command resolves to a clear, actionable notice
            // naming the subsystem that owns the future implementation. This
            // keeps the migration surface 100% auditable: every oracle command
            // is recognized, none silently errors as "Unknown".
            other => deferred_outcome(other),
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
        // Phase 7: the help text now reflects the full oracle surface. We
        // group commands by category so the user can scan them quickly, and
        // we surface the deferred commands with their owning subsystem so the
        // migration surface is visible from inside the TUI.
        let mut buffer = String::new();
        buffer.push_str("Vesper TUI commands (Tier C Phase 7 — full oracle parity)\n\n");
        buffer.push_str("Plan Mode:\n");
        buffer.push_str("  /plan <PRD>        enter Plan Mode and interrogate the requirements\n");
        buffer.push_str("  /approve           finalize the reviewed plan and start execution\n");
        buffer.push_str("  /cancel            abort the in-flight plan\n");
        buffer.push_str("  /clear-plan        clear Plan Mode back to NORMAL\n");
        buffer.push_str("\nSuperpowers (resolved against the active provider):\n");
        buffer.push_str("  /thinking <lvl>    session reasoning (disabled/enabled/high/max)\n");
        buffer.push_str("  /model <name>      switch the active model\n");
        buffer.push_str("\nContext:\n");
        buffer.push_str("  /clear-view        clear the visible transcript\n");
        buffer.push_str("  /clear-history     alias for /clear-plan\n");
        buffer.push_str(
            "  /compact [N]       drop all but the last N transcript lines (default 20)\n",
        );
        buffer.push_str("  /recap             one-line session summary\n");
        buffer.push_str("  /context           context-window usage by segment\n");
        buffer.push_str("  /status            session, model, permissions, context summary\n");
        buffer.push_str("  /tasks             session dashboard\n");
        buffer.push_str("  /max-iterations    show the per-turn tool-call iteration cap\n");
        buffer.push_str("  /usage             live quota / API-plan usage (deferred)\n");
        buffer.push_str("\nWorkflows (construct a prompt + trigger a background agent turn):\n");
        buffer.push_str("  /security-review   scan the working-tree diff for vulnerabilities\n");
        buffer.push_str(
            "  /smart <name>      expand a smart-prompt template (pr|review|commit|fix-ci)\n",
        );
        buffer.push_str("  /release [bump]    cut a release (patch|minor|major, default patch)\n");
        buffer.push_str("  /insights          analyze the session for friction and improvements\n");
        buffer.push_str("  /diff              summarize the working-tree diff\n");
        buffer.push_str("\nMeta:\n");
        buffer.push_str("  /version           print the agent version\n");
        buffer.push_str("  /help              show this help\n");
        buffer.push_str("  /quit | /exit      exit the TUI\n");
        buffer.push_str("\nDeferred (subsystem not yet built — see ADR 0010 Phase 7 matrix):\n");
        buffer.push_str("  mobile, sound, mcp, plugins, sessions-new, sessions, lineage,\n");
        buffer.push_str("  branch, rename, loop, checkpoint, rollback, rewind, undo,\n");
        buffer.push_str("  goal, subgoal, awareness, metacognition, deliberation, repository,\n");
        buffer.push_str("  meta-learning, observability, memory, skills, profile, curator,\n");
        buffer.push_str("  journey, ci, export, copy, history, search, prompt, btw, blocks,\n");
        buffer.push_str("  annotate, theme, vim, keybinds, statusline, screen-reader,\n");
        buffer.push_str("  native-mouse, reasoning-panel, toggle-thinking, settings,\n");
        buffer.push_str("  permission, mode, generation, auxiliary, mixture, image,\n");
        buffer.push_str("  image-render, screenshot, attach\n");
        buffer
    }
}

// ===========================================================================
// Phase 7 helpers — pure functions used by the resolver.
// ===========================================================================

/// Maps a slash command name to the superpower alias it targets.
///
/// `/reasoning` is an alias for `/thinking` (oracle convention); `/model`
/// and `/thinking` target their own alias. Returns the alias the descriptor
/// lookup should use.
fn superpower_alias(name: &str) -> &str {
    match name {
        "reasoning" => "thinking",
        other => other,
    }
}

/// Parses the optional keep-count for `/compact [N]`. Defaults to 20 (the
/// oracle's compact default keeps a reasonable recent window). Bounded to
/// `[0, 1000]` so a typo can't drain the whole transcript silently.
fn parse_compact_keep(argument: &str) -> usize {
    if argument.trim().is_empty() {
        return 20;
    }
    match argument.trim().parse::<usize>() {
        Ok(n) => n.min(1000),
        Err(_) => 20,
    }
}

/// Resolves the bump level for `/release [patch|minor|major]`. Defaults to
/// `patch` when no argument is given or the argument is unrecognized.
fn release_bump(argument: &str) -> &'static str {
    match argument.trim().to_ascii_lowercase().as_str() {
        "minor" => "minor",
        "major" => "major",
        _ => "patch",
    }
}

/// Resolves `/smart <name>` against the oracle's `SMART_PROMPTS` table
/// (`glm_acp/tui.py:SMART_PROMPTS`). Unknown names list the available
/// templates; bare `/smart` lists them too.
fn resolve_smart(argument: &str) -> CommandOutcome {
    let name = argument.trim();
    if name.is_empty() {
        return CommandOutcome::Help(
            "Smart-prompt templates (Tier C Phase 7):\n  \
             /smart pr       — create a GitHub pull request for the current branch\n  \
             /smart review   — review the uncommitted working-tree changes\n  \
             /smart commit   — write a conventional commit message for the diff\n  \
             /smart fix-ci   — run CI checks locally and fix the failures"
                .into(),
        );
    }
    let (label, prompt) = match name {
        "pr" => (
            "Create a PR",
            "Create a GitHub pull request for the current branch. Run `git log \
             --oneline origin/main..HEAD` to see the commits, then generate a \
             descriptive title and body. Use `gh pr create` with the title and body.",
        ),
        "review" => (
            "Review changes",
            "Review the uncommitted changes in the working tree for correctness, \
             style, and potential bugs. Run `git diff` to see the changes, then \
             report findings ranked by severity.",
        ),
        "commit" => (
            "Write commit message",
            "Write a clear conventional commit message for the current changes. \
             Run `git diff` and `git status` to see what changed, then propose a \
             commit message. Do not commit yet — show the message for review.",
        ),
        "fix-ci" => (
            "Fix CI failures",
            "The CI may be failing on the current branch. Run the local \
             verification gate (`cargo xtask verify`), identify the failures, \
             diagnose the root cause, and fix them.",
        ),
        other => {
            return CommandOutcome::Error(format!(
                "Unknown smart-prompt: /smart {other}. Available: pr, review, commit, fix-ci."
            ));
        }
    };
    CommandOutcome::Workflow {
        display: format!("smart {name}: {label}."),
        prompt: prompt.into(),
    }
}

/// Returns the [`CommandOutcome::Deferred`] notice for a command that depends
/// on a subsystem Vesper has not built yet. The reason names the owning
/// subsystem / stage so the migration surface stays auditable.
fn deferred_outcome(command: &str) -> CommandOutcome {
    let reason = deferred_reason(command)
        .unwrap_or_else(|| "deferred to a future stage (no subsystem owns it yet)".to_string());
    CommandOutcome::Deferred {
        command: command.to_string(),
        reason,
    }
}

/// Maps a deferred command name to a human-readable reason naming the owning
/// subsystem. Returns `None` for implemented commands (which never reach the
/// deferred path).
fn deferred_reason(command: &str) -> Option<String> {
    let reason: &str = match command {
        // Mobile / voice / sound subsystem
        "mobile" => "mobile companion subsystem (Stage 13+, deferred)",
        "sound" => "notification-sound subsystem (Stage 13+, deferred)",

        // MCP / plugins
        "mcp" => "MCP server-connection subsystem (Stage 14+, deferred)",
        "plugins" => "plugin publisher / install subsystem (Stage 14+, deferred)",

        // Worktree session subsystem
        "sessions-new" | "sessions" | "lineage" | "branch" | "rename" => {
            "worktree session subsystem (Stage 12+, deferred)"
        }

        // Cron / loop scheduler
        "loop" => "cron / loop scheduler subsystem (Stage 15+, deferred)",

        // Checkpoint / rollback subsystem
        "checkpoint" | "rollback" | "rewind" | "undo" => {
            "conversation checkpoint subsystem (Stage 12+, deferred)"
        }

        // Awareness / memory / skills views (the data lives in the harness;
        // surfacing it in the TUI is a later stage)
        "goal" | "subgoal" => "persistent-goal awareness subsystem (Stage 16+, deferred)",
        "awareness" => "epistemic-awareness view (Stage 16+, deferred)",
        "metacognition" => "metacognitive-assessment view (Stage 16+, deferred)",
        "deliberation" => "grounded-deliberation view (Stage 16+, deferred)",
        "repository" => "repository-intelligence view (Stage 16+, deferred)",
        "meta-learning" => "metacognitive-learning view (Stage 16+, deferred)",
        "observability" => "local reliability-metrics view (Stage 16+, deferred)",
        "memory" => "project-memory view (Stage 16+, deferred)",
        "skills" => "learned-skills view (Stage 16+, deferred)",
        "profile" => "user-profile view (Stage 16+, deferred)",
        "curator" => "skill-curator subsystem (Stage 16+, deferred)",
        "journey" => "memory + skills timeline view (Stage 16+, deferred)",

        // CI integration
        "ci" => "CI-status integration (Stage 17+, deferred)",

        // Export / clipboard
        "export" => "session-export subsystem (Stage 12+, deferred)",
        "copy" => "clipboard subsystem (Stage 12+, deferred)",

        // Composer features
        "history" => "session-history browser (Stage 12+, deferred)",
        "search" => "conversation-search composer (Stage 12+, deferred)",
        "prompt" => "$EDITOR multi-line composer (Stage 12+, deferred)",
        "btw" => "side-question composer (Stage 12+, deferred)",
        "blocks" => "code-block picker composer (Stage 12+, deferred)",
        "annotate" => "diff-hunk annotation composer (Stage 12+, deferred)",

        // Textual-specific UI (Vesper uses ratatui, not Textual; these need
        // native reimplementation)
        "theme" => "ratatui theme subsystem (Stage 18+, deferred)",
        "vim" => "vim-mode composer (Stage 18+, deferred)",
        "keybinds" => "configurable keybind subsystem (Stage 18+, deferred)",
        "statusline" => "configurable statusline subsystem (Stage 18+, deferred)",
        "screen-reader" => "screen-reader mode (Stage 18+, deferred)",
        "native-mouse" => "native mouse-mode toggle (Stage 18+, deferred)",
        "reasoning-panel" | "toggle-thinking" => "reasoning-panel UI toggle (Stage 18+, deferred)",

        // Live session settings (provider-quota / mode UI)
        "settings" => "live session-settings panel (Stage 18+, deferred)",
        "permission" => "permission-mode UI (Stage 18+, deferred)",
        "mode" => "session-mode UI (Stage 18+, deferred)",
        "generation" => "generation-profile UI (Stage 18+, deferred)",
        "auxiliary" => "auxiliary-model UI (Stage 18+, deferred)",
        "mixture" => "mixture-of-agents UI (Stage 18+, deferred)",

        // Image subsystem
        "image" | "attach" => "image-queue subsystem (Stage 19+, deferred)",
        "image-render" => "inline image-render subsystem (Stage 19+, deferred)",
        "screenshot" => "screenshot-capture subsystem (Stage 19+, deferred)",

        // Anything else that slipped through — fail closed with a generic
        // reason so the migration matrix stays complete.
        _ => return None,
    };
    Some(reason.to_string())
}

// ===========================================================================
// The oracle command surface — the single source of truth for the migration
// matrix. Every entry in `glm_acp/tui.py:LOCAL_COMMANDS` is listed here in
// the oracle's declaration order, so a diff against the oracle is trivial.
// ===========================================================================

/// One row of the oracle command surface: name + one-line description.
struct OracleCommandEntry {
    name: &'static str,
    #[allow(dead_code)]
    description: &'static str,
}

/// The complete command surface Vesper recognizes: every entry in the
/// Python oracle `LOCAL_COMMANDS` (`glm_acp/tui.py:86`) PLUS the Vesper-native
/// Plan Mode gestures (`approve`, `cancel`) and the `quit` exit command, which
/// the oracle handles via keybindings rather than slash commands but Vesper
/// surfaces as first-class slash commands.
///
/// This is the authoritative migration matrix: every command here is
/// registered in [`CommandRegistry::stage_11b`] and resolved by
/// [`CommandRegistry::resolve_known`] (either to a real handler or to a
/// [`CommandOutcome::Deferred`] notice naming the owning subsystem).
#[rustfmt::skip]
const ORACLE_COMMAND_SURFACE: &[OracleCommandEntry] = &[
    // === Vesper-native (not in oracle LOCAL_COMMANDS; handled via keybindings there) ===
    OracleCommandEntry { name: "approve",           description: "Finalize the reviewed plan and start execution (Vesper-native)" },
    OracleCommandEntry { name: "cancel",            description: "Abort the in-flight plan (Vesper-native)" },
    OracleCommandEntry { name: "quit",              description: "Exit the TUI (Vesper-native; oracle uses Ctrl+X)" },
    // === Python oracle LOCAL_COMMANDS (glm_acp/tui.py:86) — in declaration order ===
    OracleCommandEntry { name: "plan",              description: "Switch between Coding Plan, Standard API, and BigModel (CN)" },
    OracleCommandEntry { name: "thinking",          description: "Change provider thinking: Off, Standard, Deep High, or Deep Max" },
    OracleCommandEntry { name: "model",             description: "Change the active GLM model" },
    OracleCommandEntry { name: "usage",             description: "Refresh live 5-hour, weekly, and MCP Coding Plan quota" },
    OracleCommandEntry { name: "permission",        description: "Change Ask, Read Only, or Bypass permissions" },
    OracleCommandEntry { name: "mode",              description: "Change Ask or Code session mode" },
    OracleCommandEntry { name: "generation",        description: "Change the generation style" },
    OracleCommandEntry { name: "auxiliary",         description: "Change the auxiliary model" },
    OracleCommandEntry { name: "mixture",           description: "Enable or disable Mixture of Agents" },
    OracleCommandEntry { name: "settings",          description: "Open all live session settings" },
    OracleCommandEntry { name: "reasoning",         description: "Alias for /thinking" },
    OracleCommandEntry { name: "api-plan",          description: "Alias for /plan" },
    OracleCommandEntry { name: "endpoint",          description: "Alias for /plan" },
    OracleCommandEntry { name: "reasoning-panel",   description: "Show or hide the live reasoning panel" },
    OracleCommandEntry { name: "toggle-thinking",   description: "Alias for /reasoning-panel" },
    OracleCommandEntry { name: "clear-view",        description: "Clear only the visible transcript" },
    OracleCommandEntry { name: "max-iterations",    description: "Show or set the per-turn tool-call iteration cap" },
    OracleCommandEntry { name: "recap",             description: "Show a one-line summary of the session so far" },
    OracleCommandEntry { name: "blocks",            description: "Pick a code block from recent responses to copy or save" },
    OracleCommandEntry { name: "statusline",        description: "Choose which sidebar segments are visible" },
    OracleCommandEntry { name: "context",           description: "Visualize context-window usage by segment" },
    OracleCommandEntry { name: "btw",               description: "Ask a side question without polluting the conversation" },
    OracleCommandEntry { name: "theme",             description: "Switch the visual theme" },
    OracleCommandEntry { name: "tasks",             description: "Show the session dashboard" },
    OracleCommandEntry { name: "release",           description: "Cut a release from the workspace" },
    OracleCommandEntry { name: "insights",          description: "Analyze the session for friction points" },
    OracleCommandEntry { name: "loop",              description: "Run a prompt repeatedly at an interval" },
    OracleCommandEntry { name: "security-review",   description: "Scan the working-tree diff for security vulnerabilities" },
    OracleCommandEntry { name: "rewind",            description: "Alias for /rollback — rewind to a prior checkpoint" },
    OracleCommandEntry { name: "smart",             description: "Expand a smart-prompt template with git context" },
    OracleCommandEntry { name: "sound",             description: "Toggle notification sounds on/off for this session" },
    OracleCommandEntry { name: "screen-reader",     description: "Toggle screen-reader mode" },
    OracleCommandEntry { name: "keybinds",          description: "Customize TUI F-key and Ctrl-key bindings" },
    OracleCommandEntry { name: "vim",               description: "Toggle vim-mode composer" },
    OracleCommandEntry { name: "annotate",          description: "Annotate working-tree diff hunks for the next prompt" },
    OracleCommandEntry { name: "image-render",      description: "Render the last image inline" },
    OracleCommandEntry { name: "screenshot",        description: "Capture a screenshot and queue it for the next prompt" },
    OracleCommandEntry { name: "attach",            description: "Queue an image file for the next prompt" },
    OracleCommandEntry { name: "sessions-new",      description: "Create and switch to a parallel Git worktree session" },
    OracleCommandEntry { name: "mobile",            description: "Start or stop scan-to-approve mobile companion" },
    OracleCommandEntry { name: "rename",            description: "Rename the current session" },
    OracleCommandEntry { name: "branch",            description: "Fork the current session to try a different direction" },
    OracleCommandEntry { name: "status",            description: "Show session, model, permissions, context, and live evidence" },
    OracleCommandEntry { name: "compact",           description: "Compact older context" },
    OracleCommandEntry { name: "diff",              description: "Show the working-tree diff in the transcript" },
    OracleCommandEntry { name: "clear-plan",        description: "Clear the active plan" },
    OracleCommandEntry { name: "clear-history",     description: "Clear saved session history for this workspace" },
    OracleCommandEntry { name: "checkpoint",        description: "Manage conversation checkpoints" },
    OracleCommandEntry { name: "rollback",          description: "Roll back to a prior checkpoint" },
    OracleCommandEntry { name: "plugins",           description: "List trusted plugin publishers and installed plugins" },
    OracleCommandEntry { name: "goal",              description: "Set or inspect a persistent goal" },
    OracleCommandEntry { name: "subgoal",           description: "Add an acceptance criterion to the current persistent goal" },
    OracleCommandEntry { name: "awareness",         description: "Show the live epistemic state" },
    OracleCommandEntry { name: "metacognition",     description: "Show the metacognitive assessment" },
    OracleCommandEntry { name: "deliberation",      description: "Show the active grounded-deliberation hypotheses" },
    OracleCommandEntry { name: "repository",        description: "Show repository-intelligence metadata" },
    OracleCommandEntry { name: "meta-learning",     description: "Show metacognitive-learning candidates" },
    OracleCommandEntry { name: "observability",     description: "Show secret-safe local reliability metrics" },
    OracleCommandEntry { name: "memory",            description: "Show project-local memory entries" },
    OracleCommandEntry { name: "skills",            description: "List learned project skills" },
    OracleCommandEntry { name: "profile",           description: "Show approved cross-project user-profile preferences" },
    OracleCommandEntry { name: "curator",           description: "Run deterministic skill maintenance" },
    OracleCommandEntry { name: "sessions",          description: "Search past sessions" },
    OracleCommandEntry { name: "lineage",           description: "Show the session-lineage chain" },
    OracleCommandEntry { name: "mcp",               description: "Manage MCP server connections" },
    OracleCommandEntry { name: "ci",                description: "Show CI status for the current branch" },
    OracleCommandEntry { name: "version",           description: "Show package, Python, and platform version info" },
    OracleCommandEntry { name: "help",              description: "Show the full harness command reference" },
    OracleCommandEntry { name: "copy",              description: "Copy the last response to clipboard" },
    OracleCommandEntry { name: "history",           description: "Browse and resume past sessions" },
    OracleCommandEntry { name: "search",            description: "Grep the current conversation" },
    OracleCommandEntry { name: "export",            description: "Export current session" },
    OracleCommandEntry { name: "undo",              description: "Take back the last N user turns" },
    OracleCommandEntry { name: "prompt",            description: "Compose your next prompt in $EDITOR" },
    OracleCommandEntry { name: "journey",           description: "Show the timeline of memories + skills + profile" },
    OracleCommandEntry { name: "native-mouse",      description: "Toggle native terminal mouse mode" },
    OracleCommandEntry { name: "planmode",          description: "Activate Plan Mode with a PRD" },
    OracleCommandEntry { name: "image",             description: "Queue an image for the next prompt" },
    OracleCommandEntry { name: "exit",              description: "Close the terminal agent" },
];

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
        // Phase 7 (ADR 0010): the registry now covers the ENTIRE Python oracle
        // LOCAL_COMMANDS surface (80 commands) PLUS the three Vesper-native
        // commands (approve, cancel, quit) the oracle handles via keybindings.
        // Every command below must be recognized.
        for known in [
            "plan",
            "approve",
            "cancel",
            "thinking",
            "model",
            "version",
            "clear-view",
            "help",
            "quit",
            "exit",
            "reasoning",
            "api-plan",
            "endpoint",
            "planmode",
            "clear-plan",
            "clear-history",
            "compact",
            "recap",
            "context",
            "status",
            "tasks",
            "max-iterations",
            "usage",
            "security-review",
            "smart",
            "release",
            "insights",
            "diff",
            "mobile",
            "mcp",
            "plugins",
            "checkpoint",
            "rollback",
            "rewind",
            "undo",
            "goal",
            "awareness",
            "memory",
            "skills",
            "ci",
            "export",
        ] {
            assert!(
                registry.contains(known),
                "Phase 7 parity: /{known} must be registered"
            );
        }
        // Retired commands are NOT in the surface.
        assert!(!registry.contains("effort"), "ADR 0009 retires /effort");
        assert!(
            !registry.contains("review"),
            "ADR 0010 (Tier C) retires /review — the model drives PLANNING → REVIEW"
        );
        // Genuinely unknown commands are still unknown.
        assert!(!registry.contains("frobnicate"));
        // The full surface count: 79 distinct oracle command names (the
        // oracle's `/export last` parses as `/export` with arg `last`) + 3
        // Vesper-native (approve, cancel, quit) = 82 total.
        assert_eq!(
            registry.names().len(),
            82,
            "Phase 7 parity: 79 oracle commands + 3 Vesper-native = 82 total"
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

    // ===================================================================
    // Phase 7 (ADR 0010) — full-surface parity tests.
    // ===================================================================

    /// Helper: resolve a bare slash command (no argument) against the
    /// Stage-11b registry with an empty plan state and no superpowers.
    fn resolve_bare(name: &str) -> CommandOutcome {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        registry.resolve(
            &CommandIntent::Slash {
                name: name.into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        )
    }

    #[test]
    fn phase7_plan_aliases_resolve_to_plan() {
        // /planmode, /api-plan, /endpoint are oracle aliases for /plan.
        for alias in ["planmode", "api-plan", "endpoint"] {
            let registry = CommandRegistry::stage_11b();
            let plan_state = PlanState::default();
            let provider = provider();
            let outcome = registry.resolve(
                &CommandIntent::Slash {
                    name: alias.into(),
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
                },
                "/{alias} should alias /plan"
            );
        }
    }

    #[test]
    fn phase7_clear_plan_and_clear_history_resolve_to_clearplan() {
        for name in ["clear-plan", "clear-history"] {
            assert_eq!(resolve_bare(name), CommandOutcome::ClearPlan);
        }
    }

    #[test]
    fn phase7_compact_defaults_to_20_and_parses_argument() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        // Bare /compact → keep 20.
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "compact".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::Compact { keep: 20 });

        // /compact 5 → keep 5.
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "compact".into(),
                argument: "5".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::Compact { keep: 5 });

        // /compact bogus → falls back to 20.
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "compact".into(),
                argument: "bogus".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::Compact { keep: 20 });
    }

    #[test]
    fn phase7_context_views_resolve_to_the_right_kind() {
        assert_eq!(
            resolve_bare("recap"),
            CommandOutcome::ContextView(ViewKind::Recap)
        );
        assert_eq!(
            resolve_bare("context"),
            CommandOutcome::ContextView(ViewKind::Context)
        );
        assert_eq!(
            resolve_bare("status"),
            CommandOutcome::ContextView(ViewKind::Status)
        );
        assert_eq!(
            resolve_bare("tasks"),
            CommandOutcome::ContextView(ViewKind::Tasks)
        );
        assert_eq!(
            resolve_bare("max-iterations"),
            CommandOutcome::ContextView(ViewKind::MaxIterations)
        );
        assert_eq!(
            resolve_bare("usage"),
            CommandOutcome::ContextView(ViewKind::Usage)
        );
    }

    #[test]
    fn phase7_security_review_builds_a_workflow_prompt() {
        let outcome = resolve_bare("security-review");
        match outcome {
            CommandOutcome::Workflow { display, prompt } => {
                assert!(display.contains("security-review"));
                assert!(prompt.to_lowercase().contains("security"));
                assert!(prompt.contains("git diff"));
            }
            other => panic!("expected Workflow, got {other:?}"),
        }
    }

    #[test]
    fn phase7_smart_templates_expand_correctly() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        for (name, expected_label_fragment) in [
            ("pr", "Create a PR"),
            ("review", "Review changes"),
            ("commit", "Write commit message"),
            ("fix-ci", "Fix CI failures"),
        ] {
            let outcome = registry.resolve(
                &CommandIntent::Slash {
                    name: "smart".into(),
                    argument: name.into(),
                },
                &plan_state,
                &provider,
                &[],
            );
            match outcome {
                CommandOutcome::Workflow { display, prompt } => {
                    assert!(
                        display.contains(expected_label_fragment),
                        "smart {name} display should mention {expected_label_fragment}: {display}"
                    );
                    assert!(!prompt.is_empty());
                }
                other => panic!("smart {name} should be Workflow, got {other:?}"),
            }
        }
        // Bare /smart lists templates.
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "smart".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(outcome, CommandOutcome::Help(_)));
        // Unknown template errors.
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "smart".into(),
                argument: "bogus".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));
    }

    #[test]
    fn phase7_release_picks_the_right_bump() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        for (arg, expected) in [
            ("", "patch"),
            ("patch", "patch"),
            ("minor", "minor"),
            ("major", "major"),
            ("bogus", "patch"),
        ] {
            let outcome = registry.resolve(
                &CommandIntent::Slash {
                    name: "release".into(),
                    argument: arg.into(),
                },
                &plan_state,
                &provider,
                &[],
            );
            match outcome {
                CommandOutcome::Workflow { display, .. } => {
                    assert!(
                        display.contains(expected),
                        "release {arg:?} should mention {expected}: {display}"
                    );
                }
                other => panic!("release {arg:?} should be Workflow, got {other:?}"),
            }
        }
    }

    #[test]
    fn phase7_insights_and_diff_build_workflow_prompts() {
        for name in ["insights", "diff"] {
            match resolve_bare(name) {
                CommandOutcome::Workflow { display, prompt } => {
                    assert!(display.starts_with(name));
                    assert!(!prompt.is_empty());
                }
                other => panic!("/{name} should be Workflow, got {other:?}"),
            }
        }
    }

    #[test]
    fn phase7_every_deferred_command_names_a_subsystem() {
        // Every deferred command must resolve to Deferred with a non-empty
        // reason naming the owning subsystem — never Error("Unknown command").
        // This is the heart of the parity contract: no oracle command is
        // silently dropped.
        let deferred_commands = [
            "mobile",
            "sound",
            "mcp",
            "plugins",
            "sessions-new",
            "sessions",
            "lineage",
            "branch",
            "rename",
            "loop",
            "checkpoint",
            "rollback",
            "rewind",
            "undo",
            "goal",
            "subgoal",
            "awareness",
            "metacognition",
            "deliberation",
            "repository",
            "meta-learning",
            "observability",
            "memory",
            "skills",
            "profile",
            "curator",
            "journey",
            "ci",
            "export",
            "copy",
            "history",
            "search",
            "prompt",
            "btw",
            "blocks",
            "annotate",
            "theme",
            "vim",
            "keybinds",
            "statusline",
            "screen-reader",
            "native-mouse",
            "reasoning-panel",
            "toggle-thinking",
            "settings",
            "permission",
            "mode",
            "generation",
            "auxiliary",
            "mixture",
            "image",
            "attach",
            "image-render",
            "screenshot",
            "usage",
        ];
        for name in deferred_commands {
            match resolve_bare(name) {
                CommandOutcome::Deferred { command, reason } => {
                    assert_eq!(command, name);
                    assert!(
                        !reason.is_empty(),
                        "/{name} must name an owning subsystem in its reason"
                    );
                }
                CommandOutcome::ContextView(ViewKind::Usage) => {
                    // /usage is a ContextView in our resolver; fine.
                }
                other => {
                    panic!("/{name} should be Deferred (or ContextView for usage), got {other:?}")
                }
            }
        }
    }

    #[test]
    fn phase7_no_oracle_command_errors_as_unknown() {
        // The decisive parity assertion: iterate EVERY registered command
        // name and confirm none of them resolve to "Unknown command" error.
        // (Some may resolve to Error for missing arguments, like /plan with
        // no PRD — but the error message must NOT be "Unknown command: ...".)
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        for name in registry.names() {
            let outcome = registry.resolve(
                &CommandIntent::Slash {
                    name: name.clone(),
                    argument: "test-arg".into(),
                },
                &plan_state,
                &provider,
                &[],
            );
            if let CommandOutcome::Error(message) = &outcome {
                assert!(
                    !message.starts_with("Unknown command"),
                    "/{name} must not error as Unknown — it is in the oracle surface"
                );
            }
        }
    }

    // Compile-time guard so the test module name does not get pruned.
    const _: PlanPhase = PlanPhase::Normal;
}
