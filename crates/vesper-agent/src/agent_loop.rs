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
    CapabilityId, CapabilityRequest, ContentPart, ContentText, ConversationMessage, ExtensionMap,
    FeatureRequirement, FinishOutcome, MessageId, MessageRole, ProviderId, ProviderRequestId,
    QualifiedModelId, SessionOperatingMode, SessionPermissionMode, SystemInstruction, ToolCall,
    ToolDefinition, ToolResultId, WorkspaceRoot,
};
use vesper_provider::{
    CancellationSignal, ProviderError, ProviderRequest, ProviderStreamEvent, StructuredOutputIntent,
};
use vesper_runtime::{ProviderRegistry, RuntimeCancellation, RuntimeError};

use crate::executor::{ToolContext, ToolResult};
use crate::permission::{
    DenyPermissionPort, PermissionDecision, PermissionPort, check_tool_permission,
};
use crate::registry::ToolRegistry;
use crate::vro::loop_detector::{LoopDetector, LoopGuardAction};

/// Live, bounded progress emitted while an agent turn is running.
///
/// Frontends may render these events in memory. They are not persisted by the
/// loop and deliberately omit tool arguments, tool output, paths, and secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentProgressEvent {
    /// A new user turn entered the loop.
    TurnStarted,
    /// One provider iteration started.
    ProviderTurnStarted { iteration: u32 },
    /// Provider-visible reasoning text arrived.
    ReasoningDelta { text: ContentText },
    /// User-visible assistant text arrived.
    ContentDelta { text: ContentText },
    /// A named tool is about to pass through permission gating and execution.
    /// `hint` is a secret-safe display digest of the call (VRO-11.8):
    /// derived ONLY from whitelisted argument keys (`path`, `file_path`,
    /// `pattern`, `command`, …) by [`tool_arg_hint`] — never from content,
    /// body, text, or credential-shaped payloads — and truncated to a
    /// terminal-friendly width.
    ToolStarted { name: String, hint: String },
    /// A named tool finished. `success` is false for denied/failed results.
    /// `note` (VRO-11.8) is a bounded result summary: on success it carries
    /// ONLY a size digest ("43 lines" / "120 chars") — never result content,
    /// which may hold file bytes or secrets; on failure it carries the
    /// first line of the harness's own error text.
    ToolFinished {
        name: String,
        success: bool,
        note: String,
    },
    /// The model replaced the current task plan.
    PlanUpdated { markdown: String },
    /// Cumulative provider token usage for the running turn arrived.
    /// Frontends render this as a live token/context indicator.
    UsageUpdated {
        /// Normalized cumulative usage reported by the provider. Boxed to
        /// keep the shared progress-event enum small.
        usage: Box<vesper_domain::NormalizedUsage>,
    },
}

/// Argument keys whose values are safe to surface in the UI telemetry
/// (VRO-11.8). Deliberately a WHITELIST: paths, patterns, and command
/// heads are public-shaped; `content`, `body`, `text`, `json`, and any
/// credential-shaped key are never eligible, whatever the tool.
const TOOL_HINT_ARG_KEYS: &[&str] = &[
    "path",
    "file_path",
    "file",
    "dir",
    "directory",
    "folder",
    "pattern",
    "query",
    "command",
    "cmd",
    "url",
    "title",
    "selector",
    "name",
];

/// Maximum display width of a telemetry hint, in chars.
const TOOL_HINT_MAX_CHARS: usize = 48;

/// Derives the secret-safe display hint for a tool call (VRO-11.8).
///
/// Takes the FIRST whitelisted key present with a string value, collapses
/// whitespace to single spaces, and truncates on a char boundary with an
/// ellipsis. Returns the empty string when no whitelisted argument exists
/// (the TUI then renders the bare tool name).
///
/// Pure; unit-tested below.
#[must_use]
pub fn tool_arg_hint(args: &serde_json::Value) -> String {
    for key in TOOL_HINT_ARG_KEYS {
        if let Some(value) = args.get(*key).and_then(|v| v.as_str()) {
            let collapsed: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
            if collapsed.is_empty() {
                continue;
            }
            if collapsed.chars().count() <= TOOL_HINT_MAX_CHARS {
                return collapsed;
            }
            let truncated: String = collapsed.chars().take(TOOL_HINT_MAX_CHARS - 1).collect();
            return format!("{truncated}…");
        }
    }
    String::new()
}

/// Maximum display width of a failure note, in chars.
const TOOL_NOTE_MAX_CHARS: usize = 72;

/// Derives the bounded result note for a tool completion (VRO-11.8).
///
/// Success: a SIZE digest only — line count when the output has lines,
/// otherwise char count. Result bytes never leave the loop (they may be
/// file contents or secrets). Failure: the first line of the harness's
/// own error text, truncated.
///
/// Pure; unit-tested below.
#[must_use]
pub fn tool_result_note(output: &str, success: bool) -> String {
    if success {
        let trimmed = output.trim_end();
        if trimmed.is_empty() {
            return String::new();
        }
        let lines = trimmed.lines().count();
        if lines > 1 {
            return format!("{lines} lines");
        }
        format!("{} chars", trimmed.chars().count())
    } else {
        let first = output.lines().next().unwrap_or("").trim();
        if first.is_empty() {
            return String::new();
        }
        if first.chars().count() <= TOOL_NOTE_MAX_CHARS {
            return first.to_string();
        }
        let truncated: String = first.chars().take(TOOL_NOTE_MAX_CHARS - 1).collect();
        format!("{truncated}…")
    }
}

/// Host-owned sink for live agent progress.
pub trait AgentProgressPort: Send + Sync {
    /// Receives one bounded progress event.
    fn emit(&self, event: AgentProgressEvent);
}

#[derive(Debug)]
struct NoopProgressPort;

impl AgentProgressPort for NoopProgressPort {
    fn emit(&self, _event: AgentProgressEvent) {}
}

/// Hard upper bound on tool iterations when the caller omits one.
pub const DEFAULT_MAX_TOOL_ITERATIONS: u32 = 50;
/// Maximum retained messages in one provider request. Hosts may keep the
/// complete history separately; the loop compacts the request window before
/// dispatch so a long-lived session cannot grow without bound.
pub const MAX_CONTEXT_MESSAGES: usize = 256;

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
    /// Visible output was preserved, but the provider stream ended before a
    /// terminal response and automatic continuation was unsafe or exhausted.
    Interrupted {
        /// Partial user-visible assistant content committed to session history.
        assistant_content: Vec<ContentPart>,
        /// Classified provider-neutral interruption source.
        cause: vesper_domain::StreamInterruptionCause,
        /// True when replay/continuation was withheld because any tool-call
        /// fragment had already appeared on the wire.
        tool_call_started: bool,
        /// Provider turns executed before interruption.
        iterations: u32,
        /// Every tool result completed before the interrupted provider turn.
        tool_results: Vec<ToolResult>,
        /// Most recently published native plan.
        plan: Option<String>,
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
    /// The provider ended without a normal stop after exhausting or refusing
    /// generation. Treating this as `Completed` would make hosts falsely
    /// report a truncated or interrupted implementation as successful.
    #[error("provider ended before task completion: {0:?}")]
    Incomplete(FinishOutcome),
    /// The model persisted in a deterministic repeated/no-progress tool loop.
    #[error("tool loop stopped: {0}")]
    LoopDetected(String),
}

/// The Tier C multi-turn agent loop.
#[derive(Clone)]
pub struct AgentLoop {
    registry: Arc<ProviderRegistry>,
    tools: ToolRegistry,
    config: AgentLoopConfig,
    permission_port: Arc<dyn PermissionPort>,
    progress_port: Arc<dyn AgentProgressPort>,
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
            permission_port: Arc::new(DenyPermissionPort),
            progress_port: Arc::new(NoopProgressPort),
        }
    }

    /// Installs the host-owned one-time approval channel.
    #[must_use]
    pub fn with_permission_port(mut self, permission_port: Arc<dyn PermissionPort>) -> Self {
        self.permission_port = permission_port;
        self
    }

    /// Installs a host-owned live progress sink.
    #[must_use]
    pub fn with_progress_port(mut self, progress_port: Arc<dyn AgentProgressPort>) -> Self {
        self.progress_port = progress_port;
        self
    }

    /// Disables frontend progress for private advisory provider calls.
    #[must_use]
    pub fn without_progress(mut self) -> Self {
        self.progress_port = Arc::new(NoopProgressPort);
        self
    }

    /// Replaces the provider/model configuration for a subsequent turn while
    /// preserving the tool registry and host ports.
    #[must_use]
    pub fn with_turn_configuration(mut self, config: AgentLoopConfig) -> Self {
        self.config = config;
        self
    }

    /// Replaces the tool surface, used for bounded provider-only advisers.
    #[must_use]
    pub fn with_tool_registry(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    /// Returns the current composition configuration for host-side cloning.
    #[must_use]
    pub fn configuration(&self) -> &AgentLoopConfig {
        &self.config
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
        let (outcome, _) = self
            .run_prompt_with_history(vec![user_message], mode, permission)
            .await?;
        Ok(outcome)
    }

    /// Runs one agent turn against caller-owned conversation history.
    ///
    /// The returned history contains the supplied messages plus every
    /// assistant/tool message produced by this invocation. Keeping ownership
    /// at the composition boundary lets a TUI, ACP session, or another host
    /// persist multi-turn context without making the provider loop global.
    pub async fn run_prompt_with_history(
        &self,
        messages: Vec<ConversationMessage>,
        mode: SessionOperatingMode,
        permission: SessionPermissionMode,
    ) -> Result<(AgentTurnOutcome, Vec<ConversationMessage>), AgentLoopError> {
        let cancellation: Arc<dyn CancellationSignal> = Arc::new(RuntimeCancellation::new());
        self.run_prompt_with_history_with_cancellation(messages, mode, permission, cancellation)
            .await
    }

    /// Runs one turn with a host-owned cancellation signal.
    ///
    /// ACP and other interactive hosts use this port to preserve cancellation
    /// responsiveness while the loop is inside a provider stream or tool.
    pub async fn run_prompt_with_history_with_cancellation(
        &self,
        mut messages: Vec<ConversationMessage>,
        mode: SessionOperatingMode,
        permission: SessionPermissionMode,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> Result<(AgentTurnOutcome, Vec<ConversationMessage>), AgentLoopError> {
        self.progress_port.emit(AgentProgressEvent::TurnStarted);
        let mut advertised_tools = self.tools.definitions_for(mode);
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
        let mut tool_results: Vec<ToolResult> = Vec::new();
        let mut plan: Option<String> = None;
        let mut iteration: u32 = 0;
        let mut loop_detector = LoopDetector::new();

        loop {
            if iteration >= self.config.max_tool_iterations {
                return Ok((
                    AgentTurnOutcome::MaxIterationsReached {
                        iterations: iteration,
                    },
                    messages,
                ));
            }
            self.progress_port
                .emit(AgentProgressEvent::ProviderTurnStarted { iteration });
            let request_messages = compact_history(&messages);
            let request = self.build_request(&ids, &request_messages, &advertised_tools, iteration);
            let mut stream = session
                .start(request, Arc::clone(&cancellation))
                .await
                .map_err(AgentLoopError::ProviderTurn)?;

            let (assistant_parts, tool_calls, finish) =
                consume_stream(&mut stream, self.progress_port.as_ref()).await?;
            // Append the assistant turn (text + any tool invocations).
            messages.push(ConversationMessage {
                id: ids.message(),
                role: MessageRole::Assistant,
                content: assistant_parts.clone(),
                extensions: ExtensionMap::default(),
            });

            if tool_calls.is_empty() {
                if let FinishOutcome::StreamInterrupted {
                    cause,
                    tool_call_started,
                } = finish
                {
                    messages.retain(|message| !is_plan_continuation_message(message));
                    return Ok((
                        AgentTurnOutcome::Interrupted {
                            assistant_content: assistant_parts,
                            cause,
                            tool_call_started,
                            iterations: iteration + 1,
                            tool_results,
                            plan,
                        },
                        messages,
                    ));
                }
                if !matches!(finish, FinishOutcome::Stop) {
                    return Err(AgentLoopError::Incomplete(finish));
                }
                if plan_has_open_items(plan.as_deref()) {
                    messages.push(plan_continuation_message(&ids));
                    iteration += 1;
                    continue;
                }
                messages.retain(|message| !is_plan_continuation_message(message));
                return Ok((
                    AgentTurnOutcome::Completed {
                        assistant_content: assistant_parts,
                        iterations: iteration + 1,
                        tool_results,
                        plan,
                    },
                    messages,
                ));
            }
            // Even if the provider finished with ToolCalls, we only loop when
            // calls are actually present; an empty batch terminates the turn.
            let _ = finish;

            let context = ToolContext {
                workspace_roots: self.config.workspace_roots.clone(),
                operating_mode: mode,
                permission_mode: permission,
                conversation: request_messages,
                cancellation: Arc::clone(&cancellation),
            };
            for call in tool_calls {
                let tool_name = call.tool_id.as_str().to_string();
                self.progress_port.emit(AgentProgressEvent::ToolStarted {
                    name: tool_name.clone(),
                    hint: tool_arg_hint(&call.arguments),
                });
                let outcome = self
                    .gate_and_execute(&call, &context, &advertised_tools)
                    .await;
                let mut output = outcome.text;
                let injected = outcome.injected;
                let execution_succeeded = !output.starts_with("tool error:")
                    && !output.starts_with("permission denied:")
                    && !output.starts_with("unknown tool:");
                if execution_succeeded {
                    match loop_detector.record(&tool_name, &call.arguments, &output) {
                        LoopGuardAction::Clear => {}
                        LoopGuardAction::Warn(warning) => {
                            output.push_str("\n\n");
                            output.push_str(&warning.message);
                        }
                        LoopGuardAction::Block(message) => output = message,
                        LoopGuardAction::Break(reason) => {
                            return Err(AgentLoopError::LoopDetected(reason));
                        }
                    }
                }
                // Phase 5: capture the model-generated plan when the model
                // emits `update_plan`, so callers (the TUI) can drive the
                // PLANNING → REVIEW transition without a human-authored body.
                if call.tool_id.as_str() == "update_plan" {
                    plan = Some(output.clone());
                    self.progress_port.emit(AgentProgressEvent::PlanUpdated {
                        markdown: output.clone(),
                    });
                }
                let success =
                    execution_succeeded && !output.starts_with("[SYSTEM OVERRIDE: LOOP BLOCKED");
                let note = tool_result_note(&output, success);
                self.progress_port.emit(AgentProgressEvent::ToolFinished {
                    name: tool_name,
                    success,
                    note,
                });
                let bounded = ContentText::new(output).unwrap_or_else(|_| {
                    ContentText::new("[tool output too large]").expect("bounded")
                });
                tool_results.push(ToolResult {
                    text: bounded.clone(),
                    injected_tools: injected.clone(),
                });
                // Phase 2 deferred loading: if the executor returned injected
                // schemas, splice them into the advertised pool so the next
                // `build_request` iteration advertises them to the model.
                if !injected.is_empty() {
                    merge_injected_tools(&mut advertised_tools, injected);
                }
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
            capabilities: vec![
                CapabilityRequest {
                    capability: CapabilityId::new("provider:tools").expect("static capability"),
                    requirement: FeatureRequirement::Require,
                    fallback: None,
                },
                CapabilityRequest {
                    capability: CapabilityId::new("provider:tool-choice")
                        .expect("static capability"),
                    requirement: FeatureRequirement::Require,
                    fallback: None,
                },
            ],
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
    ///
    /// Returns a [`GateOutcome`] so the loop can both feed the text back to
    /// the model and, when the executor opted in, splice the
    /// `injected_tools` it returned into the next iteration's advertised
    /// pool (deferred-loading Phase 2).
    ///
    /// The definition lookup consults the loop's live `advertised_tools`
    /// pool (deferred-loading Phase 3) rather than the registry's static
    /// entries. This lets dynamically injected schemas — discovered by a
    /// tool call earlier in the same turn — pass the permission gate and
    /// route to the registry's gateway executor without being
    /// pre-registered as full entries.
    async fn gate_and_execute(
        &self,
        call: &ToolCall,
        context: &ToolContext,
        advertised_tools: &[ToolDefinition],
    ) -> GateOutcome {
        // Look up the definition in the live advertised pool first (covers
        // dynamically injected schemas). Fall back to the registry's static
        // entries so a model that hallucinates a call to a registered-but-
        // mode-filtered tool (e.g. `write_file` in Plan mode) is denied by
        // the permission gate rather than reported as "unknown tool".
        let definition = advertised_tools
            .iter()
            .find(|definition| definition.harness_name.as_str() == call.tool_id.as_str())
            .or_else(|| self.tools.definition(call.tool_id.as_str()));
        let Some(definition) = definition else {
            return GateOutcome::text(format!("unknown tool: {}", call.tool_id));
        };
        let decision = check_tool_permission(
            context.operating_mode,
            context.permission_mode,
            definition.execution_class,
        );
        match decision {
            PermissionDecision::Allow => match self.tools.execute(call, context).await {
                Ok(result) => GateOutcome {
                    text: result.text.as_str().to_string(),
                    injected: result.injected_tools,
                },
                Err(error) => GateOutcome::text(format!("tool error: {error}")),
            },
            PermissionDecision::Ask(reason) => {
                match self
                    .permission_port
                    .authorize(call, definition, context)
                    .await
                {
                    PermissionDecision::Allow => match self.tools.execute(call, context).await {
                        Ok(result) => GateOutcome {
                            text: result.text.as_str().to_string(),
                            injected: result.injected_tools,
                        },
                        Err(error) => GateOutcome::text(format!("tool error: {error}")),
                    },
                    PermissionDecision::Ask(nested_reason)
                    | PermissionDecision::Deny(nested_reason) => {
                        GateOutcome::text(format!("permission denied: {reason}; {nested_reason}"))
                    }
                }
            }
            PermissionDecision::Deny(reason) => {
                GateOutcome::text(format!("permission denied: {reason}"))
            }
        }
    }
}

const PLAN_CONTINUATION_EXTENSION: &str = "vesper:internal-plan-continuation";

fn plan_has_open_items(plan: Option<&str>) -> bool {
    plan.is_some_and(|markdown| {
        markdown.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("[ ]") || line.starts_with("[~]")
        })
    })
}

fn plan_continuation_message(ids: &IdGenerator) -> ConversationMessage {
    let mut extensions = ExtensionMap::default();
    extensions
        .insert(PLAN_CONTINUATION_EXTENSION, serde_json::Value::Bool(true))
        .expect("static extension key and bounded value");
    ConversationMessage {
        id: ids.message(),
        role: MessageRole::User,
        content: vec![ContentPart::Text(
            ContentText::new(
                "[SYSTEM CONTINUATION] The active plan still has pending or in-progress items. Continue autonomously, complete the remaining work, update the plan statuses, run required verification, and only then provide the final report.",
            )
            .expect("static bounded continuation"),
        )],
        extensions,
    }
}

fn is_plan_continuation_message(message: &ConversationMessage) -> bool {
    matches!(
        message.extensions.get(PLAN_CONTINUATION_EXTENSION),
        Some(serde_json::Value::Bool(true))
    )
}

/// One tool call's outcome after passing the permission gate.
///
/// `text` is fed back to the model as a `role: Tool` message; `injected`
/// carries schemas the executor asked the loop to advertise on the next
/// iteration. Permission denials, unknown tools, and executor errors all
/// produce an empty `injected` list (the executor never ran successfully).
struct GateOutcome {
    /// Tool-result text fed back to the model.
    text: String,
    /// Tool schemas to inject into the advertised pool on the next iteration.
    injected: Vec<ToolDefinition>,
}

impl GateOutcome {
    /// Builds a text-only outcome (denials, errors, unknown tools).
    fn text(text: String) -> Self {
        Self {
            text,
            injected: Vec::new(),
        }
    }
}

/// Merges newly-injected tool schemas into the advertised pool, deduplicating
/// by `ToolId` or `harness_name`. A tool already present under either key is
/// skipped so a discovery call that returns the same schema twice (or returns
/// a schema the loop already advertises) cannot bloat the context window.
fn merge_injected_tools(advertised: &mut Vec<ToolDefinition>, new_tools: Vec<ToolDefinition>) {
    for definition in new_tools {
        let already_present = advertised.iter().any(|existing| {
            existing.id == definition.id || existing.harness_name == definition.harness_name
        });
        if !already_present {
            advertised.push(definition);
        }
    }
}

/// Keeps the initial user turn and the newest bounded window. Tool loops add
/// assistant/tool pairs, so retaining a fixed tail is deterministic and never
/// leaves an unbounded request in flight. The host-owned history returned from
/// `run_prompt_with_history` is intentionally bounded to the same request
/// window; callers can persist a full transcript independently when needed.
fn compact_history(messages: &[ConversationMessage]) -> Vec<ConversationMessage> {
    if messages.len() <= MAX_CONTEXT_MESSAGES {
        return messages.to_vec();
    }
    let keep_tail = MAX_CONTEXT_MESSAGES.saturating_sub(1);
    let first = messages.first().cloned();
    let tail_start = messages.len().saturating_sub(keep_tail);
    let mut compacted = Vec::with_capacity(MAX_CONTEXT_MESSAGES);
    if let Some(first) = first {
        compacted.push(first);
    }
    compacted.extend(messages[tail_start..].iter().cloned());
    compacted
}

/// Consumes one provider stream, returning assistant content, tool calls, and
/// the terminal finish outcome.
///
/// Streamed text deltas are coalesced into a single contiguous
/// [`ContentPart::Text`] so one assistant turn renders as one message block
/// (wrapped by the renderer) instead of one token chunk per line. A non-text
/// content part or a completed tool call flushes the buffer to preserve
/// ordering.
async fn consume_stream(
    stream: &mut vesper_provider::ProviderEventStream,
    progress: &dyn AgentProgressPort,
) -> Result<(Vec<ContentPart>, Vec<ToolCall>, FinishOutcome), AgentLoopError> {
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    let mut finish = None;
    let mut text_buffer = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(ProviderStreamEvent::ReasoningDelta { text, kind, .. }) => {
                if matches!(
                    kind,
                    vesper_domain::ReasoningKind::ProviderVisible
                        | vesper_domain::ReasoningKind::Summary
                ) {
                    progress.emit(AgentProgressEvent::ReasoningDelta { text });
                }
            }
            Ok(ProviderStreamEvent::ContentDelta { part, .. }) => match part {
                ContentPart::Text(text) => {
                    progress.emit(AgentProgressEvent::ContentDelta { text: text.clone() });
                    text_buffer.push_str(text.as_str());
                }
                other => {
                    flush_text_buffer(&mut text_buffer, &mut parts);
                    parts.push(other);
                }
            },
            Ok(ProviderStreamEvent::ToolCallCompleted(call)) => {
                flush_text_buffer(&mut text_buffer, &mut parts);
                calls.push(call);
            }
            Ok(ProviderStreamEvent::Usage(usage)) => {
                progress.emit(AgentProgressEvent::UsageUpdated {
                    usage: Box::new(usage),
                });
            }
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
    flush_text_buffer(&mut text_buffer, &mut parts);
    let finish = finish.ok_or(AgentLoopError::StreamWithoutTerminal)?;
    Ok((parts, calls, finish))
}

/// Flushes accumulated streamed text into one (or, past the 1 MiB
/// [`ContentText`] bound, a few) content parts. Splits on UTF-8 char
/// boundaries so a very large turn is never silently dropped.
fn flush_text_buffer(buffer: &mut String, parts: &mut Vec<ContentPart>) {
    if buffer.is_empty() {
        return;
    }
    const CONTENT_TEXT_MAX: usize = 1_048_576;
    let accumulated = std::mem::take(buffer);
    let mut cursor = 0;
    while cursor < accumulated.len() {
        let mut end = cursor
            .saturating_add(CONTENT_TEXT_MAX)
            .min(accumulated.len());
        // Walk back to a UTF-8 char boundary. (`str::floor_char_boundary`
        // needs Rust 1.91+, above the workspace MSRV 1.88.)
        while end > cursor && !accumulated.is_char_boundary(end) {
            end -= 1;
        }
        if end <= cursor {
            break;
        }
        if let Ok(text) = ContentText::new(&accumulated[cursor..end]) {
            parts.push(ContentPart::Text(text));
        }
        cursor = end;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_compaction_keeps_first_and_newest_messages() {
        let mut messages = Vec::new();
        for index in 0..(MAX_CONTEXT_MESSAGES + 20) {
            messages.push(ConversationMessage {
                id: MessageId::new(format!("message-{index}")).unwrap(),
                role: MessageRole::User,
                content: vec![ContentPart::Text(
                    ContentText::new(index.to_string()).unwrap(),
                )],
                extensions: ExtensionMap::default(),
            });
        }
        let window = compact_history(&messages);
        assert_eq!(window.len(), MAX_CONTEXT_MESSAGES);
        assert_eq!(
            window[0].content[0],
            ContentPart::Text(ContentText::new("0").unwrap())
        );
        assert_eq!(
            window.last().unwrap().content[0],
            ContentPart::Text(ContentText::new((MAX_CONTEXT_MESSAGES + 19).to_string()).unwrap())
        );
        assert_eq!(messages.len(), MAX_CONTEXT_MESSAGES + 20);
    }
    // ------------------------------------------------------------------
    // VRO-11.8 — telemetry hint/note derivation
    // ------------------------------------------------------------------

    #[test]
    fn tool_arg_hint_prefers_whitelisted_keys() {
        let args = serde_json::json!({"content": "SECRET BODY", "path": "dashboard.html"});
        assert_eq!(tool_arg_hint(&args), "dashboard.html");
        // First whitelisted key in priority order wins even if several exist.
        let both = serde_json::json!({"file_path": "a.rs", "pattern": "fn main"});
        assert_eq!(tool_arg_hint(&both), "a.rs");
    }

    #[test]
    fn tool_arg_hint_never_surfaces_payload_keys() {
        // Content/body/text/json and credential-shaped keys are excluded
        // even when they are the ONLY arguments.
        for key in [
            "content", "body", "text", "json", "api_key", "token", "password",
        ] {
            let args = serde_json::json!({key: "should never appear"});
            assert_eq!(tool_arg_hint(&args), "", "{key} must not be hinted");
        }
    }

    #[test]
    fn tool_arg_hint_collapses_and_truncates() {
        let messy = serde_json::json!({"command": "cargo   test\n--workspace"});
        assert_eq!(tool_arg_hint(&messy), "cargo test --workspace");
        let long = serde_json::json!({"path": "x".repeat(120)});
        let hint = tool_arg_hint(&long);
        assert!(hint.chars().count() <= 48, "hint bounded: {hint}");
        assert!(hint.ends_with('…'), "truncation ellipsis: {hint}");
    }

    #[test]
    fn tool_result_note_success_is_size_only() {
        let content = "line1\nline2\nline3";
        assert_eq!(tool_result_note(content, true), "3 lines");
        assert_eq!(tool_result_note("one-liner", true), "9 chars");
        assert_eq!(tool_result_note("   ", true), "");
    }

    #[test]
    fn tool_result_note_failure_carries_first_line_bounded() {
        assert_eq!(
            tool_result_note("tool error: no such file: missing.rs", false),
            "tool error: no such file: missing.rs"
        );
        let long_error = format!("tool error: {}", "e".repeat(200));
        let note = tool_result_note(&long_error, false);
        assert!(note.chars().count() <= 72, "failure note bounded: {note}");
        assert!(note.ends_with('…'));
    }
}
