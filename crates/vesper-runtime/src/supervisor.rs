use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use futures_util::StreamExt;
use tokio::{
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
};
use vesper_domain::{
    BoundedString, ContentPart, CorrelationId, EndpointId, EventId, EventSchemaVersion,
    EventSequence, ExtensionMap, FinishOutcome, HarnessCommand, HarnessCommandPayload,
    HarnessEvent, HarnessEventPayload, MessageId, MessageRole, ProviderRequestId, QualifiedModelId,
    Revision, RuntimeAuthenticationMethod, RuntimeCapability, SafeMessage, SessionId,
    SessionListFilter, SessionOperatingMode, SessionPermissionMode, SessionSummary,
    SystemInstruction, ToolChoiceIntent, TurnId, WorkspaceRoot,
};
use vesper_provider::{
    FallbackPolicy, ProviderConfiguration, ProviderRequest, ProviderStreamContract,
    ProviderStreamEvent, ReasoningIntent, SamplingIntent, StructuredOutputIntent,
};
use vesper_sessions::{
    SessionListFilter as PersistentSessionListFilter, SessionReadIntent, SessionSource,
};

use crate::{
    ProviderRegistry, RuntimeCancellation, RuntimeError, RuntimeSessionReads, RuntimeSessionWrites,
    SessionSnapshot, SessionTurnResult, WriteOutcome,
};

const EVENT_CAPACITY: usize = 64;
const SESSION_COMMAND_CAPACITY: usize = 16;

/// Provider/model defaults injected by the composition boundary.
#[derive(Debug, Clone)]
pub struct RuntimeDefaults {
    /// Provider-owned configuration.
    pub provider_configuration: ProviderConfiguration,
    /// Provider-qualified model.
    pub model: QualifiedModelId,
    /// Default endpoint reference assigned to freshly created sessions so the
    /// persisted record carries a stable endpoint identity. The runtime is
    /// provider-neutral; the composition boundary supplies the concrete value.
    pub endpoint: EndpointId,
    /// Minimal ordered system instructions.
    pub system_instructions: Vec<SystemInstruction>,
    /// Optional reasoning intent.
    pub reasoning: Option<ReasoningIntent>,
    /// Optional generation controls.
    pub sampling: Option<SamplingIntent>,
    /// Optional output bound.
    pub maximum_output_tokens: Option<u64>,
}

/// Single-consumer bounded runtime event receiver.
pub struct RuntimeEventReceiver {
    receiver: mpsc::Receiver<HarnessEvent>,
}

impl RuntimeEventReceiver {
    /// Receives the next event.
    pub async fn recv(&mut self) -> Option<HarnessEvent> {
        self.receiver.recv().await
    }
}

/// Correlated result for a runtime command.
#[derive(Debug, Clone)]
pub enum RuntimeResponse {
    /// Runtime initialization accepted.
    Initialized,
    /// Session state snapshot.
    Session(Box<SessionSnapshot>),
    /// Ordered session list.
    Sessions(Vec<SessionSummary>),
    /// Prompt began asynchronously.
    PromptStarted {
        /// Assigned turn.
        turn_id: TurnId,
        /// Completion receiver.
        completion: PromptCompletion,
    },
    /// Command completed without a payload.
    Accepted,
    /// Runtime shut down.
    Shutdown,
}

type PromptCompletion =
    Arc<Mutex<Option<oneshot::Receiver<Result<SessionTurnResult, RuntimeError>>>>>;

impl RuntimeResponse {
    /// Waits for a prompt completion response exactly once.
    pub async fn wait_prompt(&self) -> Result<SessionTurnResult, RuntimeError> {
        let Self::PromptStarted { completion, .. } = self else {
            return Err(RuntimeError::UnsupportedCommand);
        };
        let receiver = completion
            .lock()
            .await
            .take()
            .ok_or(RuntimeError::ChannelClosed)?;
        receiver.await.map_err(|_| RuntimeError::ChannelClosed)?
    }
}

struct SessionHandle {
    sender: mpsc::Sender<SessionCommand>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl SessionHandle {
    async fn request(
        &self,
        command: SessionCommand,
    ) -> Result<(), mpsc::error::SendError<SessionCommand>> {
        self.sender.send(command).await
    }
}

/// Minimal runtime supervisor with tracked actor ownership.
pub struct RuntimeSupervisor {
    providers: Arc<ProviderRegistry>,
    defaults: RuntimeDefaults,
    sessions: RwLock<BTreeMap<SessionId, Arc<SessionHandle>>>,
    event_sender: mpsc::Sender<HarnessEvent>,
    event_receiver: Mutex<Option<mpsc::Receiver<HarnessEvent>>>,
    cancellation: RuntimeCancellation,
    ids: Arc<AtomicU64>,
    control_sequence: AtomicU64,
    session_reads: Option<Arc<RuntimeSessionReads>>,
    session_writes: Option<Arc<RuntimeSessionWrites>>,
    load_gates: Mutex<BTreeMap<SessionId, Arc<Mutex<()>>>>,
}

impl std::fmt::Debug for RuntimeSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeSupervisor")
            .field("defaults", &self.defaults)
            .finish_non_exhaustive()
    }
}

impl RuntimeSupervisor {
    /// Creates a runtime. The event receiver can be taken once.
    #[must_use]
    pub fn new(providers: Arc<ProviderRegistry>, defaults: RuntimeDefaults) -> Self {
        let (event_sender, event_receiver) = mpsc::channel(EVENT_CAPACITY);
        Self {
            providers,
            defaults,
            sessions: RwLock::new(BTreeMap::new()),
            event_sender,
            event_receiver: Mutex::new(Some(event_receiver)),
            cancellation: RuntimeCancellation::new(),
            ids: Arc::new(AtomicU64::new(1)),
            control_sequence: AtomicU64::new(0),
            session_reads: None,
            session_writes: None,
            load_gates: Mutex::new(BTreeMap::new()),
        }
    }

    /// Injects the optional read-only persistence path.
    #[must_use]
    pub fn with_session_reads(mut self, session_reads: Arc<RuntimeSessionReads>) -> Self {
        self.session_reads = Some(session_reads);
        self
    }

    /// Injects the optional transactional persistence write path.
    ///
    /// When present, `save_session` persists any live actor snapshot as a
    /// version-1 Agent Vesper record. The runtime itself performs no filesystem
    /// I/O; all writes flow through the injected writer port.
    #[must_use]
    pub fn with_session_writes(mut self, session_writes: Arc<RuntimeSessionWrites>) -> Self {
        self.session_writes = Some(session_writes);
        self
    }

    /// Takes the one bounded event stream.
    pub async fn take_events(&self) -> Result<RuntimeEventReceiver, RuntimeError> {
        self.event_receiver
            .lock()
            .await
            .take()
            .map(|receiver| RuntimeEventReceiver { receiver })
            .ok_or(RuntimeError::ChannelClosed)
    }

    /// Validates that the configured provider can resolve its configuration and
    /// credentials without dispatching a model request.
    pub async fn validate_provider_authentication(&self) -> Result<(), RuntimeError> {
        let cancellation: Arc<dyn vesper_provider::CancellationSignal> =
            Arc::new(self.cancellation.child());
        self.providers
            .create_session(
                &self.defaults.model.provider_id,
                &self.defaults.provider_configuration,
                cancellation,
            )
            .await
            .map(|_| ())
    }

    /// Routes an existing shared command contract.
    pub async fn execute(&self, command: HarnessCommand) -> Result<RuntimeResponse, RuntimeError> {
        if self.cancellation.is_cancelled() {
            return Err(RuntimeError::ShuttingDown);
        }
        match command.payload {
            HarnessCommandPayload::InitializeRuntime(_) => {
                self.emit_runtime(
                    command.correlation_id,
                    HarnessEventPayload::RuntimeInitialized {
                        capabilities: supported_capabilities(),
                        authentication_methods: vec![RuntimeAuthenticationMethod {
                            method_id: BoundedString::new("zai-api-key-setup")
                                .expect("static auth method"),
                            display_name: BoundedString::new("Z.ai API key")
                                .expect("static auth display"),
                            external_runtime_owned: false,
                        }],
                        metadata: ExtensionMap::default(),
                    },
                )
                .await?;
                Ok(RuntimeResponse::Initialized)
            }
            HarnessCommandPayload::CreateSession {
                workspace_roots,
                requested_session_id,
            } => {
                let id = requested_session_id.unwrap_or_else(|| self.next_session_id());
                self.create_session(id, workspace_roots, command.correlation_id)
                    .await
            }
            HarnessCommandPayload::LoadSession {
                session_id,
                workspace_roots,
            } => {
                self.load_or_resume(
                    session_id,
                    workspace_roots,
                    SessionReadIntent::Load,
                    command.correlation_id,
                )
                .await
            }
            HarnessCommandPayload::ResumeSession {
                session_id,
                workspace_roots,
            } => {
                self.load_or_resume(
                    session_id,
                    workspace_roots,
                    SessionReadIntent::Resume,
                    command.correlation_id,
                )
                .await
            }
            HarnessCommandPayload::ListSessions(filter) => {
                let sessions = self.list_sessions(filter).await?;
                self.emit_runtime(
                    command.correlation_id,
                    HarnessEventPayload::SessionListProduced {
                        sessions: sessions.clone(),
                    },
                )
                .await?;
                Ok(RuntimeResponse::Sessions(sessions))
            }
            HarnessCommandPayload::ForkSession {
                session_id,
                requested_session_id,
            } => {
                let child = requested_session_id.unwrap_or_else(|| self.next_session_id());
                self.fork_session(&session_id, child, command.correlation_id)
                    .await
            }
            HarnessCommandPayload::CloseSession { session_id } => {
                self.close_session(&session_id, command.correlation_id)
                    .await?;
                Ok(RuntimeResponse::Accepted)
            }
            HarnessCommandPayload::SubmitPrompt { session_id, prompt } => {
                self.submit_prompt(
                    &session_id,
                    prompt.message_id,
                    prompt.content,
                    command.correlation_id,
                    command.expected_revision,
                )
                .await
            }
            HarnessCommandPayload::CancelTurn {
                session_id,
                turn_id,
            } => {
                self.cancel_turn(&session_id, turn_id).await?;
                Ok(RuntimeResponse::Accepted)
            }
            HarnessCommandPayload::UpdateSessionMode {
                session_id,
                operating_mode,
                permission_mode,
            } => {
                self.update_mode(
                    &session_id,
                    operating_mode,
                    permission_mode,
                    command.expected_revision,
                )
                .await?;
                Ok(RuntimeResponse::Accepted)
            }
            HarnessCommandPayload::RequestRuntimeShutdown => {
                self.shutdown(command.correlation_id).await?;
                Ok(RuntimeResponse::Shutdown)
            }
            HarnessCommandPayload::ExecuteSlashCommand { .. }
            | HarnessCommandPayload::UpdateProviderConfiguration { .. }
            | HarnessCommandPayload::UpdateRuntimeConfiguration { .. }
            | HarnessCommandPayload::ProvidePermissionDecision { .. } => {
                Err(RuntimeError::UnsupportedCommand)
            }
        }
    }

    async fn create_session(
        &self,
        id: SessionId,
        roots: Vec<WorkspaceRoot>,
        correlation: CorrelationId,
    ) -> Result<RuntimeResponse, RuntimeError> {
        let snapshot = SessionSnapshot::initial(
            id.clone(),
            roots,
            self.defaults.model.provider_id.clone(),
            self.defaults.model.clone(),
            self.defaults.provider_configuration.clone(),
            self.defaults.endpoint.clone(),
        );
        self.insert_actor(snapshot.clone()).await?;
        self.emit_session(
            &id,
            None,
            correlation,
            HarnessEventPayload::SessionCreated {
                revision: snapshot.revision,
                metadata: ExtensionMap::default(),
            },
        )
        .await?;
        Ok(RuntimeResponse::Session(Box::new(snapshot)))
    }

    async fn load_or_resume(
        &self,
        id: SessionId,
        requested_roots: Vec<WorkspaceRoot>,
        intent: SessionReadIntent,
        correlation: CorrelationId,
    ) -> Result<RuntimeResponse, RuntimeError> {
        let load_gate = {
            let mut gates = self.load_gates.lock().await;
            Arc::clone(
                gates
                    .entry(id.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let load_guard = load_gate.lock().await;
        let result = self
            .load_or_resume_locked(id.clone(), requested_roots, intent, correlation)
            .await;
        drop(load_guard);
        self.release_load_gate(&id, &load_gate).await;
        result
    }

    async fn load_or_resume_locked(
        &self,
        id: SessionId,
        requested_roots: Vec<WorkspaceRoot>,
        intent: SessionReadIntent,
        correlation: CorrelationId,
    ) -> Result<RuntimeResponse, RuntimeError> {
        if self.sessions.read().await.contains_key(&id) {
            if !requested_roots.is_empty() {
                self.validate_and_apply_roots(&id, requested_roots).await?;
            }
            let snapshot = self.snapshot(&id).await?;
            self.emit_loaded(&snapshot, correlation).await?;
            return Ok(RuntimeResponse::Session(Box::new(snapshot)));
        }

        let persisted = match &self.session_reads {
            Some(reads) => reads.load(&id, intent).await?,
            None => None,
        };
        if self.sessions.read().await.contains_key(&id) {
            let snapshot = self.snapshot(&id).await?;
            self.emit_loaded(&snapshot, correlation).await?;
            return Ok(RuntimeResponse::Session(Box::new(snapshot)));
        }
        let mut snapshot = match persisted {
            Some(state) => SessionSnapshot::from_persisted(state),
            None => SessionSnapshot::initial(
                id.clone(),
                requested_roots.clone(),
                self.defaults.model.provider_id.clone(),
                self.defaults.model.clone(),
                self.defaults.provider_configuration.clone(),
                self.defaults.endpoint.clone(),
            ),
        };
        if !requested_roots.is_empty() && snapshot.source != SessionSource::InMemory {
            apply_requested_roots(&mut snapshot, requested_roots)?;
        }
        self.insert_actor(snapshot.clone()).await?;
        self.emit_loaded(&snapshot, correlation).await?;
        Ok(RuntimeResponse::Session(Box::new(snapshot)))
    }

    async fn release_load_gate(&self, id: &SessionId, gate: &Arc<Mutex<()>>) {
        let mut gates = self.load_gates.lock().await;
        if Arc::strong_count(gate) == 2
            && gates
                .get(id)
                .is_some_and(|stored| Arc::ptr_eq(stored, gate))
        {
            gates.remove(id);
        }
    }

    async fn emit_loaded(
        &self,
        snapshot: &SessionSnapshot,
        correlation: CorrelationId,
    ) -> Result<(), RuntimeError> {
        let replay_event_count = snapshot
            .replay
            .as_ref()
            .map_or(0, |plan| plan.updates().len());
        self.emit_session(
            &snapshot.session_id,
            None,
            correlation,
            HarnessEventPayload::SessionLoaded {
                revision: snapshot.revision,
                replay_event_count: u64::try_from(replay_event_count).unwrap_or(u64::MAX),
            },
        )
        .await
    }

    async fn validate_and_apply_roots(
        &self,
        session_id: &SessionId,
        roots: Vec<WorkspaceRoot>,
    ) -> Result<(), RuntimeError> {
        let mut snapshot = self.snapshot(session_id).await?;
        apply_requested_roots(&mut snapshot, roots)?;
        self.update_roots(session_id, snapshot.workspace_roots)
            .await
    }

    async fn insert_actor(&self, snapshot: SessionSnapshot) -> Result<(), RuntimeError> {
        let id = snapshot.session_id.clone();
        let mut sessions = self.sessions.write().await;
        if sessions.contains_key(&id) {
            return Err(RuntimeError::DuplicateSession(id));
        }
        let (sender, receiver) = mpsc::channel(SESSION_COMMAND_CAPACITY);
        let actor = SessionActor {
            state: snapshot,
            receiver,
            providers: Arc::clone(&self.providers),
            defaults: self.defaults.clone(),
            events: self.event_sender.clone(),
            runtime_cancellation: self.cancellation.child(),
            ids: Arc::clone(&self.ids),
            pending: VecDeque::new(),
        };
        let join = tokio::spawn(actor.run());
        sessions.insert(
            id,
            Arc::new(SessionHandle {
                sender,
                join: Mutex::new(Some(join)),
            }),
        );
        Ok(())
    }

    async fn handle(&self, id: &SessionId) -> Result<Arc<SessionHandle>, RuntimeError> {
        self.sessions
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| RuntimeError::SessionNotFound(id.clone()))
    }

    /// Returns an immutable snapshot.
    pub async fn snapshot(&self, id: &SessionId) -> Result<SessionSnapshot, RuntimeError> {
        let handle = self.handle(id).await?;
        let (sender, receiver) = oneshot::channel();
        handle
            .request(SessionCommand::Snapshot(sender))
            .await
            .map_err(|_| RuntimeError::SessionNotFound(id.clone()))?;
        receiver.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    /// Transactionally persists a live session snapshot when a write path has
    /// been injected. Returns `Ok(None)` when persistence is not configured so
    /// callers can treat saving as optional without branching on injection.
    ///
    /// Writes flow entirely through the injected `RuntimeSessionWrites` port;
    /// the supervisor performs no filesystem I/O itself and serializes one
    /// actor at a time so concurrent distinct-ID saves remain parallel.
    pub async fn save_session(&self, id: &SessionId) -> Result<Option<WriteOutcome>, RuntimeError> {
        let Some(writes) = self.session_writes.clone() else {
            return Ok(None);
        };
        let snapshot = self.snapshot(id).await?;
        writes.store_snapshot(&snapshot).await.map(Some)
    }

    async fn list_sessions(
        &self,
        filter: SessionListFilter,
    ) -> Result<Vec<SessionSummary>, RuntimeError> {
        let handles: Vec<_> = self.sessions.read().await.values().cloned().collect();
        let mut selected = BTreeMap::new();
        for handle in handles {
            let (sender, receiver) = oneshot::channel();
            if handle
                .request(SessionCommand::Snapshot(sender))
                .await
                .is_err()
            {
                continue;
            }
            let snapshot = receiver.await.map_err(|_| RuntimeError::ChannelClosed)??;
            if !filter.include_closed && snapshot.closed {
                continue;
            }
            if filter.workspace.as_ref().is_some_and(|wanted| {
                !snapshot
                    .workspace_roots
                    .iter()
                    .any(|root| root.path.as_str() == wanted.as_str())
            }) {
                continue;
            }
            selected.insert(
                snapshot.session_id.clone(),
                SessionSummary {
                    session_id: snapshot.session_id,
                    title: None,
                    revision: snapshot.revision,
                    closed: snapshot.closed,
                    metadata: {
                        let mut metadata = ExtensionMap::default();
                        if let Some(root) =
                            snapshot.workspace_roots.iter().find(|root| root.primary)
                        {
                            metadata
                                .insert("runtime:cwd", serde_json::json!(root.path.as_str()))
                                .map_err(|_| RuntimeError::ProviderStream)?;
                        }
                        metadata
                    },
                },
            );
        }
        let mut summaries = selected.into_values().collect::<Vec<_>>();
        summaries.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        let mut seen = summaries
            .iter()
            .map(|summary| summary.session_id.clone())
            .collect::<BTreeSet<_>>();
        if let Some(reads) = &self.session_reads {
            let persistent = reads
                .repository()
                .list_filtered(PersistentSessionListFilter {
                    cwd: filter
                        .workspace
                        .as_ref()
                        .map(|value| value.as_str().to_owned()),
                })
                .await
                .map_err(RuntimeError::from_session_store)?;
            for metadata in persistent {
                let session_id = metadata.session_id.clone();
                if !seen.insert(session_id.clone()) {
                    continue;
                }
                let title = metadata
                    .title
                    .map(BoundedString::new)
                    .transpose()
                    .map_err(|_| RuntimeError::PersistentSessionRejectedByBounds)?;
                let mut extensions = ExtensionMap::default();
                extensions
                    .insert("runtime:cwd", serde_json::json!(metadata.cwd))
                    .map_err(|_| RuntimeError::ProviderStream)?;
                extensions
                    .insert(
                        "runtime:source",
                        serde_json::json!(match metadata.source {
                            SessionSource::AgentVesper => "agent-vesper",
                            SessionSource::LegacyNativeGlm { .. } => "legacy",
                            _ => "memory",
                        }),
                    )
                    .map_err(|_| RuntimeError::ProviderStream)?;
                summaries.push(SessionSummary {
                    session_id,
                    title,
                    revision: Revision::new(0),
                    closed: false,
                    metadata: extensions,
                });
            }
        }
        if let Some(limit) = filter.limit {
            summaries.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        Ok(summaries)
    }

    async fn fork_session(
        &self,
        parent: &SessionId,
        child: SessionId,
        correlation: CorrelationId,
    ) -> Result<RuntimeResponse, RuntimeError> {
        let snapshot = self.snapshot(parent).await?.fork(child.clone());
        self.insert_actor(snapshot.clone()).await?;
        self.emit_session(
            &child,
            None,
            correlation,
            HarnessEventPayload::SessionCreated {
                revision: snapshot.revision,
                metadata: ExtensionMap::default(),
            },
        )
        .await?;
        Ok(RuntimeResponse::Session(Box::new(snapshot)))
    }

    async fn submit_prompt(
        &self,
        session_id: &SessionId,
        message_id: MessageId,
        content: Vec<ContentPart>,
        correlation: CorrelationId,
        expected_revision: Option<Revision>,
    ) -> Result<RuntimeResponse, RuntimeError> {
        let handle = self.handle(session_id).await?;
        let turn_id = self.next_turn_id();
        let (sender, receiver) = oneshot::channel();
        handle
            .request(SessionCommand::Prompt {
                turn_id: turn_id.clone(),
                message_id,
                content,
                correlation,
                expected_revision,
                completion: sender,
            })
            .await
            .map_err(|_| RuntimeError::SessionClosed(session_id.clone()))?;
        Ok(RuntimeResponse::PromptStarted {
            turn_id,
            completion: Arc::new(Mutex::new(Some(receiver))),
        })
    }

    async fn cancel_turn(
        &self,
        session_id: &SessionId,
        turn_id: TurnId,
    ) -> Result<(), RuntimeError> {
        let handle = self.handle(session_id).await?;
        let (sender, receiver) = oneshot::channel();
        handle
            .request(SessionCommand::Cancel {
                turn_id,
                response: sender,
            })
            .await
            .map_err(|_| RuntimeError::SessionClosed(session_id.clone()))?;
        receiver.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    async fn update_mode(
        &self,
        session_id: &SessionId,
        operating: Option<SessionOperatingMode>,
        permission: Option<SessionPermissionMode>,
        expected_revision: Option<Revision>,
    ) -> Result<(), RuntimeError> {
        let handle = self.handle(session_id).await?;
        let (sender, receiver) = oneshot::channel();
        handle
            .request(SessionCommand::UpdateMode {
                operating,
                permission,
                expected_revision,
                response: sender,
            })
            .await
            .map_err(|_| RuntimeError::SessionClosed(session_id.clone()))?;
        receiver.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    async fn update_roots(
        &self,
        session_id: &SessionId,
        roots: Vec<WorkspaceRoot>,
    ) -> Result<(), RuntimeError> {
        let handle = self.handle(session_id).await?;
        let (sender, receiver) = oneshot::channel();
        handle
            .request(SessionCommand::AddRoots {
                roots,
                response: sender,
            })
            .await
            .map_err(|_| RuntimeError::SessionClosed(session_id.clone()))?;
        receiver.await.map_err(|_| RuntimeError::ChannelClosed)?
    }

    async fn close_session(
        &self,
        session_id: &SessionId,
        correlation: CorrelationId,
    ) -> Result<(), RuntimeError> {
        let handle = self
            .sessions
            .write()
            .await
            .remove(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.clone()))?;
        let (sender, receiver) = oneshot::channel();
        handle
            .request(SessionCommand::Close(sender))
            .await
            .map_err(|_| RuntimeError::SessionClosed(session_id.clone()))?;
        receiver.await.map_err(|_| RuntimeError::ChannelClosed)??;
        if let Some(join) = handle.join.lock().await.take() {
            join.await.map_err(|_| RuntimeError::ChannelClosed)?;
        }
        self.emit_session(
            session_id,
            None,
            correlation,
            HarnessEventPayload::SessionClosed,
        )
        .await
    }

    /// Cancels and joins all session actors.
    pub async fn shutdown(&self, correlation: CorrelationId) -> Result<(), RuntimeError> {
        self.cancellation.cancel();
        let handles = std::mem::take(&mut *self.sessions.write().await);
        for (_, handle) in handles {
            let (sender, receiver) = oneshot::channel();
            let _ = handle.request(SessionCommand::Close(sender)).await;
            let _ = receiver.await;
            if let Some(join) = handle.join.lock().await.take() {
                let _ = join.await;
            }
        }
        self.emit_runtime(correlation, HarnessEventPayload::RuntimeShutdown)
            .await
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.ids.fetch_add(1, Ordering::Relaxed))
    }

    fn next_session_id(&self) -> SessionId {
        SessionId::new(self.next_id("session")).expect("generated session ID")
    }

    fn next_turn_id(&self) -> TurnId {
        TurnId::new(self.next_id("turn")).expect("generated turn ID")
    }

    async fn emit_runtime(
        &self,
        correlation: CorrelationId,
        payload: HarnessEventPayload,
    ) -> Result<(), RuntimeError> {
        self.event_sender
            .send(HarnessEvent {
                schema_version: EventSchemaVersion::CURRENT,
                event_id: EventId::new(self.next_id("event")).expect("generated event ID"),
                correlation_id: Some(correlation),
                session_id: None,
                turn_id: None,
                sequence: EventSequence::new(self.control_sequence.fetch_add(1, Ordering::Relaxed)),
                payload,
            })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)
    }

    async fn emit_session(
        &self,
        session: &SessionId,
        turn: Option<TurnId>,
        correlation: CorrelationId,
        payload: HarnessEventPayload,
    ) -> Result<(), RuntimeError> {
        self.event_sender
            .send(HarnessEvent {
                schema_version: EventSchemaVersion::CURRENT,
                event_id: EventId::new(self.next_id("event")).expect("generated event ID"),
                correlation_id: Some(correlation),
                session_id: Some(session.clone()),
                turn_id: turn,
                sequence: EventSequence::new(self.control_sequence.fetch_add(1, Ordering::Relaxed)),
                payload,
            })
            .await
            .map_err(|_| RuntimeError::ChannelClosed)
    }
}

fn apply_requested_roots(
    snapshot: &mut SessionSnapshot,
    requested: Vec<WorkspaceRoot>,
) -> Result<(), RuntimeError> {
    let requested_primary = requested.iter().find(|root| root.primary);
    let stored_primary = snapshot.workspace_roots.iter().find(|root| root.primary);
    if let (Some(requested), Some(stored)) = (requested_primary, stored_primary)
        && requested.path != stored.path
    {
        return Err(RuntimeError::PersistentSessionWorkspaceMismatch);
    }
    if let Some(primary) = stored_primary.cloned() {
        snapshot.workspace_roots = std::iter::once(primary)
            .chain(requested.into_iter().filter(|root| !root.primary))
            .collect();
    } else {
        snapshot.workspace_roots = requested;
    }
    Ok(())
}

fn supported_capabilities() -> std::collections::BTreeSet<RuntimeCapability> {
    [
        RuntimeCapability::NewSession,
        RuntimeCapability::LoadSession,
        RuntimeCapability::ResumeSession,
        RuntimeCapability::ListSessions,
        RuntimeCapability::ForkSession,
        RuntimeCapability::CloseSession,
        RuntimeCapability::Cancellation,
        RuntimeCapability::AdditionalWorkspaceRoots,
        RuntimeCapability::UsageUpdates,
    ]
    .into_iter()
    .collect()
}

enum SessionCommand {
    Snapshot(oneshot::Sender<Result<SessionSnapshot, RuntimeError>>),
    Prompt {
        turn_id: TurnId,
        message_id: MessageId,
        content: Vec<ContentPart>,
        correlation: CorrelationId,
        expected_revision: Option<Revision>,
        completion: oneshot::Sender<Result<SessionTurnResult, RuntimeError>>,
    },
    Cancel {
        turn_id: TurnId,
        response: oneshot::Sender<Result<(), RuntimeError>>,
    },
    UpdateMode {
        operating: Option<SessionOperatingMode>,
        permission: Option<SessionPermissionMode>,
        expected_revision: Option<Revision>,
        response: oneshot::Sender<Result<(), RuntimeError>>,
    },
    AddRoots {
        roots: Vec<WorkspaceRoot>,
        response: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Close(oneshot::Sender<Result<(), RuntimeError>>),
}

struct SessionActor {
    state: SessionSnapshot,
    receiver: mpsc::Receiver<SessionCommand>,
    providers: Arc<ProviderRegistry>,
    defaults: RuntimeDefaults,
    events: mpsc::Sender<HarnessEvent>,
    runtime_cancellation: RuntimeCancellation,
    ids: Arc<AtomicU64>,
    pending: VecDeque<SessionCommand>,
}

impl SessionActor {
    async fn run(mut self) {
        loop {
            let command = match self.pending.pop_front() {
                Some(command) => command,
                None => {
                    let Some(command) = self.receiver.recv().await else {
                        return;
                    };
                    command
                }
            };
            if self.handle_idle(command).await {
                return;
            }
        }
    }

    async fn handle_idle(&mut self, command: SessionCommand) -> bool {
        match command {
            SessionCommand::Snapshot(response) => {
                let _ = response.send(if self.state.closed {
                    Err(RuntimeError::SessionClosed(self.state.session_id.clone()))
                } else {
                    Ok(self.state.clone())
                });
            }
            SessionCommand::Prompt {
                turn_id,
                message_id,
                content,
                correlation,
                expected_revision,
                completion,
            } => {
                if let Some(expected) = expected_revision
                    && expected != self.state.revision
                {
                    let _ = completion.send(Err(RuntimeError::RevisionConflict {
                        expected,
                        actual: self.state.revision,
                    }));
                    return false;
                }
                if !self.state.configuration_status.is_ready() {
                    let _ = completion.send(Err(RuntimeError::ConfigurationRequired));
                    return false;
                }
                self.drive_prompt(turn_id, message_id, content, correlation, completion)
                    .await;
            }
            SessionCommand::Cancel { turn_id, response } => {
                let _ = response.send(Err(RuntimeError::TurnNotActive(turn_id)));
            }
            SessionCommand::UpdateMode {
                operating,
                permission,
                expected_revision,
                response,
            } => {
                let result = if expected_revision.is_some_and(|value| value != self.state.revision)
                {
                    Err(RuntimeError::RevisionConflict {
                        expected: expected_revision.expect("checked"),
                        actual: self.state.revision,
                    })
                } else {
                    if let Some(value) = operating {
                        self.state.operating_mode = value;
                    }
                    if let Some(value) = permission {
                        self.state.permission_mode = value;
                    }
                    self.increment_revision();
                    Ok(())
                };
                let _ = response.send(result);
            }
            SessionCommand::AddRoots { roots, response } => {
                self.state.workspace_roots.extend(roots);
                self.increment_revision();
                let _ = response.send(Ok(()));
            }
            SessionCommand::Close(response) => {
                self.runtime_cancellation.cancel();
                self.state.closed = true;
                let _ = response.send(Ok(()));
                return true;
            }
        }
        false
    }

    async fn drive_prompt(
        &mut self,
        turn_id: TurnId,
        message_id: MessageId,
        content: Vec<ContentPart>,
        correlation: CorrelationId,
        completion: oneshot::Sender<Result<SessionTurnResult, RuntimeError>>,
    ) {
        let user = vesper_domain::ConversationMessage {
            id: message_id.clone(),
            role: MessageRole::User,
            content,
            extensions: ExtensionMap::default(),
        };
        self.state.history.push(user);
        self.increment_revision();
        self.state.active_turn = Some(turn_id.clone());
        let cancellation = self.runtime_cancellation.child();
        let turn_cancel = cancellation.clone();
        let input = TurnInput {
            snapshot: self.state.clone(),
            defaults: self.defaults.clone(),
            turn_id: turn_id.clone(),
            message_id: message_id.clone(),
            correlation,
            events: self.events.clone(),
            providers: Arc::clone(&self.providers),
            cancellation,
            ids: Arc::clone(&self.ids),
        };
        let mut join = tokio::spawn(run_turn(input));
        let result = loop {
            tokio::select! {
                joined = &mut join => {
                    break joined.map_err(|_| RuntimeError::ChannelClosed).and_then(std::convert::identity);
                }
                command = self.receiver.recv() => {
                    match command {
                        Some(SessionCommand::Cancel { turn_id: requested, response }) => {
                            let result = if requested == turn_id {
                                turn_cancel.cancel();
                                Ok(())
                            } else {
                                Err(RuntimeError::TurnNotActive(requested))
                            };
                            let _ = response.send(result);
                        }
                        Some(SessionCommand::Close(response)) => {
                            turn_cancel.cancel();
                            let result = join.await.map_err(|_| RuntimeError::ChannelClosed)
                                .and_then(std::convert::identity);
                            self.state.closed = true;
                            let _ = response.send(Ok(()));
                            self.apply_turn_result(&result);
                            let _ = completion.send(result);
                            return;
                        }
                        Some(other) => self.pending.push_back(other),
                        None => {
                            turn_cancel.cancel();
                            break join.await.map_err(|_| RuntimeError::ChannelClosed)
                                .and_then(std::convert::identity);
                        }
                    }
                }
            }
        };
        self.apply_turn_result(&result);
        let _ = completion.send(result);
    }

    fn apply_turn_result(&mut self, result: &Result<SessionTurnResult, RuntimeError>) {
        self.state.active_turn = None;
        if let Ok(result) = result {
            if !result.assistant_content.is_empty() {
                let assistant_id = MessageId::new(format!("assistant-{}", result.turn_id.as_str()))
                    .expect("bounded generated message ID");
                self.state.history.push(vesper_domain::ConversationMessage {
                    id: assistant_id,
                    role: MessageRole::Assistant,
                    content: result.assistant_content.clone(),
                    extensions: ExtensionMap::default(),
                });
            }
            if let Some(usage) = &result.usage {
                self.state.cumulative_usage = usage.clone();
            }
            self.increment_revision();
        }
    }

    fn increment_revision(&mut self) {
        self.state.revision = Revision::new(self.state.revision.get().saturating_add(1));
    }
}

struct TurnInput {
    snapshot: SessionSnapshot,
    defaults: RuntimeDefaults,
    turn_id: TurnId,
    message_id: MessageId,
    correlation: CorrelationId,
    events: mpsc::Sender<HarnessEvent>,
    providers: Arc<ProviderRegistry>,
    cancellation: RuntimeCancellation,
    ids: Arc<AtomicU64>,
}

async fn run_turn(input: TurnInput) -> Result<SessionTurnResult, RuntimeError> {
    let mut emitter = TurnEmitter::new(&input);
    emitter
        .emit(HarnessEventPayload::UserMessageAccepted {
            message_id: input.message_id.clone(),
        })
        .await?;
    if input.cancellation.is_cancelled() {
        return emitter.cancelled(input.message_id, false).await;
    }
    let provider_cancellation: Arc<dyn vesper_provider::CancellationSignal> =
        Arc::new(input.cancellation.clone());
    let provider_session = input
        .providers
        .create_session(
            &input.snapshot.provider_id,
            &input.snapshot.provider_configuration,
            Arc::clone(&provider_cancellation),
        )
        .await?;
    if input.cancellation.is_cancelled() {
        return emitter.cancelled(input.message_id, false).await;
    }
    let request = ProviderRequest {
        request_id: ProviderRequestId::new(format!("request-{}", input.turn_id.as_str()))
            .expect("bounded generated request ID"),
        provider_id: input.snapshot.provider_id.clone(),
        model: input.snapshot.model.clone(),
        endpoint_id: None,
        system_instructions: input.defaults.system_instructions,
        messages: input.snapshot.history,
        tools: Vec::new(),
        tool_choice: ToolChoiceIntent::None,
        capabilities: Vec::new(),
        reasoning: input.defaults.reasoning,
        structured_output: StructuredOutputIntent::None,
        sampling: input.defaults.sampling,
        maximum_output_tokens: input.defaults.maximum_output_tokens,
        continuation: None,
        fallback_policy: FallbackPolicy::Strict,
        provider_extensions: None,
    };
    let mut stream = provider_session
        .start(request, Arc::clone(&provider_cancellation))
        .await
        .map_err(|_| RuntimeError::Provider)?;
    let mut contract = ProviderStreamContract::default();
    let mut visible = false;
    let mut assistant = Vec::new();
    let mut terminal = None;
    let mut terminal_event_emitted = false;
    let mut usage = None;
    loop {
        let next = tokio::select! {
            () = input.cancellation.cancelled() => {
                return emitter.cancelled(input.message_id, visible).await;
            }
            event = stream.next() => event,
        };
        let Some(event) = next else {
            break;
        };
        match event {
            Ok(event) => {
                contract
                    .accept_event(&event)
                    .map_err(|_| RuntimeError::ProviderStream)?;
                visible |= matches!(
                    event,
                    ProviderStreamEvent::ReasoningDelta { .. }
                        | ProviderStreamEvent::ContentDelta { .. }
                        | ProviderStreamEvent::ToolCallStarted { .. }
                        | ProviderStreamEvent::ToolCallDelta { .. }
                        | ProviderStreamEvent::ToolCallCompleted(_)
                );
                match event {
                    ProviderStreamEvent::ResponseStarted { metadata, .. } => {
                        emitter
                            .emit(HarnessEventPayload::ResponseStarted { metadata })
                            .await?;
                    }
                    ProviderStreamEvent::ReasoningDelta {
                        stream_id,
                        text,
                        kind,
                        retention,
                    } => {
                        emitter
                            .emit(HarnessEventPayload::ReasoningDelta {
                                stream_id,
                                text,
                                kind,
                                retention,
                            })
                            .await?;
                    }
                    ProviderStreamEvent::ContentDelta { stream_id, part } => {
                        assistant.push(part.clone());
                        emitter
                            .emit(HarnessEventPayload::ContentDelta { stream_id, part })
                            .await?;
                    }
                    ProviderStreamEvent::ToolCallStarted {
                        index,
                        call_id,
                        name,
                    } => {
                        emitter
                            .emit(HarnessEventPayload::ToolCallStarted {
                                index,
                                call_id,
                                name,
                            })
                            .await?;
                    }
                    ProviderStreamEvent::ToolCallDelta {
                        index,
                        id_fragment,
                        name_fragment,
                        arguments_fragment,
                    } => {
                        emitter
                            .emit(HarnessEventPayload::ToolCallUpdated {
                                index,
                                id_fragment,
                                name_fragment,
                                arguments_fragment,
                            })
                            .await?;
                    }
                    ProviderStreamEvent::ToolCallCompleted(call) => {
                        emitter
                            .emit(HarnessEventPayload::ToolCallCompleted(call))
                            .await?;
                        emitter
                            .emit(HarnessEventPayload::Warning {
                                message: SafeMessage::new(
                                    "tool execution is unavailable in the minimal runtime",
                                )
                                .expect("static warning"),
                            })
                            .await?;
                        terminal = Some(FinishOutcome::ToolCalls);
                        break;
                    }
                    ProviderStreamEvent::Usage(provider_usage) => {
                        emitter
                            .emit(HarnessEventPayload::UsageUpdated(provider_usage.clone()))
                            .await?;
                        usage = Some(provider_usage);
                    }
                    ProviderStreamEvent::RateLimit(update) => {
                        let mut status = update.metadata;
                        status
                            .insert("runtime:remaining", serde_json::json!(update.remaining))
                            .map_err(|_| RuntimeError::ProviderStream)?;
                        emitter
                            .emit(HarnessEventPayload::ProviderStatusUpdated { status })
                            .await?;
                    }
                    ProviderStreamEvent::Quota(update) => {
                        let mut status = update.metadata;
                        status
                            .insert(
                                "runtime:quota-remaining",
                                serde_json::json!(update.remaining),
                            )
                            .map_err(|_| RuntimeError::ProviderStream)?;
                        emitter
                            .emit(HarnessEventPayload::ProviderStatusUpdated { status })
                            .await?;
                    }
                    ProviderStreamEvent::Warning { message, .. } => {
                        emitter
                            .emit(HarnessEventPayload::Warning { message })
                            .await?;
                    }
                    ProviderStreamEvent::Completed { finish, metadata } => {
                        terminal = Some(finish.clone());
                        emitter
                            .emit(HarnessEventPayload::TurnCompleted {
                                outcome: finish,
                                metadata,
                            })
                            .await?;
                        terminal_event_emitted = true;
                        break;
                    }
                }
            }
            Err(error) => {
                contract
                    .accept_error(&error)
                    .map_err(|_| RuntimeError::ProviderStream)?;
                emitter
                    .emit(HarnessEventPayload::RecoverableError(error.info))
                    .await?;
                terminal = Some(FinishOutcome::ProviderError);
                emitter
                    .emit(HarnessEventPayload::TurnCompleted {
                        outcome: FinishOutcome::ProviderError,
                        metadata: ExtensionMap::default(),
                    })
                    .await?;
                terminal_event_emitted = true;
                break;
            }
        }
    }
    if terminal.is_none() {
        contract
            .finish()
            .map_err(|_| RuntimeError::ProviderStream)?;
    }
    let outcome = terminal.ok_or(RuntimeError::ProviderStream)?;
    if !terminal_event_emitted {
        emitter
            .emit(HarnessEventPayload::TurnCompleted {
                outcome: outcome.clone(),
                metadata: ExtensionMap::default(),
            })
            .await?;
    }
    Ok(SessionTurnResult {
        turn_id: input.turn_id,
        user_message_id: input.message_id,
        outcome,
        visible_output_emitted: visible,
        assistant_content: assistant,
        usage,
    })
}

struct TurnEmitter {
    session_id: SessionId,
    turn_id: TurnId,
    correlation: CorrelationId,
    sender: mpsc::Sender<HarnessEvent>,
    ids: Arc<AtomicU64>,
    sequence: u64,
}

impl TurnEmitter {
    fn new(input: &TurnInput) -> Self {
        Self {
            session_id: input.snapshot.session_id.clone(),
            turn_id: input.turn_id.clone(),
            correlation: input.correlation.clone(),
            sender: input.events.clone(),
            ids: Arc::clone(&input.ids),
            sequence: 0,
        }
    }

    async fn emit(&mut self, payload: HarnessEventPayload) -> Result<(), RuntimeError> {
        let event = HarnessEvent {
            schema_version: EventSchemaVersion::CURRENT,
            event_id: EventId::new(format!(
                "event-{}",
                self.ids.fetch_add(1, Ordering::Relaxed)
            ))
            .expect("generated event ID"),
            correlation_id: Some(self.correlation.clone()),
            session_id: Some(self.session_id.clone()),
            turn_id: Some(self.turn_id.clone()),
            sequence: EventSequence::new(self.sequence),
            payload,
        };
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(RuntimeError::ProviderStream)?;
        self.sender
            .send(event)
            .await
            .map_err(|_| RuntimeError::ChannelClosed)
    }

    async fn cancelled(
        mut self,
        message_id: MessageId,
        visible: bool,
    ) -> Result<SessionTurnResult, RuntimeError> {
        self.emit(HarnessEventPayload::TurnCancelled {
            visible_output_emitted: visible,
        })
        .await?;
        Ok(SessionTurnResult {
            turn_id: self.turn_id,
            user_message_id: message_id,
            outcome: FinishOutcome::Cancelled,
            visible_output_emitted: visible,
            assistant_content: Vec::new(),
            usage: None,
        })
    }
}
