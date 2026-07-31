use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{
    BoundedString, CommandId, CommandSchemaVersion, ContentPart, CorrelationId, ExtensionMap,
    MessageId, PermissionOutcome, PermissionRequestId, Revision, SessionId, SessionOperatingMode,
    SessionPermissionMode, VersionedExtensionEnvelope,
};

/// Adapter-neutral runtime capability negotiated during initialization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeCapability {
    /// Create sessions.
    NewSession,
    /// Load sessions.
    LoadSession,
    /// Resume sessions.
    ResumeSession,
    /// List sessions.
    ListSessions,
    /// Fork sessions.
    ForkSession,
    /// Close sessions.
    CloseSession,
    /// Cancel active turns.
    Cancellation,
    /// Interactive permission decisions.
    Permissions,
    /// Additional workspace roots.
    AdditionalWorkspaceRoots,
    /// Usage updates.
    UsageUpdates,
    /// Slash-command dispatch.
    SlashCommands,
}

/// Authentication method descriptor for protocol/front-end negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAuthenticationMethod {
    /// Stable method ID.
    pub method_id: BoundedString<128>,
    /// Safe user-facing label.
    pub display_name: BoundedString<256>,
    /// Whether an external runtime owns the authentication flow.
    pub external_runtime_owned: bool,
}

/// Runtime command initiator without coupling to an ACP/CLI/TUI implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandInitiator {
    /// ACP adapter.
    Acp,
    /// Plain command-line frontend.
    Cli,
    /// Full-screen terminal frontend.
    Tui,
    /// Scheduled runtime activity.
    Automation,
    /// Internal runtime recovery or continuation.
    Runtime,
    /// A future namespaced adapter.
    ExternalAdapter,
}

/// Workspace root supplied during initialization or session creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRoot {
    /// Adapter-visible logical name.
    pub name: BoundedString<256>,
    /// Path string interpreted only by a later authorized runtime.
    pub path: BoundedString<32768>,
    /// Whether this is the primary root.
    pub primary: bool,
}

/// Session listing filter that carries no persistence implementation detail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListFilter {
    /// Optional workspace path match.
    pub workspace: Option<BoundedString<32768>>,
    /// Whether closed sessions are included.
    pub include_closed: bool,
    /// Bounded requested result count.
    pub limit: Option<u32>,
}

/// Runtime initialization intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeInitialization {
    /// Adapter-reported client identity.
    pub client_name: BoundedString<256>,
    /// Ordered workspace roots.
    pub workspace_roots: Vec<WorkspaceRoot>,
    /// Client-side capabilities.
    pub client_capabilities: BTreeSet<RuntimeCapability>,
    /// Authentication methods the adapter can represent.
    pub authentication_methods: Vec<RuntimeAuthenticationMethod>,
    /// Frontend-owned versioned metadata.
    pub frontend: Option<VersionedExtensionEnvelope>,
}

/// Prompt payload accepted by the future session actor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptSubmission {
    /// User-message identity, preserved across adapters.
    pub message_id: MessageId,
    /// Ordered prompt content.
    pub content: Vec<ContentPart>,
    /// Adapter-provided metadata.
    #[serde(default)]
    pub extensions: ExtensionMap,
}

/// Provider-neutral runtime command payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "kebab-case")]
pub enum HarnessCommandPayload {
    /// Initialize the runtime boundary.
    InitializeRuntime(RuntimeInitialization),
    /// Create a new session.
    CreateSession {
        /// Primary workspace plus additional roots.
        workspace_roots: Vec<WorkspaceRoot>,
        /// Optional requested session identity.
        requested_session_id: Option<SessionId>,
    },
    /// Load a session without implying persistence implementation.
    LoadSession {
        /// Session identity.
        session_id: SessionId,
        /// Caller workspace context: one primary root plus additions.
        workspace_roots: Vec<WorkspaceRoot>,
    },
    /// Resume a loaded or persisted session.
    ResumeSession {
        /// Session identity.
        session_id: SessionId,
        /// Caller workspace context: one primary root plus additions.
        workspace_roots: Vec<WorkspaceRoot>,
    },
    /// List sessions.
    ListSessions(SessionListFilter),
    /// Fork an existing session.
    ForkSession {
        /// Source session.
        session_id: SessionId,
        /// Optional requested child ID.
        requested_session_id: Option<SessionId>,
    },
    /// Close a session.
    CloseSession {
        /// Session identity.
        session_id: SessionId,
    },
    /// Submit one prompt.
    SubmitPrompt {
        /// Session identity.
        session_id: SessionId,
        /// Prompt data.
        prompt: PromptSubmission,
    },
    /// Execute a slash command through the same correlated runtime boundary.
    ExecuteSlashCommand {
        /// Session identity.
        session_id: SessionId,
        /// User-message identity associated with the command.
        message_id: MessageId,
        /// Stable command name without the leading slash.
        name: BoundedString<128>,
        /// Ordered command arguments.
        arguments: Vec<BoundedString<4096>>,
    },
    /// Cancel an active turn.
    CancelTurn {
        /// Session identity.
        session_id: SessionId,
        /// Turn identity.
        turn_id: crate::TurnId,
    },
    /// Change code/plan or permission mode.
    UpdateSessionMode {
        /// Session identity.
        session_id: SessionId,
        /// Optional operating-mode update.
        operating_mode: Option<SessionOperatingMode>,
        /// Optional permission-mode update.
        permission_mode: Option<SessionPermissionMode>,
    },
    /// Update the session-scoped provider reasoning mode (e.g. the GLM
    /// `thought_level` dial). `None` resets to the runtime default. The mode
    /// is an opaque, provider-defined label kept provider-neutral here so the
    /// domain never depends on a concrete adapter.
    UpdateSessionReasoning {
        /// Session identity.
        session_id: SessionId,
        /// Provider-defined reasoning-mode label (`disabled`/`enabled`/`high`/
        /// `max` for GLM). `None` clears any session override.
        mode: Option<BoundedString<128>>,
    },
    /// Apply provider-owned validated configuration.
    UpdateProviderConfiguration {
        /// Session identity where configuration is session-scoped.
        session_id: Option<SessionId>,
        /// Provider-owned data.
        configuration: VersionedExtensionEnvelope,
    },
    /// Apply runtime-owned configuration.
    UpdateRuntimeConfiguration {
        /// Versioned runtime settings.
        configuration: VersionedExtensionEnvelope,
    },
    /// Resolve one pending permission request.
    ProvidePermissionDecision {
        /// Session identity.
        session_id: SessionId,
        /// Permission request.
        request_id: PermissionRequestId,
        /// Fail-closed terminal outcome.
        outcome: PermissionOutcome,
    },
    /// Request orderly runtime shutdown.
    RequestRuntimeShutdown,
}

/// Versioned, correlated command boundary shared by future adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessCommand {
    /// Command schema version.
    pub schema_version: CommandSchemaVersion,
    /// Stable command identity.
    pub command_id: CommandId,
    /// Cross-command/event correlation identity.
    pub correlation_id: CorrelationId,
    /// Initiating boundary.
    pub initiator: CommandInitiator,
    /// Optimistic state revision where applicable.
    pub expected_revision: Option<Revision>,
    /// Typed command payload.
    pub payload: HarnessCommandPayload,
}
