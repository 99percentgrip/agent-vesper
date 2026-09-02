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
//! - Every registered command has a concrete route. An accidental missing
//!   route is reported as an internal parity violation so it cannot masquerade
//!   as a supported feature.
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

    /// One validated live session setting selected through the command picker.
    SessionConfig {
        /// Stable setting identity.
        key: SessionConfigKey,
        /// Canonical selected value.
        value: String,
    },

    /// A terminal-only view action.
    Ui(UiAction),

    /// Search the visible conversation for a case-insensitive query.
    Search { query: String },
    /// Load one persisted TUI session selected from the native history picker.
    History { session_id: String },
    /// Queue, render, or capture an image through the native media bridge.
    Media(MediaOp),
    /// Ask the configured auxiliary model without mutating main history.
    AuxiliaryQuestion { question: String },
    /// Query the active provider's live account/plan usage endpoint.
    ProviderUsage,
    /// Copy or write one fenced code block from recent assistant output.
    CodeBlock { index: usize, write: bool },

    // === Tier C Phase 7 (ADR 0010) — context mutations ===
    /// `/clear-plan`, `/clear-history` — clear Plan Mode back to NORMAL.
    ClearPlan,
    /// `/compact [N]` — drop all but the last `keep` transcript lines.
    Compact { keep: usize },

    // === Tier C Phase 7 (ADR 0010) — context views ===
    /// Read-only view of session state. The dispatch surface inspects
    /// [`crate::dispatch::SessionState`] to produce the view text.
    ContextView(ViewKind),

    // === VRO-13 PR-4 — sandbox host parity ===
    /// `/sandbox on|off` — restart-instruction arms of the sandbox panel.
    /// The route is boot-resolved (once-only holder), so `on`/`off` answer
    /// with the honest edit-config-and-restart step rather than a runtime
    /// toggle, mirroring the ACP host byte-for-byte.
    SandboxControl(SandboxControl),

    // === Tier C Phase 7 (ADR 0010) — workflow prompts ===
    /// A workflow command built a prompt that should drive a background
    /// `AgentLoop` turn. `display` is shown in the transcript; `prompt` is
    /// what the binary feeds to the loop (drained via
    /// [`crate::dispatch::SessionState::pending_prompt`]).
    Workflow { display: String, prompt: String },

    // === Tier C Phase 8 (ADR 0011) — memory subsystem commands ===
    /// A memory command resolved to a structured [`MemoryOp`] that the
    /// binary will execute against the durable
    /// [`vesper_memory::MemoryStore`] / [`vesper_memory::SkillStore`] /
    /// [`vesper_memory::UserProfile`] / [`vesper_memory::AwarenessLedger`].
    /// `dispatch` records this on `SessionState.pending_memory_op`; the
    /// binary drains it after dispatch (same pattern as
    /// `pending_prompt`).
    Memory(MemoryOp),

    // === Tier C Phase 9 (ADR 0012) — checkpoints subsystem commands ===
    /// A checkpoint command resolved to a structured [`CheckpointOp`] that
    /// the binary will execute against the durable
    /// [`vesper_checkpoints::CheckpointsLedger`] /
    /// [`vesper_checkpoints::SessionLineage`] /
    /// [`vesper_checkpoints::CronRegistry`] /
    /// [`vesper_checkpoints::SessionExporter`] /
    /// [`vesper_checkpoints::ClipboardPort`] /
    /// [`vesper_checkpoints::CiStatusReader`]. `dispatch` records this on
    /// `SessionState.pending_checkpoint_op`; the binary drains it after
    /// dispatch (same pattern as `pending_memory_op`).
    Checkpoint(CheckpointOp),

    // === Tier C Phase 10 (ADR 0013) — MCP & plugins subsystem commands ===
    /// An MCP or plugins command resolved to a structured [`McpOp`] that
    /// the binary will execute against the durable
    /// [`vesper_mcp::McpRegistry`] / [`vesper_mcp::McpClient`] /
    /// [`vesper_mcp::PluginLoader`] / [`vesper_mcp::TrustedPublishers`].
    /// `dispatch` records this on `SessionState.pending_mcp_op`; the
    /// binary drains it after dispatch (same pattern as
    /// `pending_checkpoint_op`).
    Mcp(McpOp),

    // === Phase 11 (ADR 0015 — Stage 16) — cognitive-memory commands ===
    /// A cognitive-memory command (`/remember`, `/recall`, `/forget`)
    /// resolved to a structured [`CognitionOp`] that the binary will
    /// execute against the durable [`vesper_cognition::CognitiveMemory`].
    Cognition(CognitionOp),

    /// ADR 0016 — `/embedding` command. Resolved to a structured
    /// [`EmbeddingOp`] that the binary drains against the
    /// `CognitionBundle` (write `embedding.json`, hot-reload the embedder,
    /// or render the live status block).
    Embedding(EmbeddingOp),

    /// VRO-8 (PRD §8.1) — `/reasoning set mode=<auto|fast|balanced|deep|
    /// maximum|off>` overrides the TaskProfiler. The dispatch surface
    /// records this on `SessionState.reasoning_mode_override`; the binary
    /// computes the effective mode and routes the next VRO turn through it.
    /// `mode == Auto` clears any prior override (returns to profiler
    /// defaults).
    ReasoningOverride {
        /// The user-selected reasoning mode. `Auto` clears any prior override.
        mode: vesper_domain::ReasoningMode,
    },
    /// VRO-8 — bare `/reasoning` with no argument. Surfaces the current
    /// override (or "auto") in the status line; `dispatch` reads the live
    /// `SessionState.reasoning_mode_override`.
    ReasoningStatus,

    /// Set the session-scoped VesperLens planning-interview question policy.
    InterviewLimit(InterviewQuestionLimit),
    /// Show the current VesperLens planning-interview question policy.
    InterviewLimitStatus,

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
    /// `/permission` — current session permission mode.
    Permission,
    /// `/firewall` — command firewall panel (view + disable-with-restart).
    Firewall,
    /// `/sandbox` — scope-demanded sandbox panel (view + backend choice,
    /// resolved once at boot from `[sandbox]` + `AGENT_VESPER_SANDBOX`).
    Sandbox,
}

/// VRO-13 PR-4: `/sandbox on|off` control arms. The sandbox route is
/// process-global and boot-resolved (once-only holder), so neither arm can
/// be a live runtime toggle — both answer with the honest restart
/// instruction, byte-identical in the TUI and ACP hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxControl {
    /// `/sandbox on` — the demand lives in `[sandbox]`; explains the
    /// edit-config-and-restart step.
    On,
    /// `/sandbox off` — explains the `AGENT_VESPER_SANDBOX=off` restart.
    Off,
}

/// Live session settings implemented by the native Rust harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionConfigKey {
    EndpointPlan,
    Permission,
    OperatingMode,
    GenerationProfile,
    AuxiliaryModel,
    MixtureMode,
    Theme,
    MaxIterations,
}

/// Session-scoped question policy for VesperLens planning interviews.
///
/// A fixed value is a maximum, not a quota: the agent should ask fewer
/// questions when the requirements are already clear. `Auto` lets the agent
/// choose the count while retaining a deterministic safety ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterviewQuestionLimit {
    Auto,
    Fixed(u8),
}

/// Backward-compatible default used until `/interview-limit` changes it.
pub const DEFAULT_INTERVIEW_QUESTION_LIMIT: u8 = 4;
/// Safety ceiling for both explicit and agent-selected interview sizes.
pub const MAX_INTERVIEW_QUESTIONS: u8 = 12;

impl Default for InterviewQuestionLimit {
    fn default() -> Self {
        Self::Fixed(DEFAULT_INTERVIEW_QUESTION_LIMIT)
    }
}

impl InterviewQuestionLimit {
    #[must_use]
    pub fn max_questions(self) -> usize {
        match self {
            Self::Auto => usize::from(MAX_INTERVIEW_QUESTIONS),
            Self::Fixed(value) => usize::from(value),
        }
    }

    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Auto => format!("auto (agent chooses 1-{MAX_INTERVIEW_QUESTIONS})"),
            Self::Fixed(DEFAULT_INTERVIEW_QUESTION_LIMIT) => {
                format!("{DEFAULT_INTERVIEW_QUESTION_LIMIT} (default maximum)")
            }
            Self::Fixed(value) => format!("{value} (maximum)"),
        }
    }
}

/// Terminal projections which do not mutate provider/domain state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    OpenSettings,
    /// Re-open the provider authentication screen (provider-routed `/auth`).
    OpenAuth,
    /// Open the LM Studio provider settings screen (adjust LAN/localhost URL + model).
    OpenLmStudioSettings,
    /// Open the provider selection modal (arrow-key picker).
    OpenProviderSwitcher,
    ToggleReasoning,
    ToggleTasks,
    ToggleSidebar,
    ToggleChatOnly,
    ToggleScreenReader,
    ToggleNativeMouse,
    ToggleSound,
    OpenPromptEditor,
    OpenDiffAnnotator,
    ToggleVim,
    ToggleMobile,
    OpenKeybindEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaOp {
    Queue { path: String },
    Render { protocol: Option<String> },
    Screenshot,
}

/// Plan-related slash gestures that take no argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanGesture {
    /// `/approve` — finalize the in-review plan.
    Approve,
    /// `/cancel` — abort any in-flight plan.
    Cancel,
}

/// Phase 8 (ADR 0011): one structured operation against the durable
/// memory subsystem ([`vesper_memory::MemoryStore`] /
/// [`vesper_memory::SkillStore`] / [`vesper_memory::UserProfile`] /
/// [`vesper_memory::AwarenessLedger`]).
///
/// The resolver returns this from a slash command; `dispatch` records it on
/// [`crate::dispatch::SessionState::pending_memory_op`]; the binary owns the
/// real stores and drains the op after dispatch (mirroring the
/// `pending_reasoning` / `pending_prompt` drain pattern).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryOp {
    /// `/memory [needle]` — list every memory entry, or query by substring.
    MemoryList { needle: Option<String> },
    /// `/goal <text>` — append a durable [`vesper_memory::MemoryKind::Goal`].
    GoalAdd { summary: String },
    /// `/subgoal <text>` — append a durable
    /// [`vesper_memory::MemoryKind::Subgoal`].
    SubgoalAdd { summary: String },
    /// `/skills` — list every learned-skill markdown file.
    SkillsList,
    /// `/profile` — show the cross-project user profile.
    ProfileShow,
    /// `/awareness [kind]` — list epistemic records, optionally filtered.
    AwarenessList {
        kind: Option<vesper_memory::MemoryKind>,
    },
    /// `/metacognition` — list metacognitive assessments.
    MetacognitionList,
    /// `/deliberation` — list grounded-deliberation hypotheses.
    DeliberationList,
    /// `/repository` — list repository-intelligence observations.
    RepositoryList,
    /// `/meta-learning` — list meta-learning candidates.
    MetaLearningList,
    /// `/observability` — list local reliability-metric observations.
    ObservabilityList,
    /// `/curator` — run the deterministic curation pass (dedupe + trim).
    Curate,
    /// `/journey` — chronological timeline of memory + skills + profile.
    Journey,
}

impl MemoryOp {
    /// Returns the slash-command name that produced this op (used by
    /// `dispatch` to format the in-flight status notice).
    #[must_use]
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::MemoryList { .. } => "memory",
            Self::GoalAdd { .. } => "goal",
            Self::SubgoalAdd { .. } => "subgoal",
            Self::SkillsList => "skills",
            Self::ProfileShow => "profile",
            Self::AwarenessList { .. } => "awareness",
            Self::MetacognitionList => "metacognition",
            Self::DeliberationList => "deliberation",
            Self::RepositoryList => "repository",
            Self::MetaLearningList => "meta-learning",
            Self::ObservabilityList => "observability",
            Self::Curate => "curator",
            Self::Journey => "journey",
        }
    }
}

/// Phase 9 (ADR 0012): one structured operation against the workspace
/// snapshot / rollback / session-lineage / cron / export / clipboard / CI
/// surface backed by [`vesper_checkpoints`].
///
/// The resolver returns this from a slash command; `dispatch` records it
/// on [`crate::dispatch::SessionState::pending_checkpoint_op`]; the binary
/// owns the real [`vesper_checkpoints`] stores and drains the op after
/// dispatch (mirroring the `pending_memory_op` drain pattern).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointOp {
    /// `/sessions-new [name]` — create a new session in the lineage.
    SessionCreate { name: Option<String> },
    /// `/sessions` — list every known session.
    SessionList,
    /// `/lineage` — show the parent→child chain for the active session.
    LineageShow,
    /// `/branch [name]` — fork the active session to try a different
    /// direction (parent stays Active).
    SessionBranch { name: Option<String> },
    /// `/rename <name>` — rename the active session.
    SessionRename { new_name: String },
    /// `/checkpoint [label]` — explicitly snapshot the workspace file
    /// state NOW. Auto-snapshotting is never performed.
    CheckpointCreate { label: Option<String> },
    /// `/rollback <id>` — restore the workspace from a prior checkpoint.
    CheckpointRollback { id: String },
    /// `/rewind <id>` — alias for `/rollback <id>`.
    CheckpointRewind { id: String },
    /// `/undo [N]` — roll back to the N-th most recent checkpoint
    /// (default N=1). The architect's "take back the last N mutations".
    CheckpointUndo { count: usize },
    /// `/loop <prompt>` — register a cron entry (the TUI is not a daemon,
    /// so no actual scheduling happens here; the entry is recorded for a
    /// future long-running process).
    CronRegister { prompt: String, schedule: String },
    /// `/export` — write transcript + lineage to a bounded markdown file
    /// under `<root>/exports/`.
    SessionExport,
    /// `/export last` — write only the final assistant response to a bounded
    /// markdown file under `<root>/exports/`.
    SessionExportLast,
    /// `/copy [target]` — copy the last response / a target to the
    /// clipboard (with persistence fallback when no clipboard is
    /// reachable).
    ClipboardCopy { target: String },
    /// `/ci` — show CI status for the current branch via `gh` (with a
    /// clear "unavailable" notice when `gh` is not on PATH).
    CiStatus,
}

impl CheckpointOp {
    /// Returns the slash-command name that produced this op (used by
    /// `dispatch` to format the in-flight status notice).
    #[must_use]
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::SessionCreate { .. } => "sessions-new",
            Self::SessionList => "sessions",
            Self::LineageShow => "lineage",
            Self::SessionBranch { .. } => "branch",
            Self::SessionRename { .. } => "rename",
            Self::CheckpointCreate { .. } => "checkpoint",
            Self::CheckpointRollback { .. } => "rollback",
            Self::CheckpointRewind { .. } => "rewind",
            Self::CheckpointUndo { .. } => "undo",
            Self::CronRegister { .. } => "loop",
            Self::SessionExport | Self::SessionExportLast => "export",
            Self::ClipboardCopy { .. } => "copy",
            Self::CiStatus => "ci",
        }
    }
}

/// Phase 10 (ADR 0013): one structured operation against the MCP /
/// plugins subsystem backed by [`vesper_mcp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpOp {
    /// `/mcp` (no arg) — list configured MCP servers.
    McpList,
    /// `/mcp add <id> <command> [args...]` — add a stdio server config.
    McpAdd {
        id: String,
        command: String,
        args: Vec<String>,
    },
    /// `/mcp remove <id>` — remove a server config.
    McpRemove { id: String },
    /// `/mcp tools <id>` — connect to a server and list its tools.
    McpTools { id: String },
    /// `/plugins` (no arg) — list loaded plugins.
    PluginsList,
    /// `/plugins publishers` — list trusted publishers.
    PluginsPublishers,
    /// `/plugins verify <path>` — verify a plugin package's signature.
    PluginsVerify { path: String },
    /// `/plugins load <path>` — load a signed plugin.
    PluginsLoad { path: String },
    /// `/plugins trust <publisher> <pubkey-hex>` — add a trusted publisher.
    PluginsTrust {
        publisher: String,
        public_key_hex: String,
    },
}

impl McpOp {
    /// Returns the slash-command name that produced this op.
    #[must_use]
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::McpList
            | Self::McpAdd { .. }
            | Self::McpRemove { .. }
            | Self::McpTools { .. } => "mcp",
            Self::PluginsList
            | Self::PluginsPublishers
            | Self::PluginsVerify { .. }
            | Self::PluginsLoad { .. }
            | Self::PluginsTrust { .. } => "plugins",
        }
    }
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
/// implemented through a typed immediate or pending operation. No oracle
/// command falls through to "Unknown command".
/// Phase 11 (ADR 0015 — Stage 16): one structured operation against the
/// cognitive-memory engine backed by [`vesper_cognition::CognitiveMemory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CognitionOp {
    /// `/remember [--global|--project] <text>` — route a fact to one store.
    Remember { text: String, scope: CognitionScope },
    /// `/recall [--global|--project] <query>` — search one or both stores.
    Recall {
        query: String,
        scope: CognitionScope,
    },
    /// `/forget [--global|--project] <id>` — delete from one or both stores.
    Forget { id: String, scope: CognitionScope },
    /// `/promote <id>` — move a project memory into the global store.
    Promote { id: String },
    /// `/demote <id>` — move a global memory into the project store.
    Demote { id: String },
    /// `/memories [query]` — audit global and project memories side by side.
    Audit { query: Option<String> },
}

/// User-selectable cognitive-memory destination. `Smart` means automatic
/// routing for writes and both stores for reads/deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CognitionScope {
    Smart,
    Global,
    Project,
}

impl CognitionOp {
    #[must_use]
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::Remember { .. } => "remember",
            Self::Recall { .. } => "recall",
            Self::Forget { .. } => "forget",
            Self::Promote { .. } => "promote",
            Self::Demote { .. } => "demote",
            Self::Audit { .. } => "memories",
        }
    }
}

fn cognition_scope_and_body(argument: &str) -> Result<(CognitionScope, String), String> {
    let mut scope = CognitionScope::Smart;
    let mut body = Vec::new();
    for token in argument.split_whitespace() {
        match token {
            "--global" => {
                if scope != CognitionScope::Smart {
                    return Err("choose only one of --global or --project".into());
                }
                scope = CognitionScope::Global;
            }
            "--project" | "--local" => {
                if scope != CognitionScope::Smart {
                    return Err("choose only one of --global or --project".into());
                }
                scope = CognitionScope::Project;
            }
            value if value.starts_with("--") => {
                return Err(format!("unknown memory scope flag: {value}"));
            }
            value => body.push(value),
        }
    }
    Ok((scope, body.join(" ")))
}

/// ADR 0016 — `/embedding` slash command. The command lives in the
/// Vesper-native surface (next to `/auth`, `/provider`, `/lmstudio`) and
/// lets the user inspect or mutate
/// `.agent-vesper/cognition/embedding.json` from the composer. Mutations
/// trigger a hot-reload of the embedder at drain time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingOp {
    /// `/embedding` (no arg) — show the current config + live engine mode.
    Status,
    /// `/embedding set source=lmstudio endpoint=... model=... api_key=...
    /// dimension=...` — merge the parsed key=value pairs into the
    /// persisted `embedding.json`. Unknown keys are rejected up front.
    Set { pairs: EmbeddingPairs },
    /// `/embedding clear` — delete `embedding.json` so the bundle reverts
    /// to provider-routed behavior on next startup.
    Clear,
}

/// Parsed `key=value` pairs from `/embedding set ...`. Every field is
/// optional; the user typically updates only the fields they care about
/// (e.g. `/embedding set source=lmstudio model=nomic-embed-text-v1.5`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmbeddingPairs {
    pub source: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub dimension: Option<usize>,
}

impl EmbeddingPairs {
    /// Parses a `/embedding set <body>` argument into structured pairs.
    /// Returns `Err` with a list of the rejected tokens so the user can
    /// fix typos (e.g. `source` vs `sauce`). Empty body returns an empty
    /// `EmbeddingPairs` (the drain step surfaces a clear error).
    ///
    /// Accepted keys: `source`, `endpoint`, `model`, `api_key`, `dimension`.
    /// `source` must be one of `local`, `lmstudio`, `bigmodel` (case-
    /// insensitive). `dimension` must parse as a positive integer.
    pub fn parse(body: &str) -> Result<Self, String> {
        let mut out = Self::default();
        let mut errors: Vec<String> = Vec::new();
        for raw in body.split_whitespace() {
            let Some((key, value)) = raw.split_once('=') else {
                errors.push(format!("'{raw}' is not key=value"));
                continue;
            };
            match key {
                "source" => {
                    let lower = value.to_ascii_lowercase();
                    if matches!(lower.as_str(), "local" | "lmstudio" | "bigmodel") {
                        out.source = Some(lower);
                    } else {
                        errors.push(format!(
                            "source must be one of: local, lmstudio, bigmodel (got '{value}')"
                        ));
                    }
                }
                "endpoint" => {
                    if value.is_empty() {
                        errors.push("endpoint cannot be empty".into());
                    } else {
                        out.endpoint = Some(value.to_string());
                    }
                }
                "model" => {
                    if value.is_empty() {
                        errors.push("model cannot be empty".into());
                    } else {
                        out.model = Some(value.to_string());
                    }
                }
                "api_key" => {
                    // Allow explicit `null` / `none` to clear the field.
                    let normalized = match value.to_ascii_lowercase().as_str() {
                        "null" | "none" => None,
                        _ => Some(value.to_string()),
                    };
                    out.api_key = normalized;
                }
                "dimension" => match value.parse::<usize>() {
                    Ok(d) if d > 0 => out.dimension = Some(d),
                    Ok(_) => errors.push("dimension must be > 0".into()),
                    Err(_) => errors.push(format!("dimension must be an integer (got '{value}')")),
                },
                other => {
                    errors.push(format!(
                        "unknown key '{other}' (allowed: source, endpoint, model, api_key, dimension)"
                    ));
                }
            }
        }
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors.join("; "))
        }
    }

    /// Returns true when at least one field is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.source.is_none()
            && self.endpoint.is_none()
            && self.model.is_none()
            && self.api_key.is_none()
            && self.dimension.is_none()
    }
}

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
    /// Every command resolves to a concrete typed handler.
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

    /// Returns the oracle command palette entries matching the current
    /// composer value. The returned names include the leading slash and are
    /// kept in the oracle's registration order so the terminal UI can offer
    /// the same discoverable slash surface as the Python composer.
    #[must_use]
    pub fn completion_candidates(&self, input: &str) -> Vec<(String, String)> {
        let trimmed = input.trim_start().to_ascii_lowercase();
        let Some(query) = trimmed.strip_prefix('/') else {
            return Vec::new();
        };

        ORACLE_COMMAND_SURFACE
            .iter()
            .filter(|entry| query.is_empty() || entry.name.starts_with(query))
            .map(|entry| (format!("/{}", entry.name), entry.description.to_string()))
            .collect()
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
            "planmode" => {
                if argument.is_empty() {
                    CommandOutcome::Error("Usage: /planmode <your requirements as a PRD>".into())
                } else {
                    CommandOutcome::Plan {
                        prd: argument.to_string(),
                    }
                }
            }
            "plan" | "api-plan" | "endpoint" => resolve_advertised_choice(
                active_provider,
                superpowers,
                name,
                argument,
                SessionConfigKey::EndpointPlan,
                "plan",
            ),
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
            // VRO-8 (PRD §8.1): `/reasoning set mode=<X>` (and `/reasoning
            // clear`) override the TaskProfiler; bare `/reasoning` surfaces
            // the current override. Any other `/reasoning <level>` argument
            // shape falls through to the existing thinking superpower alias
            // (oracle convention). `/thinking` and `/model` always take the
            // superpower path.
            "reasoning" => {
                let trimmed = argument.trim();
                if trimmed.is_empty() {
                    // Bare `/reasoning` → show current override in status.
                    CommandOutcome::ReasoningStatus
                } else if trimmed.eq_ignore_ascii_case("clear") {
                    // `/reasoning clear` is an alias for `set mode=auto`.
                    CommandOutcome::ReasoningOverride {
                        mode: vesper_domain::ReasoningMode::Auto,
                    }
                } else if let Some(rest) = trimmed.strip_prefix("set ") {
                    let rest = rest.trim();
                    if let Some(mode_str) = rest.strip_prefix("mode=") {
                        match parse_reasoning_mode(mode_str) {
                            Some(mode) => CommandOutcome::ReasoningOverride { mode },
                            None => CommandOutcome::Error(format!(
                                "Unknown reasoning mode: {mode_str}. Allowed: \
                                 auto, fast, balanced, deep, maximum, off."
                            )),
                        }
                    } else {
                        CommandOutcome::Error(
                            "Usage: /reasoning set mode=<auto|fast|balanced|deep|maximum|off>"
                                .into(),
                        )
                    }
                } else {
                    // Fall through to the thinking superpower alias
                    // (preserves `/reasoning <level>` for backward compat).
                    self.resolve_superpower(
                        superpower_alias(name),
                        argument,
                        active_provider,
                        superpowers,
                    )
                }
            }
            "thinking" | "model" => self.resolve_superpower(
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
            // VRO-13 PR-2: view-only firewall panel. Disabling requires an
            // env restart (AGENT_VESPER_FIREWALL=off); there is no runtime
            // toggle by design.
            "firewall" => CommandOutcome::ContextView(ViewKind::Firewall),
            // VRO-13 PR-4: sandbox panel with `on|off|status` argument
            // parity. The route is boot-resolved (once-only holder), so
            // `on`/`off` are honest restart instructions, not runtime
            // toggles — byte-identical semantics to the ACP host's
            // `/sandbox on|off|status` surface.
            "sandbox" => {
                let value = argument.trim().to_ascii_lowercase();
                match value.as_str() {
                    "" | "status" => CommandOutcome::ContextView(ViewKind::Sandbox),
                    "on" | "enable" | "enabled" => {
                        CommandOutcome::SandboxControl(SandboxControl::On)
                    }
                    "off" | "disable" | "disabled" => {
                        CommandOutcome::SandboxControl(SandboxControl::Off)
                    }
                    _ => CommandOutcome::Error(
                        "Usage: /sandbox [on|off|status] (route is boot-resolved; \
                         on/off explain the restart step)"
                            .into(),
                    ),
                }
            }

            "tasks" => CommandOutcome::Ui(UiAction::ToggleTasks),
            "max-iterations" => {
                let value = argument.trim().to_ascii_lowercase();
                if value.is_empty() {
                    CommandOutcome::ContextView(ViewKind::MaxIterations)
                } else if matches!(value.as_str(), "disable" | "disabled" | "off") {
                    CommandOutcome::SessionConfig {
                        key: SessionConfigKey::MaxIterations,
                        value: "0".into(),
                    }
                } else if matches!(value.as_str(), "enable" | "enabled" | "on") {
                    CommandOutcome::SessionConfig {
                        key: SessionConfigKey::MaxIterations,
                        value: vesper_agent::ENABLED_DEFAULT_MAX_TOOL_ITERATIONS.to_string(),
                    }
                } else {
                    match value.parse::<u32>() {
                        Ok(value @ 1..=1000) => CommandOutcome::SessionConfig {
                            key: SessionConfigKey::MaxIterations,
                            value: value.to_string(),
                        },
                        _ => CommandOutcome::Error(
                            "Usage: /max-iterations [enable|disable|1-1000]".into(),
                        ),
                    }
                }
            }
            "interview-limit" => {
                let value = argument.trim();
                if value.is_empty() {
                    CommandOutcome::InterviewLimitStatus
                } else if value.eq_ignore_ascii_case("auto") {
                    CommandOutcome::InterviewLimit(InterviewQuestionLimit::Auto)
                } else {
                    match value.parse::<u8>() {
                        Ok(value @ 1..=MAX_INTERVIEW_QUESTIONS) => {
                            CommandOutcome::InterviewLimit(InterviewQuestionLimit::Fixed(value))
                        }
                        _ => CommandOutcome::Error(format!(
                            "Usage: /interview-limit [auto|1-{MAX_INTERVIEW_QUESTIONS}]"
                        )),
                    }
                }
            }
            "usage" => {
                // Provider-routed: available to every provider. Providers
                // without a quota integration surface a graceful error when
                // the agent loop queries them.
                CommandOutcome::ProviderUsage
            }

            // === Live session settings / native UI ===
            "permission" if argument.trim().is_empty() => {
                CommandOutcome::ContextView(ViewKind::Permission)
            }
            "permission" => resolve_session_choice(
                name,
                argument,
                SessionConfigKey::Permission,
                &["ask", "read", "bypass"],
            ),
            "mode" => resolve_session_choice(
                name,
                argument,
                SessionConfigKey::OperatingMode,
                &["ask", "code"],
            ),
            "generation" => resolve_advertised_choice(
                active_provider,
                superpowers,
                name,
                argument,
                SessionConfigKey::GenerationProfile,
                "generation",
            ),
            "auxiliary" => resolve_advertised_choice(
                active_provider,
                superpowers,
                name,
                argument,
                SessionConfigKey::AuxiliaryModel,
                "auxiliary",
            ),
            // Mixture-of-agents is a harness-owned control: the off/enabled
            // scale is structural, and adviser eligibility is capability- and
            // policy-gated in the palette and at turn dispatch (PRD FR-6).
            "mixture" => resolve_session_choice(
                name,
                argument,
                SessionConfigKey::MixtureMode,
                &["off", "enabled"],
            ),
            "settings" => CommandOutcome::Ui(UiAction::OpenSettings),
            "auth" => CommandOutcome::Ui(UiAction::OpenAuth),
            "lmstudio" => CommandOutcome::Ui(UiAction::OpenLmStudioSettings),
            "provider" => CommandOutcome::Ui(UiAction::OpenProviderSwitcher),
            "reasoning-panel" | "toggle-thinking" => CommandOutcome::Ui(UiAction::ToggleReasoning),
            "statusline" => CommandOutcome::Ui(UiAction::ToggleSidebar),
            "chat-only" => CommandOutcome::Ui(UiAction::ToggleChatOnly),
            "theme" => resolve_session_choice(
                name,
                argument,
                SessionConfigKey::Theme,
                &[
                    "chatgpt-black",
                    "chatgpt-white",
                    "ansi",
                    "light",
                    "dracula",
                    "nord",
                ],
            ),
            "screen-reader" => CommandOutcome::Ui(UiAction::ToggleScreenReader),
            "native-mouse" => CommandOutcome::Ui(UiAction::ToggleNativeMouse),
            "sound" => CommandOutcome::Ui(UiAction::ToggleSound),
            "search" => {
                let query = argument.trim();
                if query.is_empty() {
                    CommandOutcome::Error("Usage: /search <conversation text>".into())
                } else {
                    CommandOutcome::Search {
                        query: query.to_owned(),
                    }
                }
            }
            "history" => {
                let session_id = argument.trim();
                if session_id.is_empty() {
                    CommandOutcome::Error("Choose a session from the /history picker.".into())
                } else {
                    CommandOutcome::History {
                        session_id: session_id.to_owned(),
                    }
                }
            }
            "prompt" => CommandOutcome::Ui(UiAction::OpenPromptEditor),
            "annotate" => CommandOutcome::Ui(UiAction::OpenDiffAnnotator),
            "vim" => CommandOutcome::Ui(UiAction::ToggleVim),
            "mobile" => CommandOutcome::Ui(UiAction::ToggleMobile),
            "keybinds" => CommandOutcome::Ui(UiAction::OpenKeybindEditor),
            "image" | "attach" => {
                let path = argument.trim();
                if path.is_empty() {
                    CommandOutcome::Error(format!("Usage: /{name} <image path>"))
                } else {
                    CommandOutcome::Media(MediaOp::Queue {
                        path: path.to_owned(),
                    })
                }
            }
            "image-render" => {
                let protocol = argument.trim().to_ascii_lowercase();
                if protocol.is_empty()
                    || matches!(protocol.as_str(), "auto" | "kitty" | "sixel" | "iterm2")
                {
                    CommandOutcome::Media(MediaOp::Render {
                        protocol: (!protocol.is_empty()).then_some(protocol),
                    })
                } else {
                    CommandOutcome::Error("Usage: /image-render [auto|kitty|sixel|iterm2]".into())
                }
            }
            "screenshot" => CommandOutcome::Media(MediaOp::Screenshot),
            "btw" => {
                let question = argument.trim();
                if question.is_empty() {
                    CommandOutcome::Error("Usage: /btw <side question>".into())
                } else {
                    CommandOutcome::AuxiliaryQuestion {
                        question: question.to_owned(),
                    }
                }
            }
            "blocks" => {
                let mut parts = argument.split_whitespace();
                let action = parts.next().unwrap_or_default();
                let index = parts.next().and_then(|value| value.parse::<usize>().ok());
                if !matches!(action, "copy" | "write") || parts.next().is_some() {
                    CommandOutcome::Error("Choose a code block from the /blocks picker.".into())
                } else if let Some(index) = index {
                    CommandOutcome::CodeBlock {
                        index: index.saturating_sub(1),
                        write: action == "write",
                    }
                } else {
                    CommandOutcome::Error("Choose a code block from the /blocks picker.".into())
                }
            }

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

            // === Phase 8 (ADR 0011) — memory subsystem commands ===
            // Each command resolves to a structured MemoryOp that the binary
            // drains after dispatch and executes against the durable
            // vesper_memory stores. These are no longer deferred — they have
            // a real, persistent backing subsystem.
            "memory" => {
                let trimmed = argument.trim();
                let needle = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                CommandOutcome::Memory(MemoryOp::MemoryList { needle })
            }
            "goal" => {
                if argument.trim().is_empty() {
                    CommandOutcome::Error("Usage: /goal <one-line persistent goal>".into())
                } else {
                    CommandOutcome::Memory(MemoryOp::GoalAdd {
                        summary: argument.trim().to_string(),
                    })
                }
            }
            "subgoal" => {
                if argument.trim().is_empty() {
                    CommandOutcome::Error("Usage: /subgoal <one-line acceptance criterion>".into())
                } else {
                    CommandOutcome::Memory(MemoryOp::SubgoalAdd {
                        summary: argument.trim().to_string(),
                    })
                }
            }
            "skills" => CommandOutcome::Memory(MemoryOp::SkillsList),
            "profile" => CommandOutcome::Memory(MemoryOp::ProfileShow),
            "awareness" => CommandOutcome::Memory(MemoryOp::AwarenessList { kind: None }),
            "metacognition" => CommandOutcome::Memory(MemoryOp::MetacognitionList),
            "deliberation" => CommandOutcome::Memory(MemoryOp::DeliberationList),
            "repository" => CommandOutcome::Memory(MemoryOp::RepositoryList),
            "meta-learning" => CommandOutcome::Memory(MemoryOp::MetaLearningList),
            "observability" => CommandOutcome::Memory(MemoryOp::ObservabilityList),
            "curator" => CommandOutcome::Memory(MemoryOp::Curate),
            "journey" => CommandOutcome::Memory(MemoryOp::Journey),

            // === Phase 9 (ADR 0012) — checkpoints subsystem commands ===
            // Each command resolves to a structured CheckpointOp that the
            // binary drains after dispatch and executes against the durable
            // vesper_checkpoints stores. These are no longer deferred — they
            // have a real, persistent backing subsystem with strict RAII
            // file-descriptor discipline (no SQLite, no git refs).
            "sessions-new" => {
                let trimmed = argument.trim();
                let name = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                CommandOutcome::Checkpoint(CheckpointOp::SessionCreate { name })
            }
            "sessions" => CommandOutcome::Checkpoint(CheckpointOp::SessionList),
            "lineage" => CommandOutcome::Checkpoint(CheckpointOp::LineageShow),
            "branch" => {
                let trimmed = argument.trim();
                let name = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                CommandOutcome::Checkpoint(CheckpointOp::SessionBranch { name })
            }
            "rename" => {
                if argument.trim().is_empty() {
                    CommandOutcome::Error("Usage: /rename <new-session-name>".into())
                } else {
                    CommandOutcome::Checkpoint(CheckpointOp::SessionRename {
                        new_name: argument.trim().to_string(),
                    })
                }
            }
            "checkpoint" => {
                let trimmed = argument.trim();
                let label = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                CommandOutcome::Checkpoint(CheckpointOp::CheckpointCreate { label })
            }
            "rollback" => {
                if argument.trim().is_empty() {
                    CommandOutcome::Error("Usage: /rollback <checkpoint-id>".into())
                } else {
                    CommandOutcome::Checkpoint(CheckpointOp::CheckpointRollback {
                        id: argument.trim().to_string(),
                    })
                }
            }
            "rewind" => {
                if argument.trim().is_empty() {
                    CommandOutcome::Error("Usage: /rewind <checkpoint-id>".into())
                } else {
                    CommandOutcome::Checkpoint(CheckpointOp::CheckpointRewind {
                        id: argument.trim().to_string(),
                    })
                }
            }
            "undo" => {
                let count = if argument.trim().is_empty() {
                    1
                } else {
                    argument.trim().parse::<usize>().unwrap_or(1).max(1)
                };
                CommandOutcome::Checkpoint(CheckpointOp::CheckpointUndo { count })
            }
            "loop" => {
                // /loop <schedule> <prompt> — the oracle's cron surface.
                // Parse: optional schedule (default "every 30m"), then the
                // prompt. Bare `/loop` is an error.
                let trimmed = argument.trim();
                if trimmed.is_empty() {
                    CommandOutcome::Error(
                        "Usage: /loop <prompt>  (or /loop every 1h <prompt>)".into(),
                    )
                } else {
                    let (schedule, prompt) = match trimmed.split_once(char::is_whitespace) {
                        Some((first, rest))
                            if first.starts_with("every") || first.starts_with("daily") =>
                        {
                            (first.to_string(), rest.trim().to_string())
                        }
                        _ => ("every 30m".to_string(), trimmed.to_string()),
                    };
                    if prompt.is_empty() {
                        return CommandOutcome::Error(
                            "Usage: /loop <prompt>  (or /loop every 1h <prompt>)".into(),
                        );
                    }
                    CommandOutcome::Checkpoint(CheckpointOp::CronRegister { prompt, schedule })
                }
            }
            "export" => {
                if argument.trim() == "last" {
                    CommandOutcome::Checkpoint(CheckpointOp::SessionExportLast)
                } else {
                    CommandOutcome::Checkpoint(CheckpointOp::SessionExport)
                }
            }
            "export last" => CommandOutcome::Checkpoint(CheckpointOp::SessionExportLast),
            "copy" => {
                let target = if argument.trim().is_empty() {
                    "last-response".to_string()
                } else {
                    argument.trim().to_string()
                };
                CommandOutcome::Checkpoint(CheckpointOp::ClipboardCopy { target })
            }
            "ci" => CommandOutcome::Checkpoint(CheckpointOp::CiStatus),

            // === Phase 10 (ADR 0013) — MCP & plugins subsystem commands ===
            // The 2 final commands move from Deferred to real Mcp(McpOp)
            // outcomes backed by vesper_mcp (McpRegistry + McpClient +
            // PluginLoader + TrustedPublishers). This completes the
            // achievable oracle command surface.
            "mcp" => {
                let trimmed = argument.trim();
                if trimmed.is_empty() {
                    CommandOutcome::Mcp(McpOp::McpList)
                } else {
                    // Parse the subcommand: list | add | remove | tools.
                    let (sub, rest) = trimmed
                        .split_once(char::is_whitespace)
                        .unwrap_or((trimmed, ""));
                    match sub {
                        "list" => CommandOutcome::Mcp(McpOp::McpList),
                        "add" => {
                            let rest = rest.trim();
                            if rest.is_empty() {
                                CommandOutcome::Error(
                                    "Usage: /mcp add <id> <command> [args...]".into(),
                                )
                            } else {
                                let mut parts = rest.split_whitespace();
                                let id = parts.next().unwrap_or("").to_string();
                                let command = parts.next().unwrap_or("").to_string();
                                if id.is_empty() || command.is_empty() {
                                    return CommandOutcome::Error(
                                        "Usage: /mcp add <id> <command> [args...]".into(),
                                    );
                                }
                                let args = parts.map(String::from).collect();
                                CommandOutcome::Mcp(McpOp::McpAdd { id, command, args })
                            }
                        }
                        "remove" => {
                            let id = rest.trim().to_string();
                            if id.is_empty() {
                                CommandOutcome::Error("Usage: /mcp remove <id>".into())
                            } else {
                                CommandOutcome::Mcp(McpOp::McpRemove { id })
                            }
                        }
                        "tools" => {
                            let id = rest.trim().to_string();
                            if id.is_empty() {
                                CommandOutcome::Error("Usage: /mcp tools <id>".into())
                            } else {
                                CommandOutcome::Mcp(McpOp::McpTools { id })
                            }
                        }
                        _ => CommandOutcome::Error(format!(
                            "Unknown /mcp subcommand: {sub}. Available: list, add, remove, tools."
                        )),
                    }
                }
            }
            "plugins" => {
                let trimmed = argument.trim();
                if trimmed.is_empty() {
                    CommandOutcome::Mcp(McpOp::PluginsList)
                } else {
                    let (sub, rest) = trimmed
                        .split_once(char::is_whitespace)
                        .unwrap_or((trimmed, ""));
                    match sub {
                        "" | "list" => CommandOutcome::Mcp(McpOp::PluginsList),
                        "publishers" => CommandOutcome::Mcp(McpOp::PluginsPublishers),
                        "verify" => {
                            let path = rest.trim().to_string();
                            if path.is_empty() {
                                CommandOutcome::Error("Usage: /plugins verify <path>".into())
                            } else {
                                CommandOutcome::Mcp(McpOp::PluginsVerify { path })
                            }
                        }
                        "load" => {
                            let path = rest.trim().to_string();
                            if path.is_empty() {
                                CommandOutcome::Error("Usage: /plugins load <path>".into())
                            } else {
                                CommandOutcome::Mcp(McpOp::PluginsLoad { path })
                            }
                        }
                        "trust" => {
                            let mut parts = rest.split_whitespace();
                            let publisher = parts.next().unwrap_or("").to_string();
                            let public_key_hex = parts.next().unwrap_or("").to_string();
                            if publisher.is_empty() || public_key_hex.is_empty() {
                                return CommandOutcome::Error(
                                    "Usage: /plugins trust <publisher> <pubkey-hex>".into(),
                                );
                            }
                            CommandOutcome::Mcp(McpOp::PluginsTrust {
                                publisher,
                                public_key_hex,
                            })
                        }
                        _ => CommandOutcome::Error(format!(
                            "Unknown /plugins subcommand: {sub}. Available: list, publishers, verify, load, trust."
                        )),
                    }
                }
            }

            // === Phase 11 (ADR 0015 — Stage 16) — cognitive-memory commands ===
            "remember" => match cognition_scope_and_body(argument) {
                Ok((scope, text)) if !text.is_empty() => {
                    CommandOutcome::Cognition(CognitionOp::Remember { text, scope })
                }
                Ok(_) => CommandOutcome::Error(
                    "Usage: /remember [--global|--project] <text to remember>".into(),
                ),
                Err(error) => CommandOutcome::Error(format!("/remember: {error}")),
            },
            "recall" => match cognition_scope_and_body(argument) {
                Ok((scope, query)) if !query.is_empty() => {
                    CommandOutcome::Cognition(CognitionOp::Recall { query, scope })
                }
                Ok(_) => CommandOutcome::Error(
                    "Usage: /recall [--global|--project] <search query>".into(),
                ),
                Err(error) => CommandOutcome::Error(format!("/recall: {error}")),
            },
            "forget" => match cognition_scope_and_body(argument) {
                Ok((scope, id)) if !id.is_empty() => {
                    CommandOutcome::Cognition(CognitionOp::Forget { id, scope })
                }
                Ok(_) => {
                    CommandOutcome::Error("Usage: /forget [--global|--project] <memory-id>".into())
                }
                Err(error) => CommandOutcome::Error(format!("/forget: {error}")),
            },
            "promote" => {
                let id = argument.trim();
                if id.is_empty() {
                    CommandOutcome::Error("Usage: /promote <project-memory-id>".into())
                } else {
                    CommandOutcome::Cognition(CognitionOp::Promote { id: id.into() })
                }
            }
            "demote" => {
                let id = argument.trim();
                if id.is_empty() {
                    CommandOutcome::Error("Usage: /demote <global-memory-id>".into())
                } else {
                    CommandOutcome::Cognition(CognitionOp::Demote { id: id.into() })
                }
            }
            "memories" => CommandOutcome::Cognition(CognitionOp::Audit {
                query: (!argument.trim().is_empty()).then(|| argument.trim().to_string()),
            }),

            // === ADR 0016 — provider-independent embedding layer command ===
            // `/embedding`              → status (config + live engine mode)
            // `/embedding set k=v ...`  → merge into embedding.json + hot reload
            // `/embedding clear`        → delete embedding.json (revert on restart)
            "embedding" => {
                let body = argument.trim();
                if body.is_empty() {
                    CommandOutcome::Embedding(EmbeddingOp::Status)
                } else if let Some(set_body) = body.strip_prefix("set") {
                    let set_body = set_body.trim();
                    if set_body.is_empty() {
                        CommandOutcome::Error(
                            "Usage: /embedding set source=<local|lmstudio|bigmodel> \
                             [endpoint=...] [model=...] [api_key=...] [dimension=...]"
                                .into(),
                        )
                    } else {
                        match EmbeddingPairs::parse(set_body) {
                            Ok(pairs) if pairs.is_empty() => CommandOutcome::Error(
                                "Usage: /embedding set <key=value> [key=value ...]".into(),
                            ),
                            Ok(pairs) => CommandOutcome::Embedding(EmbeddingOp::Set { pairs }),
                            Err(err) => {
                                CommandOutcome::Error(format!("/embedding set rejected: {err}"))
                            }
                        }
                    }
                } else if body == "clear" {
                    CommandOutcome::Embedding(EmbeddingOp::Clear)
                } else {
                    CommandOutcome::Error(format!(
                        "Unknown /embedding subcommand: {body:?}. Available: set, clear, \
                         (no arg → status)."
                    ))
                }
            }

            // A registered command without a concrete route is a parity bug,
            // not a user-visible feature state. Fail closed so tests and the
            // audit surface expose the missing implementation immediately.
            other => CommandOutcome::Error(format!(
                "internal parity violation: registered command /{other} has no implementation"
            )),
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
        // group concrete commands by category so the user can scan them.
        let mut buffer = String::new();
        buffer.push_str("Agent Vesper command surface\n\n");
        buffer.push_str("Plan Mode:\n");
        buffer.push_str("  /planmode <PRD>    enter Plan Mode and interrogate the requirements\n");
        buffer.push_str("  /approve           finalize the reviewed plan and start execution\n");
        buffer.push_str("  /cancel            abort the in-flight plan\n");
        buffer.push_str("  /clear-plan        clear Plan Mode back to NORMAL\n");
        buffer.push_str("\nSuperpowers (resolved against the active provider):\n");
        buffer.push_str("  /thinking <lvl>    session reasoning (disabled/enabled/high/max)\n");
        buffer.push_str(
            "  /reasoning <args>  override the VRO reasoning mode (set mode=...|clear)\n",
        );
        buffer.push_str("  /model <name>      switch the active model\n");
        buffer.push_str("  /plan <value>      select coding, standard, or bigmodel API plan\n");
        buffer.push_str("  /api-plan          alias for /plan\n");
        buffer.push_str("  /endpoint          alias for /plan\n");
        buffer.push_str("  /permission        select Ask, Read Only, or Bypass\n");
        buffer.push_str("  /mode              select Ask or Code operating mode\n");
        buffer.push_str("  /generation        select balanced, precise, or exploratory\n");
        buffer.push_str("  /auxiliary         select the auxiliary model\n");
        buffer.push_str("  /mixture           toggle reference review\n");
        buffer.push_str("  /settings          browse all settings without typing values\n");
        buffer.push_str("  /theme             select a native terminal theme\n");
        buffer.push_str("  /screen-reader     toggle the plain accessibility layout\n");
        buffer.push_str("  /native-mouse      release or recapture terminal mouse input\n");
        buffer.push_str("  /sound             toggle completion bell notifications\n");
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
        buffer.push_str("  /reasoning-panel   show or hide streamed reasoning\n");
        buffer.push_str("  /toggle-thinking   alias for /reasoning-panel\n");
        buffer.push_str("  /statusline        show or hide the session sidebar\n");
        buffer.push_str("  /max-iterations    show the per-turn tool-call iteration cap\n");
        buffer.push_str(
            "  /interview-limit    show or set VesperLens questions (auto|1-12; default 4)\n",
        );
        buffer.push_str("  /usage             show the current usage summary\n");
        buffer.push_str("  /history           browse and resume persisted sessions\n");
        buffer.push_str("  /search <text>     grep the visible conversation\n");
        buffer.push_str("  /prompt            compose a multi-line prompt in $VISUAL/$EDITOR\n");
        buffer.push_str(
            "  /btw <question>    ask the configured auxiliary model without changing history\n",
        );
        buffer
            .push_str("  /image <path>      queue a PNG, JPEG, or WebP for a vision-model turn\n");
        buffer.push_str("  /attach <path>     alias for /image\n");
        buffer.push_str(
            "  /image-render      render the last image with a detected terminal protocol\n",
        );
        buffer.push_str("  /screenshot        capture and queue a desktop screenshot\n");
        buffer.push_str("  /blocks            pick a recent fenced code block to copy or write\n");
        buffer.push_str("  /annotate          annotate the live git diff in $VISUAL/$EDITOR\n");
        buffer.push_str("  /vim               toggle the native modal composer\n");
        buffer.push_str("  /mobile            toggle the credential-free approval companion\n");
        buffer
            .push_str("  /keybinds          edit persistent live keybindings in $VISUAL/$EDITOR\n");
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
        buffer.push_str("\nMemory & awareness (durable):\n");
        buffer.push_str("  /memory [query]     list project-local memory entries\n");
        buffer.push_str("  /goal <summary>     add a persistent goal\n");
        buffer.push_str("  /subgoal <criterion> add a goal acceptance criterion\n");
        buffer.push_str("  /skills             list learned skills\n");
        buffer.push_str("  /profile            show approved profile preferences\n");
        buffer.push_str("  /awareness          show epistemic state\n");
        buffer.push_str("  /metacognition      show metacognitive state\n");
        buffer.push_str("  /deliberation       show grounded-deliberation state\n");
        buffer.push_str("  /repository         show repository metadata\n");
        buffer.push_str("  /meta-learning      show learning candidates\n");
        buffer.push_str("  /observability      show local reliability metrics\n");
        buffer.push_str("  /curator            run deterministic skill maintenance\n");
        buffer.push_str("  /journey            show the memory/skill/profile timeline\n");
        buffer.push_str("\nCognitive memory (durable):\n");
        buffer.push_str("  /remember [--global|--project] <text>  remember with visible scope\n");
        buffer.push_str("  /recall [--global|--project] <query>   search scoped memories\n");
        buffer.push_str("  /forget [--global|--project] <id>      delete a scoped memory\n");
        buffer.push_str("  /memories [query]   audit global and project memories\n");
        buffer.push_str("  /promote <id>       move project memory to global\n");
        buffer.push_str("  /demote <id>        move global memory to this project\n");
        buffer.push_str("\nSessions, checkpoints & export (durable):\n");
        buffer.push_str("  /sessions-new [name] create a session\n");
        buffer.push_str("  /sessions           list sessions\n");
        buffer.push_str("  /lineage            show the active session lineage\n");
        buffer.push_str("  /branch [name]      branch the active session\n");
        buffer.push_str("  /rename <name>      rename the active session\n");
        buffer.push_str("  /checkpoint [label] capture an explicit workspace snapshot\n");
        buffer.push_str("  /rollback <id>      restore a checkpoint\n");
        buffer.push_str("  /rewind <id>        alias for /rollback\n");
        buffer.push_str("  /undo [N]           restore an earlier checkpoint\n");
        buffer.push_str("  /loop <prompt>      register a bounded cron entry\n");
        buffer.push_str("  /export             export the full session\n");
        buffer.push_str("  /export last        export only the final assistant response\n");
        buffer.push_str("  /copy [target]      copy a response with a safe fallback\n");
        buffer.push_str("  /ci                 show bounded CI status\n");
        buffer.push_str("\nMCP & plugins (durable):\n");
        buffer.push_str("  /mcp [list|add|remove|tools] manage MCP servers\n");
        buffer.push_str("  /plugins [list|publishers|verify|load|trust] manage plugins\n");
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

/// Parses a `/reasoning set mode=<value>` argument into a
/// [`vesper_domain::ReasoningMode`] (VRO-8, PRD §8.1).
///
/// Accepts the six PRD §8.1 mode names (case-insensitive) plus the `max`
/// shorthand for `Maximum`. Returns `None` for any other value so the
/// caller surfaces a usage error rather than silently inventing a mode.
///
/// **Provider-neutrality**: this parser maps *only* the user's text to the
/// PRD's six values; it never names a concrete provider or invents a new
/// variant. The dispatched override flows through `SessionState` and the
/// binary's VRO turn dispatcher; it never reaches a provider match arm.
#[must_use]
pub(crate) fn parse_reasoning_mode(value: &str) -> Option<vesper_domain::ReasoningMode> {
    use vesper_domain::ReasoningMode;
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(ReasoningMode::Auto),
        "fast" => Some(ReasoningMode::Fast),
        "balanced" => Some(ReasoningMode::Balanced),
        "deep" => Some(ReasoningMode::Deep),
        "maximum" | "max" => Some(ReasoningMode::Maximum),
        "off" => Some(ReasoningMode::Off),
        _ => None,
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

fn resolve_session_choice(
    command: &str,
    argument: &str,
    key: SessionConfigKey,
    allowed: &[&str],
) -> CommandOutcome {
    let value = argument.trim().to_ascii_lowercase();
    if value.is_empty() {
        return CommandOutcome::Error(format!(
            "Usage: /{command} <value>. Allowed: {}",
            allowed.join(", ")
        ));
    }
    if !allowed.contains(&value.as_str()) {
        return CommandOutcome::Error(format!(
            "Invalid /{command} value `{value}`. Allowed: {}",
            allowed.join(", ")
        ));
    }
    CommandOutcome::SessionConfig { key, value }
}

fn resolve_advertised_choice(
    active_provider: &ProviderId,
    superpowers: &[SuperpowerDescriptor],
    command: &str,
    argument: &str,
    key: SessionConfigKey,
    alias: &str,
) -> CommandOutcome {
    // Provider-routed (PRD provider-capability-gating FR-1/FR-2): the values
    // of a harness session control come from the descriptor the ACTIVE
    // provider advertises under `alias`. Providers that do not advertise the
    // control get a truthful "not available" error instead of a foreign
    // provider's value list; the provider's `SuperpowerPolicy` validates the
    // change at dispatch. No provider match arm here.
    let Some(descriptor) = superpowers.iter().find(|descriptor| {
        descriptor.provider_id == *active_provider
            && descriptor
                .command_alias
                .as_ref()
                .map(|bounded| bounded.as_str())
                .is_some_and(|bounded| bounded == alias)
    }) else {
        return CommandOutcome::Error(format!(
            "/{command} is not available on the active provider."
        ));
    };
    let allowed: Vec<&str> = descriptor
        .allowed_values
        .iter()
        .filter_map(|value| match value {
            SuperpowerValue::Choice { value } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    resolve_session_choice(command, argument, key, &allowed)
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
/// [`CommandRegistry::resolve_known`] through a concrete typed handler.
#[rustfmt::skip]
const ORACLE_COMMAND_SURFACE: &[OracleCommandEntry] = &[
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
    OracleCommandEntry { name: "reasoning",         description: "Override the VRO reasoning mode: set mode=<auto|fast|balanced|deep|maximum|off> | clear" },
    OracleCommandEntry { name: "api-plan",          description: "Alias for /plan" },
    OracleCommandEntry { name: "endpoint",          description: "Alias for /plan" },
    OracleCommandEntry { name: "reasoning-panel",   description: "Show or hide the live reasoning panel" },
    OracleCommandEntry { name: "toggle-thinking",   description: "Alias for /reasoning-panel" },
    OracleCommandEntry { name: "clear-view",        description: "Clear only the visible transcript" },
    OracleCommandEntry { name: "max-iterations",    description: "Show or set the per-turn tool-call iteration cap" },
    OracleCommandEntry { name: "interview-limit",   description: "Show or set the VesperLens interview question limit (auto or 1-12)" },
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
    OracleCommandEntry { name: "remember",          description: "Manually add a fact to the cognitive memory store" },
    OracleCommandEntry { name: "recall",            description: "Search the cognitive memory store for relevant context" },
    OracleCommandEntry { name: "forget",            description: "Delete a cognitive memory by its ID" },
    OracleCommandEntry { name: "memories",          description: "Audit global and project cognitive memories" },
    OracleCommandEntry { name: "promote",           description: "Move a project memory into global memory" },
    OracleCommandEntry { name: "demote",            description: "Move a global memory into this project" },
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
    OracleCommandEntry { name: "export last",       description: "Export the last response to a Markdown file" },
    OracleCommandEntry { name: "image",             description: "Queue an image for the next prompt" },
    OracleCommandEntry { name: "exit",              description: "Close the terminal agent" },
    // === Vesper-native additions (the oracle handles these via keybindings) ===
    // Keep these after the oracle surface so opening `/` starts with the same
    // command order as the Python composer.
    OracleCommandEntry { name: "approve",           description: "Finalize the reviewed plan and start execution (Vesper-native)" },
    OracleCommandEntry { name: "cancel",            description: "Abort the in-flight plan (Vesper-native)" },
    OracleCommandEntry { name: "auth",              description: "Re-authenticate or rotate the active provider's credential (Vesper-native, provider-routed)" },
    OracleCommandEntry { name: "lmstudio",          description: "Configure the LM Studio LAN/localhost endpoint (Vesper-native)" },
    OracleCommandEntry { name: "provider",          description: "Switch the active provider (Vesper-native, arrow-key picker)" },
    OracleCommandEntry { name: "embedding",         description: "View or set the cognitive-memory embedding source (ADR 0016, Vesper-native)" },
    OracleCommandEntry { name: "chat-only",         description: "Toggle the F11 chat-only layout collapse (Vesper-native)" },
    OracleCommandEntry {
        name: "firewall",
        description: "Show the VRO-13 command firewall status (view/disable-with-restart only)",
    },
    OracleCommandEntry {
        name: "sandbox",
        description: "Show or steer the VRO-13 sandbox (on|off|status; route is boot-resolved)",
    },
    OracleCommandEntry { name: "quit",              description: "Exit the TUI (Vesper-native; oracle uses Ctrl+X)" },
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
        ProviderId::new("zai").unwrap()
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
    fn bare_permission_reports_status_while_values_still_mutate() {
        let registry = CommandRegistry::stage_11b();
        let plan = PlanState::default();
        let provider = provider();
        assert_eq!(
            registry.resolve(&CommandIntent::parse("/permission"), &plan, &provider, &[]),
            CommandOutcome::ContextView(ViewKind::Permission)
        );
        assert!(matches!(
            registry.resolve(
                &CommandIntent::parse("/permission bypass"),
                &plan,
                &provider,
                &[]
            ),
            CommandOutcome::SessionConfig {
                key: SessionConfigKey::Permission,
                ..
            }
        ));
    }

    #[test]
    fn completion_candidates_match_slash_prefixes_and_export_last() {
        let registry = CommandRegistry::stage_11b();

        let root = registry.completion_candidates("/");
        assert_eq!(root.len(), registry.names().len());
        assert!(root.iter().all(|(name, _)| name.starts_with('/')));

        let help = registry.completion_candidates("/hel");
        assert_eq!(help.first().map(|(name, _)| name.as_str()), Some("/help"));

        let export = registry.completion_candidates("/export l");
        assert_eq!(
            export.first().map(|(name, _)| name.as_str()),
            Some("/export last")
        );

        assert!(registry.completion_candidates("prompt").is_empty());
    }

    #[test]
    fn resolve_planmode_requires_a_prd() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "planmode".into(),
                argument: "".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert!(matches!(outcome, CommandOutcome::Error(_)));

        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "planmode".into(),
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
    fn help_marks_shipped_commands_active_and_lists_current_exclusions() {
        let registry = CommandRegistry::stage_11b();
        let help = match registry.resolve(
            &CommandIntent::Slash {
                name: "help".into(),
                argument: String::new(),
            },
            &PlanState::default(),
            &provider(),
            &[],
        ) {
            CommandOutcome::Help(text) => text,
            other => panic!("expected help text, got {other:?}"),
        };
        for command in [
            "/memory",
            "/goal",
            "/sessions-new",
            "/checkpoint",
            "/export",
            "/export last",
            "/copy",
            "/ci",
            "/mcp",
            "/plugins",
            "/settings",
            "/permission",
            "/mode",
            "/generation",
            "/auxiliary",
            "/mixture",
            "/reasoning-panel",
            "/statusline",
            "/theme",
            "/screen-reader",
            "/native-mouse",
            "/sound",
            "/history",
            "/search",
            "/prompt",
            "/btw",
            "/image",
            "/attach",
            "/image-render",
            "/screenshot",
            "/blocks",
            "/annotate",
            "/vim",
            "/mobile",
            "/keybinds",
        ] {
            assert!(help.contains(command), "{command} must be active in help");
        }
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
        // LOCAL_COMMANDS surface (80 commands) PLUS the four Vesper-native
        // commands (approve, cancel, quit, auth) the oracle handles via
        // keybindings (auth is provider-routed). Every command below must be
        // recognized.
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
        // The full surface count: 80 oracle command names (including the
        // distinct `/export last` route) + 9 Vesper-native (approve, cancel,
        // quit, auth, lmstudio, provider, remember, recall, forget, memories,
        // promote, demote) plus later Vesper-native controls.
        assert!(registry.contains("export last"));
        assert!(
            registry.contains("auth"),
            "Vesper-native /auth must be registered"
        );
        assert!(
            registry.contains("lmstudio"),
            "Vesper-native /lmstudio must be registered"
        );
        assert!(
            registry.contains("provider"),
            "Vesper-native /provider must be registered"
        );
        assert!(
            registry.contains("remember"),
            "Stage 16 Vesper-native /remember must be registered"
        );
        assert!(
            registry.contains("recall"),
            "Stage 16 Vesper-native /recall must be registered"
        );
        assert!(
            registry.contains("forget"),
            "Stage 16 Vesper-native /forget must be registered"
        );
        for command in ["memories", "promote", "demote"] {
            assert!(
                registry.contains(command),
                "scoped cognitive-memory command /{command} must be registered"
            );
        }
        assert!(
            registry.contains("interview-limit"),
            "VesperLens-native /interview-limit must be registered"
        );
        assert!(
            registry.contains("chat-only"),
            "Vesper-native /chat-only must be registered"
        );
        assert_eq!(
            registry.names().len(),
            97,
            "Phase 7 parity: 80 oracle commands + 17 Vesper-native = 97 total \
             (Vesper-native: approve, cancel, auth, lmstudio, provider, embedding, \
             chat-only, quit, remember, recall, forget, memories, promote, demote, \
             interview-limit, sandbox)"
        );
    }

    #[test]
    fn scoped_cognition_commands_parse_overrides_and_lifecycle_operations() {
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/remember --global my name is Alex")),
            CommandOutcome::Cognition(CognitionOp::Remember {
                text: "my name is Alex".into(),
                scope: CognitionScope::Global,
            })
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse(
                "/remember mock server runs on port 8321"
            )),
            CommandOutcome::Cognition(CognitionOp::Remember {
                text: "mock server runs on port 8321".into(),
                scope: CognitionScope::Smart,
            })
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/recall --project mock server")),
            CommandOutcome::Cognition(CognitionOp::Recall {
                query: "mock server".into(),
                scope: CognitionScope::Project,
            })
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/memories Alex")),
            CommandOutcome::Cognition(CognitionOp::Audit {
                query: Some("Alex".into()),
            })
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/promote abc123")),
            CommandOutcome::Cognition(CognitionOp::Promote {
                id: "abc123".into(),
            })
        );
        assert!(matches!(
            resolve_bare_intent(&CommandIntent::parse(
                "/remember --global --project contradictory"
            )),
            CommandOutcome::Error(_)
        ));
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

    /// Helper: resolve a parsed [`CommandIntent`] (used by Phase 8 tests so
    /// the input can include an argument like `/memory needle`).
    fn resolve_bare_intent(intent: &CommandIntent) -> CommandOutcome {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        registry.resolve(intent, &plan_state, &provider, &[])
    }

    #[test]
    fn planmode_and_endpoint_selectors_have_distinct_oracle_semantics() {
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/planmode ship the matrix")),
            CommandOutcome::Plan {
                prd: "ship the matrix".into()
            }
        );
        // PRD provider-capability-gating FR-1: `/plan` values come from the
        // descriptor the active provider advertises. Advertised → resolves to
        // the harness EndpointPlan control with the advertised scale.
        let advertised = vec![choice_descriptor(
            "plan",
            &["coding", "standard", "bigmodel"],
        )];
        for alias in ["plan", "api-plan", "endpoint"] {
            let registry = CommandRegistry::stage_11b();
            let plan_state = PlanState::default();
            let provider = provider();
            let outcome = registry.resolve(
                &CommandIntent::Slash {
                    name: alias.into(),
                    argument: "standard".into(),
                },
                &plan_state,
                &provider,
                &advertised,
            );
            assert_eq!(
                outcome,
                CommandOutcome::SessionConfig {
                    key: SessionConfigKey::EndpointPlan,
                    value: "standard".into()
                },
                "/{alias} should select the advertised API endpoint plan"
            );
        }
        // Not advertised (e.g. a provider without endpoint plans) → truthful
        // error, never a foreign provider's value list.
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "plan".into(),
                argument: "standard".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(
            outcome,
            CommandOutcome::Error("/plan is not available on the active provider.".into())
        );
    }

    #[test]
    fn generation_and_auxiliary_require_advertised_values() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let advertised = vec![
            choice_descriptor("generation", &["balanced", "precise", "exploratory"]),
            choice_descriptor("auxiliary", &["main", "glm-5.3"]),
        ];
        assert_eq!(
            registry.resolve(
                &CommandIntent::parse("/generation precise"),
                &plan_state,
                &provider,
                &advertised
            ),
            CommandOutcome::SessionConfig {
                key: SessionConfigKey::GenerationProfile,
                value: "precise".into()
            }
        );
        assert_eq!(
            registry.resolve(
                &CommandIntent::parse("/auxiliary glm-5.3"),
                &plan_state,
                &provider,
                &advertised
            ),
            CommandOutcome::SessionConfig {
                key: SessionConfigKey::AuxiliaryModel,
                value: "glm-5.3".into()
            }
        );
        // A value outside the advertised scale is rejected.
        assert_eq!(
            registry.resolve(
                &CommandIntent::parse("/auxiliary glm-4.5v"),
                &plan_state,
                &provider,
                &advertised
            ),
            CommandOutcome::Error(
                "Invalid /auxiliary value `glm-4.5v`. Allowed: main, glm-5.3".into()
            )
        );
        // Mixture stays harness-owned and structural.
        assert_eq!(
            registry.resolve(
                &CommandIntent::parse("/mixture enabled"),
                &plan_state,
                &provider,
                &advertised
            ),
            CommandOutcome::SessionConfig {
                key: SessionConfigKey::MixtureMode,
                value: "enabled".into()
            }
        );
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
            CommandOutcome::Ui(UiAction::ToggleTasks)
        );
        assert_eq!(
            resolve_bare("max-iterations"),
            CommandOutcome::ContextView(ViewKind::MaxIterations)
        );
        assert_eq!(resolve_bare("usage"), CommandOutcome::ProviderUsage);
    }

    #[test]
    fn sandbox_command_parses_on_off_and_status_parity_arguments() {
        // VRO-13 PR-4 host parity: bare and `status` show the panel; `on`
        // and `off` are honest restart instructions (the route holder is
        // first-resolution-wins at boot, identical to the ACP host).
        assert_eq!(
            resolve_bare("sandbox"),
            CommandOutcome::ContextView(ViewKind::Sandbox)
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/sandbox status")),
            CommandOutcome::ContextView(ViewKind::Sandbox)
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/sandbox on")),
            CommandOutcome::SandboxControl(SandboxControl::On)
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/sandbox enable")),
            CommandOutcome::SandboxControl(SandboxControl::On)
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/sandbox off")),
            CommandOutcome::SandboxControl(SandboxControl::Off)
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/sandbox disable")),
            CommandOutcome::SandboxControl(SandboxControl::Off)
        );
        // Unknown arguments are a bounded usage error, never a silent
        // fallthrough to the status panel.
        assert!(matches!(
            resolve_bare_intent(&CommandIntent::parse("/sandbox maybe")),
            CommandOutcome::Error(_)
        ));
    }

    #[test]
    fn interview_limit_defaults_to_status_and_accepts_auto_or_bounded_values() {
        assert_eq!(
            resolve_bare("interview-limit"),
            CommandOutcome::InterviewLimitStatus
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/interview-limit auto")),
            CommandOutcome::InterviewLimit(InterviewQuestionLimit::Auto)
        );
        assert_eq!(
            resolve_bare_intent(&CommandIntent::parse("/interview-limit 8")),
            CommandOutcome::InterviewLimit(InterviewQuestionLimit::Fixed(8))
        );
        for invalid in ["0", "13", "many"] {
            assert!(matches!(
                resolve_bare_intent(&CommandIntent::parse(&format!(
                    "/interview-limit {invalid}"
                ))),
                CommandOutcome::Error(_)
            ));
        }
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
    fn phase8_memory_commands_resolve_to_memory_ops() {
        // Phase 8 (ADR 0011): the 13 memory/awareness commands must resolve
        // to a real `Memory(MemoryOp)` outcome — never Deferred, never Error.
        // This is the parity guarantee the lead architect demanded: no
        // command stays stubbed once its owning subsystem ships.
        let cases: &[(&str, &str, &str)] = &[
            ("/memory", "", "memory"),
            ("/memory needle", "needle", "memory"),
            ("/goal ship stage 12", "ship stage 12", "goal"),
            ("/subgoal write 27 tests", "write 27 tests", "subgoal"),
            ("/skills", "", "skills"),
            ("/profile", "", "profile"),
            ("/awareness", "", "awareness"),
            ("/metacognition", "", "metacognition"),
            ("/deliberation", "", "deliberation"),
            ("/repository", "", "repository"),
            ("/meta-learning", "", "meta-learning"),
            ("/observability", "", "observability"),
            ("/curator", "", "curator"),
            ("/journey", "", "journey"),
        ];
        for (input, expected_arg, expected_command) in cases {
            let intent = CommandIntent::parse(input);
            let outcome = resolve_bare_intent(&intent);
            match outcome {
                CommandOutcome::Memory(op) => {
                    assert_eq!(
                        op.command_name(),
                        *expected_command,
                        "/{expected_command} resolved with the wrong command_name"
                    );
                    // Argument presence matches the expected kind.
                    match (*expected_command, expected_arg) {
                        ("memory", arg) => {
                            let MemoryOp::MemoryList { needle } = op else {
                                panic!("expected MemoryList, got {op:?}");
                            };
                            let expected: Option<&str> =
                                if arg.is_empty() { None } else { Some(arg) };
                            assert_eq!(needle.as_deref(), expected);
                        }
                        ("goal", arg) => {
                            let MemoryOp::GoalAdd { summary } = op else {
                                panic!("expected GoalAdd, got {op:?}");
                            };
                            assert_eq!(summary, *arg);
                        }
                        ("subgoal", arg) => {
                            let MemoryOp::SubgoalAdd { summary } = op else {
                                panic!("expected SubgoalAdd, got {op:?}");
                            };
                            assert_eq!(summary, *arg);
                        }
                        _ => {}
                    }
                }
                other => panic!("{input} should resolve to Memory(_), got {other:?}"),
            }
        }
    }

    #[test]
    fn phase8_goal_and_subgoal_require_a_summary() {
        // /goal and /subgoal without an argument must Error with a clear
        // usage hint — they are not silently no-ops and not Deferred.
        assert!(matches!(resolve_bare("goal"), CommandOutcome::Error(_)));
        assert!(matches!(resolve_bare("subgoal"), CommandOutcome::Error(_)));
    }

    #[test]
    fn phase8_memory_command_count_matches_directive() {
        // Sanity guard: exactly 13 memory/awareness commands resolve to a
        // Memory(MemoryOp) outcome. If the directive's "Stage 12: 13
        // commands" contract drifts, this test surfaces it immediately.
        // /goal and /subgoal require an argument, so we send them with one.
        let bare_commands = [
            "memory",
            "skills",
            "profile",
            "awareness",
            "metacognition",
            "deliberation",
            "repository",
            "meta-learning",
            "observability",
            "curator",
            "journey",
        ];
        let arg_commands = [("goal", "ship stage 12"), ("subgoal", "write tests")];
        let mut matched = 0;
        for name in bare_commands {
            if matches!(resolve_bare(name), CommandOutcome::Memory(_)) {
                matched += 1;
            }
        }
        for (name, arg) in arg_commands {
            let intent = CommandIntent::Slash {
                name: name.into(),
                argument: arg.into(),
            };
            if matches!(resolve_bare_intent(&intent), CommandOutcome::Memory(_)) {
                matched += 1;
            }
        }
        assert_eq!(
            matched, 13,
            "exactly 13 memory commands must resolve to Memory(_)"
        );
    }

    #[test]
    fn shared_host_parity_commands_are_registered_in_tui() {
        for command in vesper_domain::HOST_PARITY_SLASH_COMMANDS {
            assert!(
                ORACLE_COMMAND_SURFACE
                    .iter()
                    .any(|entry| entry.name == command.name),
                "shared host-neutral command /{} is missing from TUI",
                command.name
            );
        }
    }

    #[test]
    fn phase9_checkpoint_commands_resolve_to_checkpoint_ops() {
        // Phase 9 (ADR 0012): the 13 checkpoint/session/loop/export/copy/ci
        // commands must resolve to a real `Checkpoint(CheckpointOp)` outcome
        // — never Deferred, never Error (except for argument-required ones).
        let bare_cases: &[(&str, &str)] = &[
            ("/sessions", "sessions"),
            ("/lineage", "lineage"),
            ("/checkpoint", "checkpoint"),
            ("/export", "export"),
            ("/copy", "copy"),
            ("/ci", "ci"),
        ];
        for (input, expected_command) in bare_cases {
            let intent = CommandIntent::parse(input);
            let outcome = resolve_bare_intent(&intent);
            match outcome {
                CommandOutcome::Checkpoint(op) => {
                    assert_eq!(
                        op.command_name(),
                        *expected_command,
                        "/{expected_command} resolved with the wrong command_name"
                    );
                }
                other => panic!("{input} should resolve to Checkpoint(_), got {other:?}"),
            }
        }

        assert!(matches!(
            resolve_bare_intent(&CommandIntent::parse("/export last")),
            CommandOutcome::Checkpoint(CheckpointOp::SessionExportLast)
        ));
    }

    #[test]
    fn phase9_argument_required_checkpoint_commands_error_clearly() {
        // /rollback, /rewind, /rename, /loop without an argument must Error
        // with a clear usage hint — they are not silently no-ops and not
        // Deferred.
        for command in ["rollback", "rewind", "rename", "loop"] {
            assert!(
                matches!(resolve_bare(command), CommandOutcome::Error(_)),
                "/{command} with no argument must Error"
            );
        }
    }

    #[test]
    fn phase9_checkpoint_command_count_matches_directive() {
        // Sanity guard: exactly 13 checkpoint/session/loop/export/copy/ci
        // commands resolve to a Checkpoint(CheckpointOp) outcome. If the
        // directive's "Stage 14: 13 commands" contract drifts, this test
        // surfaces it immediately.
        let bare = [
            "sessions",
            "lineage",
            "checkpoint",
            "export",
            "copy",
            "ci",
            "sessions-new",
            "branch",
            "undo",
        ];
        let arg_commands = [
            ("rollback", "ckpt-1"),
            ("rewind", "ckpt-1"),
            ("rename", "new-name"),
            ("loop", "every 1h run tests"),
        ];
        let mut matched = 0;
        for name in bare {
            if matches!(resolve_bare(name), CommandOutcome::Checkpoint(_)) {
                matched += 1;
            }
        }
        for (name, arg) in arg_commands {
            let intent = CommandIntent::Slash {
                name: name.into(),
                argument: arg.into(),
            };
            if matches!(resolve_bare_intent(&intent), CommandOutcome::Checkpoint(_)) {
                matched += 1;
            }
        }
        assert_eq!(
            matched, 13,
            "exactly 13 checkpoint commands must resolve to Checkpoint(_)"
        );
    }

    #[test]
    fn phase10_mcp_and_plugins_resolve_to_mcp_ops() {
        // Phase 10 (ADR 0013): /mcp and /plugins must resolve to a real
        // Mcp(McpOp) outcome — never Deferred, never Error (except for
        // subcommands with missing arguments).
        let bare = ["/mcp", "/plugins"];
        for input in bare {
            let intent = CommandIntent::parse(input);
            let outcome = resolve_bare_intent(&intent);
            match outcome {
                CommandOutcome::Mcp(op) => {
                    let expected = if input == "/mcp" { "mcp" } else { "plugins" };
                    assert_eq!(op.command_name(), expected);
                }
                other => panic!("{input} should resolve to Mcp(_), got {other:?}"),
            }
        }
    }

    #[test]
    fn phase10_mcp_subcommands_parse_correctly() {
        let cases: &[(&str, &str)] = &[
            ("/mcp list", "mcp"),
            ("/mcp add srv echo hi", "mcp"),
            ("/mcp remove srv", "mcp"),
            ("/mcp tools srv", "mcp"),
        ];
        for (input, expected) in cases {
            let intent = CommandIntent::parse(input);
            let outcome = resolve_bare_intent(&intent);
            match outcome {
                CommandOutcome::Mcp(op) => assert_eq!(op.command_name(), *expected),
                other => panic!("{input} should resolve to Mcp(_), got {other:?}"),
            }
        }
    }

    #[test]
    fn phase10_plugins_subcommands_parse_correctly() {
        let cases: &[(&str, &str)] = &[
            ("/plugins", "plugins"),
            ("/plugins list", "plugins"),
            ("/plugins publishers", "plugins"),
            ("/plugins verify /tmp/pkg", "plugins"),
            ("/plugins load /tmp/pkg", "plugins"),
            ("/plugins trust vesper abc123", "plugins"),
        ];
        for (input, expected) in cases {
            let intent = CommandIntent::parse(input);
            let outcome = resolve_bare_intent(&intent);
            match outcome {
                CommandOutcome::Mcp(op) => assert_eq!(op.command_name(), *expected),
                other => panic!("{input} should resolve to Mcp(_), got {other:?}"),
            }
        }
    }

    #[test]
    fn phase10_mcp_and_plugins_argument_required_subcommands_error() {
        for input in [
            "/mcp add",        // missing id + command
            "/mcp remove",     // missing id
            "/mcp tools",      // missing id
            "/plugins verify", // missing path
            "/plugins load",   // missing path
            "/plugins trust",  // missing publisher + key
        ] {
            let intent = CommandIntent::parse(input);
            let outcome = resolve_bare_intent(&intent);
            assert!(
                matches!(outcome, CommandOutcome::Error(_)),
                "{input} with missing args must Error"
            );
        }
    }

    #[test]
    fn registered_surface_has_no_missing_routes() {
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
            if let CommandOutcome::Error(message) = outcome {
                assert!(
                    !message.starts_with("internal parity violation"),
                    "/{name} lacks a concrete route: {message}"
                );
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

    // === ADR 0016 — `/embedding` slash-command tests ===
    // Directive 4: prove the parse + dispatch + BigModel resolution paths
    // work without needing the TUI binary or any network round-trip.

    #[test]
    fn embedding_pairs_parse_accepts_source_only() {
        let pairs = EmbeddingPairs::parse("source=lmstudio").unwrap();
        assert_eq!(pairs.source.as_deref(), Some("lmstudio"));
        assert!(pairs.endpoint.is_none());
        assert!(pairs.model.is_none());
        assert!(pairs.api_key.is_none());
        assert!(pairs.dimension.is_none());
        assert!(!pairs.is_empty());
    }

    #[test]
    fn embedding_pairs_parse_accepts_full_lmstudio_set() {
        let pairs = EmbeddingPairs::parse(
            "source=lmstudio endpoint=http://localhost:1234/v1/embeddings \
             model=text-embedding-nomic-embed-text-v1.5 dimension=768",
        )
        .unwrap();
        assert_eq!(pairs.source.as_deref(), Some("lmstudio"));
        assert_eq!(
            pairs.endpoint.as_deref(),
            Some("http://localhost:1234/v1/embeddings")
        );
        assert_eq!(
            pairs.model.as_deref(),
            Some("text-embedding-nomic-embed-text-v1.5")
        );
        assert_eq!(pairs.dimension, Some(768));
    }

    #[test]
    fn embedding_pairs_parse_normalizes_source_to_lowercase() {
        // Source is case-insensitive — "LMStudio" and "BigModel" must both
        // normalize so the engine's match arm finds them.
        let pairs = EmbeddingPairs::parse("source=LMStudio").unwrap();
        assert_eq!(pairs.source.as_deref(), Some("lmstudio"));
        let pairs = EmbeddingPairs::parse("source=BigModel").unwrap();
        assert_eq!(pairs.source.as_deref(), Some("bigmodel"));
        let pairs = EmbeddingPairs::parse("source=LOCAL").unwrap();
        assert_eq!(pairs.source.as_deref(), Some("local"));
    }

    #[test]
    fn embedding_pairs_parse_rejects_invalid_source() {
        let err = EmbeddingPairs::parse("source=openai").unwrap_err();
        assert!(
            err.contains("source must be one of"),
            "expected source-validation error, got: {err}"
        );
    }

    #[test]
    fn embedding_pairs_parse_rejects_unknown_key() {
        let err = EmbeddingPairs::parse("source=lmstudio batch_size=64").unwrap_err();
        assert!(
            err.contains("unknown key 'batch_size'"),
            "expected unknown-key error, got: {err}"
        );
    }

    #[test]
    fn embedding_pairs_parse_rejects_non_integer_dimension() {
        let err = EmbeddingPairs::parse("dimension=foo").unwrap_err();
        assert!(
            err.contains("dimension must be an integer"),
            "expected dimension parse error, got: {err}"
        );
    }

    #[test]
    fn embedding_pairs_parse_rejects_zero_dimension() {
        let err = EmbeddingPairs::parse("dimension=0").unwrap_err();
        assert!(
            err.contains("dimension must be > 0"),
            "expected positive-dimension error, got: {err}"
        );
    }

    #[test]
    fn embedding_pairs_parse_clears_api_key_with_null_or_none() {
        // `api_key=null` and `api_key=none` normalize to None so users can
        // explicitly unset a metered token via the slash command.
        let pairs = EmbeddingPairs::parse("api_key=null").unwrap();
        assert_eq!(pairs.api_key, None);
        let pairs = EmbeddingPairs::parse("api_key=none").unwrap();
        assert_eq!(pairs.api_key, None);
        let pairs = EmbeddingPairs::parse("api_key=sk-abc123").unwrap();
        assert_eq!(pairs.api_key.as_deref(), Some("sk-abc123"));
    }

    #[test]
    fn embedding_pairs_parse_empty_body_is_empty() {
        let pairs = EmbeddingPairs::parse("").unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn embedding_command_is_registered() {
        let registry = CommandRegistry::stage_11b();
        assert!(
            registry.contains("embedding"),
            "/embedding must be registered in the command surface"
        );
    }

    #[test]
    fn embedding_command_no_arg_resolves_to_status() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "embedding".into(),
                argument: String::new(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::Embedding(EmbeddingOp::Status));
    }

    #[test]
    fn embedding_command_set_resolves_with_parsed_pairs() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "embedding".into(),
                argument: "set source=lmstudio dimension=768".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        match outcome {
            CommandOutcome::Embedding(EmbeddingOp::Set { pairs }) => {
                assert_eq!(pairs.source.as_deref(), Some("lmstudio"));
                assert_eq!(pairs.dimension, Some(768));
            }
            other => panic!("expected Embedding::Set, got {other:?}"),
        }
    }

    #[test]
    fn embedding_command_set_with_invalid_source_errors() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "embedding".into(),
                argument: "set source=openai".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        match outcome {
            CommandOutcome::Error(msg) => assert!(
                msg.contains("source must be one of"),
                "expected source-validation error, got: {msg}"
            ),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn embedding_command_set_with_empty_body_errors_usage() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "embedding".into(),
                argument: "set".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        match outcome {
            CommandOutcome::Error(msg) => assert!(
                msg.contains("Usage: /embedding set"),
                "expected usage error, got: {msg}"
            ),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn embedding_command_clear_resolves_to_clear_op() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "embedding".into(),
                argument: "clear".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        assert_eq!(outcome, CommandOutcome::Embedding(EmbeddingOp::Clear));
    }

    #[test]
    fn embedding_command_unknown_subcommand_errors() {
        let registry = CommandRegistry::stage_11b();
        let plan_state = PlanState::default();
        let provider = provider();
        let outcome = registry.resolve(
            &CommandIntent::Slash {
                name: "embedding".into(),
                argument: "frobnicate".into(),
            },
            &plan_state,
            &provider,
            &[],
        );
        match outcome {
            CommandOutcome::Error(msg) => assert!(
                msg.contains("Unknown /embedding subcommand"),
                "expected unknown-subcommand error, got: {msg}"
            ),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn embedding_command_help_text_lists_set_and_clear() {
        // The help text in CommandRegistry::help_text() is human-curated;
        // the registry surface must keep /embedding discoverable.
        let registry = CommandRegistry::stage_11b();
        assert!(registry.contains("embedding"));
        // Stage 11b registry surfaces the entry in completion_candidates.
        let candidates = registry.completion_candidates("/embed");
        assert!(
            candidates.iter().any(|(name, _)| name == "/embedding"),
            "expected /embedding in completion candidates, got {candidates:?}"
        );
    }

    // ===================================================================
    // VRO-8 (PRD §8.1) — `/reasoning set mode=<X>` override + status.
    // ===================================================================

    #[test]
    fn vro8_parse_reasoning_mode_accepts_six_prd_variants_and_max_shorthand() {
        use vesper_domain::ReasoningMode;
        // The six PRD §8.1 mode names must all parse (case-insensitive).
        assert_eq!(parse_reasoning_mode("auto"), Some(ReasoningMode::Auto));
        assert_eq!(parse_reasoning_mode("fast"), Some(ReasoningMode::Fast));
        assert_eq!(
            parse_reasoning_mode("balanced"),
            Some(ReasoningMode::Balanced)
        );
        assert_eq!(parse_reasoning_mode("deep"), Some(ReasoningMode::Deep));
        assert_eq!(
            parse_reasoning_mode("maximum"),
            Some(ReasoningMode::Maximum)
        );
        assert_eq!(parse_reasoning_mode("off"), Some(ReasoningMode::Off));
        // `max` is an accepted shorthand for `Maximum` (matching the
        // oracle's `/thinking max` convention).
        assert_eq!(parse_reasoning_mode("max"), Some(ReasoningMode::Maximum));
        // Case-insensitive.
        assert_eq!(parse_reasoning_mode("DEEP"), Some(ReasoningMode::Deep));
        assert_eq!(parse_reasoning_mode("  Auto  "), Some(ReasoningMode::Auto));
    }

    #[test]
    fn vro8_parse_reasoning_mode_rejects_invented_modes() {
        // Invented modes (e.g. `low`/`turbo`/`reasoning-3`) must NOT parse —
        // the resolver surfaces a usage error instead of silently inventing
        // a new variant. Defense in depth: PRD §8.1 is the authoritative
        // mode list.
        for bad in ["low", "turbo", "extreme", "", "ultra", "thinking"] {
            assert_eq!(
                parse_reasoning_mode(bad),
                None,
                "invented mode {bad:?} must not parse"
            );
        }
    }

    #[test]
    fn vro8_reasoning_set_mode_resolves_to_override_outcome() {
        // Directive 2: `/reasoning set mode=deep` must resolve to a
        // CommandOutcome::ReasoningOverride carrying Deep. The dispatcher
        // then mutates `SessionState.reasoning_mode_override`.
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let intent = CommandIntent::parse("/reasoning set mode=deep");
        let outcome = registry.resolve(&intent, &plan, &provider(), &[]);
        match outcome {
            CommandOutcome::ReasoningOverride { mode } => {
                assert_eq!(mode, vesper_domain::ReasoningMode::Deep);
            }
            other => panic!("expected ReasoningOverride, got {other:?}"),
        }
    }

    #[test]
    fn vro8_reasoning_set_mode_auto_clears_via_resolver() {
        // `set mode=auto` resolves to ReasoningOverride { Auto }; the
        // dispatcher normalizes Auto to None.
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let outcome = registry.resolve(
            &CommandIntent::parse("/reasoning set mode=auto"),
            &plan,
            &provider(),
            &[],
        );
        match outcome {
            CommandOutcome::ReasoningOverride { mode } => {
                assert_eq!(mode, vesper_domain::ReasoningMode::Auto);
            }
            other => panic!("expected ReasoningOverride, got {other:?}"),
        }
    }

    #[test]
    fn vro8_reasoning_clear_alias_resolves_to_auto_override() {
        // `/reasoning clear` is an alias for `set mode=auto`.
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let outcome = registry.resolve(
            &CommandIntent::parse("/reasoning clear"),
            &plan,
            &provider(),
            &[],
        );
        match outcome {
            CommandOutcome::ReasoningOverride { mode } => {
                assert_eq!(mode, vesper_domain::ReasoningMode::Auto);
            }
            other => panic!("expected ReasoningOverride, got {other:?}"),
        }
    }

    #[test]
    fn vro8_reasoning_no_arg_resolves_to_status() {
        // Bare `/reasoning` surfaces the current override (read by dispatch
        // from SessionState). The resolver returns the marker variant.
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let outcome =
            registry.resolve(&CommandIntent::parse("/reasoning"), &plan, &provider(), &[]);
        assert!(matches!(outcome, CommandOutcome::ReasoningStatus));
    }

    #[test]
    fn vro8_reasoning_set_mode_invalid_surfaces_usage_error() {
        // An unknown mode value must surface a usage error listing the six
        // PRD §8.1 modes (defense in depth against inventing a new variant).
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let outcome = registry.resolve(
            &CommandIntent::parse("/reasoning set mode=extreme"),
            &plan,
            &provider(),
            &[],
        );
        match outcome {
            CommandOutcome::Error(msg) => {
                assert!(msg.contains("Unknown reasoning mode"), "got: {msg}");
                // The usage hint must enumerate every PRD mode.
                for allowed in ["auto", "fast", "balanced", "deep", "maximum", "off"] {
                    assert!(msg.contains(allowed), "missing {allowed:?} in error: {msg}");
                }
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn vro8_reasoning_set_without_mode_surfaces_usage_error() {
        // `/reasoning set foo=bar` must reject the unknown key.
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let outcome = registry.resolve(
            &CommandIntent::parse("/reasoning set foo=bar"),
            &plan,
            &provider(),
            &[],
        );
        match outcome {
            CommandOutcome::Error(msg) => {
                assert!(msg.contains("Usage: /reasoning set mode="), "got: {msg}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn vro8_reasoning_level_arg_falls_through_to_thinking_superpower() {
        // Backward-compat: `/reasoning <level>` (where level is one of the
        // oracle's thinking values) still routes to the thinking
        // superpower, NOT the VRO override path. The registry advertises a
        // `thinking` superpower so the alias resolves.
        let registry = CommandRegistry::stage_11b();
        let plan = crate::plan_mode::PlanState::default();
        let descriptor = choice_descriptor("thinking", &["disabled", "enabled", "high", "max"]);
        let outcome = registry.resolve(
            &CommandIntent::parse("/reasoning high"),
            &plan,
            &provider(),
            &[descriptor],
        );
        match outcome {
            CommandOutcome::Superpower { value, .. } => {
                assert!(matches!(
                    value,
                    SuperpowerValue::Choice { ref value } if value.as_str() == "high"
                ));
            }
            other => panic!("expected Superpower fallback, got {other:?}"),
        }
    }
}
