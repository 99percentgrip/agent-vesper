//! Native swarm execution through the existing AgentLoop, feature `swarm`.
//!
//! The host injects its real registry/configuration, tool executors, permission
//! channel and optional progress channel. Each task gets an independent runtime
//! session through AgentLoop; this adapter owns no provider-wire or tool loop.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::WorkerFactory;
use futures_util::future::BoxFuture;
use vesper_agent::{AgentLoop, AgentProgressPort, AgentTurnOutcome, PermissionPort, ToolRegistry};
use vesper_domain::{
    ContentPart, ContentText, ConversationMessage, MessageId, MessageRole, SessionOperatingMode,
    SessionPermissionMode,
};
use vesper_swarm::worker::{
    CancelFlag, CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort,
    WorkerTask,
};

struct WorkerHistory(Arc<Mutex<Vec<ConversationMessage>>>);
impl vesper_agent::AgentHistoryPort for WorkerHistory {
    fn checkpoint(&self, history: &[ConversationMessage]) {
        *self.0.lock().expect("worker history") = history.to_vec();
    }
}

struct WorkerProgress {
    turns: Arc<AtomicUsize>,
    downstream: Option<Arc<dyn AgentProgressPort>>,
}
impl AgentProgressPort for WorkerProgress {
    fn emit(&self, event: vesper_agent::AgentProgressEvent) {
        if matches!(
            event,
            vesper_agent::AgentProgressEvent::ProviderTurnStarted { .. }
        ) {
            self.turns.fetch_add(1, Ordering::AcqRel);
        }
        if let Some(port) = &self.downstream {
            port.emit(event);
        }
    }
}

struct CancellationBridge(CancellationSignal, CancellationSignal);
impl vesper_provider::CancellationSignal for CancellationBridge {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled() || self.1.is_cancelled()
    }
}
struct BusyGuard(Arc<AtomicBool>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// One independent worker instance. Concurrent use of the same instance is
/// refused; hosts must create separate instances for simultaneous worker turns.
#[derive(Clone)]
pub struct ProviderWorkerPort {
    factory: WorkerFactory,
    tools: ToolRegistry,
    mode: SessionOperatingMode,
    permission: SessionPermissionMode,
    permission_port: Arc<dyn PermissionPort>,
    progress_port: Option<Arc<dyn AgentProgressPort>>,
    journal: Option<(Arc<crate::swarm_journal::NativeWorkerJournal>, String)>,
    context: Option<String>,
    capabilities: WorkerCapabilities,
    busy: Arc<AtomicBool>,
    history: Arc<Mutex<Vec<ConversationMessage>>>,
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
            journal: None,
            context: None,
            busy: Arc::new(AtomicBool::new(false)),
            history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Connects partial output and tool progress to the host's existing sink.
    #[must_use]
    pub fn with_progress_port(mut self, port: Arc<dyn AgentProgressPort>) -> Self {
        self.progress_port = Some(port);
        self
    }

    #[must_use]
    pub fn with_journal(
        mut self,
        journal: Arc<crate::swarm_journal::NativeWorkerJournal>,
        worker: String,
    ) -> Self {
        self.journal = Some((journal, worker));
        self
    }
    /// Bounded, untrusted memory/reference context for this run only.
    pub fn with_context(mut self, context: Option<String>) -> Result<Self, String> {
        if context.as_ref().is_some_and(|text| text.len() > 65536) {
            return Err("Worker recalled context exceeds 64 KiB.".into());
        }
        self.context = context;
        Ok(self)
    }
    /// Last completed or interrupted native history, including tool transactions.
    /// No filesystem persistence is performed; durable checkpoints remain host opt-in.
    #[must_use]
    pub fn history(&self) -> Vec<ConversationMessage> {
        self.history.lock().expect("worker history lock").clone()
    }
}
struct AbandonTurn(CancelFlag);
impl Drop for AbandonTurn {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl WorkerPort for ProviderWorkerPort {
    fn pending_work(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

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
            let worker = self.clone();
            let owned_task = task.clone();
            let busy = BusyGuard(self.busy.clone());
            self.history.lock().expect("worker history").clear();
            let turns = Arc::new(AtomicUsize::new(0));
            let journal = self.journal.as_ref().map(|(journal, worker)| {
                journal.begin(
                    worker.clone(),
                    task.id.clone(),
                    self.history.clone(),
                    turns.clone(),
                )
            });
            let abandon = AbandonTurn(CancelFlag::new());
            let abandoned = abandon.0.signal();
            tokio::spawn(async move {
                let _busy = busy;
                let _journal = journal;
                let task = &owned_task;

                let failure = |reason: String| WorkerError::Failed(task.id.clone(), reason);
                let message = ConversationMessage {
                    id: MessageId::new(format!("swarm-{}", task.id))
                        .map_err(|error| failure(error.to_string()))?,
                    role: MessageRole::User,
                    content: vec![ContentPart::Text(
                        ContentText::new(match &worker.context {
                            Some(context) => format!("{}\n\n<untrusted-recalled-context>\n{}\n</untrusted-recalled-context>", task.prompt, context),
                            None => task.prompt.clone(),
                        })
                            .map_err(|error| failure(error.to_string()))?,
                    )],
                    extensions: Default::default(),
                };
                *worker.history.lock().expect("worker history lock") = vec![message.clone()];
                let tools = if task.kind == vesper_swarm::worker::TaskKind::Review {
                    // VRO-16 D3: review-panel turns are evaluation-only.
                    // A judge evaluates artifacts against evidence; it does
                    // not author, mutate, or re-execute work — so it gets
                    // no tools, structurally.
                    ToolRegistry::empty()
                } else if task.required_capabilities.is_empty() {
                    worker.tools.clone()
                } else {
                    worker.tools.restricted_to(&task.required_capabilities)
                };
                let mut engine = AgentLoop::new(
                    worker.factory.registry.clone(),
                    tools,
                    worker.factory.config.clone(),
                )
                .with_permission_port(worker.permission_port.clone())
                .with_history_port(Arc::new(WorkerHistory(worker.history.clone())));
                engine = engine.with_progress_port(Arc::new(WorkerProgress {
                    turns,
                    downstream: worker.progress_port.clone(),
                }));
                let started = std::time::Instant::now();
                let (outcome, history) = engine
                    .run_prompt_with_history_with_cancellation(
                        vec![message],
                        worker.mode,
                        worker.permission,
                        Arc::new(CancellationBridge(cancellation.clone(), abandoned.clone())),
                    )
                    .await
                    .map_err(|error| failure(error.to_string()))?;
                *worker.history.lock().expect("worker history lock") = history;
                if cancellation.is_cancelled() || abandoned.is_cancelled() {
                    return Err(WorkerError::Cancelled(task.id.clone()));
                }
                let (content, success) = match outcome {
                    AgentTurnOutcome::Acceptance { report, .. } => (vec![ContentPart::Text(vesper_domain::ContentText::new(report.render()).map_err(|_| failure("acceptance report too large".into()))?)], report.is_verified()),
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
            .await
            .map_err(|_| WorkerError::Failed(task.id.clone(), "native worker task failed".into()))?
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
            worker.context = template.context.clone();
            worker.journal = template
                .journal
                .as_ref()
                .map(|(journal, role)| (journal.clone(), format!("{role}-{id}")));
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
