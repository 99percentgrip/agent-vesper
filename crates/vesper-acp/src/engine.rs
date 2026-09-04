//! Optional composed agent-engine boundary.
//!
//! `vesper-acp` remains provider/runtime neutral. Hosts that need the full
//! multi-turn harness can inject an [`AcpPromptEngine`]; the adapter keeps the
//! existing runtime single-turn path when no engine is supplied.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use vesper_domain::{
    ContentPart, ConversationMessage, NormalizedUsage, QualifiedModelId, SessionId,
    SessionOperatingMode, SessionPermissionMode, WorkspaceRoot,
};
use vesper_runtime::ProviderConfiguration;

/// Boxed future returned by an ACP prompt engine.
pub type AcpPromptFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Normalized permission decision returned by an ACP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpPermissionDecision {
    /// The client selected the one-time allow option.
    Allow,
    /// The client selected a rejection option or returned an invalid outcome.
    Deny,
    /// The active ACP turn was cancelled.
    Cancelled,
}

/// Bounded permission request passed from the provider-neutral engine to ACP.
#[derive(Debug, Clone)]
pub struct AcpPermissionRequest {
    /// ACP session identity.
    pub session_id: SessionId,
    /// Stable harness tool name.
    pub tool: String,
    /// Model-supplied tool arguments.
    pub arguments: Value,
    /// Human-readable title presented by the client.
    pub title: String,
    /// Static policy reason for the approval prompt.
    pub reason: String,
}

/// Host bridge for live ACP permission prompts.
pub trait AcpPermissionRequester: Send + Sync + std::fmt::Debug {
    /// Sends one permission request and waits for the client's decision.
    fn request<'a>(
        &'a self,
        request: AcpPermissionRequest,
    ) -> AcpPromptFuture<'a, AcpPermissionDecision>;

    /// Cancels the pending request for a session, if one exists.
    fn cancel(&self, _session_id: &SessionId) {}
}

/// Live progress event pushed by a streaming engine during one turn.
///
/// Mirrors the runtime's `HarnessEventPayload` vocabulary that the
/// single-turn ACP path already maps to ACP session updates. Engines that
/// stream push these through [`AcpEventSink::event`]; the adapter translates
/// each one to the same wire shape the runtime event pump produces so ACP
/// clients observe identical behavior on both paths.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpEngineEvent {
    /// Provider-visible reasoning delta.
    ReasoningDelta {
        /// Reasoning text fragment.
        text: String,
    },
    /// User-visible assistant text delta.
    ContentDelta {
        /// Assistant text fragment.
        text: String,
    },
    /// A named tool call started (after permission gating).
    ToolStarted {
        /// Stable ACP tool-call id for this call.
        tool_call_id: String,
        /// Harness tool name.
        name: String,
        /// Secret-safe argument hint (whitelisted keys only).
        hint: String,
        /// Full raw arguments, for the client's inspector pane.
        arguments: Value,
    },
    /// A named tool call finished.
    ToolFinished {
        /// Stable ACP tool-call id matching the started event.
        tool_call_id: String,
        /// Harness tool name.
        name: String,
        /// Whether the call succeeded.
        success: bool,
        /// Bounded result note (size digest or first error line).
        note: String,
        /// Structured successful workspace edit, when the tool produced one.
        change: Option<vesper_domain::FileChangePreview>,
    },
    /// Cumulative token usage for the whole turn.
    Usage {
        /// Normalized cumulative usage reported by the provider.
        usage: NormalizedUsage,
    },
    /// The model replaced the task plan.
    PlanUpdated {
        /// Plan markdown.
        markdown: String,
    },
}

/// Sink the adapter supplies to a streaming engine so it can publish live
/// ACP session updates through the adapter's bounded output flow.
pub trait AcpEventSink: Send + Sync + std::fmt::Debug {
    /// Publishes one event. Returns an error only for unrecoverable
    /// transport failures; backpressure is handled inside the sink.
    fn event(&self, event: AcpEngineEvent);
}

/// Normalized prompt request supplied to a composed engine.
#[derive(Clone)]
pub struct AcpPromptRequest {
    /// ACP session identity.
    pub session_id: SessionId,
    /// Parsed prompt content.
    pub content: Vec<ContentPart>,
    /// Visible runtime history to seed a newly composed engine.
    pub history: Vec<ConversationMessage>,
    /// Active operating mode.
    pub operating_mode: SessionOperatingMode,
    /// Active permission mode.
    pub permission_mode: SessionPermissionMode,
    /// ACP workspace roots.
    pub workspace_roots: Vec<WorkspaceRoot>,
    /// Session-scoped provider configuration snapshot from the runtime
    /// (reflects `session/set_config_option` model/plan selections). A
    /// composed engine merges it over its own defaults so footer controls
    /// take effect on the very next turn.
    pub provider_configuration: Option<ProviderConfiguration>,
    /// Session-scoped model selection from the runtime (qualified id).
    pub model: Option<QualifiedModelId>,
    /// Optional live ACP client permission bridge.
    pub permission_requester: Option<Arc<dyn AcpPermissionRequester>>,
    /// Live event sink for streaming turns. When absent the engine must not
    /// stream and should fall back to publishing only the final text.
    pub event_sink: Option<Arc<dyn AcpEventSink>>,
}

impl std::fmt::Debug for AcpPromptRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpPromptRequest")
            .field("session_id", &self.session_id)
            .field("content", &self.content)
            .field("history", &self.history.len())
            .field("operating_mode", &self.operating_mode)
            .field("permission_mode", &self.permission_mode)
            .field("workspace_roots", &self.workspace_roots)
            .field("provider_configuration", &self.provider_configuration)
            .field("model", &self.model)
            .field(
                "has_permission_requester",
                &self.permission_requester.is_some(),
            )
            .field("has_event_sink", &self.event_sink.is_some())
            .finish()
    }
}

/// Normalized final result. Streaming engines may emit their own updates in
/// a future extension; the initial boundary remains bounded and deterministic.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpPromptResult {
    /// Final assistant text to publish as one ACP message chunk.
    pub text: String,
    /// Whether the host cancelled the turn before completion.
    pub cancelled: bool,
    /// Whether the adapter should persist this turn into the session record.
    /// Slash-command turns set `false`: the frozen oracle echoes them to the
    /// UI but does not append their text to model-visible history. A stateful
    /// command may instead provide `history_replacement`, which is committed
    /// without persisting the slash prompt/response pair.
    pub persist_turn: bool,
    /// Validated provider-working history replacement (normally produced by
    /// automatic or manual compaction). When present, the adapter commits it
    /// atomically instead of reconstructing the turn from display text.
    pub history_replacement: Option<Vec<ConversationMessage>>,
}

/// Optional full-harness prompt engine.
pub trait AcpPromptEngine: Send + Sync {
    /// Runs one multi-turn prompt.
    fn run<'a>(
        &'a self,
        request: AcpPromptRequest,
    ) -> AcpPromptFuture<'a, Result<AcpPromptResult, String>>;

    /// Requests cancellation of the active session, if the engine supports it.
    fn cancel<'a>(&'a self, _session_id: &'a SessionId) -> AcpPromptFuture<'a, bool> {
        Box::pin(async { false })
    }
}
