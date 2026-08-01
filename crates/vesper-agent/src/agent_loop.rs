//! Multi-turn agent execution loop (ADR 0010, Tier C Phase 2).
//!
//! [`AgentLoop`] composes `vesper-runtime`'s provider dispatch into a ReAct
//! loop that mirrors the Python oracle's `run_loop` (`agent.py:2837`):
//!
//! 1. Dispatch one provider turn (with mode-filtered tools).
//! 2. Collect the assistant content and any completed `ToolCall`s.
//! 3. Gate each call through [`check_tool_permission`](crate::check_tool_permission).
//! 4. Route to the [`ToolRegistry`] and append a `role: Tool` result message.
//! 5. Loop back to (1) until the model stops calling tools, or the hard
//!    `max_tool_iterations` safety cap is reached.
//!
//! The runtime stays single-turn: each iteration is one
//! `ProviderSession::start`. Multi-turn state lives in the `messages` list this
//! loop owns and threads through every `ProviderRequest`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::StreamExt;
use vesper_domain::{
    ContentPart, ContentText, ConversationMessage, ExtensionMap, FinishOutcome, MessageId,
    MessageRole, ProviderId, ProviderRequestId, QualifiedModelId, SessionOperatingMode,
    SessionPermissionMode, SystemInstruction, ToolCall, ToolDefinition, ToolResultId,
    WorkspaceRoot,
};
use vesper_provider::{
    CancellationSignal, ProviderError, ProviderRequest, ProviderStreamEvent, StructuredOutputIntent,
};
use vesper_runtime::{ProviderRegistry, RuntimeCancellation, RuntimeError};

use crate::executor::{ToolContext, ToolResult};
use crate::permission::check_tool_permission;
use crate::registry::ToolRegistry;

/// Hard upper bound on tool iterations when the caller omits one.
pub const DEFAULT_MAX_TOOL_ITERATIONS: u32 = 50;

/// Provider/turn configuration injected by the composition boundary.
#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    /// Active provider identity.
    pub provider_id: ProviderId,
    /// Provider-owned configuration validated by the adapter.
    pub provider_configuration: vesper_provider::ProviderConfiguration,
    /// Provider-qualified model.
    pub model: QualifiedModelId,
    /// Ordered system instructions prepended to every turn.
    pub system_instructions: Vec<SystemInstruction>,
    /// Confined workspace roots; the first (primary) roots the tool executors.
    pub workspace_roots: Vec<WorkspaceRoot>,
    /// Hard safety cap on tool iterations (prevents infinite loops).
    pub max_tool_iterations: u32,
}

/// Terminal outcome of one `run_prompt` invocation.
#[derive(Debug, Clone)]
pub enum AgentTurnOutcome {
    /// The model finished without outstanding tool calls.
    Completed {
        /// Final assistant content parts (text + any tool invocations).
        assistant_content: Vec<ContentPart>,
        /// Provider turns executed (1 = no tools were called).
        iterations: u32,
        /// Every tool result accumulated across the loop.
        tool_results: Vec<ToolResult>,
        /// The most recent `update_plan` plan body, when the model emitted one
        /// (Phase 5: callers drive PLANNING → REVIEW off this).
        plan: Option<String>,
    },
    /// The safety cap was reached before the model stopped calling tools.
    MaxIterationsReached {
        /// Iterations executed when the cap tripped.
        iterations: u32,
    },
}

/// Why an agent loop failed.
#[derive(Debug, thiserror::Error)]
pub enum AgentLoopError {
    /// The provider registry could not create a session.
    #[error("provider session creation failed: {0:?}")]
    ProviderSetup(RuntimeError),
    /// A provider turn returned a classified error.
    #[error("provider turn failed: {0:?}")]
    ProviderTurn(ProviderError),
    /// The stream ended without a terminal event.
    #[error("provider stream ended without a terminal outcome")]
    StreamWithoutTerminal,
}

/// The Tier C multi-turn agent loop.
pub struct AgentLoop {
    registry: Arc<ProviderRegistry>,
    tools: ToolRegistry,
    config: AgentLoopConfig,
}

impl AgentLoop {
    /// Creates a loop over a shared provider registry and tool registry.
    #[must_use]
    pub fn new(
        registry: Arc<ProviderRegistry>,
        tools: ToolRegistry,
        config: AgentLoopConfig,
    ) -> Self {
        Self {
            registry,
            tools,
            config,
        }
    }

    /// Runs one user prompt to completion (or the iteration cap).
    ///
    /// `mode` selects the advertised tool surface; `permission` gates each
    /// tool call. The loop is bounded by `config.max_tool_iterations`.
    pub async fn run_prompt(
        &self,
        user_message: ConversationMessage,
        mode: SessionOperatingMode,
        permission: SessionPermissionMode,
    ) -> Result<AgentTurnOutcome, AgentLoopError> {
        let advertised_tools = self.tools.definitions_for(mode);
        let cancellation: Arc<dyn CancellationSignal> = Arc::new(RuntimeCancellation::new());
        let session = self
            .registry
            .create_session(
                &self.config.provider_id,
                &self.config.provider_configuration,
                Arc::clone(&cancellation),
            )
            .await
            .map_err(AgentLoopError::ProviderSetup)?;

        let ids = IdGenerator::default();
        let mut messages: Vec<ConversationMessage> = vec![user_message];
        let mut tool_results: Vec<ToolResult> = Vec::new();
        let mut plan: Option<String> = None;
        let mut iteration: u32 = 0;

        loop {
            if iteration >= self.config.max_tool_iterations {
                return Ok(AgentTurnOutcome::MaxIterationsReached {
                    iterations: iteration,
                });
            }
            let request = self.build_request(&ids, &messages, &advertised_tools, iteration);
            let mut stream = session
                .start(request, Arc::clone(&cancellation))
                .await
                .map_err(AgentLoopError::ProviderTurn)?;

            let (assistant_parts, tool_calls, finish) = consume_stream(&mut stream).await?;
            // Append the assistant turn (text + any tool invocations).
            messages.push(ConversationMessage {
                id: ids.message(),
                role: MessageRole::Assistant,
                content: assistant_parts.clone(),
                extensions: ExtensionMap::default(),
            });

            if tool_calls.is_empty() {
                return Ok(AgentTurnOutcome::Completed {
                    assistant_content: assistant_parts,
                    iterations: iteration + 1,
                    tool_results,
                    plan,
                });
            }
            // Even if the provider finished with ToolCalls, we only loop when
            // calls are actually present; an empty batch terminates the turn.
            let _ = finish;

            let context = ToolContext {
                workspace_roots: self.config.workspace_roots.clone(),
                operating_mode: mode,
                permission_mode: permission,
                cancellation: Arc::clone(&cancellation),
            };
            for call in tool_calls {
                let output = self.gate_and_execute(&call, &context).await;
                // Phase 5: capture the model-generated plan when the model
                // emits `update_plan`, so callers (the TUI) can drive the
                // PLANNING → REVIEW transition without a human-authored body.
                if call.tool_id.as_str() == "update_plan" {
                    plan = Some(output.clone());
                }
                let bounded = ContentText::new(output).unwrap_or_else(|_| {
                    ContentText::new("[tool output too large]").expect("bounded")
                });
                tool_results.push(ToolResult {
                    text: bounded.clone(),
                });
                messages.push(ConversationMessage {
                    id: ids.message(),
                    role: MessageRole::Tool,
                    content: vec![ContentPart::Text(bounded)],
                    extensions: tool_result_extensions(&call),
                });
            }
            iteration += 1;
        }
    }

    /// Builds a provider-neutral single-turn request for one loop iteration.
    fn build_request(
        &self,
        ids: &IdGenerator,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        iteration: u32,
    ) -> ProviderRequest {
        use vesper_provider::{FallbackPolicy, ToolChoice};
        ProviderRequest {
            request_id: ProviderRequestId::new(format!("agent-turn-{iteration}-{}", ids.next()))
                .expect("bounded request id"),
            provider_id: self.config.provider_id.clone(),
            model: self.config.model.clone(),
            endpoint_id: None,
            system_instructions: self.config.system_instructions.clone(),
            messages: messages.to_vec(),
            tools: tools.to_vec(),
            tool_choice: ToolChoice::Auto,
            capabilities: Vec::new(),
            reasoning: None,
            structured_output: StructuredOutputIntent::None,
            sampling: None,
            maximum_output_tokens: None,
            continuation: None,
            fallback_policy: FallbackPolicy::Strict,
            provider_extensions: None,
        }
    }

    /// Applies the permission gate, then routes to the registry. Permission
    /// denials and unknown/failed tools are returned as bounded text so the
    /// model can recover on the next turn (mirroring the oracle's behavior).
    async fn gate_and_execute(&self, call: &ToolCall, context: &ToolContext) -> String {
        let Some(definition) = self.tools.definition(call.tool_id.as_str()) else {
            return format!("unknown tool: {}", call.tool_id);
        };
        let decision = check_tool_permission(
            context.operating_mode,
            context.permission_mode,
            definition.execution_class,
        );
        match decision {
            crate::permission::PermissionDecision::Allow => {
                match self.tools.execute(call, context).await {
                    Ok(result) => result.text.as_str().to_string(),
                    Err(error) => format!("tool error: {error}"),
                }
            }
            crate::permission::PermissionDecision::Deny(reason) => {
                format!("permission denied: {reason}")
            }
        }
    }
}

/// Consumes one provider stream, returning assistant content, tool calls, and
/// the terminal finish outcome.
async fn consume_stream(
    stream: &mut vesper_provider::ProviderEventStream,
) -> Result<(Vec<ContentPart>, Vec<ToolCall>, FinishOutcome), AgentLoopError> {
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    let mut finish = None;
    while let Some(event) = stream.next().await {
        match event {
            Ok(ProviderStreamEvent::ContentDelta { part, .. }) => parts.push(part),
            Ok(ProviderStreamEvent::ToolCallCompleted(call)) => calls.push(call),
            Ok(ProviderStreamEvent::Completed {
                finish: terminal, ..
            }) => {
                finish = Some(terminal);
                break;
            }
            Ok(_) => {}
            Err(error) => return Err(AgentLoopError::ProviderTurn(error)),
        }
    }
    let finish = finish.ok_or(AgentLoopError::StreamWithoutTerminal)?;
    Ok((parts, calls, finish))
}

/// Records the originating `tool_call_id` on a tool-result message so adapters
/// can link call → result when serializing to a provider dialect.
fn tool_result_extensions(call: &ToolCall) -> ExtensionMap {
    let mut map = ExtensionMap::default();
    let _ = map.insert(
        "tool-call-id",
        serde_json::Value::String(call.id.as_str().to_string()),
    );
    map
}

/// Monotonic identity generator for messages, requests, and result linkage.
#[derive(Debug, Default)]
struct IdGenerator {
    counter: AtomicU64,
    message: AtomicU64,
}

impl IdGenerator {
    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }
    fn message(&self) -> MessageId {
        let value = self.message.fetch_add(1, Ordering::Relaxed);
        MessageId::new(format!("agent-message-{value}")).expect("bounded message id")
    }
    #[expect(dead_code)]
    fn result(&self) -> ToolResultId {
        ToolResultId::new(format!("tool-result-{}", self.next())).expect("bounded result id")
    }
}
