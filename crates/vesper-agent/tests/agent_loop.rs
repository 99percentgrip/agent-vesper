//! Integration tests for the Tier C agent loop (ADR 0010, Phase 2).
//!
//! Uses `vesper-testkit`'s `FakeProviderSession` registered behind a
//! `ProviderFactory` in a real `vesper-runtime::ProviderRegistry`, so the loop
//! dispatches through the same composition seam production code uses.

use std::sync::{Arc, Mutex};

use serde_json::json;
use vesper_agent::{
    AgentLoop, AgentLoopConfig, AgentLoopError, AgentProgressEvent, AgentProgressPort,
    AgentTurnOutcome, DEFAULT_MAX_TOOL_ITERATIONS, ToolContext, ToolError, ToolExecutor,
    ToolFuture, ToolRegistry, ToolResult, ToolService, schema_definition,
};

use vesper_domain::{
    BoundedString, ContentPart, ContentText, ConversationMessage, ExtensionMap, FinishOutcome,
    MessageId, MessageRole, MessageRole::User, ProviderId, QualifiedModelId, ReasoningKind,
    ReasoningRetention, SchemaVersion, SessionOperatingMode, SessionPermissionMode,
    StreamInterruptionCause, ToolCall, ToolCallId, ToolDefinition, ToolExecutionClass, ToolId,
    VersionedExtensionEnvelope,
};
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderError, ProviderFactory, ProviderFuture,
    ProviderStreamEvent,
};
use vesper_runtime::ProviderRegistry;
use vesper_testkit::{FakeProviderSession, ScriptedProviderResponse};

#[derive(Clone)]
struct FakeFactory {
    id: ProviderId,
    session: FakeProviderSession,
}

#[derive(Default)]
struct RecordingProgressPort {
    events: Mutex<Vec<AgentProgressEvent>>,
}

impl AgentProgressPort for RecordingProgressPort {
    fn emit(&self, event: AgentProgressEvent) {
        self.events.lock().unwrap().push(event);
    }
}

impl ProviderFactory for FakeFactory {
    type Session = FakeProviderSession;

    fn provider_id(&self) -> &ProviderId {
        &self.id
    }

    fn create_session<'a>(
        &'a self,
        _config: &'a ProviderConfiguration,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
}

fn configuration(provider_id: &ProviderId) -> ProviderConfiguration {
    ProviderConfiguration {
        provider_id: provider_id.clone(),
        values: VersionedExtensionEnvelope {
            namespace: vesper_domain::ExtensionNamespace::new("provider.test").unwrap(),
            version: SchemaVersion::new(1).unwrap(),
            values: ExtensionMap::default(),
        },
    }
}

fn provider() -> ProviderId {
    ProviderId::new("test.agent").unwrap()
}

fn user_message(text: &str) -> ConversationMessage {
    ConversationMessage {
        id: MessageId::new("user-1").unwrap(),
        role: User,
        content: vec![ContentPart::Text(ContentText::new(text).unwrap())],
        extensions: ExtensionMap::default(),
    }
}

fn read_file_call() -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new("read_file").unwrap(),
        arguments: json!({"path": "src/lib.rs"}),
        extensions: ExtensionMap::default(),
    }
}

fn update_plan_call(id: &str, status: &str) -> ToolCall {
    ToolCall {
        id: ToolCallId::new(id).unwrap(),
        tool_id: ToolId::new("update_plan").unwrap(),
        arguments: json!({
            "tasks": [{
                "content": "Produce the final report",
                "status": status,
                "priority": "high"
            }]
        }),
        extensions: ExtensionMap::default(),
    }
}

fn content_delta(text: &str) -> ProviderStreamEvent {
    ProviderStreamEvent::ContentDelta {
        stream_id: BoundedString::new("content").unwrap(),
        part: ContentPart::Text(ContentText::new(text).unwrap()),
    }
}

/// Builds a provider-visible reasoning stream delta, mirroring how the GLM
/// adapter maps `delta.reasoning_content` to a `ReasoningDelta` event.
fn reasoning_delta(text: &str, kind: ReasoningKind) -> ProviderStreamEvent {
    ProviderStreamEvent::ReasoningDelta {
        stream_id: BoundedString::new("reasoning").unwrap(),
        text: ContentText::new(text).unwrap(),
        kind,
        retention: ReasoningRetention::SessionOnly,
    }
}

fn completed(finish: FinishOutcome) -> ProviderStreamEvent {
    ProviderStreamEvent::Completed {
        finish,
        metadata: ExtensionMap::default(),
    }
}

fn config(provider_id: &ProviderId, max_iterations: u32) -> AgentLoopConfig {
    AgentLoopConfig {
        provider_id: provider_id.clone(),
        provider_configuration: configuration(provider_id),
        model: QualifiedModelId {
            provider_id: provider_id.clone(),
            model_id: vesper_domain::ModelId::new("fixture-model").unwrap(),
        },
        system_instructions: Vec::new(),
        workspace_roots: Vec::new(),
        max_tool_iterations: max_iterations,
    }
}

#[tokio::test]
async fn non_stop_terminal_is_not_reported_as_completed() {
    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([Ok(vec![
        Ok(content_delta("partial implementation")),
        Ok(completed(FinishOutcome::OutputLimit)),
    ])]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let error = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 10),
    )
    .run_prompt(
        user_message("implement the change"),
        SessionOperatingMode::Code,
        SessionPermissionMode::Ask,
    )
    .await
    .expect_err("an output-limited response must not masquerade as completion");

    assert!(matches!(
        error,
        AgentLoopError::Incomplete(FinishOutcome::OutputLimit)
    ));
}

#[tokio::test]
async fn interrupted_visible_turn_returns_and_commits_partial_history() {
    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([Ok(vec![
        Ok(content_delta("preserved partial answer")),
        Ok(completed(FinishOutcome::StreamInterrupted {
            cause: StreamInterruptionCause::RemoteEof,
            tool_call_started: true,
        })),
    ])]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let (outcome, history) = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 10),
    )
    .run_prompt_with_history(
        vec![user_message("implement safely")],
        SessionOperatingMode::Code,
        SessionPermissionMode::Ask,
    )
    .await
    .expect("an interrupted visible turn is a preserved terminal state");

    assert!(matches!(
        outcome,
        AgentTurnOutcome::Interrupted {
            cause: StreamInterruptionCause::RemoteEof,
            tool_call_started: true,
            ..
        }
    ));
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].role, MessageRole::Assistant);
    assert_eq!(
        history[1].content,
        vec![ContentPart::Text(
            ContentText::new("preserved partial answer").unwrap()
        )]
    );
}

#[tokio::test]
async fn normal_stop_with_open_plan_continues_until_plan_is_completed() {
    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([
        Ok(vec![
            Ok(ProviderStreamEvent::ToolCallCompleted(update_plan_call(
                "plan-open",
                "in_progress",
            ))),
            Ok(completed(FinishOutcome::ToolCalls)),
        ]),
        Ok(vec![
            Ok(content_delta("I will stop before the report.")),
            Ok(completed(FinishOutcome::Stop)),
        ]),
        Ok(vec![
            Ok(ProviderStreamEvent::ToolCallCompleted(update_plan_call(
                "plan-done",
                "completed",
            ))),
            Ok(completed(FinishOutcome::ToolCalls)),
        ]),
        Ok(vec![
            Ok(content_delta("Final audit report.")),
            Ok(completed(FinishOutcome::Stop)),
        ]),
    ]);
    let requests = fake.clone();
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let root = tempfile::tempdir().unwrap();
    let mut agent_config = config(&provider_id, 10);
    agent_config.workspace_roots = vec![vesper_domain::WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new(root.path().to_string_lossy().to_string()).unwrap(),
        primary: true,
    }];
    let (outcome, history) = AgentLoop::new(registry, ToolRegistry::parity_default(), agent_config)
        .run_prompt_with_history(
            vec![user_message("audit every requirement")],
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        )
        .await
        .expect("an open plan must continue to its completed report");

    assert!(matches!(
        outcome,
        AgentTurnOutcome::Completed {
            assistant_content,
            ..
        } if assistant_content.iter().any(|part| matches!(
            part,
            ContentPart::Text(text) if text.as_str() == "Final audit report."
        ))
    ));
    let captured = requests.requests();
    assert_eq!(captured.len(), 4, "the premature stop must trigger a retry");
    assert!(captured[2].messages.iter().any(|message| {
        message.content.iter().any(|part| {
            matches!(
                part,
                ContentPart::Text(text) if text.as_str().contains("active plan still has")
            )
        })
    }));
    assert!(history.iter().all(|message| {
        !message.content.iter().any(|part| {
            matches!(
                part,
                ContentPart::Text(text) if text.as_str().contains("[SYSTEM CONTINUATION]")
            )
        })
    }));
}

#[tokio::test]
async fn direct_loop_stops_repeated_identical_tool_calls_before_iteration_cap() {
    let provider_id = provider();
    let repeated = || {
        Ok(vec![
            Ok(ProviderStreamEvent::ToolCallCompleted(read_file_call())),
            Ok(completed(FinishOutcome::ToolCalls)),
        ])
    };
    let fake = FakeProviderSession::with_scripts([
        repeated(),
        repeated(),
        repeated(),
        repeated(),
        repeated(),
    ]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
    let mut agent_config = config(&provider_id, 20);
    agent_config.workspace_roots = vec![vesper_domain::WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new(root.path().to_string_lossy().to_string()).unwrap(),
        primary: true,
    }];

    let error = AgentLoop::new(registry, ToolRegistry::parity_default(), agent_config)
        .run_prompt(
            user_message("keep rereading forever"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .expect_err("the direct loop guard must break before the numeric cap");

    assert!(matches!(error, AgentLoopError::LoopDetected(reason) if reason.contains("VRO-12")));
}

#[tokio::test]
async fn loop_executes_a_tool_call_then_completes_on_the_next_turn() {
    // Turn 1: assistant text + a read_file tool call. Turn 2: plain "Done".
    let turn_with_tool: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("Reading the file.")),
        Ok(ProviderStreamEvent::ToolCallCompleted(read_file_call())),
        Ok(completed(FinishOutcome::ToolCalls)),
    ]);
    let turn_done: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("Done.")),
        Ok(completed(FinishOutcome::Stop)),
    ]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn_with_tool, turn_done]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    // read_file is a real executor: give the loop a workspace root that
    // actually contains the path the scripted tool call references.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
    let mut agent_config = config(&provider_id, 10);
    agent_config.workspace_roots = vec![vesper_domain::WorkspaceRoot {
        name: vesper_domain::BoundedString::new("workspace").unwrap(),
        path: vesper_domain::BoundedString::new(root.path().to_string_lossy().to_string()).unwrap(),
        primary: true,
    }];

    let progress = Arc::new(RecordingProgressPort::default());
    let agent = AgentLoop::new(registry, ToolRegistry::parity_default(), agent_config)
        .with_progress_port(progress.clone());
    let outcome = agent
        .run_prompt(
            user_message("read src/lib.rs"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .expect("loop must complete");

    match outcome {
        AgentTurnOutcome::Completed {
            iterations,
            tool_results,
            assistant_content,
            plan: _,
        } => {
            assert_eq!(
                iterations, 2,
                "exactly two provider turns: tool turn + final"
            );
            assert_eq!(tool_results.len(), 1, "one tool executed");
            assert!(
                tool_results[0].text.as_str().contains("fn main()"),
                "the real read_file result must be captured: {}",
                tool_results[0].text.as_str()
            );
            let final_text = assistant_content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>();
            assert!(
                final_text.contains("Done."),
                "final assistant text must surface"
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    let events = progress.events.lock().unwrap();
    assert!(matches!(
        events.first(),
        Some(AgentProgressEvent::TurnStarted)
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentProgressEvent::ContentDelta { text } if text.as_str() == "Reading the file."
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentProgressEvent::ToolStarted { name, .. } if name == "read_file"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentProgressEvent::ToolFinished { name, success: true, .. } if name == "read_file"
    )));
}

#[tokio::test]
async fn loop_terminates_when_the_model_calls_no_tools() {
    // A single turn with no tool calls must complete in one iteration.
    let turn_done: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("Nothing to do.")),
        Ok(completed(FinishOutcome::Stop)),
    ]);
    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn_done]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let agent = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 10),
    );
    let outcome = agent
        .run_prompt(
            user_message("hello"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .unwrap();

    match outcome {
        AgentTurnOutcome::Completed {
            iterations,
            tool_results,
            ..
        } => {
            assert_eq!(iterations, 1);
            assert!(tool_results.is_empty());
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn loop_trips_the_max_iterations_safety_cap() {
    // The model calls a tool every turn; with max=2 the loop must stop at the
    // cap rather than loop forever. Each script emits one tool turn; the fake
    // is seeded with enough scripts to exceed the cap.
    let tool_turn = || {
        Ok::<_, Box<ProviderError>>(vec![
            Ok(content_delta("working")),
            Ok(ProviderStreamEvent::ToolCallCompleted(read_file_call())),
            Ok(completed(FinishOutcome::ToolCalls)),
        ])
    };
    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([
        tool_turn(),
        tool_turn(),
        tool_turn(),
        tool_turn(),
        tool_turn(),
    ]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let agent = AgentLoop::new(
        Arc::clone(&registry),
        ToolRegistry::parity_default(),
        config(&provider_id, 2),
    );
    let outcome = agent
        .run_prompt(
            user_message("loop"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .unwrap();
    match outcome {
        AgentTurnOutcome::MaxIterationsReached { iterations } => {
            assert_eq!(
                iterations, 2,
                "the cap must trip exactly at max_tool_iterations"
            );
        }
        other => panic!("expected MaxIterationsReached, got {other:?}"),
    }
}

#[tokio::test]
async fn permission_gate_denies_mutating_tools_in_plan_mode() {
    // In Plan mode, a write_file tool call must be denied and surfaced to the
    // model as bounded tool text — then the model stops on the next turn.
    let call = ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new("write_file").unwrap(),
        arguments: json!({"path": "out.txt", "content": "x"}),
        extensions: ExtensionMap::default(),
    };
    let turn_with_tool: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("writing")),
        Ok(ProviderStreamEvent::ToolCallCompleted(call)),
        Ok(completed(FinishOutcome::ToolCalls)),
    ]);
    let turn_done: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("ok")),
        Ok(completed(FinishOutcome::Stop)),
    ]);
    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn_with_tool, turn_done]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let agent = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 10),
    );
    let outcome = agent
        .run_prompt(
            user_message("write out.txt"),
            SessionOperatingMode::Plan,
            SessionPermissionMode::Ask,
        )
        .await
        .unwrap();
    match outcome {
        AgentTurnOutcome::Completed { tool_results, .. } => {
            assert_eq!(tool_results.len(), 1);
            assert!(
                tool_results[0].text.as_str().contains("permission denied"),
                "write_file must be denied in Plan mode: {}",
                tool_results[0].text.as_str()
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    // Sanity: the default cap is 50, matching the oracle's default.
    assert_eq!(DEFAULT_MAX_TOOL_ITERATIONS, 50);
    // Silence unused-import warning for MessageRole re-export alias.
    let _ = MessageRole::Assistant;
}

#[tokio::test]
async fn streamed_text_deltas_coalesce_into_one_contiguous_message() {
    // Regression for the one-token-per-line rendering bug: a streamed turn
    // split across many ContentDelta events must produce a SINGLE
    // ContentPart::Text in the final assistant content, so the TUI renders one
    // wrapped "assistant: ..." block instead of one line per token chunk.
    let streamed: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("The ")),
        Ok(content_delta("quick ")),
        Ok(content_delta("brown ")),
        Ok(content_delta("fox **jumps**.")),
        Ok(completed(FinishOutcome::Stop)),
    ]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([streamed]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let agent = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 5),
    );
    let outcome = agent
        .run_prompt(
            user_message("say a sentence"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .expect("loop must complete");

    match outcome {
        AgentTurnOutcome::Completed {
            assistant_content, ..
        } => {
            let text_parts: Vec<&ContentPart> = assistant_content
                .iter()
                .filter(|part| matches!(part, ContentPart::Text(_)))
                .collect();
            assert_eq!(
                text_parts.len(),
                1,
                "streamed deltas must coalesce into one text part, got {text_parts:?}"
            );
            let combined = text_parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(combined, "The quick brown fox **jumps**.");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn provider_visible_reasoning_streams_to_the_progress_port_in_order() {
    // A reasoning model emits provider-visible thinking BEFORE the visible
    // content. The loop must forward each reasoning delta to the progress
    // port so a host can render it live — exactly as it does for content
    // deltas. This is the regression guard for the reasoning pipeline.
    let turn: ScriptedProviderResponse = Ok(vec![
        Ok(reasoning_delta("step one", ReasoningKind::ProviderVisible)),
        Ok(reasoning_delta("step two", ReasoningKind::ProviderVisible)),
        Ok(content_delta("the answer")),
        Ok(completed(FinishOutcome::Stop)),
    ]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let progress = Arc::new(RecordingProgressPort::default());
    let agent = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 10),
    )
    .with_progress_port(progress.clone());
    agent
        .run_prompt(
            user_message("think then answer"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .expect("loop must complete");

    let events = progress.events.lock().unwrap().clone();

    // Both reasoning chunks must arrive, in order, before the content delta.
    let reasoning: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            AgentProgressEvent::ReasoningDelta { text } => Some(text.as_str().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasoning,
        vec!["step one".to_string(), "step two".to_string()],
        "provider-visible reasoning deltas must stream in order"
    );

    // Content must still stream too — the reasoning branch must not swallow
    // the content path.
    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentProgressEvent::ContentDelta { text } if text.as_str() == "the answer"
        )),
        "content delta must still arrive alongside reasoning"
    );

    // Reasoning must precede content in the event stream.
    let reasoning_idx = events
        .iter()
        .position(|event| matches!(event, AgentProgressEvent::ReasoningDelta { .. }))
        .expect("at least one reasoning event");
    let content_idx = events
        .iter()
        .position(|event| matches!(event, AgentProgressEvent::ContentDelta { .. }))
        .expect("a content event");
    assert!(
        reasoning_idx < content_idx,
        "reasoning must stream before content"
    );
}

#[tokio::test]
async fn opaque_continuation_reasoning_is_not_forwarded_to_the_progress_port() {
    // Only ProviderVisible / Summary reasoning is host-displayable; opaque
    // continuation records must NOT be forwarded (they would leak non-
    // displayable provider state into the live UI).
    let turn: ScriptedProviderResponse = Ok(vec![
        Ok(reasoning_delta("hidden", ReasoningKind::OpaqueContinuation)),
        Ok(content_delta("ok")),
        Ok(completed(FinishOutcome::Stop)),
    ]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn]);
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let progress = Arc::new(RecordingProgressPort::default());
    let agent = AgentLoop::new(
        registry,
        ToolRegistry::parity_default(),
        config(&provider_id, 10),
    )
    .with_progress_port(progress.clone());
    agent
        .run_prompt(
            user_message("answer only"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Ask,
        )
        .await
        .expect("loop must complete");

    let events = progress.events.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentProgressEvent::ReasoningDelta { .. })),
        "opaque continuation reasoning must not reach the progress port"
    );
}

// =========================================================================
// Phase 2 — deferred tool loading & dynamic context injection
// =========================================================================

/// A host-injected service exposing one advertised tool (`discover_tools`)
/// that returns a brand-new tool schema via `ToolResult::injected_tools`.
/// Used to prove the agent loop mutates its advertised pool mid-turn.
struct DiscoveringToolService;

impl ToolService for DiscoveringToolService {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![schema_definition(
            "discover_tools",
            "Discover tools on demand and inject their schemas into the loop.",
            ToolExecutionClass::ReadOnly,
            &[("query", "string", true)],
        )]
    }

    fn execute<'a>(
        &'a self,
        _call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        Box::pin(async move {
            // Inject a schema that was NOT in the initial advertisement.
            let injected = schema_definition(
                "injected_runtime_tool",
                "A tool surfaced mid-session by discover_tools.",
                ToolExecutionClass::ReadOnly,
                &[("path", "string", true)],
            );
            Ok(ToolResult::new("discovered 1 tool")
                .expect("bounded")
                .with_injected_tools(vec![injected]))
        })
    }
}

#[tokio::test]
async fn loop_injects_returned_tool_schemas_into_the_next_turn_advertisement() {
    let discover_call = ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new("discover_tools").unwrap(),
        arguments: json!({"query": "files"}),
        extensions: ExtensionMap::default(),
    };
    // Turn 1: assistant text + discover_tools tool call.
    let turn_with_tool: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("Looking up tools.")),
        Ok(ProviderStreamEvent::ToolCallCompleted(discover_call)),
        Ok(completed(FinishOutcome::ToolCalls)),
    ]);
    // Turn 2: model stops, no further tool calls.
    let turn_done: ScriptedProviderResponse = Ok(vec![
        Ok(content_delta("Done.")),
        Ok(completed(FinishOutcome::Stop)),
    ]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn_with_tool, turn_done]);
    // FakeProviderSession shares its recorded-request buffer via
    // `Arc<Mutex<_>>`; clone the handle before moving the original into the
    // factory so we can inspect what the loop advertised on each turn.
    let recorded = fake.clone();
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    // Register the discover_tools service alongside the 9 parity tools. The
    // injected_runtime_tool is intentionally NOT advertised initially.
    let tools = ToolRegistry::parity_default().with_service(Arc::new(DiscoveringToolService));

    let progress = Arc::new(RecordingProgressPort::default());
    let agent = AgentLoop::new(registry, tools, config(&provider_id, 10))
        .with_progress_port(progress.clone());
    let outcome = agent
        .run_prompt(
            user_message("discover a tool"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        )
        .await
        .expect("loop must complete");

    // Two turns: discovery turn + final turn.
    assert!(
        matches!(outcome, AgentTurnOutcome::Completed { iterations: 2, .. }),
        "expected the loop to run exactly two iterations, got {outcome:?}"
    );

    let requests = recorded.requests();
    assert_eq!(requests.len(), 2, "two provider turns must be recorded");

    let initial_names: Vec<String> = requests[0]
        .tools
        .iter()
        .map(|definition| definition.harness_name.as_str().to_owned())
        .collect();
    assert!(
        initial_names.contains(&"discover_tools".to_owned()),
        "discover_tools must be advertised on turn 1: {initial_names:?}"
    );
    assert!(
        !initial_names.contains(&"injected_runtime_tool".to_owned()),
        "injected_runtime_tool must NOT be advertised on turn 1: {initial_names:?}"
    );
    // Sanity: 9 parity tools + discover_tools = 10 on turn 1.
    assert_eq!(initial_names.len(), 10);

    let subsequent_names: Vec<String> = requests[1]
        .tools
        .iter()
        .map(|definition| definition.harness_name.as_str().to_owned())
        .collect();
    assert!(
        subsequent_names.contains(&"injected_runtime_tool".to_owned()),
        "injected_runtime_tool MUST be advertised on turn 2 after discover_tools injected it: {subsequent_names:?}"
    );
    assert!(
        subsequent_names.contains(&"discover_tools".to_owned()),
        "originally advertised tools must remain after injection: {subsequent_names:?}"
    );
    // 10 initial + 1 injected, with no duplicates.
    assert_eq!(subsequent_names.len(), 11);
}

#[tokio::test]
async fn loop_deduplicates_injected_tools_that_collide_with_an_advertised_name() {
    // A service whose tool injects a schema colliding (by harness_name) with
    // an already-advertised tool. The loop must dedup by harness_name and
    // not bloat the advertised pool.
    struct DuplicateInjectingService;
    impl ToolService for DuplicateInjectingService {
        fn definitions(&self) -> Vec<ToolDefinition> {
            vec![schema_definition(
                "announce_dupe",
                "Returns a schema that collides with an existing tool name.",
                ToolExecutionClass::ReadOnly,
                &[],
            )]
        }
        fn execute<'a>(
            &'a self,
            _call: &'a ToolCall,
            _context: &'a ToolContext,
        ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
            Box::pin(async move {
                // Collides with `read_file`, which is already advertised.
                let dupe = schema_definition(
                    "read_file",
                    "A duplicate that should be deduped by harness_name.",
                    ToolExecutionClass::ReadOnly,
                    &[("path", "string", true)],
                );
                Ok(ToolResult::new("injecting a duplicate")
                    .expect("bounded")
                    .with_injected_tools(vec![dupe]))
            })
        }
    }

    let announce_call = ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new("announce_dupe").unwrap(),
        arguments: json!({}),
        extensions: ExtensionMap::default(),
    };
    let turn_with_tool: ScriptedProviderResponse = Ok(vec![
        Ok(ProviderStreamEvent::ToolCallCompleted(announce_call)),
        Ok(completed(FinishOutcome::ToolCalls)),
    ]);
    let turn_done: ScriptedProviderResponse = Ok(vec![Ok(completed(FinishOutcome::Stop))]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn_with_tool, turn_done]);
    let recorded = fake.clone();
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let tools = ToolRegistry::parity_default().with_service(Arc::new(DuplicateInjectingService));
    let progress = Arc::new(RecordingProgressPort::default());
    let agent = AgentLoop::new(registry, tools, config(&provider_id, 10))
        .with_progress_port(progress.clone());
    agent
        .run_prompt(
            user_message("trigger dupe"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        )
        .await
        .expect("loop must complete");

    let requests = recorded.requests();
    assert_eq!(requests.len(), 2);
    // Turn 2 must have exactly 10 tools (9 parity + announce_dupe); the
    // duplicate `read_file` injection must NOT add a second read_file slot.
    let subsequent_names: Vec<String> = requests[1]
        .tools
        .iter()
        .map(|definition| definition.harness_name.as_str().to_owned())
        .collect();
    let read_file_count = subsequent_names
        .iter()
        .filter(|name| name.as_str() == "read_file")
        .count();
    assert_eq!(
        read_file_count, 1,
        "dedup must keep exactly one read_file entry: {subsequent_names:?}"
    );
    assert_eq!(subsequent_names.len(), 10);
}

// =========================================================================
// Phase 3 — end-to-end deferred loading + gateway execution
// =========================================================================

/// Stub gateway executor used in the loop test. Records every dispatch so the
/// test can prove an injected tool routed through the gateway rather than
/// failing with UnknownTool.
#[derive(Clone, Default)]
struct RecordingGatewayExecutor {
    dispatched: Arc<Mutex<Vec<String>>>,
}

impl ToolExecutor for RecordingGatewayExecutor {
    fn definition(&self) -> ToolDefinition {
        schema_definition(
            "stub_gateway",
            "Stub gateway executor for tests.",
            ToolExecutionClass::NestedWorkflow,
            &[],
        )
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let name = call.tool_id.as_str().to_owned();
        let dispatched = Arc::clone(&self.dispatched);
        Box::pin(async move {
            dispatched.lock().unwrap().push(name.clone());
            Ok(ToolResult::new(format!("gateway executed `{name}`")).expect("bounded"))
        })
    }
}

/// Discovery service that injects a brand-new tool whose name matches a
/// gateway prefix registered alongside the registry.
struct DiscoverAndInjectService {
    injected_name: String,
}

impl ToolService for DiscoverAndInjectService {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![schema_definition(
            "discover",
            "Discover and inject a new tool into the loop.",
            ToolExecutionClass::ReadOnly,
            &[],
        )]
    }

    fn execute<'a>(
        &'a self,
        _call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        let injected_name = self.injected_name.clone();
        Box::pin(async move {
            let injected = schema_definition(
                &injected_name,
                "A tool surfaced by discover; routes through the gateway.",
                ToolExecutionClass::NestedWorkflow,
                &[("input", "string", true)],
            );
            Ok(ToolResult::new(format!("discovered {injected_name}"))
                .expect("bounded")
                .with_injected_tools(vec![injected]))
        })
    }
}

#[tokio::test]
async fn loop_calls_an_injected_tool_through_the_gateway_on_the_next_turn() {
    let injected_name = "stub__runtime_tool".to_owned();

    let discover_call = ToolCall {
        id: ToolCallId::new("call-discover").unwrap(),
        tool_id: ToolId::new("discover").unwrap(),
        arguments: json!({}),
        extensions: ExtensionMap::default(),
    };
    let injected_call = ToolCall {
        id: ToolCallId::new("call-injected").unwrap(),
        tool_id: ToolId::new(&injected_name).unwrap(),
        arguments: json!({"input": "hello"}),
        extensions: ExtensionMap::default(),
    };

    // Turn 1: model calls `discover`, which injects `stub__runtime_tool`.
    let turn_discover: ScriptedProviderResponse = Ok(vec![
        Ok(ProviderStreamEvent::ToolCallCompleted(discover_call)),
        Ok(completed(FinishOutcome::ToolCalls)),
    ]);
    // Turn 2: model calls the freshly-advertised injected tool.
    let turn_injected: ScriptedProviderResponse = Ok(vec![
        Ok(ProviderStreamEvent::ToolCallCompleted(injected_call)),
        Ok(completed(FinishOutcome::ToolCalls)),
    ]);
    // Turn 3: model stops with no further tool calls.
    let turn_done: ScriptedProviderResponse = Ok(vec![Ok(completed(FinishOutcome::Stop))]);

    let provider_id = provider();
    let fake = FakeProviderSession::with_scripts([turn_discover, turn_injected, turn_done]);
    let recorded = fake.clone();
    let registry = Arc::new(ProviderRegistry::new());
    registry
        .register(FakeFactory {
            id: provider_id.clone(),
            session: fake,
        })
        .await
        .unwrap();

    let gateway_dispatched: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let gateway: Arc<dyn ToolExecutor> = Arc::new(RecordingGatewayExecutor {
        dispatched: Arc::clone(&gateway_dispatched),
    });
    let tools = ToolRegistry::parity_default()
        .with_service(Arc::new(DiscoveringWrapper(Arc::new(
            DiscoverAndInjectService {
                injected_name: injected_name.clone(),
            },
        ))))
        .with_gateway("stub__", gateway);

    let progress = Arc::new(RecordingProgressPort::default());
    let agent = AgentLoop::new(registry, tools, config(&provider_id, 10))
        .with_progress_port(progress.clone());
    let outcome = agent
        .run_prompt(
            user_message("discover then call the new tool"),
            SessionOperatingMode::Code,
            SessionPermissionMode::Bypass,
        )
        .await
        .expect("loop must complete");

    // Three turns: discover → injected → stop.
    assert!(
        matches!(outcome, AgentTurnOutcome::Completed { iterations: 3, .. }),
        "expected the loop to run three iterations, got {outcome:?}"
    );

    // The injected tool must have been advertised on turn 2 (not turn 1).
    let requests = recorded.requests();
    assert_eq!(requests.len(), 3, "three provider turns must be recorded");
    let turn1_names: Vec<&str> = requests[0]
        .tools
        .iter()
        .map(|definition| definition.harness_name.as_str())
        .collect();
    let turn2_names: Vec<&str> = requests[1]
        .tools
        .iter()
        .map(|definition| definition.harness_name.as_str())
        .collect();
    assert!(
        !turn1_names.contains(&injected_name.as_str()),
        "injected tool must NOT be advertised on turn 1: {turn1_names:?}"
    );
    assert!(
        turn2_names.contains(&injected_name.as_str()),
        "injected tool MUST be advertised on turn 2: {turn2_names:?}"
    );

    // The injected tool must have been executed via the gateway, not refused
    // as UnknownTool. The progress port records both ToolStarted and
    // ToolFinished; a gateway-routed call surfaces as `success: true`.
    let events = progress.events.lock().unwrap();
    let started_injected = events.iter().any(|event| {
        matches!(
            event,
            AgentProgressEvent::ToolStarted { name, .. } if name == &injected_name
        )
    });
    assert!(
        started_injected,
        "the injected tool must have been started: {events:?}"
    );
    let finished_injected_ok = events.iter().any(|event| {
        matches!(
            event,
            AgentProgressEvent::ToolFinished { name, success: true, .. } if name == &injected_name
        )
    });
    assert!(
        finished_injected_ok,
        "the injected tool must have finished successfully (no UnknownTool): {events:?}"
    );

    // Belt-and-suspenders: the gateway executor recorded exactly one dispatch
    // for the injected name.
    let dispatched = gateway_dispatched.lock().unwrap().clone();
    assert_eq!(
        dispatched,
        vec![injected_name.clone()],
        "the gateway executor must have dispatched exactly the injected tool name once: {dispatched:?}"
    );
}

/// Thin wrapper that delegates to an inner `ToolService`. Used so the
/// end-to-end test can keep `DiscoverAndInjectService` parameterized while
/// still being constructable as `Arc<dyn ToolService>`.
struct DiscoveringWrapper(Arc<DiscoverAndInjectService>);

impl ToolService for DiscoveringWrapper {
    fn definitions(&self) -> Vec<ToolDefinition> {
        self.0.definitions()
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        self.0.execute(call, context)
    }
}
