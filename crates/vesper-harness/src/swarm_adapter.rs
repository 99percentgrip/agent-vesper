//! Native swarm execution through the existing AgentLoop, feature `swarm`.
//!
//! The host injects its real registry/configuration, tool executors, permission
//! channel and optional progress channel. Each task gets an independent runtime
//! session through AgentLoop; this adapter owns no provider-wire or tool loop.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::WorkerFactory;
use futures_util::future::BoxFuture;
use vesper_agent::{AgentLoop, AgentProgressPort, AgentTurnOutcome, PermissionPort, ToolRegistry};
use vesper_domain::{
    ContentPart, ContentText, ConversationMessage, MessageId, MessageRole, SessionOperatingMode,
    SessionPermissionMode,
};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

struct CancellationBridge(CancellationSignal);
impl vesper_provider::CancellationSignal for CancellationBridge {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}
struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// One independent worker instance. Concurrent use of the same instance is
/// refused; hosts must create separate instances for simultaneous worker turns.
pub struct ProviderWorkerPort {
    factory: WorkerFactory,
    tools: ToolRegistry,
    mode: SessionOperatingMode,
    permission: SessionPermissionMode,
    permission_port: Arc<dyn PermissionPort>,
    progress_port: Option<Arc<dyn AgentProgressPort>>,
    capabilities: WorkerCapabilities,
    busy: AtomicBool,
    history: Mutex<Vec<ConversationMessage>>,
}
impl ProviderWorkerPort {
    /// Restricts both advertising and execution to registered role tools.
    /// The supplied permission mode/port and sandbox/firewall configuration are
    /// inherited unchanged. Requirements never grant permission.
    pub fn new(
        factory: WorkerFactory,
        tools: ToolRegistry,
        allowed_tools: Vec<String>,
        mode: SessionOperatingMode,
        permission: SessionPermissionMode,
        permission_port: Arc<dyn PermissionPort>,
    ) -> Self {
        let tools = tools.restricted_to(&allowed_tools);
        let capabilities = WorkerCapabilities {
            tools: allowed_tools
                .into_iter()
                .filter(|name| tools.contains(name))
                .collect(),
            max_concurrent_tasks: 1,
        };
        Self {
            factory,
            tools,
            mode,
            permission,
            permission_port,
            progress_port: None,
            capabilities,
            busy: AtomicBool::new(false),
            history: Mutex::new(Vec::new()),
        }
    }

    /// Connects partial output and tool progress to the host's existing sink.
    #[must_use]
    pub fn with_progress_port(mut self, port: Arc<dyn AgentProgressPort>) -> Self {
        self.progress_port = Some(port);
        self
    }

    /// Last completed or interrupted native history, including tool transactions.
    /// No filesystem persistence is performed; durable checkpoints remain host opt-in.
    #[must_use]
    pub fn history(&self) -> Vec<ConversationMessage> {
        self.history.lock().expect("worker history lock").clone()
    }
}
impl WorkerPort for ProviderWorkerPort {
    fn capabilities(&self) -> WorkerCapabilities {
        self.capabilities.clone()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled(task.id.clone()));
            }
            if !self.capabilities.supports(&task.required_capabilities)
                || self
                    .busy
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return Err(WorkerError::Rejected(task.id.clone()));
            }
            let _busy = BusyGuard(&self.busy);
            let failure = |reason: String| WorkerError::Failed(task.id.clone(), reason);
            let message = ConversationMessage {
                id: MessageId::new(format!("swarm-{}", task.id))
                    .map_err(|error| failure(error.to_string()))?,
                role: MessageRole::User,
                content: vec![ContentPart::Text(
                    ContentText::new(task.prompt.clone())
                        .map_err(|error| failure(error.to_string()))?,
                )],
                extensions: Default::default(),
            };
            self.history.lock().expect("worker history lock").clear();
            let tools = if task.required_capabilities.is_empty() {
                self.tools.clone()
            } else {
                self.tools.restricted_to(&task.required_capabilities)
            };
            let mut engine = AgentLoop::new(
                self.factory.registry.clone(),
                tools,
                self.factory.config.clone(),
            )
            .with_permission_port(self.permission_port.clone());
            if let Some(port) = &self.progress_port {
                engine = engine.with_progress_port(port.clone());
            }
            let started = std::time::Instant::now();
            let (outcome, history) = engine
                .run_prompt_with_history_with_cancellation(
                    vec![message],
                    self.mode,
                    self.permission,
                    Arc::new(CancellationBridge(cancellation.clone())),
                )
                .await
                .map_err(|error| failure(error.to_string()))?;
            *self.history.lock().expect("worker history lock") = history;
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled(task.id.clone()));
            }
            let (content, success) = match outcome {
                AgentTurnOutcome::Completed {
                    assistant_content, ..
                } => (assistant_content, true),
                AgentTurnOutcome::Interrupted {
                    assistant_content, ..
                } => (assistant_content, false),
                AgentTurnOutcome::MaxIterationsReached { .. } => {
                    return Err(failure(
                        "native agent iteration safety ceiling reached".into(),
                    ));
                }
            };
            let mut output = String::new();
            for part in content {
                if let ContentPart::Text(text) = part {
                    if output.len().saturating_add(text.as_str().len()) > 1_048_576 {
                        return Err(failure("worker output byte limit exceeded".into()));
                    }
                    output.push_str(text.as_str());
                }
            }
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output,
                success,
                duration: started.elapsed(),
            })
        })
    }
}

/// Native factory recipe. It shares registry/configuration and permission/progress
/// services, never an instance's busy flag or conversation history. Provider
/// sessions are opened lazily by the existing AgentLoop at turn execution.
pub struct ProviderWorkerInstanceFactory {
    template: ProviderWorkerPort,
    sandbox: Option<(Arc<crate::swarm_sandbox::NativeSandboxLeases>, String)>,
}
impl ProviderWorkerPort {
    /// Consumes this worker as a configuration recipe for independent pool slots.
    #[must_use]
    pub fn into_instance_factory(self) -> ProviderWorkerInstanceFactory {
        ProviderWorkerInstanceFactory {
            template: self,
            sandbox: None,
        }
    }
}
impl ProviderWorkerInstanceFactory {
    /// Bind every concrete instance (including scale/replacement) to a fresh
    /// worker root and the shared native lease book. The host's permission port
    /// remains unchanged; a sandbox scope does not authorize any tool call.
    #[must_use]
    pub fn with_sandbox_leases(
        mut self,
        leases: Arc<crate::swarm_sandbox::NativeSandboxLeases>,
        role: String,
    ) -> Self {
        self.sandbox = Some((leases, role));
        self
    }
}
impl vesper_swarm::pool::WorkerInstanceFactory for ProviderWorkerInstanceFactory {
    fn capabilities(&self) -> WorkerCapabilities {
        self.template.capabilities.clone()
    }
    fn create<'a>(
        &'a self,
        id: u64,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled(format!("boot-{id}")));
            }
            let template = &self.template;
            let mut worker = ProviderWorkerPort::new(
                template.factory.clone(),
                template.tools.clone(),
                template.capabilities.tools.clone(),
                template.mode,
                template.permission,
                template.permission_port.clone(),
            );
            worker.progress_port = template.progress_port.clone();
            if let Some((leases, role)) = &self.sandbox {
                let (root, route) = leases
                    .worker(role, id, cancellation.clone())
                    .await
                    .map_err(|error| {
                        WorkerError::Failed(format!("boot-{id}"), error.to_string())
                    })?;
                worker.factory.config.workspace_roots = vec![vesper_domain::WorkspaceRoot {
                    name: vesper_domain::BoundedString::new(format!("{role}-{id}")).map_err(
                        |error| WorkerError::Failed(format!("boot-{id}"), error.to_string()),
                    )?,
                    path: vesper_domain::BoundedString::new(root.to_string_lossy().into_owned())
                        .map_err(|error| {
                            WorkerError::Failed(format!("boot-{id}"), error.to_string())
                        })?,
                    primary: true,
                }];
                worker.factory.config.sandbox = Some(route);
            }
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled(format!("boot-{id}")));
            }
            Ok(Arc::new(worker) as Arc<dyn WorkerPort>)
        })
    }
}
