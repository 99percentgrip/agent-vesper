//! Tool-Grounded ReAct loop (VRO-5.1, PRD §11.6).
//!
//! Implements the [`ToolGroundedReact`](vesper_domain::ReasoningStrategy)
//! strategy by alternating between a structured **action** ("which tool to
//! call") and the resulting **observation** (the tool's bounded output). Each
//! iteration appends one (Action, Observation) pair to the running
//! [`TrajectoryEntry`] sequence, which the [`ReactAgent`] consults to pick
//! its next move. The loop terminates when:
//!
//! - the agent emits [`ReactDecision::Finish`] (the answer is ready), or
//! - [`ReasoningBudget::max_model_calls`] is exhausted (model-side safety
//!   bound, PRD §10.9: "never enter an unrestricted review loop"), or
//! - [`ReasoningBudget::max_tool_calls`] is exhausted (tool-side safety
//!   bound, prevents infinite tool fan-out).
//!
//! ## Read-Before-Write policy (directive 3)
//!
//! When the profile requires grounding, mutating tools (`Mutating`/`Shell`/
//! `Process`/`NestedWorkflow`) are **rejected** until at least one
//! `ReadOnly` tool has produced an observation. The rejection is fed back to
//! the model as a structured failure observation, so the loop continues and
//! the model can correct itself by gathering evidence first.
//!
//! ## Permission sandbox (directive 2)
//!
//! The loop never executes tools itself — it routes every action through a
//! caller-supplied [`ToolInvoker`]. The production impl
//! [`RegistryToolInvoker`] wraps the existing [`ToolRegistry::execute`] plus
//! [`check_tool_permission`] plus the host-supplied [`PermissionPort`], so
//! operating-mode and one-time-approval sandboxing are honored identically to
//! the `agent_loop.rs` direct path. Tool errors (unknown tool, malformed
//! args, permission denial, executor failure) are converted to structured
//! observations rather than crashing the loop.
//!
//! ## Zero-breakage contract
//!
//! This module is invoked only by the orchestrator when the profiled strategy
//! is [`ToolGroundedReact`](vesper_domain::ReasoningStrategy) AND the
//! composition boundary calls [`VroOrchestrator::execute_react`] (which is
//! the only public entry point that supplies the [`ReactAgent`] +
//! [`ToolInvoker`] seams). The [`Direct`], [`GenerateVerifyRepair`], and
//! parallel paths never reach this code.
//!
//! [`Direct`]: vesper_domain::ReasoningStrategy::Direct
//! [`GenerateVerifyRepair`]: vesper_domain::ReasoningStrategy::GenerateVerifyRepair
//! [`ToolRegistry::execute`]: crate::registry::ToolRegistry::execute
//! [`check_tool_permission`]: crate::permission::check_tool_permission
//! [`PermissionPort`]: crate::permission::PermissionPort
//! [`VroOrchestrator::execute_react`]: super::VroOrchestrator::execute_react

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use vesper_domain::{
    ExtensionMap, InferenceCost, OutcomeStatus, ReasoningBudget, ReasoningOutcome,
    StructuredOutput, ToolCall, ToolCallId, ToolExecutionClass, ToolId, VerificationStatus,
    VerificationSummary,
};

// ---------------------------------------------------------------------------
// Trajectory (PRD §11.6: "appending the state to the context on each turn")
// ---------------------------------------------------------------------------

/// One entry in the running ReAct trajectory.
///
/// The loop appends (Action, Observation) pairs in order. The agent inspects
/// the full sequence to decide its next move.
#[derive(Debug, Clone, PartialEq)]
pub enum TrajectoryEntry {
    /// The agent decided to call a tool with these arguments.
    Action {
        /// Stable harness name of the tool (e.g. `read_file`, `grep`).
        name: String,
        /// Parsed JSON arguments produced by the model.
        arguments: serde_json::Value,
    },
    /// The observation recorded for the most recent [`Action`](Self::Action).
    ///
    /// `success == false` indicates a tool error, permission denial, or
    /// Read-Before-Write rejection — the model should treat these as evidence
    /// it must try a different approach.
    Observation {
        /// Bounded textual result OR a structured failure message.
        text: String,
        /// Whether the tool executed successfully.
        success: bool,
    },
}

// ---------------------------------------------------------------------------
// ReAct agent seam (the provider-side model)
// ---------------------------------------------------------------------------

/// One step produced by the [`ReactAgent`] model seam.
#[derive(Debug, Clone, PartialEq)]
pub enum ReactDecision {
    /// The model wants to invoke a tool. `name` is the harness tool name;
    /// `arguments` is the parsed JSON arguments.
    CallTool {
        /// Harness tool name (e.g. `read_file`).
        name: String,
        /// Parsed JSON arguments.
        arguments: serde_json::Value,
    },
    /// The model has produced its final answer.
    Finish {
        /// The structured final output.
        output: StructuredOutput,
    },
}

/// The provider-backed model seam for the ReAct loop.
///
/// The orchestrator drives the loop; the composition boundary supplies a real
/// provider-backed implementation. `next_action(prompt, trajectory)` returns
/// the next decision — either invoke another tool or finish with an answer.
/// The trait is async + object-safe via a boxed `Send` future (the workspace
/// has no `async_trait` dependency).
///
/// Unlike [`CandidateGenerator`](super::CandidateGenerator), this trait does
/// **not** need `boxed_clone` — the ReAct loop is single-branch and shares
/// one `&dyn ReactAgent` reference across iterations.
pub trait ReactAgent: Send + Sync {
    /// Inspects the prompt + accumulated trajectory and returns the next
    /// decision.
    fn next_action<'a>(
        &'a self,
        prompt: &'a str,
        trajectory: &'a [TrajectoryEntry],
    ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// Tool invoker seam (the executor + permission sandbox)
// ---------------------------------------------------------------------------

/// Why a [`ToolInvoker`] rejected an action.
///
/// Variants mirror [`crate::executor::ToolError`] and
/// [`crate::permission::PermissionDecision::Deny`]. The ReAct loop converts
/// every variant into a structured failure observation rather than crashing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolInvocationError {
    /// No tool of that name is registered.
    #[error("no executor registered for tool {0}")]
    UnknownTool(String),
    /// The arguments did not match the tool schema.
    #[error("invalid arguments for tool {tool}: {reason}")]
    InvalidArguments {
        /// Stable tool name.
        tool: String,
        /// Safe description of the schema mismatch.
        reason: String,
    },
    /// The permission gate denied the call.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// The executor ran but returned an error.
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
}

/// The executor + permission sandbox seam.
///
/// `class_of(name)` returns the [`ToolExecutionClass`] for a registered tool
/// (used by the Read-Before-Write policy). `invoke(name, arguments)` routes
/// the call through the existing permission gate and executor — sandboxing
/// and one-time approvals are honored identically to the
/// [`crate::agent_loop::AgentLoop`] direct path.
///
/// Production impl: [`RegistryToolInvoker`]. Test impls: any fake that
/// returns scripted responses.
pub trait ToolInvoker: Send + Sync {
    /// Returns the execution class for a registered tool, or `None` if the
    /// name is unknown (which the loop treats as a mutating tool for
    /// Read-Before-Write purposes — fail-closed).
    fn class_of(&self, name: &str) -> Option<ToolExecutionClass>;

    /// Invokes the named tool. Implementations MUST honor the permission
    /// gate and operating mode internally so the loop itself stays
    /// policy-agnostic.
    fn invoke<'a>(
        &'a self,
        name: &'a str,
        arguments: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// Production wiring: RegistryToolInvoker
// ---------------------------------------------------------------------------

/// Production [`ToolInvoker`] backed by the existing [`ToolRegistry`] and the
/// permission sandbox.
///
/// This wraps the exact same machinery as
/// [`AgentLoop::gate_and_execute`](crate::agent_loop::AgentLoop) — the
/// [`ToolRegistry::execute`] executor plus
/// [`check_tool_permission`](crate::permission::check_tool_permission) plus
/// the host-supplied [`PermissionPort`] — so operating mode and one-time
/// approval are honored identically to the direct execution loop. The
/// composition boundary constructs one per VRO turn (sharing the same
/// `Arc<ToolRegistry>` and `Arc<dyn PermissionPort>` the `AgentLoop` uses).
pub struct RegistryToolInvoker {
    registry: ToolRegistry,
    permission_port: Arc<dyn PermissionPort>,
    context: ToolContext,
    call_counter: AtomicU32,
}

impl std::fmt::Debug for RegistryToolInvoker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryToolInvoker")
            .field("registry_len", &self.registry.len())
            .field("operating_mode", &self.context.operating_mode)
            .field("permission_mode", &self.context.permission_mode)
            .finish_non_exhaustive()
    }
}

impl RegistryToolInvoker {
    /// Creates a new invoker bound to the given registry, permission port,
    /// and execution context.
    ///
    /// The composition boundary typically shares one `Arc<ToolRegistry>`
    /// (via [`ToolRegistry::clone`], which is cheap — the registry is
    /// `Clone`) and the same `Arc<dyn PermissionPort>` the `AgentLoop` uses,
    /// so the ReAct path and the direct path enforce identical policy.
    #[must_use]
    pub fn new(
        registry: ToolRegistry,
        permission_port: Arc<dyn PermissionPort>,
        context: ToolContext,
    ) -> Self {
        Self {
            registry,
            permission_port,
            context,
            call_counter: AtomicU32::new(0),
        }
    }
}

impl ToolInvoker for RegistryToolInvoker {
    fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
        self.registry
            .definition(name)
            .map(|def| def.execution_class)
    }

    fn invoke<'a>(
        &'a self,
        name: &'a str,
        arguments: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>> {
        let registry = &self.registry;
        let permission_port = &self.permission_port;
        let context = &self.context;
        let counter = &self.call_counter;
        Box::pin(async move {
            // --- Definition lookup (covers gateway-prefixed tools too via
            // the registry's execute path, but class_of is restricted to the
            // static definitions list — that is intentional: Read-Before-Write
            // only gates known mutating tools; gateway/MCP tools fall through
            // to the default Mutating classification, which is fail-closed). ---
            let definition = registry
                .definition(name)
                .ok_or_else(|| ToolInvocationError::UnknownTool(name.to_string()))?;

            // --- Synthesize a normalized ToolCall so the existing executor
            // signature is honored. The call id is unique per invoker
            // instance so concurrent loops cannot collide. ---
            let sequence = counter.fetch_add(1, Ordering::Relaxed);
            let call_id_str = format!("vro-react-{sequence}");
            let call = ToolCall {
                id: ToolCallId::new(&call_id_str)
                    .map_err(|_| ToolInvocationError::ExecutionFailed("call id rejected".into()))?,
                tool_id: ToolId::new(name)
                    .map_err(|_| ToolInvocationError::UnknownTool(name.to_string()))?,
                arguments: arguments.clone(),
                extensions: ExtensionMap::default(),
            };

            // --- Permission gate: mirrors AgentLoop::gate_and_execute. ---
            let decision = check_tool_permission(
                context.operating_mode,
                context.permission_mode,
                definition.execution_class,
            );
            let allowed = match decision {
                PermissionDecision::Allow => true,
                PermissionDecision::Deny(reason) => {
                    return Err(ToolInvocationError::PermissionDenied(reason));
                }
                PermissionDecision::Ask(reason) => {
                    match permission_port.authorize(&call, definition, context).await {
                        PermissionDecision::Allow => true,
                        PermissionDecision::Deny(nested) => {
                            return Err(ToolInvocationError::PermissionDenied(format!(
                                "{reason}; {nested}"
                            )));
                        }
                        PermissionDecision::Ask(nested) => {
                            return Err(ToolInvocationError::PermissionDenied(format!(
                                "{reason}; {nested}"
                            )));
                        }
                    }
                }
            };
            if !allowed {
                return Err(ToolInvocationError::PermissionDenied(
                    "gate declined without explicit denial (internal invariant)".into(),
                ));
            }

            // --- Execute via the registry (which routes to entries or the
            // longest-matching gateway). ToolError is mapped 1:1 to the
            // invocation-error variants so the loop surfaces a structured
            // failure observation rather than crashing. ---
            match registry.execute(&call, context).await {
                Ok(result) => Ok(result.text.as_str().to_string()),
                Err(error) => Err(match error {
                    ToolError::UnknownTool(name) => ToolInvocationError::UnknownTool(name),
                    ToolError::InvalidArguments { tool, reason } => {
                        ToolInvocationError::InvalidArguments { tool, reason }
                    }
                    ToolError::OutputBoundary(error) => {
                        ToolInvocationError::ExecutionFailed(error.to_string())
                    }
                    ToolError::Failed(reason) => ToolInvocationError::ExecutionFailed(reason),
                }),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// The loop (PRD §11.6)
// ---------------------------------------------------------------------------

/// Runs the Tool-Grounded ReAct loop.
///
/// `prompt` is the user message. `agent` is the provider seam that decides
/// the next action. `invoker` is the executor + permission seam.
/// `requires_grounding` toggles the Read-Before-Write policy: when `true`,
/// mutating tools are rejected until at least one `ReadOnly` observation has
/// been recorded.
///
/// ## Halt conditions
///
/// - [`ReactDecision::Finish`] ⇒ [`OutcomeStatus::Succeeded`] (PRD §11.6).
/// - `max_model_calls` exhausted ⇒ [`OutcomeStatus::BudgetExceeded`].
/// - `max_tool_calls` exhausted when the model still wants to call tools ⇒
///   [`OutcomeStatus::BudgetExceeded`].
///
/// ## Tool errors
///
/// Any [`ToolInvocationError`] (unknown tool, malformed args, permission
/// denial, executor failure) is converted to a structured failure
/// observation and appended to the trajectory — the loop continues and the
/// model can correct itself.
///
/// ## Read-Before-Write
///
/// When `requires_grounding` is `true` and the model attempts a mutating
/// tool before any `ReadOnly` tool has produced an observation, the loop
/// synthesizes a structured rejection observation and continues (it does
/// **not** consume a `max_tool_calls` unit for the rejected attempt — only
/// successful dispatches count).
pub async fn run_tool_grounded_react(
    prompt: &str,
    agent: &dyn ReactAgent,
    invoker: &dyn ToolInvoker,
    budget: ReasoningBudget,
    requires_grounding: bool,
) -> ReasoningOutcome {
    let mut trajectory: Vec<TrajectoryEntry> = Vec::new();
    let mut model_calls = 0u32;
    let mut tool_calls = 0u32;
    let mut has_read_evidence = false;
    let mut unresolved_risks: Vec<String> = Vec::new();
    let max_model_calls = budget.max_model_calls.max(1);
    let max_tool_calls = budget.max_tool_calls;

    loop {
        // --- Halt: model-call safety budget exhausted (PRD §10.9). ---
        // Checked BEFORE the model call so we never exceed the ceiling.
        if model_calls >= max_model_calls {
            unresolved_risks.push(format!(
                "max_model_calls exhausted ({max_model_calls}) before the agent emitted Finish"
            ));
            return build_budget_exceeded(model_calls, tool_calls, unresolved_risks);
        }

        // --- THINK: ask the agent for the next action. ---
        let decision = agent.next_action(prompt, &trajectory).await;
        model_calls += 1;

        match decision {
            ReactDecision::Finish { output } => {
                return build_succeeded(model_calls, tool_calls, output, &trajectory);
            }
            ReactDecision::CallTool { name, arguments } => {
                // --- Halt: tool-call safety budget exhausted. ---
                // The model wanted to call a tool but we have no budget left;
                // surface this as BudgetExceeded (directive 4: loop halts
                // when max_tool_calls is exhausted).
                if tool_calls >= max_tool_calls {
                    unresolved_risks.push(format!(
                        "max_tool_calls exhausted ({max_tool_calls}); agent wanted to call `{name}`"
                    ));
                    return build_budget_exceeded(model_calls, tool_calls, unresolved_risks);
                }

                // --- Read-Before-Write policy (directive 3). ---
                // When grounding is required, mutating tools are rejected
                // until at least one ReadOnly observation exists. The
                // rejection is fed back as a structured observation so the
                // model can self-correct. The rejected attempt does NOT
                // consume a max_tool_calls unit (it never reached the
                // executor) — only successful dispatches count.
                let class = invoker
                    .class_of(&name)
                    .unwrap_or(ToolExecutionClass::Mutating);
                let is_read_only = matches!(class, ToolExecutionClass::ReadOnly);
                if requires_grounding && !is_read_only && !has_read_evidence {
                    trajectory.push(TrajectoryEntry::Action {
                        name: name.clone(),
                        arguments: arguments.clone(),
                    });
                    trajectory.push(TrajectoryEntry::Observation {
                        text: format!(
                            "Read-Before-Write policy: mutating tool `{name}` rejected. \
                             You MUST first call a read-only tool (e.g. `read_file`, `grep`, \
                             `list_directory`, `search_files`) to gather evidence before any \
                             mutation."
                        ),
                        success: false,
                    });
                    continue;
                }

                // --- ACT: route through the invoker (which honors the
                // permission gate and operating mode). Tool errors become
                // observations rather than crashing the loop (directive 2). ---
                let result = invoker.invoke(&name, &arguments).await;
                tool_calls += 1;
                trajectory.push(TrajectoryEntry::Action {
                    name: name.clone(),
                    arguments: arguments.clone(),
                });
                match result {
                    Ok(text) => {
                        if is_read_only {
                            has_read_evidence = true;
                        }
                        trajectory.push(TrajectoryEntry::Observation {
                            text,
                            success: true,
                        });
                    }
                    Err(error) => {
                        // Directive 2 + 4: structured failure observation,
                        // loop continues. The model sees the precise reason
                        // so it can correct (call a different tool, fix args,
                        // or finish).
                        trajectory.push(TrajectoryEntry::Observation {
                            text: error.to_string(),
                            success: false,
                        });
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Outcome builders
// ---------------------------------------------------------------------------

fn build_succeeded(
    model_calls: u32,
    tool_calls: u32,
    output: StructuredOutput,
    trajectory: &[TrajectoryEntry],
) -> ReasoningOutcome {
    let observation_count = trajectory
        .iter()
        .filter(|entry| matches!(entry, TrajectoryEntry::Observation { .. }))
        .count() as u32;
    ReasoningOutcome {
        status: OutcomeStatus::Succeeded,
        final_output: Some(output),
        selected_candidate: None,
        verification_summary: VerificationSummary {
            passed: 0,
            failed: 0,
            // The ReAct loop runs no deterministic verifier; "verification"
            // here is the count of successful tool observations that fed the
            // final answer. PRD §11.6 treats tool grounding as the
            // verification mechanism for this strategy.
            overall: VerificationStatus::Passed,
        },
        unresolved_risks: vec![],
        cost: InferenceCost {
            model_calls,
            // Heuristic token bookkeeping: one unit per observation. Real
            // accounting lands when the ReactAgent reports its own token
            // cost (deferred to a later phase).
            total_tokens: u64::from(model_calls)
                + u64::from(tool_calls)
                + u64::from(observation_count),
        },
    }
}

fn build_budget_exceeded(
    model_calls: u32,
    tool_calls: u32,
    unresolved_risks: Vec<String>,
) -> ReasoningOutcome {
    ReasoningOutcome {
        status: OutcomeStatus::BudgetExceeded,
        final_output: None,
        selected_candidate: None,
        verification_summary: VerificationSummary {
            passed: 0,
            failed: 0,
            overall: VerificationStatus::Inconclusive,
        },
        unresolved_risks,
        cost: InferenceCost {
            model_calls,
            total_tokens: u64::from(model_calls) + u64::from(tool_calls),
        },
    }
}

// ---------------------------------------------------------------------------
// Convenience re-exports for callers of this module
// ---------------------------------------------------------------------------

// Path-bearing aliases so doc links above resolve from outside this crate.
// They reference `crate::` items so the doc renderer can find them.
use crate::executor::{ToolContext, ToolError};
use crate::permission::{PermissionDecision, PermissionPort, check_tool_permission};
use crate::registry::ToolRegistry;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use vesper_domain::{
        OutcomeStatus, PrivacyMode, ReasoningMode, ReasoningRequest, RequestId, SessionId,
        ToolExecutionClass, VerificationStatus,
    };

    // -----------------------------------------------------------------------
    // Fakes
    // -----------------------------------------------------------------------

    /// Scripts a fixed sequence of `ReactDecision`s. Repeats the last entry
    /// forever so a loop that doesn't halt on its own (e.g. a budget-halt
    /// test) still terminates.
    struct ScriptedAgent {
        decisions: Mutex<Vec<ReactDecision>>,
    }
    impl ScriptedAgent {
        fn new(decisions: Vec<ReactDecision>) -> Self {
            Self {
                decisions: Mutex::new(decisions),
            }
        }
    }
    impl ReactAgent for ScriptedAgent {
        fn next_action<'a>(
            &'a self,
            _prompt: &'a str,
            _trajectory: &'a [TrajectoryEntry],
        ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
            Box::pin(async move {
                let mut decisions = self.decisions.lock().expect("poisoned");
                if decisions.len() == 1 {
                    decisions[0].clone()
                } else {
                    decisions.remove(0)
                }
            })
        }
    }

    /// Fake invoker that maps `(name, arguments)` → a scripted result or
    /// error. Records every observed call for assertions.
    struct FakeInvoker {
        responses: Mutex<std::collections::HashMap<String, Result<String, ToolInvocationError>>>,
        classes: std::collections::HashMap<String, ToolExecutionClass>,
        observed: Mutex<Vec<(String, serde_json::Value)>>,
    }
    impl FakeInvoker {
        fn new() -> Self {
            Self {
                responses: Mutex::new(std::collections::HashMap::new()),
                classes: std::collections::HashMap::new(),
                observed: Mutex::new(Vec::new()),
            }
        }
        fn with_read(name: &str, output: &str) -> Self {
            let mut invoker = Self::new();
            invoker
                .classes
                .insert(name.to_string(), ToolExecutionClass::ReadOnly);
            invoker
                .responses
                .lock()
                .expect("poisoned")
                .insert(name.to_string(), Ok(output.to_string()));
            invoker
        }
        fn register(
            &mut self,
            name: &str,
            class: ToolExecutionClass,
            response: Result<String, ToolInvocationError>,
        ) {
            self.classes.insert(name.to_string(), class);
            self.responses
                .lock()
                .expect("poisoned")
                .insert(name.to_string(), response);
        }
        #[allow(dead_code)]
        fn observed(&self) -> Vec<(String, serde_json::Value)> {
            self.observed.lock().expect("poisoned").clone()
        }
    }
    impl ToolInvoker for FakeInvoker {
        fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
            self.classes.get(name).copied()
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            arguments: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            self.observed
                .lock()
                .expect("poisoned")
                .push((name.to_string(), arguments.clone()));
            let result = self
                .responses
                .lock()
                .expect("poisoned")
                .get(name)
                .cloned()
                .unwrap_or_else(|| Err(ToolInvocationError::UnknownTool(name.to_string())));
            Box::pin(async move { result })
        }
    }

    fn budget(max_model_calls: u32, max_tool_calls: u32) -> ReasoningBudget {
        ReasoningBudget {
            max_model_calls,
            max_tool_calls,
            ..ReasoningBudget::balanced()
        }
    }

    // -----------------------------------------------------------------------
    // Directive 1 — scaffold + budget enforcement
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn finish_decision_halts_the_loop_with_succeeded() {
        // Single decision: the agent finishes immediately. One model call,
        // zero tool calls. The output is the finished payload.
        let agent = ScriptedAgent::new(vec![ReactDecision::Finish {
            output: serde_json::json!({"answer": "main.rs contains the program entry point"}),
        }]);
        let invoker = FakeInvoker::new();
        let outcome = run_tool_grounded_react(
            "What does the main.rs file do?",
            &agent,
            &invoker,
            budget(5, 5),
            true,
        )
        .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(
            outcome.final_output,
            Some(serde_json::json!({"answer": "main.rs contains the program entry point"}))
        );
        assert_eq!(outcome.cost.model_calls, 1);
        assert_eq!(
            outcome.cost.total_tokens, 1,
            "no tools, no observations -> 1 token (model call)"
        );
    }

    #[tokio::test]
    async fn loop_halts_when_max_tool_calls_is_exhausted() {
        // Directive 4: prove the loop halts when max_tool_calls is hit. The
        // agent always wants to call a tool and never finishes; with
        // max_tool_calls=2, the third CallTool attempt must trip the halt.
        let agent = ScriptedAgent::new(vec![ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        }]);
        let invoker = FakeInvoker::with_read("read_file", "file contents");
        let outcome = run_tool_grounded_react(
            "Read main.rs over and over",
            &agent,
            &invoker,
            budget(20, 2),
            true,
        )
        .await;
        assert_eq!(
            outcome.status,
            OutcomeStatus::BudgetExceeded,
            "max_tool_calls=2 must halt with BudgetExceeded"
        );
        assert_eq!(
            outcome.cost.model_calls, 3,
            "3 model calls: two successful tool dispatches + one blocked CallTool"
        );
        assert_eq!(outcome.cost.total_tokens, 5);
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("max_tool_calls exhausted") && r.contains("read_file")),
            "unresolved risk must name the rejected tool: {:?}",
            outcome.unresolved_risks
        );
    }

    #[tokio::test]
    async fn loop_halts_when_max_model_calls_is_exhausted() {
        // The agent never finishes and always wants to call tools. With
        // max_model_calls=2 the loop must halt BEFORE the third model call
        // (the ceiling is checked before each next_action).
        let agent = ScriptedAgent::new(vec![ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a"}),
        }]);
        let invoker = FakeInvoker::with_read("read_file", "ok");
        let outcome =
            run_tool_grounded_react("Read forever", &agent, &invoker, budget(2, 50), true).await;
        assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
        assert_eq!(
            outcome.cost.model_calls, 2,
            "model_calls ceiling checked before each call"
        );
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("max_model_calls exhausted")),
            "risk must mention max_model_calls: {:?}",
            outcome.unresolved_risks
        );
    }

    // -----------------------------------------------------------------------
    // Directive 2 — graceful tool-error handling
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tool_error_returns_structured_failure_observation_and_loop_continues() {
        // Directive 4: a tool error (malformed JSON, missing file, etc.) is
        // fed back to the model as a structured observation, NOT a crash. The
        // model then finishes on the next iteration. The trajectory MUST
        // contain the failed observation so the model could self-correct.
        let mut invoker = FakeInvoker::new();
        invoker.register(
            "read_file",
            ToolExecutionClass::ReadOnly,
            Err(ToolInvocationError::ExecutionFailed(
                "no such file: missing.rs".into(),
            )),
        );
        let agent = ScriptedAgent::new(vec![
            ReactDecision::CallTool {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "missing.rs"}),
            },
            ReactDecision::Finish {
                output: serde_json::json!({"answer": "the file does not exist"}),
            },
        ]);
        let outcome = run_tool_grounded_react(
            "What is in missing.rs?",
            &agent,
            &invoker,
            budget(5, 5),
            true,
        )
        .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(
            outcome.cost.model_calls, 2,
            "1 failed tool call + 1 Finish decision"
        );
        assert_eq!(
            outcome.cost.total_tokens, 4,
            "model_calls (2) + tool_calls (1) + observations (1)"
        );
        assert_eq!(
            outcome.verification_summary.overall,
            VerificationStatus::Passed,
            "tool-grounded verification is the observation; Succeeded -> Passed"
        );
    }

    #[tokio::test]
    async fn unknown_tool_surfaces_as_failure_observation() {
        // The agent calls a tool the invoker doesn't know. The loop must not
        // panic; the error observation is fed back.
        let agent = ScriptedAgent::new(vec![
            ReactDecision::CallTool {
                name: "nonexistent_tool".to_string(),
                arguments: serde_json::json!({}),
            },
            ReactDecision::Finish {
                output: serde_json::json!({"answer": "no such tool"}),
            },
        ]);
        let invoker = FakeInvoker::new(); // empty
        let outcome = run_tool_grounded_react(
            "Call a made-up tool",
            &agent,
            &invoker,
            budget(5, 5),
            // requires_grounding=false so the unknown tool (default Mutating)
            // is not blocked by Read-Before-Write — we are testing the
            // unknown-tool path, not R/B/W.
            false,
        )
        .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.cost.model_calls, 2);
    }

    // -----------------------------------------------------------------------
    // Directive 3 — Read-Before-Write policy
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_before_write_blocks_first_mutation_when_grounding_required() {
        // requires_grounding=true. The agent's first action is a mutating
        // tool. The loop MUST reject it with a Read-Before-Write observation
        // (NOT call the executor, NOT consume a max_tool_calls unit).
        let mut invoker = FakeInvoker::new();
        invoker.register(
            "write_file",
            ToolExecutionClass::Mutating,
            Ok("written".into()),
        );
        invoker.register(
            "read_file",
            ToolExecutionClass::ReadOnly,
            Ok("read contents".into()),
        );
        let agent = ScriptedAgent::new(vec![
            // First attempt: mutating tool — must be rejected.
            ReactDecision::CallTool {
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt", "content": "x"}),
            },
            // Second attempt: read-only tool — must succeed and unlock writes.
            ReactDecision::CallTool {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            // Third attempt: mutating tool — now allowed (read evidence exists).
            ReactDecision::CallTool {
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt", "content": "x"}),
            },
            ReactDecision::Finish {
                output: serde_json::json!({"answer": "done"}),
            },
        ]);
        let outcome =
            run_tool_grounded_react("Edit a.txt", &agent, &invoker, budget(20, 20), true).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // model_calls: 1 (write reject) + 1 (read) + 1 (write allow) + 1 (Finish) = 4.
        assert_eq!(outcome.cost.model_calls, 4);
        // tool_calls: only the two SUCCESSFUL dispatches count (the rejected
        // attempt never reached the executor). This is the directive's "loop
        // continues and corrects it" guarantee.
        assert_eq!(
            outcome.cost.total_tokens,
            4 + 2 + 3,
            "model_calls (4) + tool_calls (2) + observations (3: \
             R/B/W rejection + read result + write result)"
        );
    }

    #[tokio::test]
    async fn read_before_write_does_not_apply_when_grounding_not_required() {
        // requires_grounding=false: mutating tools can fire immediately.
        let mut invoker = FakeInvoker::new();
        invoker.register(
            "write_file",
            ToolExecutionClass::Mutating,
            Ok("written".into()),
        );
        let agent = ScriptedAgent::new(vec![
            ReactDecision::CallTool {
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            ReactDecision::Finish {
                output: serde_json::json!({"answer": "done"}),
            },
        ]);
        let outcome =
            run_tool_grounded_react("Just write", &agent, &invoker, budget(5, 5), false).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.cost.model_calls, 2);
    }

    #[tokio::test]
    async fn failed_read_does_not_unlock_writes_under_read_before_write() {
        // Audit gap fix: a read-only tool that ERRORS must NOT count as
        // evidence. Otherwise a model whose first read fails (e.g. file not
        // found) could immediately mutate based on absence of evidence rather
        // than retrying with a different read tool. The model MUST produce at
        // least one SUCCESSFUL ReadOnly observation before mutating.
        let mut invoker = FakeInvoker::new();
        invoker.register(
            "read_file",
            ToolExecutionClass::ReadOnly,
            Err(ToolInvocationError::ExecutionFailed("no such file".into())),
        );
        invoker.register(
            "write_file",
            ToolExecutionClass::Mutating,
            Ok("written".into()),
        );
        let agent = ScriptedAgent::new(vec![
            // Failed read attempt — does NOT unlock writes.
            ReactDecision::CallTool {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "missing.rs"}),
            },
            // Mutation attempt — still blocked because no successful read.
            ReactDecision::CallTool {
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt", "content": "x"}),
            },
            // Give up and finish without mutating.
            ReactDecision::Finish {
                output: serde_json::json!({"answer": "could not gather evidence"}),
            },
        ]);
        let outcome =
            run_tool_grounded_react("Edit a.txt", &agent, &invoker, budget(10, 10), true).await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        // model_calls: 1 (failed read) + 1 (write reject) + 1 (Finish) = 3.
        assert_eq!(outcome.cost.model_calls, 3);
        // tool_calls: only the FAILED read dispatched (1). The write was
        // rejected by R/B/W and never reached the executor.
        assert_eq!(
            outcome.cost.total_tokens,
            3 + 1 + 2,
            "model_calls (3) + tool_calls (1: failed read) + observations (2: \
             failed-read observation + R/B/W rejection)"
        );
    }

    #[tokio::test]
    async fn max_tool_calls_zero_halts_on_first_call_tool_attempt() {
        // Audit gap fix: a degenerate budget of max_tool_calls=0 must halt
        // with BudgetExceeded the moment the agent wants a tool — there is
        // no budget for even one dispatch. The model still got one
        // next_action call (the ceiling is checked against tool_calls, not
        // before the model call), so model_calls=1.
        let agent = ScriptedAgent::new(vec![ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a"}),
        }]);
        let invoker = FakeInvoker::with_read("read_file", "ok");
        let outcome = run_tool_grounded_react("Read a", &agent, &invoker, budget(5, 0), true).await;
        assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
        assert_eq!(
            outcome.cost.model_calls, 1,
            "one model call before the halt"
        );
        assert_eq!(
            outcome.cost.total_tokens, 1,
            "model_calls (1) + tool_calls (0)"
        );
        assert!(
            outcome
                .unresolved_risks
                .iter()
                .any(|r| r.contains("max_tool_calls exhausted (0)")),
            "risk must name the zero budget: {:?}",
            outcome.unresolved_risks
        );
    }

    // -----------------------------------------------------------------------
    // Directive 4 — task profiler routing (also exercised in profiler.rs
    // tests, but re-asserted here so the React contract is self-documenting)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn react_loop_appends_action_observation_pairs_to_trajectory() {
        // Sanity: the trajectory handed to next_action grows by exactly two
        // entries per successful tool call (Action + Observation). We capture
        // the trajectory lengths the agent sees.
        #[derive(Default)]
        struct TrajectoryLengthsAgent {
            seen: Mutex<Vec<usize>>,
        }
        impl ReactAgent for TrajectoryLengthsAgent {
            fn next_action<'a>(
                &'a self,
                _prompt: &'a str,
                trajectory: &'a [TrajectoryEntry],
            ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
                let seen = &self.seen;
                let len = trajectory.len();
                Box::pin(async move {
                    seen.lock().expect("poisoned").push(len);
                    if len < 4 {
                        ReactDecision::CallTool {
                            name: "read_file".to_string(),
                            arguments: serde_json::json!({"path": format!("f{len}")}),
                        }
                    } else {
                        ReactDecision::Finish {
                            output: serde_json::json!({"answer": "ok"}),
                        }
                    }
                })
            }
        }
        let agent = TrajectoryLengthsAgent::default();
        let invoker = FakeInvoker::with_read("read_file", "ok");
        let outcome =
            run_tool_grounded_react("Read several files", &agent, &invoker, budget(20, 20), true)
                .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        let seen = agent.seen.lock().expect("poisoned").clone();
        // Each iteration the trajectory grew by exactly 2 entries before the
        // agent is re-queried: 0, 2, 4, then Finish at length 4 (no growth).
        assert_eq!(seen, vec![0, 2, 4], "trajectory grows by 2 per tool call");
    }

    // -----------------------------------------------------------------------
    // RegistryToolInvoker routes through the real permission gate
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn registry_invoker_routes_read_only_tools_without_permission_prompt() {
        // ReadOnly tools always pass the static gate. The invoker runs them
        // against a real ToolRegistry with a real read_file executor in a
        // temp workspace.
        use crate::executor::uncancellable_context;
        use vesper_domain::{
            BoundedString, SessionOperatingMode, SessionPermissionMode, WorkspaceRoot,
        };

        let registry = ToolRegistry::parity_default();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("note.txt"), "registry invoker").unwrap();
        let roots = vec![WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new(root.path().to_string_lossy().to_string()).unwrap(),
            primary: true,
        }];
        // Plan mode + ReadOnly permission — read_file is ReadOnly, so it
        // must pass the static gate without ever consulting the (deny-all)
        // permission port.
        let context = uncancellable_context(
            roots,
            SessionOperatingMode::Plan,
            SessionPermissionMode::ReadOnly,
        );
        let invoker = RegistryToolInvoker::new(
            registry,
            Arc::new(crate::permission::DenyPermissionPort),
            context,
        );

        assert_eq!(
            invoker.class_of("read_file"),
            Some(ToolExecutionClass::ReadOnly),
            "read_file must be classified ReadOnly"
        );

        let result = invoker
            .invoke("read_file", &serde_json::json!({"path": "note.txt"}))
            .await
            .expect("ReadOnly tool must pass the static gate");
        assert_eq!(result, "registry invoker");
    }

    #[tokio::test]
    async fn registry_invoker_denies_mutating_tool_in_plan_mode() {
        // Mutating tools require Code mode. Plan mode + Ask permission with
        // a DenyPermissionPort must surface PermissionDenied (NOT consult
        // the executor). This proves the invoker honors the same gate as
        // AgentLoop::gate_and_execute.
        use crate::executor::uncancellable_context;
        use vesper_domain::{SessionOperatingMode, SessionPermissionMode};

        let registry = ToolRegistry::parity_default();
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Plan,
            SessionPermissionMode::Ask,
        );
        let invoker = RegistryToolInvoker::new(
            registry,
            Arc::new(crate::permission::DenyPermissionPort),
            context,
        );
        let result = invoker
            .invoke(
                "write_file",
                &serde_json::json!({"path": "a.txt", "content": "x"}),
            )
            .await;
        assert!(
            matches!(result, Err(ToolInvocationError::PermissionDenied(_))),
            "mutating tool in Plan mode must be denied: {result:?}"
        );
    }

    #[tokio::test]
    async fn registry_invoker_returns_unknown_tool_for_unregistered_name() {
        use crate::executor::uncancellable_context;
        use vesper_domain::{SessionOperatingMode, SessionPermissionMode};
        let registry = ToolRegistry::parity_default();
        let context = uncancellable_context(
            Vec::new(),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        );
        let invoker = RegistryToolInvoker::new(
            registry,
            Arc::new(crate::permission::DenyPermissionPort),
            context,
        );
        let result = invoker
            .invoke("totally_unknown", &serde_json::json!({}))
            .await;
        assert!(
            matches!(result, Err(ToolInvocationError::UnknownTool(_))),
            "unregistered name must surface UnknownTool: {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // End-to-end: full ReAct loop with a real RegistryToolInvoker
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn full_react_loop_with_registry_invoker_gathers_then_answers() {
        // The agent first reads a file (gathering evidence), then finishes
        // with an answer that references the file. This exercises the full
        // production path: ReactAgent -> run_tool_grounded_react ->
        // RegistryToolInvoker -> ToolRegistry::execute -> real read_file.
        use crate::executor::uncancellable_context;
        use vesper_domain::{
            BoundedString, SessionOperatingMode, SessionPermissionMode, WorkspaceRoot,
        };

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("main.rs"), "fn main() {}").unwrap();
        let roots = vec![WorkspaceRoot {
            name: BoundedString::new("ws").unwrap(),
            path: BoundedString::new(root.path().to_string_lossy().to_string()).unwrap(),
            primary: true,
        }];
        let context = uncancellable_context(
            roots,
            SessionOperatingMode::Plan,
            SessionPermissionMode::ReadOnly,
        );
        let invoker = RegistryToolInvoker::new(
            ToolRegistry::parity_default(),
            Arc::new(crate::permission::DenyPermissionPort),
            context,
        );
        let agent = ScriptedAgent::new(vec![
            ReactDecision::CallTool {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "main.rs"}),
            },
            ReactDecision::Finish {
                output: serde_json::json!({"answer": "main.rs defines the program entry point"}),
            },
        ]);
        let outcome = run_tool_grounded_react(
            "What does the main.rs file do?",
            &agent,
            &invoker,
            budget(5, 5),
            true,
        )
        .await;
        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.cost.model_calls, 2);
        // total_tokens = model_calls (2) + tool_calls (1) + observations (1) = 4.
        assert_eq!(outcome.cost.total_tokens, 4);
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Suppress unused-import warnings in builds where the helper isn't
    /// needed (the PrivacyMode / ReasoningMode / RequestId / SessionId
    /// imports above are part of the documented public surface contract but
    /// only some tests use them).
    #[allow(dead_code)]
    fn _silence_unused() {
        let _ = PrivacyMode::Private;
        let _ = ReasoningMode::Auto;
        let _ = RequestId::new("r").unwrap();
        let _ = SessionId::new("s").unwrap();
        let _ = ReasoningRequest {
            request_id: RequestId::new("r").unwrap(),
            session_id: SessionId::new("s").unwrap(),
            user_message: String::new(),
            context_refs: Vec::new(),
            mode: ReasoningMode::Auto,
            risk_hint: None,
            budget_override: None,
            privacy_mode: PrivacyMode::Private,
        };
    }
}
