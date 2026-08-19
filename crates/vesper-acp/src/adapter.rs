use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
};

use agent_client_protocol::{
    Agent, Client, ConnectionTo, LineDirection, Responder, Stdio,
    schema::{
        ProtocolVersion,
        v1::{
            AuthenticateResponse, ClientNotification, ClientRequest, CloseSessionResponse,
            ContentBlock, ContentChunk, DeleteSessionResponse, ForkSessionResponse,
            ListSessionsResponse, LoadSessionResponse, LogoutResponse, NewSessionResponse,
            PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
            RequestPermissionRequest, RequestPermissionResponse, ResumeSessionResponse,
            SelectedPermissionOutcome, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigSelectOption, SessionConfigValueId, SessionInfo, SessionMode,
            SessionModeState, SessionNotification, SessionUpdate, SetSessionConfigOptionResponse,
            SetSessionModeResponse, TextContent, ToolCallUpdate, ToolCallUpdateFields,
        },
    },
};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, Semaphore, mpsc, oneshot},
    task::JoinSet,
};
use vesper_domain::{
    BoundedString, CommandId, CommandInitiator, CommandSchemaVersion, ContentPart,
    ConversationMessage, CorrelationId, HarnessCommand, HarnessCommandPayload, MessageRole,
    PromptSubmission, SessionId, SessionListFilter, SessionOperatingMode, SessionPermissionMode,
    TurnId,
};
use vesper_runtime::{
    ReplayError, ReplayFuture, ReplayMessage, ReplayMetadata, ReplayPlan, ReplayPlanPriority,
    ReplayPlanStatus, ReplaySink, ReplayUpdate, RuntimeError, RuntimeResponse, RuntimeSupervisor,
};

use crate::{
    compat::prompt_response_value,
    engine::{
        AcpPermissionDecision, AcpPermissionRequest, AcpPermissionRequester, AcpPromptEngine,
        AcpPromptRequest,
    },
    mapping::{
        AcpEventMapper, content_from_acp, message_id_from_meta, session_id, stop_reason,
        truthful_initialize_response, workspace_roots,
    },
};

const INBOUND_CAPACITY: usize = 32;

/// ACP adapter configuration with no provider-specific fields.
#[derive(Debug, Clone)]
pub struct AcpAdapterConfig {
    /// Context size exposed through ACP usage updates.
    pub context_window: u64,
}

impl Default for AcpAdapterConfig {
    fn default() -> Self {
        Self {
            context_window: 202_752,
        }
    }
}

/// Production ACP SDK adapter.
pub struct AcpAdapter {
    runtime: Arc<RuntimeSupervisor>,
    config: AcpAdapterConfig,
    ids: Arc<AtomicU64>,
    prompt_engine: Option<Arc<dyn AcpPromptEngine>>,
}

impl std::fmt::Debug for AcpAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpAdapter")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl AcpAdapter {
    /// Creates an adapter around one runtime.
    #[must_use]
    pub fn new(runtime: Arc<RuntimeSupervisor>, config: AcpAdapterConfig) -> Self {
        Self {
            runtime,
            config,
            ids: Arc::new(AtomicU64::new(1)),
            prompt_engine: None,
        }
    }

    /// Injects an optional full multi-turn engine. Without this injection the
    /// adapter retains the runtime's provider-neutral single-turn behavior.
    #[must_use]
    pub fn with_prompt_engine(mut self, engine: Arc<dyn AcpPromptEngine>) -> Self {
        self.prompt_engine = Some(engine);
        self
    }

    /// Runs official SDK dispatch over stdin/stdout until clean EOF.
    pub async fn run_stdio(self) -> Result<(), agent_client_protocol::Error> {
        let mut events = self
            .runtime
            .take_events()
            .await
            .map_err(sdk_runtime_error)?;
        let (request_sender, request_receiver) = mpsc::channel(INBOUND_CAPACITY);
        let (notification_sender, notification_receiver) = mpsc::channel(INBOUND_CAPACITY);
        let request_callback = request_sender.clone();
        let notification_callback = notification_sender.clone();
        let runtime = Arc::clone(&self.runtime);
        let config = self.config.clone();
        let ids = Arc::clone(&self.ids);
        let prompt_engine = self.prompt_engine.clone();
        let output_flow = OutputFlow::default();
        let transport_flow = output_flow.clone();
        let transport = Stdio::new().with_debug(move |line, direction| {
            if direction == LineDirection::Stdout
                && serde_json::from_str::<Value>(line)
                    .ok()
                    .is_some_and(|value| {
                        value.get("method").and_then(Value::as_str) == Some("session/update")
                    })
            {
                transport_flow.accepted_by_writer();
            }
        });

        Agent
            .builder()
            .name("agent-vesper-acp")
            .on_receive_request(
                async move |request: ClientRequest,
                            responder: Responder<Value>,
                            _connection: ConnectionTo<Client>| {
                    request_callback
                        .try_send(InboundRequest { request, responder })
                        .map_err(|error| {
                            agent_client_protocol::util::internal_error(format!(
                                "bounded ACP request queue unavailable: {error}"
                            ))
                        })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |notification: ClientNotification,
                            _connection: ConnectionTo<Client>| {
                    notification_callback
                        .try_send(notification)
                        .map_err(|error| {
                            agent_client_protocol::util::internal_error(format!(
                                "bounded ACP notification queue unavailable: {error}"
                            ))
                        })
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(transport, async move |connection| {
                let barriers = Arc::new(TerminalBarriers::default());
                let mut dispatcher = tokio::spawn(dispatch_requests(
                    Arc::clone(&runtime),
                    request_receiver,
                    notification_receiver,
                    connection.clone(),
                    Arc::clone(&barriers),
                    Arc::clone(&ids),
                    output_flow.clone(),
                    prompt_engine,
                    config.context_window,
                ));
                let event_connection = connection.clone();
                let event_barriers = Arc::clone(&barriers);
                let event_output_flow = output_flow.clone();
                let mut event_pump = tokio::spawn(async move {
                    let mut mapper = AcpEventMapper::new(config.context_window);
                    while let Some(event) = events.recv().await {
                        if let Some(notification) = mapper.notification(&event) {
                            event_connection.send_notification(notification)?;
                            event_output_flow.wait_until_writer_accepts().await?;
                        }
                        if event.payload.is_turn_terminal()
                            && let (Some(session), Some(turn)) =
                                (event.session_id.as_ref(), event.turn_id.as_ref())
                        {
                            event_barriers.release(session, turn).await;
                        }
                    }
                    Ok::<(), agent_client_protocol::Error>(())
                });

                tokio::select! {
                    () = connection.incoming_closed() => {}
                    result = &mut dispatcher => {
                        result.map_err(|_| agent_client_protocol::util::internal_error("ACP dispatcher task failed"))??;
                    }
                    result = &mut event_pump => {
                        result.map_err(|_| agent_client_protocol::util::internal_error("ACP event task failed"))??;
                    }
                }
                dispatcher.abort();
                event_pump.abort();
                let correlation = CorrelationId::new("acp-eof").expect("static correlation");
                let _ = runtime.shutdown(correlation).await;
                Ok(())
            })
            .await
    }
}

#[derive(Clone)]
struct OutputFlow {
    accepted_session_updates: Arc<Semaphore>,
}

impl Default for OutputFlow {
    fn default() -> Self {
        Self {
            accepted_session_updates: Arc::new(Semaphore::new(0)),
        }
    }
}

impl OutputFlow {
    fn accepted_by_writer(&self) {
        self.accepted_session_updates.add_permits(1);
    }

    async fn wait_until_writer_accepts(&self) -> Result<(), agent_client_protocol::Error> {
        self.accepted_session_updates
            .acquire()
            .await
            .map_err(agent_client_protocol::util::internal_error)?
            .forget();
        Ok(())
    }
}

struct InboundRequest {
    request: ClientRequest,
    responder: Responder<Value>,
}

#[derive(Default)]
struct TerminalBarriers {
    waiters: Mutex<BTreeMap<(SessionId, TurnId), oneshot::Sender<()>>>,
}

impl TerminalBarriers {
    async fn register(&self, session: SessionId, turn: TurnId) -> oneshot::Receiver<()> {
        let (sender, receiver) = oneshot::channel();
        self.waiters.lock().await.insert((session, turn), sender);
        receiver
    }

    async fn release(&self, session: &SessionId, turn: &TurnId) {
        if let Some(sender) = self
            .waiters
            .lock()
            .await
            .remove(&(session.clone(), turn.clone()))
        {
            let _ = sender.send(());
        }
    }
}

#[allow(clippy::too_many_arguments)] // bounded protocol-dispatch composition boundary
async fn dispatch_requests(
    runtime: Arc<RuntimeSupervisor>,
    mut requests: mpsc::Receiver<InboundRequest>,
    mut notifications: mpsc::Receiver<ClientNotification>,
    connection: ConnectionTo<Client>,
    barriers: Arc<TerminalBarriers>,
    ids: Arc<AtomicU64>,
    output_flow: OutputFlow,
    prompt_engine: Option<Arc<dyn AcpPromptEngine>>,
    context_window: u64,
) -> Result<(), agent_client_protocol::Error> {
    let active = Arc::new(Mutex::new(BTreeMap::<SessionId, TurnId>::new()));
    let engine_active = Arc::new(Mutex::new(BTreeSet::<SessionId>::new()));
    let permission_requester = Arc::new(AcpClientPermissionRequester::new(connection.clone()));
    let context = RequestContext {
        connection,
        barriers,
        active: Arc::clone(&active),
        engine_active: Arc::clone(&engine_active),
        ids,
        output_flow,
        prompt_engine,
        permission_requester,
        context_window,
    };
    let mut prompts = JoinSet::new();
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { break };
                handle_request(
                    Arc::clone(&runtime),
                    request,
                    context.clone(),
                    &mut prompts,
                ).await?;
            }
            notification = notifications.recv() => {
                let Some(notification) = notification else { break };
                if let ClientNotification::CancelNotification(cancel) = notification {
                    let session = session_id(&cancel.session_id);
                    context.permission_requester.cancel(&session);
                    if let Some(engine) = context.prompt_engine.as_ref()
                        && engine_active.lock().await.contains(&session)
                    {
                        let _ = engine.cancel(&session).await;
                    }
                    if let Some(turn) = active.lock().await.get(&session).cloned() {
                        let command = command(
                            HarnessCommandPayload::CancelTurn {
                                session_id: session,
                                turn_id: turn,
                            },
                            next_text_id(&context.ids, "cancel"),
                        );
                        let _ = runtime.execute(command).await;
                    }
                }
            }
            joined = prompts.join_next(), if !prompts.is_empty() => {
                joined
                    .ok_or_else(|| agent_client_protocol::util::internal_error("prompt task set closed"))?
                    .map_err(|_| agent_client_protocol::util::internal_error("prompt response task failed"))??;
            }
        }
    }
    while let Some(joined) = prompts.join_next().await {
        joined.map_err(|_| {
            agent_client_protocol::util::internal_error("prompt response task failed")
        })??;
    }
    Ok(())
}

struct AcpClientPermissionRequester {
    connection: ConnectionTo<Client>,
    pending: Arc<StdMutex<BTreeMap<SessionId, tokio::sync::oneshot::Sender<()>>>>,
}

impl std::fmt::Debug for AcpClientPermissionRequester {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AcpClientPermissionRequester(..)")
    }
}

impl AcpClientPermissionRequester {
    fn new(connection: ConnectionTo<Client>) -> Self {
        Self {
            connection,
            pending: Arc::new(StdMutex::new(BTreeMap::new())),
        }
    }
}

impl AcpPermissionRequester for AcpClientPermissionRequester {
    fn request<'a>(
        &'a self,
        request: AcpPermissionRequest,
    ) -> crate::engine::AcpPromptFuture<'a, AcpPermissionDecision> {
        let connection = self.connection.clone();
        let session_id = request.session_id.clone();
        let tool_call = ToolCallUpdate::new(
            format!("vesper-permission-{}", request.tool),
            ToolCallUpdateFields::new()
                .title(format!("{}: {}", request.title, request.reason))
                .raw_input(request.arguments),
        );
        let permission = RequestPermissionRequest::new(
            session_id.as_str().to_owned(),
            tool_call,
            vec![
                PermissionOption::new("allow-once", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(
                    "reject-once",
                    "Reject once",
                    PermissionOptionKind::RejectOnce,
                ),
            ],
        );
        let (cancel_sender, mut cancel_receiver) = tokio::sync::oneshot::channel();
        if let Ok(mut pending) = self.pending.lock()
            && let Some(previous) = pending.insert(session_id.clone(), cancel_sender)
        {
            let _ = previous.send(());
        }
        let pending = Arc::clone(&self.pending);
        Box::pin(async move {
            let response = connection.send_request(permission).block_task();
            let decision = tokio::select! {
                result = response => match result {
                    Ok(RequestPermissionResponse { outcome: RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }), .. })
                        if option_id.to_string() == "allow-once" || option_id.to_string() == "allow-always" => AcpPermissionDecision::Allow,
                    Ok(RequestPermissionResponse { outcome: RequestPermissionOutcome::Cancelled, .. }) => AcpPermissionDecision::Cancelled,
                    Ok(RequestPermissionResponse { outcome: RequestPermissionOutcome::Selected(_), .. }) => AcpPermissionDecision::Deny,
                    Ok(_) => AcpPermissionDecision::Deny,
                    Err(_) => AcpPermissionDecision::Deny,
                },
                _ = &mut cancel_receiver => AcpPermissionDecision::Cancelled,
            };
            if let Ok(mut pending) = pending.lock() {
                pending.remove(&session_id);
            }
            decision
        })
    }

    fn cancel(&self, session_id: &SessionId) {
        if let Ok(mut pending) = self.pending.lock()
            && let Some(sender) = pending.remove(session_id)
        {
            let _ = sender.send(());
        }
    }
}

#[derive(Clone)]
struct RequestContext {
    connection: ConnectionTo<Client>,
    barriers: Arc<TerminalBarriers>,
    active: Arc<Mutex<BTreeMap<SessionId, TurnId>>>,
    engine_active: Arc<Mutex<BTreeSet<SessionId>>>,
    ids: Arc<AtomicU64>,
    output_flow: OutputFlow,
    prompt_engine: Option<Arc<dyn AcpPromptEngine>>,
    permission_requester: Arc<dyn AcpPermissionRequester>,
    /// Adapter context-window size forwarded to engine event sinks for
    /// `usage_update` notifications.
    context_window: u64,
}

async fn handle_request(
    runtime: Arc<RuntimeSupervisor>,
    inbound: InboundRequest,
    context: RequestContext,
    prompts: &mut JoinSet<Result<(), agent_client_protocol::Error>>,
) -> Result<(), agent_client_protocol::Error> {
    let RequestContext {
        connection,
        barriers,
        active,
        engine_active,
        ids,
        output_flow,
        prompt_engine,
        permission_requester,
        context_window,
    } = context;
    let InboundRequest { request, responder } = inbound;
    macro_rules! execute {
        ($future:expr) => {
            match $future.await {
                Ok(response) => response,
                Err(error) => return responder.respond_with_error(sdk_runtime_error(error)),
            }
        };
    }
    match request {
        ClientRequest::InitializeRequest(request) => {
            if request.protocol_version != ProtocolVersion::V1 {
                return responder
                    .respond_with_error(agent_client_protocol::Error::invalid_params());
            }
            respond_json(
                responder,
                truthful_initialize_response(request.protocol_version),
            )
        }
        ClientRequest::AuthenticateRequest(request) => {
            if request.method_id.to_string() != "zai-api-key-setup" {
                return responder
                    .respond_with_error(agent_client_protocol::Error::invalid_params());
            }
            match runtime.validate_provider_authentication().await {
                Ok(()) => respond_json(responder, AuthenticateResponse::new()),
                Err(_) => {
                    responder.respond_with_error(agent_client_protocol::Error::auth_required())
                }
            }
        }
        ClientRequest::NewSessionRequest(request) => {
            // Client-declared MCP servers are accepted and ignored (oracle
            // parity): the frozen Python oracle receives `mcp_servers` and
            // never rejects a session over it, and clients such as Zed pass
            // every configured context server on each `session/new`, so
            // rejecting here made the agent unloadable for any user with MCP
            // servers configured. The harness keeps its own MCP registry; no
            // client-provided server is ever launched or advertised.
            if !request.mcp_servers.is_empty() {
                tracing::warn!(
                    count = request.mcp_servers.len(),
                    "ignoring client-declared MCP servers; the harness MCP registry owns MCP"
                );
            }
            let response = execute!(runtime.execute(command(
                HarnessCommandPayload::CreateSession {
                    workspace_roots: workspace_roots(request.cwd, request.additional_directories,),
                    requested_session_id: None,
                },
                next_text_id(&ids, "new"),
            )));
            let RuntimeResponse::Session(snapshot) = response else {
                return responder.respond_with_internal_error("unexpected runtime response");
            };
            let modes = session_modes(snapshot.operating_mode);
            let config_options = session_config_options(&snapshot);
            let session_id = snapshot.session_id.as_str().to_owned();
            // The catalog advertisement must FOLLOW the session/new response:
            // ACP clients register the session only once the response is
            // processed, and Zed drops `session/update` notifications for
            // unregistered sessions ("unknown session"). Advertising before
            // the response left the Zed slash-command menu permanently empty.
            respond_json(
                responder,
                NewSessionResponse::new(session_id)
                    .modes(modes)
                    .config_options(config_options),
            )?;
            advertise_available_commands(
                &connection,
                &agent_session_id(&snapshot.session_id),
                &output_flow,
            )
            .await
        }
        ClientRequest::LoadSessionRequest(request) => {
            let id = session_id(&request.session_id);
            let response = execute!(runtime.execute(command(
                HarnessCommandPayload::LoadSession {
                    session_id: id.clone(),
                    workspace_roots: workspace_roots(request.cwd, request.additional_directories,),
                },
                next_text_id(&ids, "load"),
            )));
            let RuntimeResponse::Session(snapshot) = response else {
                return responder.respond_with_internal_error("unexpected runtime response");
            };
            replay_snapshot(&connection, *snapshot, &output_flow).await?;
            advertise_available_commands(&connection, &request.session_id, &output_flow).await?;
            let snapshot = execute!(runtime.snapshot(&id));
            respond_json(
                responder,
                LoadSessionResponse::new()
                    .modes(session_modes(snapshot.operating_mode))
                    .config_options(session_config_options(&snapshot)),
            )
        }
        ClientRequest::ResumeSessionRequest(request) => {
            let response = execute!(runtime.execute(command(
                HarnessCommandPayload::ResumeSession {
                    session_id: session_id(&request.session_id),
                    workspace_roots: workspace_roots(request.cwd, request.additional_directories,),
                },
                next_text_id(&ids, "resume"),
            )));
            let RuntimeResponse::Session(snapshot) = response else {
                return responder.respond_with_internal_error("unexpected runtime response");
            };
            replay_snapshot(&connection, *snapshot, &output_flow).await?;
            advertise_available_commands(&connection, &request.session_id, &output_flow).await?;
            let snapshot = execute!(runtime.snapshot(&session_id(&request.session_id)));
            respond_json(
                responder,
                ResumeSessionResponse::new()
                    .modes(session_modes(snapshot.operating_mode))
                    .config_options(session_config_options(&snapshot)),
            )
        }
        ClientRequest::ListSessionsRequest(request) => {
            let response = execute!(runtime.execute(command(
                HarnessCommandPayload::ListSessions(SessionListFilter {
                    workspace: request.cwd.map(|path| {
                        vesper_domain::BoundedString::new(path.to_string_lossy().into_owned())
                            .expect("ACP path bound")
                    }),
                    include_closed: false,
                    limit: None,
                }),
                next_text_id(&ids, "list"),
            )));
            let RuntimeResponse::Sessions(summaries) = response else {
                return responder.respond_with_internal_error("unexpected runtime response");
            };
            let infos = summaries
                .into_iter()
                .map(|summary| {
                    let cwd = summary
                        .metadata
                        .get("runtime:cwd")
                        .and_then(Value::as_str)
                        .map_or_else(|| PathBuf::from("/"), PathBuf::from);
                    SessionInfo::new(summary.session_id.as_str().to_owned(), cwd)
                })
                .collect();
            respond_json(responder, ListSessionsResponse::new(infos))
        }
        ClientRequest::ForkSessionRequest(request) => {
            let response = execute!(runtime.execute(command(
                HarnessCommandPayload::ForkSession {
                    session_id: session_id(&request.session_id),
                    requested_session_id: None,
                },
                next_text_id(&ids, "fork"),
            )));
            let RuntimeResponse::Session(snapshot) = response else {
                return responder.respond_with_internal_error("unexpected runtime response");
            };
            // Oracle parity (fixtures/acp/fork-session): the fork response
            // carries config options only; no available_commands_update is
            // advertised for the child session.
            respond_json(
                responder,
                ForkSessionResponse::new(snapshot.session_id.as_str().to_owned()),
            )
        }
        ClientRequest::CloseSessionRequest(request) => {
            execute!(runtime.execute(command(
                HarnessCommandPayload::CloseSession {
                    session_id: session_id(&request.session_id),
                },
                next_text_id(&ids, "close"),
            )));
            respond_json(responder, CloseSessionResponse::new())
        }
        ClientRequest::PromptRequest(request) => {
            let session = session_id(&request.session_id);
            let message_id =
                message_id_from_meta(request.meta.as_ref(), || next_text_id(&ids, "message"))
                    .map_err(agent_client_protocol::util::internal_error)?;
            if request.prompt.len() == 1
                && matches!(
                    &request.prompt[0],
                    agent_client_protocol::schema::v1::ContentBlock::Text(text)
                        if text.text.trim_start().starts_with('/')
                )
                && prompt_engine.is_none()
            {
                // Conformance-only path (no injected engine): slash text has
                // no executor in the provider-neutral single-turn runtime, so
                // it fails closed instead of dispatching the provider. With a
                // composed engine the text flows to the engine, which owns
                // catalog execution (fixtures/acp/slash-command parity).
                return responder
                    .respond_with_error(agent_client_protocol::Error::method_not_found());
            }
            let content = content_from_acp(request.prompt)
                .map_err(agent_client_protocol::util::internal_error)?;
            if content.is_empty() {
                return responder.respond(prompt_response_value(
                    agent_client_protocol::schema::v1::StopReason::EndTurn,
                    &message_id,
                ));
            }
            if let Some(engine) = prompt_engine {
                let snapshot = execute!(runtime.snapshot(&session));
                let workspace_roots = snapshot.workspace_roots.clone();
                let operating_mode = snapshot.operating_mode;
                let permission_mode = snapshot.permission_mode;
                let history = snapshot.history.clone();
                let engine_session = session.clone();
                let engine_content = content.clone();
                let engine_message_id = message_id.clone();
                let response_message_id = message_id.clone();
                let save_runtime = Arc::clone(&runtime);
                engine_active.lock().await.insert(session.clone());
                let engine_active_task = Arc::clone(&engine_active);
                let connection = connection.clone();
                let output_flow = output_flow.clone();
                let request_session = request.session_id.clone();
                let engine_context_window = context_window;
                prompts.spawn(async move {
                    let result = engine
                        .run(AcpPromptRequest {
                            session_id: engine_session.clone(),
                            content: engine_content.clone(),
                            history,
                            operating_mode,
                            permission_mode,
                            workspace_roots,
                            permission_requester: Some(Arc::clone(&permission_requester)),
                            event_sink: Some(Arc::new(AcpEngineEventSink {
                                connection: connection.clone(),
                                session_id: request_session.clone(),
                                output_flow: output_flow.clone(),
                                context_window: engine_context_window,
                            })),
                        })
                        .await;
                    match result {
                        Ok(result) => {
                            engine_active_task.lock().await.remove(&engine_session);
                            if result.cancelled {
                                return responder.respond(prompt_response_value(
                                    agent_client_protocol::schema::v1::StopReason::Cancelled,
                                    &response_message_id,
                                ));
                            }
                            let assistant_content = if result.text.is_empty() {
                                Vec::new()
                            } else {
                                match vesper_domain::ContentText::new(result.text.clone()) {
                                    Ok(text) => vec![ContentPart::Text(text)],
                                    Err(_) => {
                                        return responder.respond_with_error(
                                            agent_client_protocol::Error::invalid_params().data(
                                                json!({
                                                    "reason": "harness-output-too-large"
                                                }),
                                            ),
                                        );
                                    }
                                }
                            };
                            let user = ConversationMessage {
                                id: engine_message_id,
                                role: MessageRole::User,
                                content: engine_content,
                                extensions: vesper_domain::ExtensionMap::default(),
                            };
                            // Slash-command turns report `persist_turn == false`
                            // (oracle parity: echoed to the UI, never appended
                            // to model-visible history or the persisted
                            // record). Model turns persist the full exchange.
                            if result.persist_turn {
                                if let Err(error) = save_runtime
                                    .accept_external_turn(&engine_session, user, assistant_content)
                                    .await
                                {
                                    return responder.respond_with_error(sdk_runtime_error(error));
                                }
                                if let Err(error) = save_runtime.save_session(&engine_session).await
                                {
                                    return responder.respond_with_error(sdk_runtime_error(error));
                                }
                            }
                            let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(
                                result.text.as_str(),
                            )));
                            connection
                                .send_notification(SessionNotification::new(
                                    request_session,
                                    SessionUpdate::AgentMessageChunk(chunk),
                                ))
                                .map_err(agent_client_protocol::util::internal_error)?;
                            output_flow.wait_until_writer_accepts().await?;
                            responder.respond(prompt_response_value(
                                agent_client_protocol::schema::v1::StopReason::EndTurn,
                                &response_message_id,
                            ))
                        }
                        Err(error) => {
                            engine_active_task.lock().await.remove(&engine_session);
                            tracing::warn!(error = %error, "harness engine failed");
                            responder.respond_with_error(
                                agent_client_protocol::util::internal_error(
                                    "harness engine failed",
                                ),
                            )
                        }
                    }
                });
                return Ok(());
            }
            let response = execute!(runtime.execute(command(
                HarnessCommandPayload::SubmitPrompt {
                    session_id: session.clone(),
                    prompt: PromptSubmission {
                        message_id: message_id.clone(),
                        content,
                        extensions: vesper_domain::ExtensionMap::default(),
                    },
                },
                next_text_id(&ids, "prompt"),
            )));
            let RuntimeResponse::PromptStarted { turn_id, .. } = &response else {
                return responder.respond_with_internal_error("unexpected runtime response");
            };
            let turn_id = turn_id.clone();
            active.lock().await.insert(session.clone(), turn_id.clone());
            let barrier = barriers.register(session.clone(), turn_id).await;
            let save_runtime = Arc::clone(&runtime);
            prompts.spawn(async move {
                match response.wait_prompt().await {
                    Ok(result) => {
                        let _ = barrier.await;
                        active.lock().await.remove(&session);
                        // Persist the mutated session through the injected
                        // writer when configured. The save runs entirely inside
                        // this detached task, so the dispatcher loop never
                        // blocks; the writer owns its bounded blocking
                        // semaphore. A persistence failure surfaces a sanitized
                        // request error with a stable reason and the dispatcher
                        // continues serving other requests.
                        match save_runtime.save_session(&session).await {
                            Ok(_) => responder.respond(prompt_response_value(
                                stop_reason(&result.outcome),
                                &result.user_message_id,
                            )),
                            Err(error) => responder.respond_with_error(sdk_runtime_error(error)),
                        }
                    }
                    Err(error) => {
                        active.lock().await.remove(&session);
                        responder.respond_with_error(sdk_runtime_error(error))
                    }
                }
            });
            Ok(())
        }
        ClientRequest::SetSessionModeRequest(request) => {
            let session = session_id(&request.session_id);
            let mode = request.mode_id.to_string();
            let operating_mode = match mode.as_str() {
                "code" | "normal" => Some(SessionOperatingMode::Code),
                "plan" | "planning" => Some(SessionOperatingMode::Plan),
                _ => {
                    return responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data(json!({"reason": "unsupported-session-mode"})),
                    );
                }
            };
            execute!(runtime.execute(command(
                HarnessCommandPayload::UpdateSessionMode {
                    session_id: session.clone(),
                    operating_mode,
                    permission_mode: None,
                },
                next_text_id(&ids, "mode"),
            )));
            connection
                .send_notification(agent_client_protocol::schema::v1::SessionNotification::new(
                    request.session_id,
                    agent_client_protocol::schema::v1::SessionUpdate::CurrentModeUpdate(
                        agent_client_protocol::schema::v1::CurrentModeUpdate::new(mode),
                    ),
                ))
                .map_err(agent_client_protocol::util::internal_error)?;
            output_flow.wait_until_writer_accepts().await?;
            respond_json(responder, SetSessionModeResponse::default())
        }
        ClientRequest::SetSessionConfigOptionRequest(request) => {
            let session = session_id(&request.session_id);
            let config_id = request.config_id.to_string();
            let value = request.value.as_value_id().map(|value| value.to_string());
            let payload = match config_id.as_str() {
                "thought_level" | "reasoning" => {
                    let Some(value) = value else {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params()
                                .data(json!({"reason": "thought-level-requires-value"})),
                        );
                    };
                    HarnessCommandPayload::UpdateSessionReasoning {
                        session_id: session.clone(),
                        mode: Some(
                            BoundedString::new(value)
                                .map_err(agent_client_protocol::util::internal_error)?,
                        ),
                    }
                }
                "permission" | "permission_mode" => {
                    let Some(value) = value else {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params()
                                .data(json!({"reason": "permission-mode-requires-value"})),
                        );
                    };
                    let permission_mode = match value.as_str() {
                        "ask" => SessionPermissionMode::Ask,
                        "bypass" => SessionPermissionMode::Bypass,
                        "read-only" | "readonly" => SessionPermissionMode::ReadOnly,
                        _ => {
                            return responder.respond_with_error(
                                agent_client_protocol::Error::invalid_params()
                                    .data(json!({"reason": "unsupported-permission-mode"})),
                            );
                        }
                    };
                    HarnessCommandPayload::UpdateSessionMode {
                        session_id: session.clone(),
                        operating_mode: None,
                        permission_mode: Some(permission_mode),
                    }
                }
                "mode" | "operating_mode" => {
                    let Some(value) = value else {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params()
                                .data(json!({"reason": "mode-requires-value"})),
                        );
                    };
                    let operating_mode = match value.as_str() {
                        "code" | "normal" => SessionOperatingMode::Code,
                        "plan" | "planning" => SessionOperatingMode::Plan,
                        _ => {
                            return responder.respond_with_error(
                                agent_client_protocol::Error::invalid_params()
                                    .data(json!({"reason": "unsupported-operating-mode"})),
                            );
                        }
                    };
                    HarnessCommandPayload::UpdateSessionMode {
                        session_id: session.clone(),
                        operating_mode: Some(operating_mode),
                        permission_mode: None,
                    }
                }
                _ => {
                    return responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params()
                            .data(json!({"reason": "unsupported-session-config-option"})),
                    );
                }
            };
            execute!(runtime.execute(command(payload, next_text_id(&ids, "config"))));
            let snapshot = execute!(runtime.snapshot(&session));
            respond_json(
                responder,
                SetSessionConfigOptionResponse::new(session_config_options(&snapshot)),
            )
        }
        ClientRequest::DeleteSessionRequest(request) => {
            execute!(runtime.execute(command(
                HarnessCommandPayload::CloseSession {
                    session_id: session_id(&request.session_id),
                },
                next_text_id(&ids, "delete"),
            )));
            respond_json(responder, DeleteSessionResponse::new())
        }
        ClientRequest::LogoutRequest(_) => {
            // Credentials are resolved from the provider-owned environment
            // source and never stored by the Rust harness. Logout therefore
            // has no local secret mutation to perform, but remains a valid
            // ACP operation instead of a silent method-not-found.
            respond_json(responder, LogoutResponse::new())
        }
        ClientRequest::ExtMethodRequest(_) => {
            responder.respond_with_error(agent_client_protocol::Error::method_not_found())
        }
        _ => responder.respond_with_error(agent_client_protocol::Error::method_not_found()),
    }
}

fn respond_json(
    responder: Responder<Value>,
    value: impl serde::Serialize,
) -> Result<(), agent_client_protocol::Error> {
    responder
        .respond(serde_json::to_value(value).map_err(agent_client_protocol::util::internal_error)?)
}

/// Converts a domain session id into the ACP schema id shape.
fn agent_session_id(
    value: &vesper_domain::SessionId,
) -> agent_client_protocol::schema::v1::SessionId {
    agent_client_protocol::schema::v1::SessionId::new(value.as_str())
}

/// The frozen-oracle command catalog as ACP `AvailableCommand` entries, in
/// oracle registration order. Byte-stable names/descriptions come from
/// `vesper-domain` so ACP advertisement, harness execution, and persisted
/// replay share one source of truth.
fn catalog_commands() -> Vec<agent_client_protocol::schema::v1::AvailableCommand> {
    use agent_client_protocol::schema::v1::AvailableCommand;
    vesper_domain::ORACLE_SLASH_COMMANDS
        .iter()
        .map(|command| AvailableCommand::new(command.name, command.description))
        .collect()
}

/// Emits `available_commands_update` for one session through the adapter's
/// bounded output flow. Called on session/new, load, and resume so every
/// live session carries the full command surface (oracle parity: 28
/// commands). On session/new the notification is sent only AFTER the
/// response: clients such as Zed drop `session/update` notifications for
/// sessions they have not registered yet, so a pre-response advertisement
/// never reached the slash-command menu. Load/resume sessions are already
/// registered client-side, so those advertisements stay before the response
/// (replay ordering); the fork fixture records no advertisement.
async fn advertise_available_commands(
    connection: &ConnectionTo<Client>,
    session: &agent_client_protocol::schema::v1::SessionId,
    output_flow: &OutputFlow,
) -> Result<(), agent_client_protocol::Error> {
    use agent_client_protocol::schema::v1::{
        AvailableCommandsUpdate, SessionNotification, SessionUpdate,
    };
    let notification = SessionNotification::new(
        session.clone(),
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(catalog_commands())),
    );
    connection
        .send_notification(notification)
        .map_err(agent_client_protocol::util::internal_error)?;
    output_flow.wait_until_writer_accepts().await?;
    Ok(())
}

/// Sink handed to a composed streaming engine. Each event is mapped to the
/// same ACP wire shape the runtime event pump produces, so ACP clients see
/// identical `agent_thought_chunk` / `agent_message_chunk` / tool-call /
/// usage updates on the full-harness path as on the single-turn path.
struct AcpEngineEventSink {
    connection: ConnectionTo<Client>,
    session_id: agent_client_protocol::schema::v1::SessionId,
    output_flow: OutputFlow,
    context_window: u64,
}

impl std::fmt::Debug for AcpEngineEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpEngineEventSink").finish_non_exhaustive()
    }
}

impl AcpEngineEventSink {
    fn context_window(&self) -> u64 {
        self.context_window
    }
}

impl AcpEngineEventSink {
    fn publish(&self, update: agent_client_protocol::schema::v1::SessionUpdate) {
        let connection = self.connection.clone();
        let flow = self.output_flow.clone();
        let session = self.session_id.clone();
        // Fire-and-forget: the spawned task applies the adapter's bounded
        // output-flow backpressure itself; publish() must never block the
        // engine's synchronous event stream.
        tokio::spawn(async move {
            connection
                .send_notification(agent_client_protocol::schema::v1::SessionNotification::new(
                    session, update,
                ))
                .map_err(agent_client_protocol::util::internal_error)?;
            flow.wait_until_writer_accepts().await?;
            Ok::<(), agent_client_protocol::Error>(())
        });
    }
}

impl crate::engine::AcpEventSink for AcpEngineEventSink {
    fn event(&self, event: crate::engine::AcpEngineEvent) {
        use agent_client_protocol::schema::v1::{
            ContentBlock, ContentChunk, SessionUpdate, TextContent, ToolCall as AcpToolCall,
            ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
        };
        match event {
            crate::engine::AcpEngineEvent::ReasoningDelta { text } => {
                self.publish(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(text.as_str())),
                )));
            }
            crate::engine::AcpEngineEvent::ContentDelta { text } => {
                self.publish(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(text.as_str())),
                )));
            }
            crate::engine::AcpEngineEvent::ToolStarted {
                tool_call_id,
                name,
                hint,
                arguments,
            } => {
                let _ = hint;
                self.publish(SessionUpdate::ToolCall(AcpToolCall::new(
                    tool_call_id,
                    name,
                )));
                let _ = arguments;
            }
            crate::engine::AcpEngineEvent::ToolFinished {
                tool_call_id,
                name,
                success,
                note,
            } => {
                let status = if success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                };
                self.publish(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    tool_call_id,
                    ToolCallUpdateFields::new()
                        .title(name)
                        .status(status)
                        .raw_output(serde_json::json!({ "note": note })),
                )));
            }
            crate::engine::AcpEngineEvent::Usage { usage } => {
                let used = usage
                    .total
                    .value
                    .or_else(|| {
                        usage
                            .input
                            .value
                            .zip(usage.output.value)
                            .and_then(|(a, b)| a.checked_add(b))
                    })
                    .unwrap_or(0);
                self.publish(SessionUpdate::UsageUpdate(UsageUpdate::new(
                    used,
                    self.context_window(),
                )));
            }
            crate::engine::AcpEngineEvent::PlanUpdated { markdown } => {
                let _ = markdown;
            }
        }
    }
}

fn session_modes(operating_mode: SessionOperatingMode) -> SessionModeState {
    let current = match operating_mode {
        SessionOperatingMode::Code => "code",
        SessionOperatingMode::Plan => "plan",
    };
    SessionModeState::new(
        current,
        vec![
            SessionMode::new("code", "Code").description("Normal coding mode"),
            SessionMode::new("plan", "Plan").description("Read-only planning mode"),
        ],
    )
}

fn session_config_options(snapshot: &vesper_runtime::SessionSnapshot) -> Vec<SessionConfigOption> {
    let thought_level = snapshot
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.mode.as_ref())
        .map(|mode| mode.as_str().to_owned())
        .unwrap_or_else(|| "default".to_owned());
    vec![
        SessionConfigOption::select(
            "thought_level",
            "Thought level",
            SessionConfigValueId::new(thought_level),
            vec![
                SessionConfigSelectOption::new("default", "Default"),
                SessionConfigSelectOption::new("disabled", "Disabled"),
                SessionConfigSelectOption::new("enabled", "Enabled"),
                SessionConfigSelectOption::new("high", "High"),
                SessionConfigSelectOption::new("max", "Maximum"),
            ],
        )
        .category(SessionConfigOptionCategory::ThoughtLevel),
        SessionConfigOption::select(
            "permission_mode",
            "Permission mode",
            match snapshot.permission_mode {
                SessionPermissionMode::Ask => "ask",
                SessionPermissionMode::Bypass => "bypass",
                SessionPermissionMode::ReadOnly => "read-only",
            },
            vec![
                SessionConfigSelectOption::new("ask", "Ask"),
                SessionConfigSelectOption::new("bypass", "Bypass"),
                SessionConfigSelectOption::new("read-only", "Read only"),
            ],
        ),
    ]
}

async fn replay_snapshot(
    connection: &ConnectionTo<Client>,
    snapshot: vesper_runtime::SessionSnapshot,
    output_flow: &OutputFlow,
) -> Result<(), agent_client_protocol::Error> {
    let session_id = snapshot.session_id.as_str().to_owned();
    let plan = snapshot
        .replay
        .clone()
        .unwrap_or_else(|| replay_for_ephemeral_snapshot(&snapshot));
    let mut sink = AcpReplaySink {
        connection,
        output_flow,
        session_id,
    };
    plan.deliver(&mut sink)
        .await
        .map_err(agent_client_protocol::util::internal_error)
}

fn replay_for_ephemeral_snapshot(snapshot: &vesper_runtime::SessionSnapshot) -> ReplayPlan {
    let messages = snapshot
        .history
        .iter()
        .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
        .flat_map(|message| {
            message.content.iter().filter_map(move |part| {
                let ContentPart::Text(text) = part else {
                    return None;
                };
                if text.as_str().is_empty() {
                    return None;
                }
                Some(ReplayMessage {
                    message_id: message.id.clone(),
                    role: message.role.clone(),
                    text: text.clone(),
                })
            })
        })
        .collect();
    ReplayPlan::new(
        messages,
        Vec::new(),
        ReplayMetadata {
            title: None,
            updated_at: None,
            operating_mode: snapshot.operating_mode,
            configuration_required: !snapshot.configuration_status.is_ready(),
        },
        Vec::new(),
    )
}

struct AcpReplaySink<'a> {
    connection: &'a ConnectionTo<Client>,
    output_flow: &'a OutputFlow,
    session_id: String,
}

impl ReplaySink for AcpReplaySink<'_> {
    fn accept<'a>(&'a mut self, update: &'a ReplayUpdate) -> ReplayFuture<'a> {
        Box::pin(async move {
            for notification in replay_notifications(&self.session_id, update) {
                self.connection
                    .send_notification(notification)
                    .map_err(replay_error)?;
                self.output_flow
                    .wait_until_writer_accepts()
                    .await
                    .map_err(replay_error)?;
            }
            Ok(())
        })
    }
}

fn replay_error(error: impl std::fmt::Display) -> ReplayError {
    ReplayError {
        message: BoundedString::new(format!("ACP replay writer rejected an update: {error}"))
            .unwrap_or_else(|_| {
                BoundedString::new("ACP replay writer rejected an update").unwrap()
            }),
    }
}

fn replay_notifications(
    session_id: &str,
    update: &ReplayUpdate,
) -> Vec<agent_client_protocol::schema::v1::SessionNotification> {
    use agent_client_protocol::schema::v1::{
        AvailableCommand, AvailableCommandsUpdate, ContentBlock, ContentChunk, CurrentModeUpdate,
        Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, SessionInfoUpdate,
        SessionNotification, SessionUpdate, TextContent,
    };

    let session = || agent_client_protocol::schema::v1::SessionId::new(session_id);
    match update {
        ReplayUpdate::VisibleMessage(message) => {
            let chunk =
                ContentChunk::new(ContentBlock::Text(TextContent::new(message.text.as_str())));
            let update = match message.role {
                MessageRole::User => SessionUpdate::UserMessageChunk(chunk),
                MessageRole::Assistant => SessionUpdate::AgentMessageChunk(chunk),
                MessageRole::Tool | MessageRole::ProviderOpaque(_) => return Vec::new(),
            };
            vec![SessionNotification::new(session(), update)]
        }
        ReplayUpdate::Plan(entries) => {
            let entries = entries
                .iter()
                .map(|entry| {
                    PlanEntry::new(
                        entry.content.as_str(),
                        match entry.priority {
                            ReplayPlanPriority::Low => PlanEntryPriority::Low,
                            ReplayPlanPriority::Medium => PlanEntryPriority::Medium,
                            ReplayPlanPriority::High => PlanEntryPriority::High,
                        },
                        match entry.status {
                            ReplayPlanStatus::Pending => PlanEntryStatus::Pending,
                            ReplayPlanStatus::InProgress => PlanEntryStatus::InProgress,
                            ReplayPlanStatus::Completed => PlanEntryStatus::Completed,
                        },
                    )
                })
                .collect();
            vec![SessionNotification::new(
                session(),
                SessionUpdate::Plan(Plan::new(entries)),
            )]
        }
        ReplayUpdate::Metadata(metadata) => {
            let mut updates = Vec::with_capacity(2);
            if metadata.title.is_some() || metadata.updated_at.is_some() {
                let mut info = SessionInfoUpdate::new();
                if let Some(title) = &metadata.title {
                    info = info.title(title.as_str().to_owned());
                }
                if let Some(updated_at) = &metadata.updated_at {
                    info = info.updated_at(updated_at.as_str().to_owned());
                }
                updates.push(SessionNotification::new(
                    session(),
                    SessionUpdate::SessionInfoUpdate(info),
                ));
            }
            updates.push(SessionNotification::new(
                session(),
                SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
                    match metadata.operating_mode {
                        vesper_domain::SessionOperatingMode::Code => "code",
                        vesper_domain::SessionOperatingMode::Plan => "plan",
                    },
                )),
            ));
            updates
        }
        ReplayUpdate::AvailableCommands(commands) => {
            let commands = commands
                .iter()
                .map(|command| {
                    AvailableCommand::new(command.name.as_str(), command.description.as_str())
                })
                .collect();
            vec![SessionNotification::new(
                session(),
                SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
            )]
        }
    }
}

fn command(payload: HarnessCommandPayload, id: String) -> HarnessCommand {
    HarnessCommand {
        schema_version: CommandSchemaVersion::CURRENT,
        command_id: CommandId::new(format!("command-{id}")).expect("generated command ID"),
        correlation_id: CorrelationId::new(format!("correlation-{id}"))
            .expect("generated correlation ID"),
        initiator: CommandInitiator::Acp,
        expected_revision: None,
        payload,
    }
}

fn next_text_id(ids: &AtomicU64, prefix: &str) -> String {
    format!("{prefix}-{}", ids.fetch_add(1, Ordering::Relaxed))
}

fn sdk_runtime_error(error: RuntimeError) -> agent_client_protocol::Error {
    match error {
        RuntimeError::SessionNotFound(_) => agent_client_protocol::Error::invalid_params()
            .data(json!({"reason": "ephemeral-session-not-found"})),
        RuntimeError::PersistentSessionCorrupt => agent_client_protocol::Error::invalid_params()
            .data(json!({"reason": "persistent-session-corrupt"})),
        RuntimeError::PersistentSessionUnsupportedVersion => {
            agent_client_protocol::Error::invalid_params()
                .data(json!({"reason": "persistent-session-unsupported-version"}))
        }
        RuntimeError::PersistentSessionRejectedByBounds => {
            agent_client_protocol::Error::invalid_params()
                .data(json!({"reason": "persistent-session-rejected-by-bounds"}))
        }
        RuntimeError::PersistentSessionPermissionDenied => {
            agent_client_protocol::Error::invalid_params()
                .data(json!({"reason": "persistent-session-permission-denied"}))
        }
        RuntimeError::PersistentSessionUnsafePath => agent_client_protocol::Error::invalid_params()
            .data(json!({"reason": "persistent-session-unsafe-path"})),
        RuntimeError::PersistentSessionWriteFailed => {
            agent_client_protocol::Error::invalid_params()
                .data(json!({"reason": "persistent-session-write-failed"}))
        }
        RuntimeError::PersistentSessionWorkspaceMismatch => {
            agent_client_protocol::Error::invalid_params()
                .data(json!({"reason": "persistent-session-workspace-mismatch"}))
        }
        RuntimeError::Provider => agent_client_protocol::Error::auth_required(),
        RuntimeError::UnsupportedCommand => agent_client_protocol::Error::method_not_found(),
        _ => agent_client_protocol::util::internal_error(error),
    }
}

#[cfg(test)]
mod replay_tests {
    use serde_json::Value;
    use vesper_domain::{BoundedString, MessageId, MessageRole, SessionOperatingMode};
    use vesper_runtime::{
        ReplayMessage, ReplayMetadata, ReplayPlanEntry, ReplayPlanPriority, ReplayPlanStatus,
        ReplayUpdate,
    };

    use super::replay_notifications;

    fn update_kind(
        notification: &agent_client_protocol::schema::v1::SessionNotification,
    ) -> String {
        serde_json::to_value(notification)
            .unwrap()
            .pointer("/update/sessionUpdate")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned()
    }

    #[test]
    fn compatibility_replay_maps_visible_plan_metadata_and_command_updates() {
        let message = ReplayUpdate::VisibleMessage(ReplayMessage {
            message_id: MessageId::new("stable-user").unwrap(),
            role: MessageRole::User,
            text: BoundedString::new("visible").unwrap(),
        });
        assert_eq!(
            update_kind(&replay_notifications("session", &message)[0]),
            "user_message_chunk"
        );

        let plan = ReplayUpdate::Plan(vec![ReplayPlanEntry {
            content: BoundedString::new("Inspect").unwrap(),
            status: ReplayPlanStatus::InProgress,
            priority: ReplayPlanPriority::High,
        }]);
        assert_eq!(
            update_kind(&replay_notifications("session", &plan)[0]),
            "plan"
        );

        let metadata = ReplayUpdate::Metadata(ReplayMetadata {
            title: Some(BoundedString::new("Loaded").unwrap()),
            updated_at: Some(BoundedString::new("2026-07-30T00:00:00Z").unwrap()),
            operating_mode: SessionOperatingMode::Plan,
            configuration_required: true,
        });
        let metadata = replay_notifications("session", &metadata);
        assert_eq!(
            metadata.iter().map(update_kind).collect::<Vec<_>>(),
            ["session_info_update", "current_mode_update"]
        );

        let commands = ReplayUpdate::AvailableCommands(Vec::new());
        assert_eq!(
            update_kind(&replay_notifications("session", &commands)[0]),
            "available_commands_update"
        );
    }

    #[test]
    fn tool_and_provider_internal_roles_cannot_be_replayed() {
        for role in [
            MessageRole::Tool,
            MessageRole::ProviderOpaque("internal".into()),
        ] {
            let update = ReplayUpdate::VisibleMessage(ReplayMessage {
                message_id: MessageId::new("internal").unwrap(),
                role,
                text: BoundedString::new("must-not-appear").unwrap(),
            });
            assert!(replay_notifications("session", &update).is_empty());
        }
    }
}
