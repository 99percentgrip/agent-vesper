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
    ContentPart, ConversationMessage, SessionId, SessionOperatingMode, SessionPermissionMode,
    WorkspaceRoot,
};

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

/// Normalized prompt request supplied to a composed engine.
#[derive(Debug, Clone)]
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
    /// Optional live ACP client permission bridge.
    pub permission_requester: Option<Arc<dyn AcpPermissionRequester>>,
}

/// Normalized final result. Streaming engines may emit their own updates in
/// a future extension; the initial boundary remains bounded and deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptResult {
    /// Final assistant text to publish as one ACP message chunk.
    pub text: String,
    /// Whether the host cancelled the turn before completion.
    pub cancelled: bool,
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
