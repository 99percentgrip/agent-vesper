use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
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
    controls::SessionControlSurface,
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

/// ACP adapter configuration.
///
/// The provider-routed session control surface (`controls`) supplies the
/// config options advertised on `session/new`/`load`/`resume`/`set` and the
/// values accepted by `session/set_config_option`. Without it only the
/// runtime-modeled `thought_level` and `permission_mode` options exist
/// (provider-neutral fallback).
#[derive(Debug, Clone)]
pub struct AcpAdapterConfig {
    /// Context size exposed through ACP usage updates.
    pub context_window: u64,
    /// Provider-routed session control surface (model, plan, reasoning dial).
    pub controls: Option<SessionControlSurface>,
    /// Additional implemented host-neutral commands advertised after the
    /// frozen oracle catalog.
    pub additional_commands: Vec<vesper_domain::SlashCommandDescriptor>,
}

impl Default for AcpAdapterConfig {
    fn default() -> Self {
        Self {
            context_window: 202_752,
            controls: None,
            additional_commands: Vec::new(),
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
                    config.controls,
                    config.additional_commands,
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

/// How long a `session/cancel` for an ENGINE-ACTIVE session is held before
/// it is executed (mid-turn slash grace).
///
/// Editors (Zed) interrupt a running turn by sending `session/cancel`
/// immediately followed by the new prompt — even when that "prompt" is an
/// informational slash command like `/status` or `/usage` that can be
/// answered without touching the turn. Executing such a cancel would stop
/// the user's work for no benefit. Within this grace window a prompt whose
/// sole text part is a [`CONCURRENT_SAFE_SLASH_COMMANDS`] command aborts/// the pending cancel (the turn keeps running and the slash answers
/// concurrently); any other prompt, or grace expiry, executes the cancel.
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_millis(400);

/// Slash commands that may be answered while an engine turn keeps running.
///
/// Membership rule: read-only reports, next-turn overrides, and commands
/// whose durable stores are independent of the live turn. Deliberately
/// EXCLUDED: commands that mutate conversation/session state or drive a
/// real turn (`compact`, `clear-history`, `clear-plan`, `undo`,
/// `checkpoint`, `rollback`, `export`, `diff`, `release`) and subprocess/
/// registry mutators (`plugins`, `mcp`) — those must stop the world.
const CONCURRENT_SAFE_SLASH_COMMANDS: &[&str] = &[
    "status",
    "usage",
    "version",
    "help",
    "memory",
    "skills",
    "profile",
    "awareness",
    "metacognition",
    "deliberation",
    "curator",
    "max-iterations",
    "goal",
    "subgoal",
    "sessions",
    "lineage",
    "ci",
    // Vesper-native cognitive-memory surface (durable stores are
    // independent of the running turn).
    "remember",
    "recall",
    "forget",
    "memories",
    "promote",
    "demote",
    "embedding",
    // Per-session reasoning-mode override (applies to the next turn).
    "reasoning",
    "repository",
    "meta-learning",
    "observability",
    "journey",
    "firewall",
    "sandbox",
    // Headless daemon health is a read-only report; answering it can never
    // disturb a running turn (VRO-13 PR-7).
    "daemon",
    // Explicit export writes a report from a transcript snapshot but does
    // not mutate the live model history, plan, workspace, or tool registry.
    "export",
];

/// Commands whose concurrency safety depends on the requested subcommand.
const CONDITIONAL_CONCURRENT_SLASH_COMMANDS: &[&str] = &["checkpoint", "plugins", "mcp"];

/// Advertised commands that must interrupt a live implementation turn.
const INTERRUPTING_SLASH_COMMANDS: &[&str] = &[
    "compact",
    "clear-plan",
    "clear-history",
    "diff",
    "undo",
    "rollback",
    "release",
    // Watcher registration mutates the durable watcher store the daemon
    // sweeps; it must stop the world to register coherently (VRO-13 PR-7).
    "watch",
];

/// True when `text` is a single slash command that can be answered
/// concurrently with a running engine turn (see
/// [`CONCURRENT_SAFE_SLASH_COMMANDS`]).
fn is_concurrent_safe_slash(text: &str) -> bool {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    let mut parts = rest.split_whitespace();
    let first = parts.next().unwrap_or("");
    if first.is_empty() {
        return false;
    }
    let lowered = first.to_ascii_lowercase();
    if CONCURRENT_SAFE_SLASH_COMMANDS
        .iter()
        .any(|command| *command == lowered)
    {
        return true;
    }
    if INTERRUPTING_SLASH_COMMANDS
        .iter()
        .any(|command| *command == lowered)
    {
        return false;
    }
    if !CONDITIONAL_CONCURRENT_SLASH_COMMANDS
        .iter()
        .any(|command| *command == lowered)
    {
        return false;
    }
    let subcommand = parts.next().unwrap_or("").to_ascii_lowercase();
    match lowered.as_str() {
        "checkpoint" => subcommand == "list",
        "plugins" => matches!(subcommand.as_str(), "" | "list" | "publishers" | "verify"),
        "mcp" => matches!(subcommand.as_str(), "" | "list" | "tools"),
        _ => false,
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
    controls: Option<SessionControlSurface>,
    additional_commands: Vec<vesper_domain::SlashCommandDescriptor>,
) -> Result<(), agent_client_protocol::Error> {
    let active = Arc::new(Mutex::new(BTreeMap::<SessionId, TurnId>::new()));
    let engine_active = Arc::new(Mutex::new(BTreeMap::<SessionId, usize>::new()));
    let pending_cancels = Arc::new(Mutex::new(BTreeMap::<SessionId, Instant>::new()));
    let permission_requester = Arc::new(AcpClientPermissionRequester::new(connection.clone()));
    let context = RequestContext {
        connection,
        barriers,
        active: Arc::clone(&active),
        engine_active: Arc::clone(&engine_active),
        pending_cancels: Arc::clone(&pending_cancels),
        ids,
        output_flow,
        prompt_engine,
        permission_requester,
        context_window,
        controls,
        additional_commands,
    };
    let mut prompts = JoinSet::new();
    loop {
        // Earliest pending grace cancel: fires the expiry branch below.
        // Recomputed every iteration so any add/remove resets the timer.
        // The lock is taken and RELEASED synchronously — the timer future
        // must never hold the guard across its await, or a prompt arriving
        // in this same task would self-deadlock on the grace lookup.
        let next_grace_deadline = pending_cancels.lock().await.values().min().copied();
        let cancel_timer = async {
            match next_grace_deadline {
                Some(deadline) => {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await
                }
                None => std::future::pending().await,
            }
        };
        tokio::pin!(cancel_timer);
        // `biased` + notifications-first: an editor sends cancel IMMEDIATELY
        // before its replacement/slash prompt. Polling the notification
        // branch first guarantees the pending cancel is registered before
        // the prompt's grace resolution runs, so the pair is always
        // evaluated together — never cancel-after-the-fact against the
        // replacement turn.
        tokio::select! {
            biased;
            notification = notifications.recv() => {
                let Some(notification) = notification else { break };
                if let ClientNotification::CancelNotification(cancel) = notification {
                    let session = session_id(&cancel.session_id);
                    if engine_active.lock().await.contains_key(&session) {
                        // Mid-turn slash grace (see CANCEL_GRACE): editors
                        // interrupt-then-prompt. Hold the cancel briefly;
                        // a safe slash prompt for this session will drop it
                        // and keep the turn running, everything else (or
                        // expiry) performs it.
                        pending_cancels
                            .lock()
                            .await
                            .insert(session, Instant::now() + CANCEL_GRACE);
                    } else {
                        perform_session_cancel(&runtime, &context, session).await;
                    }
                }
            }
            request = requests.recv() => {
                let Some(request) = request else { break };
                handle_request(
                    Arc::clone(&runtime),
                    request,
                    context.clone(),
                    &mut prompts,
                ).await?;
            }
            _ = &mut cancel_timer => {
                let now = Instant::now();
                let expired: Vec<SessionId> = pending_cancels
                    .lock()
                    .await
                    .iter()
                    .filter(|(_, deadline)| **deadline <= now)
                    .map(|(session, _)| session.clone())
                    .collect();
                for session in expired {
                    pending_cancels.lock().await.remove(&session);
                    perform_session_cancel(&runtime, &context, session).await;
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
    /// In-flight engine prompt count per session. Safe slash commands run
    /// concurrently with the implementation turn, so boolean/set tracking
    /// would let the first completed slash erase the still-running turn.
    engine_active: Arc<Mutex<BTreeMap<SessionId, usize>>>,
    /// Engine-turn cancels held inside the [`CANCEL_GRACE`] window, keyed by
    /// session (deadline as value). A safe slash prompt for the session
    /// removes the entry (turn survives); anything else executes it.
    pending_cancels: Arc<Mutex<BTreeMap<SessionId, Instant>>>,
    ids: Arc<AtomicU64>,
    output_flow: OutputFlow,
    prompt_engine: Option<Arc<dyn AcpPromptEngine>>,
    permission_requester: Arc<dyn AcpPermissionRequester>,
    /// Adapter context-window size forwarded to engine event sinks for
    /// `usage_update` notifications.
    context_window: u64,
    /// Provider-routed session control surface advertised as ACP config
    /// options (`None` keeps the provider-neutral fallbacks).
    controls: Option<SessionControlSurface>,
    additional_commands: Vec<vesper_domain::SlashCommandDescriptor>,
}

async fn decrement_engine_active(active: &Mutex<BTreeMap<SessionId, usize>>, session: &SessionId) {
    let mut active = active.lock().await;
    if let Some(count) = active.get_mut(session) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            active.remove(session);
        }
    }
}

/// Executes one session cancel end to end: permission-request cancellation,
/// engine-turn cancellation, and the runtime `CancelTurn` command for the
/// tracked runtime turn. Shared by the immediate path, the grace-expiry
/// path, and the interrupt-on-non-slash-prompt path.
async fn perform_session_cancel(
    runtime: &RuntimeSupervisor,
    context: &RequestContext,
    session: SessionId,
) {
    context.permission_requester.cancel(&session);
    if let Some(engine) = context.prompt_engine.as_ref()
        && context.engine_active.lock().await.contains_key(&session)
    {
        let _ = engine.cancel(&session).await;
    }
    if let Some(turn) = context.active.lock().await.get(&session).cloned() {
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

async fn handle_request(
    runtime: Arc<RuntimeSupervisor>,
    inbound: InboundRequest,
    context: RequestContext,
    prompts: &mut JoinSet<Result<(), agent_client_protocol::Error>>,
) -> Result<(), agent_client_protocol::Error> {
    let InboundRequest { request, responder } = inbound;
    // Mid-turn slash grace (see [`CANCEL_GRACE`]): resolve a cancel that an
    // editor sent immediately before this prompt. A safe slash command
    // aborts the pending cancel so the running engine turn keeps working
    // and this prompt answers concurrently; any other prompt is a genuine
    // interrupt — perform the cancel before dispatching it.
    if let ClientRequest::PromptRequest(prompt) = &request {
        let prompt_session = session_id(&prompt.session_id);
        if context
            .pending_cancels
            .lock()
            .await
            .remove(&prompt_session)
            .is_some()
        {
            let slash_text = match prompt.prompt.first() {
                Some(agent_client_protocol::schema::v1::ContentBlock::Text(text)) => {
                    Some(text.text.clone())
                }
                _ => None,
            };
            let keep_turn = matches!(&slash_text, Some(text) if is_concurrent_safe_slash(text));
            if !keep_turn {
                perform_session_cancel(&runtime, &context, prompt_session).await;
            }
        }
    }
    let RequestContext {
        connection,
        barriers,
        active,
        engine_active,
        pending_cancels: _,
        ids,
        output_flow,
        prompt_engine,
        permission_requester,
        context_window,
        controls,
        additional_commands,
    } = context;
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
            let config_options = session_config_options(&snapshot, controls.as_ref());
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
                &additional_commands,
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
            advertise_available_commands(
                &connection,
                &request.session_id,
                &output_flow,
                &additional_commands,
            )
            .await?;
            let snapshot = execute!(runtime.snapshot(&id));
            respond_json(
                responder,
                LoadSessionResponse::new()
                    .modes(session_modes(snapshot.operating_mode))
                    .config_options(session_config_options(&snapshot, controls.as_ref())),
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
            advertise_available_commands(
                &connection,
                &request.session_id,
                &output_flow,
                &additional_commands,
            )
            .await?;
            let snapshot = execute!(runtime.snapshot(&session_id(&request.session_id)));
            respond_json(
                responder,
                ResumeSessionResponse::new()
                    .modes(session_modes(snapshot.operating_mode))
                    .config_options(session_config_options(&snapshot, controls.as_ref())),
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
                let provider_configuration = Some(snapshot.provider_configuration.clone());
                let engine_model = Some(snapshot.model.clone());
                let engine_session = session.clone();
                let engine_content = content.clone();
                let engine_message_id = message_id.clone();
                let response_message_id = message_id.clone();
                let save_runtime = Arc::clone(&runtime);
                *engine_active
                    .lock()
                    .await
                    .entry(session.clone())
                    .or_insert(0) += 1;
                let engine_active_task = Arc::clone(&engine_active);
                let connection = connection.clone();
                let output_flow = output_flow.clone();
                let request_session = request.session_id.clone();
                let engine_context_window = context_window;
                prompts.spawn(async move {
                    let event_sink = Arc::new(AcpEngineEventSink::new(
                        connection.clone(),
                        request_session.clone(),
                        output_flow.clone(),
                        engine_context_window,
                    ));
                    let result = engine
                        .run(AcpPromptRequest {
                            session_id: engine_session.clone(),
                            content: engine_content.clone(),
                            history,
                            operating_mode,
                            permission_mode,
                            workspace_roots,
                            provider_configuration,
                            model: engine_model,
                            permission_requester: Some(Arc::clone(&permission_requester)),
                            event_sink: Some(event_sink.clone()),
                        })
                        .await;
                    match result {
                        Ok(result) => {
                            event_sink.drain().await?;
                            decrement_engine_active(&engine_active_task, &engine_session).await;
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
                            if let Some(history) = result.history_replacement {
                                if let Err(error) =
                                    save_runtime.replace_history(&engine_session, history).await
                                {
                                    return responder.respond_with_error(sdk_runtime_error(error));
                                }
                                if let Err(error) = save_runtime.save_session(&engine_session).await
                                {
                                    return responder.respond_with_error(sdk_runtime_error(error));
                                }
                            } else if result.persist_turn {
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
                            if !event_sink.streamed_content() {
                                let chunk = ContentChunk::new(ContentBlock::Text(
                                    TextContent::new(result.text.as_str()),
                                ));
                                connection
                                    .send_notification(SessionNotification::new(
                                        request_session,
                                        SessionUpdate::AgentMessageChunk(chunk),
                                    ))
                                    .map_err(agent_client_protocol::util::internal_error)?;
                                output_flow.wait_until_writer_accepts().await?;
                            }
                            responder.respond(prompt_response_value(
                                agent_client_protocol::schema::v1::StopReason::EndTurn,
                                &response_message_id,
                            ))
                        }
                        Err(error) => {
                            decrement_engine_active(&engine_active_task, &engine_session).await;
                            tracing::warn!(error = %error, "harness engine failed");
                            responder.respond_with_error(
                                agent_client_protocol::util::internal_error(error),
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
                        // `read` is the advertised oracle value id; the harness
                        // aliases are accepted for older clients.
                        "read" | "read-only" | "readonly" => SessionPermissionMode::ReadOnly,
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
                    // Provider-routed options (model picker, API plan,
                    // generation profile, …) are accepted only when the
                    // injected control surface contributed them; the value
                    // must be one of the provider's own options.
                    let Some(surface) = controls.as_ref() else {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params().data(json!({
                                "reason": "unsupported-session-config-option"
                            })),
                        );
                    };
                    let Some(value) = value else {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params().data(json!({
                                "reason": "session-config-option-requires-value"
                            })),
                        );
                    };
                    if !surface.accepts(&config_id, &value) {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params().data(json!({
                                "reason": "unsupported-session-config-value"
                            })),
                        );
                    }
                    let snapshot = execute!(runtime.snapshot(&session));
                    let Some(applied) =
                        surface.apply(&snapshot.provider_configuration, &config_id, &value)
                    else {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params().data(json!({
                                "reason": "unsupported-session-config-option"
                            })),
                        );
                    };
                    let configuration = applied.configuration;
                    let model = applied.model;
                    HarnessCommandPayload::UpdateProviderConfiguration {
                        session_id: Some(session.clone()),
                        configuration: configuration.values.clone(),
                        model,
                    }
                }
            };
            execute!(runtime.execute(command(payload, next_text_id(&ids, "config"))));
            let snapshot = execute!(runtime.snapshot(&session));
            respond_json(
                responder,
                SetSessionConfigOptionResponse::new(session_config_options(
                    &snapshot,
                    controls.as_ref(),
                )),
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
fn catalog_commands(
    additional: &[vesper_domain::SlashCommandDescriptor],
) -> Vec<agent_client_protocol::schema::v1::AvailableCommand> {
    use agent_client_protocol::schema::v1::AvailableCommand;
    vesper_domain::ORACLE_SLASH_COMMANDS
        .iter()
        .map(|command| AvailableCommand::new(command.name, command.description))
        .chain(
            additional
                .iter()
                .map(|command| AvailableCommand::new(command.name, command.description)),
        )
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
    additional_commands: &[vesper_domain::SlashCommandDescriptor],
) -> Result<(), agent_client_protocol::Error> {
    use agent_client_protocol::schema::v1::{
        AvailableCommandsUpdate, SessionNotification, SessionUpdate,
    };
    let notification = SessionNotification::new(
        session.clone(),
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(catalog_commands(
            additional_commands,
        ))),
    );
    connection
        .send_notification(notification)
        .map_err(agent_client_protocol::util::internal_error)?;
    output_flow.wait_until_writer_accepts().await?;
    Ok(())
}

#[cfg(test)]
mod command_catalog_tests {
    use super::*;

    #[test]
    fn frozen_catalog_stays_exact_and_extensions_append() {
        assert_eq!(catalog_commands(&[]).len(), 28);
        let commands = catalog_commands(&vesper_domain::HOST_PARITY_SLASH_COMMANDS);
        assert_eq!(commands.len(), 44);
        let json = serde_json::to_value(commands).unwrap();
        let names: Vec<_> = json
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.get("name").and_then(serde_json::Value::as_str))
            .collect();
        for command in vesper_domain::HOST_PARITY_SLASH_COMMANDS {
            assert!(
                names.contains(&command.name),
                "/{} was not advertised",
                command.name
            );
        }
    }
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
    tail: StdMutex<Option<oneshot::Receiver<Result<(), String>>>>,
    streamed_content: AtomicBool,
}

impl std::fmt::Debug for AcpEngineEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpEngineEventSink").finish_non_exhaustive()
    }
}

impl AcpEngineEventSink {
    fn new(
        connection: ConnectionTo<Client>,
        session_id: agent_client_protocol::schema::v1::SessionId,
        output_flow: OutputFlow,
        context_window: u64,
    ) -> Self {
        Self {
            connection,
            session_id,
            output_flow,
            context_window,
            tail: StdMutex::new(None),
            streamed_content: AtomicBool::new(false),
        }
    }

    fn context_window(&self) -> u64 {
        self.context_window
    }

    fn streamed_content(&self) -> bool {
        self.streamed_content.load(Ordering::Acquire)
    }

    fn publish(&self, update: agent_client_protocol::schema::v1::SessionUpdate) {
        let connection = self.connection.clone();
        let flow = self.output_flow.clone();
        let session = self.session_id.clone();
        let (completion_sender, completion_receiver) = oneshot::channel();
        let previous = self
            .tail
            .lock()
            .expect("ACP event-tail mutex poisoned")
            .replace(completion_receiver);
        // AcpEventSink is synchronous, so each update owns an async task. The
        // completion chain keeps those tasks strictly ordered and lets the
        // prompt path drain physical-writer acceptance before end_turn.
        tokio::spawn(async move {
            let result = if let Some(previous) = previous {
                previous
                    .await
                    .unwrap_or_else(|_| Err("ordered ACP event task ended early".to_owned()))
            } else {
                Ok(())
            };
            let result = match result {
                Ok(()) => match connection.send_notification(
                    agent_client_protocol::schema::v1::SessionNotification::new(session, update),
                ) {
                    Ok(()) => flow
                        .wait_until_writer_accepts()
                        .await
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                },
                Err(error) => Err(error),
            };
            let _ = completion_sender.send(result);
        });
    }

    async fn drain(&self) -> Result<(), agent_client_protocol::Error> {
        let tail = self
            .tail
            .lock()
            .expect("ACP event-tail mutex poisoned")
            .take();
        let Some(tail) = tail else {
            return Ok(());
        };
        tail.await
            .map_err(|_| {
                agent_client_protocol::util::internal_error(
                    "ordered ACP event task ended before completion",
                )
            })?
            .map_err(agent_client_protocol::util::internal_error)
    }
}

impl crate::engine::AcpEventSink for AcpEngineEventSink {
    fn event(&self, event: crate::engine::AcpEngineEvent) {
        use agent_client_protocol::schema::v1::{
            ContentBlock, ContentChunk, Plan, SessionUpdate, TextContent, ToolCall as AcpToolCall,
            ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
        };
        match event {
            crate::engine::AcpEngineEvent::ReasoningDelta { text } => {
                self.publish(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(text.as_str())),
                )));
            }
            crate::engine::AcpEngineEvent::ContentDelta { text } => {
                self.streamed_content.store(true, Ordering::Release);
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
                change,
            } => {
                let status = if success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                };
                let change_metadata = change.as_ref().map(|change| {
                    serde_json::json!({
                        "path": change.absolute_path,
                        "operation": change.operation,
                        "additions": change.additions,
                        "deletions": change.deletions,
                    })
                });
                let fields = ToolCallUpdateFields::new()
                    .title(name)
                    .status(status)
                    .raw_output(serde_json::json!({
                        "note": note,
                        "change": change_metadata,
                    }));
                self.publish(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    tool_call_id,
                    fields,
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
                self.publish(SessionUpdate::Plan(Plan::new(plan_entries_from_markdown(
                    &markdown,
                ))));
            }
        }
    }
}

/// Converts the canonical `update_plan` markdown artifact into ACP's
/// structured plan update so editor clients can render their native TODO UI.
fn plan_entries_from_markdown(markdown: &str) -> Vec<agent_client_protocol::schema::v1::PlanEntry> {
    use agent_client_protocol::schema::v1::{PlanEntry, PlanEntryPriority, PlanEntryStatus};

    markdown
        .lines()
        .filter_map(|line| {
            let (marker, remainder) = if let Some(rest) = line.strip_prefix("[x] ") {
                (PlanEntryStatus::Completed, rest)
            } else if let Some(rest) = line.strip_prefix("[~] ") {
                (PlanEntryStatus::InProgress, rest)
            } else if let Some(rest) = line.strip_prefix("[ ] ") {
                (PlanEntryStatus::Pending, rest)
            } else {
                return None;
            };
            let (_, after_number) = remainder.split_once(' ')?;
            let (metadata, content) = after_number.split_once(' ')?;
            let priority = match metadata
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
                .and_then(|value| value.rsplit_once('/'))
                .map(|(_, priority)| priority)
            {
                Some("high") => PlanEntryPriority::High,
                Some("low") => PlanEntryPriority::Low,
                _ => PlanEntryPriority::Medium,
            };
            Some(PlanEntry::new(content, priority, marker))
        })
        .collect()
}

#[cfg(test)]
mod live_plan_tests {
    use agent_client_protocol::schema::v1::{PlanEntryPriority, PlanEntryStatus};

    use super::plan_entries_from_markdown;

    #[test]
    fn canonical_markdown_maps_to_native_acp_todo_entries() {
        let entries = plan_entries_from_markdown(
            "# Plan\n\n[x] #1 (completed/high) Inspect lifecycle\n\
             [~] #2 (in_progress/medium) Fix ACP mapping\n\
             [ ] #3 (pending/low) Run regression tests\n",
        );

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].content, "Inspect lifecycle");
        assert_eq!(entries[0].priority, PlanEntryPriority::High);
        assert_eq!(entries[0].status, PlanEntryStatus::Completed);
        assert_eq!(entries[1].content, "Fix ACP mapping");
        assert_eq!(entries[1].status, PlanEntryStatus::InProgress);
        assert_eq!(entries[2].content, "Run regression tests");
        assert_eq!(entries[2].priority, PlanEntryPriority::Low);
        assert_eq!(entries[2].status, PlanEntryStatus::Pending);
    }

    #[test]
    fn cleared_or_taskless_markdown_maps_to_an_empty_plan() {
        assert!(plan_entries_from_markdown("").is_empty());
        assert!(plan_entries_from_markdown("# Plan\n\n_(no tasks)_\n").is_empty());
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

/// Provider-routed session control surface advertised as ACP config options
/// (see [`crate::controls::SessionControlSurface`]).
fn session_config_options(
    snapshot: &vesper_runtime::SessionSnapshot,
    surface: Option<&crate::controls::SessionControlSurface>,
) -> Vec<SessionConfigOption> {
    let mut options = Vec::new();
    if let Some(surface) = surface {
        options.extend(surface.acp_config_options(snapshot));
    } else {
        // No provider-routed surface: keep the provider-neutral fallbacks.
        options.push(thought_level_option(snapshot));
    }
    options.push(permission_option(snapshot));
    options
}

/// Provider-neutral `thought_level` fallback used only when no provider-routed
/// surface was injected. Mirrors the oracle dial but without current-value
/// tracking beyond the session reasoning override.
fn thought_level_option(snapshot: &vesper_runtime::SessionSnapshot) -> SessionConfigOption {
    let thought_level = snapshot
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.mode.as_ref())
        .map(|mode| mode.as_str().to_owned())
        .unwrap_or_else(|| "enabled".to_owned());
    SessionConfigOption::select(
        "thought_level",
        "Reasoning",
        SessionConfigValueId::new(thought_level),
        vec![
            SessionConfigSelectOption::new("disabled", "Off")
                .description("No reasoning — fast responses for simple tasks"),
            SessionConfigSelectOption::new("enabled", "Standard")
                .description("Full reasoning traces streamed live"),
            SessionConfigSelectOption::new("high", "Deep · High")
                .description("Deeper multi-step reasoning for complex tasks"),
            SessionConfigSelectOption::new("max", "Deep · Max")
                .description("Maximum reasoning depth — deepest analysis"),
        ],
    )
    .description("Live reasoning trace level")
    .category(SessionConfigOptionCategory::ThoughtLevel)
}

fn permission_option(snapshot: &vesper_runtime::SessionSnapshot) -> SessionConfigOption {
    let (current, read_only_value) = match snapshot.permission_mode {
        SessionPermissionMode::Ask => ("ask", "read"),
        SessionPermissionMode::Bypass => ("bypass", "read"),
        // Oracle value for read-only is `read`; the harness mode is
        // `read-only`. Advertise `read` so both agree.
        SessionPermissionMode::ReadOnly => ("read", "read"),
    };
    SessionConfigOption::select(
        "permission_mode",
        "Permissions",
        SessionConfigValueId::new(current),
        vec![
            SessionConfigSelectOption::new("ask", "Ask")
                .description("Approve file edits and commands before they run"),
            SessionConfigSelectOption::new("bypass", "Bypass")
                .description("Auto-approve everything — no prompts"),
            SessionConfigSelectOption::new(read_only_value, "Read Only")
                .description("Block all file edits and commands — read-only mode"),
        ],
    )
    .description("Tool execution permission mode")
    .category(SessionConfigOptionCategory::Other("permissions".to_owned()))
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

#[cfg(test)]
mod slash_grace_tests {
    use std::collections::BTreeSet;

    use super::{
        CONCURRENT_SAFE_SLASH_COMMANDS, CONDITIONAL_CONCURRENT_SLASH_COMMANDS,
        INTERRUPTING_SLASH_COMMANDS, is_concurrent_safe_slash,
    };

    #[test]
    fn informational_commands_are_safe_mid_turn() {
        for command in [
            "/status",
            "/usage",
            "/max-iterations 250",
            "/version",
            "/help",
            "/memory",
            "/skills",
            "/profile",
            "/awareness",
            "/curator",
            "/goal ship the release",
            "/remember --global my name is Alex",
            "/recall project layout",
            "/reasoning set mode=deep",
            "/memories",
            "/firewall",
            "/sandbox",
            "/export session.md",
            "/checkpoint list",
            "/plugins verify manifest.json",
            "/mcp tools local",
        ] {
            assert!(
                is_concurrent_safe_slash(command),
                "`{command}` must be safe mid-turn"
            );
        }
    }

    #[test]
    fn mutating_and_turn_driving_commands_are_not_safe() {
        for command in [
            "/compact",
            "/clear-history",
            "/clear-plan",
            "/undo",
            "/checkpoint ship",
            "/rollback 3",
            "/diff",
            "/release minor",
            "/plugins load x",
            "/mcp add srv",
        ] {
            assert!(
                !is_concurrent_safe_slash(command),
                "`{command}` must stop the world"
            );
        }
    }

    #[test]
    fn non_slash_text_and_edge_cases_are_not_safe() {
        assert!(!is_concurrent_safe_slash("status"));
        assert!(!is_concurrent_safe_slash("just a normal prompt"));
        assert!(!is_concurrent_safe_slash("/"));
        // Leading/trailing whitespace is trimmed: still a slash command.
        assert!(is_concurrent_safe_slash("  /status"));
        assert!(is_concurrent_safe_slash("  /status  extra"));
        // Unknown command names are not in the safe set.
        assert!(!is_concurrent_safe_slash("/totally-unknown"));
    }

    #[test]
    fn every_advertised_command_has_exactly_one_concurrency_classification() {
        let advertised = vesper_domain::ORACLE_SLASH_COMMANDS
            .iter()
            .chain(vesper_domain::HOST_PARITY_SLASH_COMMANDS.iter())
            .map(|command| command.name)
            .collect::<BTreeSet<_>>();
        let always = CONCURRENT_SAFE_SLASH_COMMANDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let conditional = CONDITIONAL_CONCURRENT_SLASH_COMMANDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let interrupting = INTERRUPTING_SLASH_COMMANDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        for name in &advertised {
            let classifications = usize::from(always.contains(name))
                + usize::from(conditional.contains(name))
                + usize::from(interrupting.contains(name));
            assert_eq!(
                classifications, 1,
                "advertised command `/{name}` must have exactly one concurrency classification"
            );
        }
        for classified in always
            .iter()
            .chain(conditional.iter())
            .chain(interrupting.iter())
        {
            assert!(
                advertised.contains(classified),
                "classified command `/{classified}` is not advertised"
            );
        }
    }
}
