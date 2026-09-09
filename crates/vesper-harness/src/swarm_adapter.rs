//! Provider-facing WorkerPort adapter (VRO-15 PR-9).
//!
//! Bridges [`vesper_swarm::worker::WorkerPort`] to Vesper's real
//! provider-neutral execution seams: one bounded turn maps to one
//! [`ProviderSession::start`] stream, and the task's required
//! capabilities filter the tool registry per task. This is the
//! composition boundary — the only place the swarm meets providers.
//!
//! Feature-gated: the entire module compiles only under the harness's
//! default-off `swarm` feature, so the single-agent ReAct loop, the TUI,
//! and the ACP host build and behave identically without it.

use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use vesper_domain::{
    ContentPart, ConversationMessage, MessageId, MessageRole, ProviderId, ProviderRequestId,
    QualifiedModelId, SystemInstruction,
};
use vesper_provider::{
    CancellationSignal as ProviderCancellation, FallbackPolicy, ProviderError, ProviderEventStream,
    ProviderRequest, ProviderSession, ProviderStreamEvent, StructuredOutputIntent, ToolChoice,
};
use vesper_swarm::worker::{
    CancellationSignal as SwarmCancellation, TurnReceipt, WorkerCapabilities, WorkerError,
    WorkerPort, WorkerTask,
};

/// Shared session + model identity for swarm turns.
#[derive(Clone)]
pub struct SwarmSession {
    /// The provider-neutral session adapters expose.
    pub session: Arc<dyn ProviderSession>,
    /// Provider identity for requests.
    pub provider_id: ProviderId,
    /// The qualified model every swarm turn uses.
    pub model: QualifiedModelId,
    /// System instructions every swarm turn receives (role instructions
    /// are appended per task).
    pub system_instructions: Vec<SystemInstruction>,
    /// Full tool registry; filtered per task by required capabilities.
    pub registry: Vec<vesper_domain::ToolDefinition>,
}

/// Maps the swarm's cooperative cancellation to the provider's.
struct SwarmCancellationBridge {
    signal: SwarmCancellation,
}

impl ProviderCancellation for SwarmCancellationBridge {
    fn is_cancelled(&self) -> bool {
        self.signal.is_cancelled()
    }
}

/// The adapter: one `WorkerPort` implementation per hive class.
pub struct ProviderWorkerPort {
    session: SwarmSession,
    /// Tool names this class may use (the per-task filter applied on
    /// top of the registry).
    allowed_tools: Vec<String>,
}

impl ProviderWorkerPort {
    /// Creates an adapter for one role class.
    pub fn new(session: SwarmSession, allowed_tools: Vec<String>) -> Self {
        Self {
            session,
            allowed_tools,
        }
    }

    /// Registry filtered to the intersection of the class allowlist and
    /// the task's required capabilities (dynamic per-task filtering).
    fn filtered_tools(&self, task: &WorkerTask) -> Vec<vesper_domain::ToolDefinition> {
        self.session
            .registry
            .iter()
            .filter(|definition| {
                let name = definition.harness_name.as_str();
                self.allowed_tools.iter().any(|allowed| allowed == name)
                    && (task.required_capabilities.is_empty()
                        || task
                            .required_capabilities
                            .iter()
                            .any(|required| required == name))
            })
            .cloned()
            .collect()
    }

    fn build_request(&self, task: &WorkerTask) -> ProviderRequest {
        let request_id = ProviderRequestId::new(format!("swarm-{}", task.id))
            .unwrap_or_else(|_| ProviderRequestId::new("swarm-turn").expect("non-empty"));
        ProviderRequest {
            request_id,
            provider_id: self.session.provider_id.clone(),
            model: self.session.model.clone(),
            endpoint_id: None,
            system_instructions: self.session.system_instructions.clone(),
            messages: vec![ConversationMessage {
                id: MessageId::new(format!("swarm-msg-{}", task.id))
                    .unwrap_or_else(|_| MessageId::new("swarm-msg").expect("non-empty")),
                role: MessageRole::User,
                content: vec![ContentPart::Text(
                    vesper_domain::ContentText::new(task.prompt.clone())
                        .expect("bounded task prompt"),
                )],
                extensions: Default::default(),
            }],
            tools: self.filtered_tools(task),
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
}

impl WorkerPort for ProviderWorkerPort {
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: SwarmCancellation,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled(task.id.clone()));
            }
            let request = self.build_request(task);
            let bridge = Arc::new(SwarmCancellationBridge {
                signal: cancellation,
            });
            let started = std::time::Instant::now();
            let stream: ProviderEventStream = self
                .session
                .session
                .start(request, bridge)
                .await
                .map_err(map_provider_error(task))?;
            let mut output = String::new();
            let mut success = false;
            let mut stream = stream;
            while let Some(event) = stream.next().await {
                match event {
                    Ok(ProviderStreamEvent::ContentDelta { part, .. }) => {
                        // Accumulate visible text parts only.
                        if let ContentPart::Text(text) = part {
                            output.push_str(text.as_str());
                        }
                    }
                    Ok(ProviderStreamEvent::Completed { .. }) => {
                        success = true;
                    }
                    Ok(_) => {}
                    Err(error) => return Err(map_provider_error(task)(error)),
                }
            }
            let duration = started.elapsed();
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output,
                success,
                duration,
            })
        })
    }

    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities {
            tools: self.allowed_tools.clone(),
            max_concurrent_tasks: 1,
        }
    }
}

fn map_provider_error(task: &WorkerTask) -> impl Fn(ProviderError) -> WorkerError + '_ {
    move |error: ProviderError| WorkerError::Failed(task.id.clone(), error.to_string())
}
