//! Integration tests for the Tier C agent loop (ADR 0010, Phase 2).
//!
//! Uses `vesper-testkit`'s `FakeProviderSession` registered behind a
//! `ProviderFactory` in a real `vesper-runtime::ProviderRegistry`, so the loop
//! dispatches through the same composition seam production code uses.

use std::sync::{Arc, Mutex};

use serde_json::json;
use vesper_agent::{
    AgentLoop, AgentLoopConfig, AgentProgressEvent, AgentProgressPort, AgentTurnOutcome,
    DEFAULT_MAX_TOOL_ITERATIONS, ToolRegistry,
};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ConversationMessage, ExtensionMap, FinishOutcome,
    MessageId, MessageRole, MessageRole::User, ProviderId, QualifiedModelId, SchemaVersion,
    SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId,
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

fn content_delta(text: &str) -> ProviderStreamEvent {
    ProviderStreamEvent::ContentDelta {
        stream_id: BoundedString::new("content").unwrap(),
        part: ContentPart::Text(ContentText::new(text).unwrap()),
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
        AgentProgressEvent::ToolStarted { name } if name == "read_file"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentProgressEvent::ToolFinished { name, success: true } if name == "read_file"
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
