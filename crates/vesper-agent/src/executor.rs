//! Tool executor trait and execution context (ADR 0010, Tier C Phase 1).
//!
//! Each parity-critical tool implements [`ToolExecutor`]. The registry routes a
//! normalized [`ToolCall`] to its executor; the executor returns bounded text
//! that the agent loop feeds back to the model as a `role: Tool` message.
//! Core executors perform bounded, confined filesystem and shell I/O; host
//! capabilities are injected through the same trait.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use vesper_domain::{
    ContentPart, ContentText, ConversationMessage, FileChangePreview, SessionOperatingMode,
    SessionPermissionMode, ToolCall, ToolDefinition, WorkspaceRoot,
};

/// Maximum media parts one tool result may contribute to provider history.
pub const MAX_TOOL_MEDIA_PARTS: usize = 8;
use vesper_provider::CancellationSignal;

use crate::sandbox_route::SandboxRoute;

/// Boxed executor future (runtime-agnostic, like the provider ports).
pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Bounded workspace context handed to every executor.
///
/// Carries the authority envelope (roots, mode, permission) and a hierarchical
/// cancellation signal. It never carries secrets — credential resolution stays
/// provider-owned.
pub struct ToolContext {
    /// Confined workspace roots; path-bearing tools must not escape these.
    pub workspace_roots: Vec<WorkspaceRoot>,
    /// VRO-13 hard-denial firewall for shell-class tools (PR-1 core,
    /// PR-2 wiring). `None` is the structural off-path: `AGENT_VESPER_FIREWALL=off`
    /// leaves this empty and `RunCommand` never invokes a scan — the
    /// executor path is byte-identical to the pre-VRO-13 path. `Some` means
    /// every shell command is normalized and matched before `run_bounded`.
    pub firewall: Option<Arc<vesper_policy::firewall::CommandFirewall>>,
    /// VRO-13 PR-4: scope-demanded sandbox route for shell-class tools.
    /// `None` (the default) is the structural off-path: `RunCommand` runs
    /// `run_bounded` exactly as before, byte-identical to PR-3 behavior.
    /// `Some` carries the isolation demand (resolved from
    /// `.agent-vesper/config.toml` `[sandbox]` or a tool-level demand) plus
    /// the backend factory; the executor consults `vesper-security`'s
    /// fail-closed capability check before provisioning.
    pub sandbox: Option<Arc<SandboxRoute>>,
    /// Active session operating mode (gates tool eligibility upstream).
    pub operating_mode: SessionOperatingMode,
    /// Active permission mode (gates destructive tools upstream).
    pub permission_mode: SessionPermissionMode,
    /// Conversation visible to the current turn. Stateful hosts populate this
    /// so context-aware tools can search the active session without owning
    /// persistence.
    pub conversation: Vec<ConversationMessage>,
    /// Hierarchical cancellation view; executors must observe it on long ops.
    pub cancellation: Arc<dyn CancellationSignal>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Cancellation signals are not Debug; describe them opaquely.
        formatter
            .debug_struct("ToolContext")
            .field("workspace_roots", &self.workspace_roots)
            .field("operating_mode", &self.operating_mode)
            .field("permission_mode", &self.permission_mode)
            .field("conversation_messages", &self.conversation.len())
            .field("cancellation", &"<cancellation-signal>")
            .finish()
    }
}

/// One executor's bounded result fed back to the model.
///
/// As of deferred-loading Phase 2, a result may carry
/// [`ToolDefinition`]s that the agent loop splices into its advertised
/// tool pool for the next iteration (Claude Code-style deferred loading).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    /// Bounded textual result returned to the provider loop.
    pub text: ContentText,
    /// Tool schemas dynamically injected into the agent loop's advertised
    /// tool pool after this result is fed back. Used for deferred loading:
    /// a discovery-style tool returns the schemas it surfaced here so the
    /// next loop iteration advertises them to the model. Defaults to empty
    /// for backward compatibility.
    pub injected_tools: Vec<ToolDefinition>,
    /// Bounded provider-visible media returned by the tool. Only image parts
    /// are currently accepted; the authoritative agent gate evaluates them
    /// before the next provider request.
    pub media: Vec<ContentPart>,
    /// Bounded, truthful workspace change produced by a successful text-file
    /// executor. Non-editing tools leave this empty.
    pub change: Option<FileChangePreview>,
}

impl ToolResult {
    /// Wraps a result string in a bounded [`ContentText`]; `injected_tools`
    /// starts empty.
    pub fn new(text: impl Into<String>) -> Result<Self, ToolError> {
        Ok(Self {
            text: ContentText::new(text).map_err(ToolError::output_boundary)?,
            injected_tools: Vec::new(),
            media: Vec::new(),
            change: None,
        })
    }

    /// Builder: attaches tool definitions to inject into the next turn's
    /// advertised pool after this result is fed back to the loop.
    #[must_use]
    pub fn with_injected_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.injected_tools = tools;
        self
    }

    /// Attaches bounded image parts returned by a hosted tool.
    pub fn with_media(mut self, media: Vec<ContentPart>) -> Result<Self, ToolError> {
        if media.len() > MAX_TOOL_MEDIA_PARTS
            || media
                .iter()
                .any(|part| !matches!(part, ContentPart::Image(_)))
        {
            return Err(ToolError::Failed(
                "tool media must contain at most 8 image parts".into(),
            ));
        }
        self.media = media;
        Ok(self)
    }

    /// Attaches the bounded before/after projection produced by an editing
    /// executor after its filesystem mutation succeeds.
    #[must_use]
    pub fn with_change(mut self, change: FileChangePreview) -> Self {
        self.change = Some(change);
        self
    }
}

/// Why an executor rejected a call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// No executor is registered for the call's tool id.
    #[error("no executor registered for tool {0}")]
    UnknownTool(String),
    /// The executor's arguments did not match its schema.
    #[error("invalid arguments for tool {tool}: {reason}")]
    InvalidArguments {
        /// Stable tool name.
        tool: String,
        /// Safe description of the schema mismatch.
        reason: String,
    },
    /// The bounded output channel rejected an oversized result.
    #[error("tool output exceeded the bounded limit: {0}")]
    OutputBoundary(vesper_domain::BoundedStringError),
    /// A provider-agnostic executor failure (e.g. path escape in Phase 4).
    #[error("tool execution failed: {0}")]
    Failed(String),
    /// VRO-13 hard denial: the command firewall rejected the command before
    /// the shell was consulted. This is NOT a permission decision — it
    /// outranks every permission mode, including bypass. The Display text is
    /// a stable contract: the agent loop maps it to the model-visible
    /// observation `tool error: [VRO-13 Firewall] denied: ...`, which the
    /// VRO-12 loop detector classifies as failure, never success.
    #[error("[VRO-13 Firewall] denied: {0}")]
    FirewallDenial(String),
}

impl ToolError {
    pub(crate) fn output_boundary(error: vesper_domain::BoundedStringError) -> Self {
        Self::OutputBoundary(error)
    }
}

/// Object-safe executor contract. One implementation per parity-critical tool.
///
/// `definition()` advertises the tool to the model (and its
/// [`vesper_domain::ToolExecutionClass`] authority for the permission gate);
/// `execute()` runs the tool against a bounded [`ToolContext`].
pub trait ToolExecutor: Send + Sync {
    /// Static definition advertised to the model.
    fn definition(&self) -> ToolDefinition;

    /// Executes one normalized tool call, returning bounded result text.
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>>;
}

/// Composition-boundary service for tools owned by another subsystem.
///
/// The agent crate owns the routing contract but not memory, MCP, plugin,
/// worker, or automation state. Hosts inject those capabilities through this
/// trait so the loop can advertise and execute them without taking a
/// dependency on frontend or persistence crates.
pub trait ToolService: Send + Sync {
    /// Definitions contributed by this service.
    fn definitions(&self) -> Vec<ToolDefinition>;

    /// Executes one service-owned tool call.
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>>;
}

/// Adapter that turns one service-owned definition into a normal registry
/// executor. The registry remains unaware of the service implementation.
pub struct HostedTool {
    definition: ToolDefinition,
    service: Arc<dyn ToolService>,
}

impl HostedTool {
    /// Creates a hosted executor for one definition.
    #[must_use]
    pub fn new(definition: ToolDefinition, service: Arc<dyn ToolService>) -> Self {
        Self {
            definition,
            service,
        }
    }
}

impl ToolExecutor for HostedTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        self.service.execute(call, context)
    }
}

/// Convenience: the stable harness name as a plain string.
#[must_use]
pub fn harness_name(definition: &ToolDefinition) -> String {
    definition.harness_name.as_str().to_string()
}

/// Convenience: build a `ToolContext` with no cancellation (tests/stubs).
#[must_use]
pub fn uncancellable_context(
    roots: Vec<WorkspaceRoot>,
    operating_mode: SessionOperatingMode,
    permission_mode: SessionPermissionMode,
) -> ToolContext {
    struct NeverCancelled;
    impl CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    ToolContext {
        workspace_roots: roots,
        operating_mode,
        permission_mode,
        conversation: Vec::new(),
        cancellation: Arc::new(NeverCancelled),
        firewall: None,
        sandbox: None,
    }
}

/// Builds a stable, schema-only tool definition for the parity registry.
#[must_use]
pub fn schema_definition(
    name: &str,
    description: &str,
    class: vesper_domain::ToolExecutionClass,
    properties: &[(&str, &str, bool)],
) -> ToolDefinition {
    use serde_json::json;
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();
    for (key, ty, is_required) in properties {
        props.insert(
            (*key).to_string(),
            json!({"type": ty, "description": format!("{} parameter", key)}),
        );
        if *is_required {
            required.push((*key).to_string());
        }
    }
    ToolDefinition {
        id: vesper_domain::ToolId::new(name).expect("bounded tool id"),
        harness_name: vesper_domain::HarnessToolName::new(name).expect("bounded harness name"),
        provider_name: None,
        description: description.to_string(),
        input_schema: json!({
            "type": "object",
            "properties": props,
            "required": required,
        }),
        execution_class: class,
        extensions: vesper_domain::ExtensionMap::default(),
        defer_loading: false,
    }
}

#[cfg(test)]
mod tests {
    //! ToolResult boundary and helper wiring.

    use super::*;

    #[test]
    fn tool_result_wraps_bounded_text() {
        let result = ToolResult::new("ok").unwrap();
        assert_eq!(result.text.as_str(), "ok");
        assert!(ToolResult::new("").is_ok(), "empty is allowed (bounded)");
    }

    #[test]
    fn uncancellable_context_never_reports_cancellation() {
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        );
        assert!(!context.cancellation.is_cancelled());
    }
}
