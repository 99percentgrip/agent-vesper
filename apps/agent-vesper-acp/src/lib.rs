#![forbid(unsafe_code)]
//! Thin composition shared by the release binary and process-only conformance driver.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

mod controls;
mod lmstudio_provider;

use tokio::sync::Mutex;
use vesper_acp::{
    AcpAdapter, AcpAdapterConfig, AcpPermissionDecision, AcpPermissionRequest,
    AcpPermissionRequester, AcpPromptEngine, AcpPromptFuture, AcpPromptRequest, AcpPromptResult,
};
use vesper_config::{PathEnvironment, Platform, ProfileName, VesperPaths};
use vesper_domain::{
    ContentPart, ConversationMessage, EndpointId, ExtensionMap, MessageId, MessageRole, ModelId,
    ProviderId, QualifiedModelId,
};
use vesper_harness::{HarnessToolService, MemoryStores, WorkerFactory};
use vesper_provider::{ProviderConfiguration, ProviderFactory};
use vesper_provider_glm::{GlmFactory, provider_id};
#[cfg(feature = "integration-test-harness")]
use vesper_provider_synthetic::SyntheticFactory;
use vesper_runtime::{
    ProviderRegistry, RuntimeCancellation, RuntimeDefaults, RuntimeSessionReads,
    RuntimeSessionWrites, RuntimeSupervisor,
};
use vesper_sessions::{
    AgentVesperSessionLayout, CompatibilityAvailability, CompositeSessionRepository,
    DiscoveryBounds, EmptySessionRepository, FilesystemSessionStore, LegacyDecodeBounds,
    LegacySessionLayout, SessionRepository, SessionSource, VesperDecodeBounds, VesperSessionWriter,
    WriteBounds,
};

/// Runs ACP stdio with an injected provider factory.
///
/// The release binary selects the factory from `AGENT_VESPER_PROVIDER` or the
/// `--provider` CLI flag (see [`run`] and [`boot`]); the non-default
/// process-test driver may wrap a selected factory with generic synchronization
/// only. The provider configuration, model, and default endpoint are resolved
/// from the factory's own identity via [`ProviderProfile`], keeping the
/// runtime provider-neutral.
pub async fn run_with_factory<F>(factory: F) -> Result<(), ()>
where
    F: ProviderFactory + 'static,
    F::Session: 'static,
{
    let providers = Arc::new(ProviderRegistry::new());
    let provider = factory.provider_id().clone();
    providers.register(factory).await.map_err(|_| ())?;

    let profile = ProviderProfile::for_identity(&provider)?;
    let qualified_model = runtime_model(&profile.model, &provider);
    let session_reads = session_reads_from_environment(&qualified_model).map_err(|_| ())?;
    let session_writes = session_writes_from_environment().map_err(|_| ())?;
    let runtime = RuntimeSupervisor::new(
        Arc::clone(&providers),
        RuntimeDefaults {
            provider_configuration: profile.provider_configuration.clone(),
            model: qualified_model.clone(),
            endpoint: profile.endpoint,
            system_instructions: Vec::new(),
            reasoning: None,
            sampling: None,
            maximum_output_tokens: None,
        },
    );
    let runtime = match session_reads {
        Some(reads) => runtime.with_session_reads(Arc::new(reads)),
        None => runtime,
    };
    let runtime = match session_writes {
        Some(writes) => runtime.with_session_writes(writes),
        None => runtime,
    };
    let runtime = Arc::new(runtime);
    let adapter = AcpAdapter::new(
        runtime,
        AcpAdapterConfig {
            context_window: controls::glm_context_window(&profile.provider_configuration),
            controls: Some(controls::glm_control_surface(
                &profile.provider_configuration,
            )),
        },
    );
    let adapter = if full_harness_enabled() {
        let agent_config = vesper_agent::AgentLoopConfig {
            provider_id: provider.clone(),
            provider_configuration: profile.provider_configuration.clone(),
            model: qualified_model,
            system_instructions: Vec::new(),
            workspace_roots: Vec::new(),
            max_tool_iterations: vesper_agent::DEFAULT_MAX_TOOL_ITERATIONS,
        };
        let worker_factory = Arc::new(WorkerFactory::new(
            Arc::clone(&providers),
            agent_config.clone(),
        ));
        let hosted = Arc::new(HarnessToolService::new(
            Arc::new(MemoryStores::open_default()),
            checkpoint_root_path(),
            mcp_root_path(),
            Some(worker_factory),
        ));
        let engine = Arc::new(AcpHarnessEngine::new(
            Arc::clone(&providers),
            agent_config,
            hosted,
        ));
        adapter.with_prompt_engine(engine)
    } else {
        adapter
    };
    adapter.run_stdio().await.map_err(|_| ())
}

fn full_harness_enabled() -> bool {
    std::env::var("AGENT_VESPER_FULL_HARNESS")
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

fn checkpoint_root_path() -> PathBuf {
    std::env::var("AGENT_VESPER_CHECKPOINT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_agent_root("checkpoints"))
}

fn mcp_root_path() -> PathBuf {
    std::env::var("AGENT_VESPER_MCP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_agent_root("mcp"))
}

fn default_agent_root(name: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".agent-vesper")
        .join(name)
}

/// ACP composition engine that routes prompts through the same bounded
/// multi-turn loop and hosted tool surface used by the TUI.
struct AcpHarnessEngine {
    registry: Arc<ProviderRegistry>,
    config: vesper_agent::AgentLoopConfig,
    hosted: Arc<HarnessToolService>,
    histories: Mutex<BTreeMap<vesper_domain::SessionId, Vec<ConversationMessage>>>,
    cancellations: Mutex<BTreeMap<vesper_domain::SessionId, Arc<RuntimeCancellation>>>,
    /// Per-session slash-command overrides (`/max-iterations`, model/plan
    /// switches). Live for the process lifetime; slash turns themselves are
    /// never persisted (oracle parity).
    overrides:
        Mutex<BTreeMap<vesper_domain::SessionId, vesper_harness::slash_commands::SessionOverrides>>,
    /// Latest agent plan markdown per session (`/clear-plan` resets it and
    /// republishes an empty plan so ACP clients clear their plan panel).
    plans: Arc<std::sync::Mutex<BTreeMap<vesper_domain::SessionId, String>>>,
}

#[derive(Debug)]
struct AcpHarnessPermissionPort {
    requester: Arc<dyn AcpPermissionRequester>,
    session_id: vesper_domain::SessionId,
}

impl vesper_agent::PermissionPort for AcpHarnessPermissionPort {
    fn authorize<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        definition: &'a vesper_domain::ToolDefinition,
        _context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, vesper_agent::PermissionDecision> {
        let requester = Arc::clone(&self.requester);
        let request = AcpPermissionRequest {
            session_id: self.session_id.clone(),
            tool: call.tool_id.as_str().to_owned(),
            arguments: call.arguments.clone(),
            title: format!("Allow {}", definition.harness_name.as_str()),
            reason: format!("{} requires one-time approval", definition.description),
        };
        Box::pin(async move {
            match requester.request(request).await {
                AcpPermissionDecision::Allow => vesper_agent::PermissionDecision::Allow,
                AcpPermissionDecision::Cancelled => vesper_agent::PermissionDecision::Deny(
                    "ACP permission request cancelled".into(),
                ),
                AcpPermissionDecision::Deny => {
                    vesper_agent::PermissionDecision::Deny("ACP client rejected permission".into())
                }
            }
        })
    }
}

impl AcpHarnessEngine {
    fn new(
        registry: Arc<ProviderRegistry>,
        config: vesper_agent::AgentLoopConfig,
        hosted: Arc<HarnessToolService>,
    ) -> Self {
        Self {
            registry,
            config,
            hosted,
            histories: Mutex::new(BTreeMap::new()),
            cancellations: Mutex::new(BTreeMap::new()),
            overrides: Mutex::new(BTreeMap::new()),
            plans: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
        }
    }

    async fn run_inner(&self, request: AcpPromptRequest) -> Result<AcpPromptResult, String> {
        let text = request
            .content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return Ok(AcpPromptResult {
                text,
                cancelled: false,
                persist_turn: true,
            });
        }
        // Slash commands either answer in-process (never dispatched, never
        // persisted) or — for `/diff` and `/release` — replace the prompt
        // with a workflow that drives a real agent turn.
        let mut text = text;
        match self.try_slash_command(&request, &text).await {
            SlashFlow::Respond(result) => return Ok(result),
            SlashFlow::Workflow(prompt) => text = prompt,
            SlashFlow::Ordinary => {}
        }
        let message = ConversationMessage {
            id: MessageId::new(format!("acp-harness-{}", next_engine_id()))
                .map_err(|_| "message id bound exceeded".to_owned())?,
            role: MessageRole::User,
            content: vec![ContentPart::Text(
                vesper_domain::ContentText::new(text).map_err(|_| "prompt too large".to_owned())?,
            )],
            extensions: ExtensionMap::default(),
        };
        let history = {
            let mut histories = self.histories.lock().await;
            let history = histories
                .entry(request.session_id.clone())
                .or_insert(request.history);
            history.push(message);
            history.clone()
        };
        let mut config = self.config.clone();
        // Runtime session state first: footer selectors (ACP
        // `session/set_config_option`) land in the runtime snapshot, and the
        // adapter forwards that snapshot here. Merge those provider values
        // and the session model over the engine defaults so a footer pick
        // takes effect on the very next turn.
        if let Some(session_configuration) = request.provider_configuration.clone() {
            for (key, value) in session_configuration.values.values.iter() {
                let _ = config
                    .provider_configuration
                    .values
                    .values
                    .insert(key.to_owned(), value.clone());
            }
        }
        if let Some(session_model) = request.model.clone() {
            // Provider switch (ACP `provider` footer picker): the session
            // model carries the acting provider id after a provider switch;
            // sync the loop's dispatch identity so the next turn routes to
            // the selected adapter (TUI `/provider` parity). The model
            // envelope follows the same identity so the adapter sees a
            // consistent (provider, model) pair.
            config.provider_id = session_model.provider_id.clone();
            config.model = session_model;
        }
        {
            let overrides = self.overrides.lock().await;
            if let Some(session_overrides) = overrides.get(&request.session_id) {
                if let Some(cap) = session_overrides.max_tool_iterations {
                    config.max_tool_iterations = cap;
                }
                if let Some(model) = &session_overrides.model
                    && let Ok(model_id) = ModelId::new(model.clone())
                {
                    let provider = config.model.provider_id.clone();
                    config.model = runtime_model(&model_id, &provider);
                }
                let entries: &[(&str, Option<&String>)] = &[
                    (
                        "zai:endpoint-plan",
                        session_overrides.endpoint_plan.as_ref(),
                    ),
                    (
                        "zai:reasoning-mode",
                        session_overrides.reasoning_mode.as_ref(),
                    ),
                    (
                        "zai:generation-profile",
                        session_overrides.generation_profile.as_ref(),
                    ),
                    (
                        "zai:auxiliary-model",
                        session_overrides.auxiliary_model.as_ref(),
                    ),
                    ("zai:mixture-mode", session_overrides.mixture_mode.as_ref()),
                ];
                for (key, value) in entries {
                    if let Some(value) = value {
                        let _ = config
                            .provider_configuration
                            .values
                            .values
                            .insert((*key).to_owned(), serde_json::json!(value));
                    }
                }
            }
        }
        if !request.workspace_roots.is_empty() {
            config.workspace_roots = request.workspace_roots;
        }
        config.system_instructions = vesper_agent::project_instructions(&config.workspace_roots);
        let hosted = Arc::clone(&self.hosted);
        let permission_port: Arc<dyn vesper_agent::PermissionPort> = request
            .permission_requester
            .as_ref()
            .map(|requester| {
                Arc::new(AcpHarnessPermissionPort {
                    requester: Arc::clone(requester),
                    session_id: request.session_id.clone(),
                }) as Arc<dyn vesper_agent::PermissionPort>
            })
            .unwrap_or_else(|| Arc::new(vesper_agent::DenyPermissionPort));
        let loop_engine = vesper_agent::AgentLoop::new(
            Arc::clone(&self.registry),
            hosted.build_default_registry(),
            config,
        )
        .with_permission_port(permission_port)
        .with_progress_port(Arc::new(AcpEngineProgressPort {
            sink: request.event_sink.clone(),
            tool_seq: std::sync::atomic::AtomicU64::new(0),
            outstanding: std::sync::Mutex::new(BTreeMap::new()),
            session_id: request.session_id.clone(),
            plans: self.plans_shared(),
        }));
        let cancellation = Arc::new(RuntimeCancellation::new());
        self.cancellations
            .lock()
            .await
            .insert(request.session_id.clone(), Arc::clone(&cancellation));
        let run_result = loop_engine
            .run_prompt_with_history_with_cancellation(
                history,
                request.operating_mode,
                request.permission_mode,
                cancellation.clone(),
            )
            .await;
        self.cancellations.lock().await.remove(&request.session_id);
        if cancellation.is_cancelled() {
            return Ok(AcpPromptResult {
                text: String::new(),
                cancelled: true,
                persist_turn: true,
            });
        }
        let (outcome, history) = run_result.map_err(|error| {
            tracing::debug!("harness prompt failed: {error:?}");
            "harness prompt failed".to_owned()
        })?;
        self.histories
            .lock()
            .await
            .insert(request.session_id, history);
        Ok(AcpPromptResult {
            text: outcome_text(&outcome),
            cancelled: false,
            persist_turn: true,
        })
    }

    /// Executes one catalog slash command, or returns an oracle-parity
    /// unknown-command response for un-catalog `/` text. Returns
    /// `SlashFlow::Ordinary` for ordinary prompts so the multi-turn loop
    /// runs. Slash turns never dispatch the provider and are never persisted
    /// (fixtures/acp/slash-command parity) — except `/diff` and `/release`,
    /// which replace the prompt with a workflow that drives one real agent
    /// turn (TUI parity).
    async fn try_slash_command(&self, request: &AcpPromptRequest, text: &str) -> SlashFlow {
        use vesper_harness::slash_commands::{
            SlashCommandContext, SlashCommandOutcome, execute_slash_command,
        };
        let trimmed = text.trim();
        if !trimmed.starts_with('/') {
            return SlashFlow::Ordinary;
        }
        let slash_result = |body: String| {
            SlashFlow::Respond(AcpPromptResult {
                text: body,
                cancelled: false,
                persist_turn: false,
            })
        };
        let Some((name, argument)) = vesper_domain::parse_slash_command(trimmed) else {
            return slash_result(unknown_command_text(trimmed));
        };
        let stores = self.hosted.stores().clone();
        let visible_messages = self
            .histories
            .lock()
            .await
            .get(&request.session_id)
            .map_or(0, Vec::len);
        let config_value = |key: &str| -> String {
            request
                .provider_configuration
                .as_ref()
                .and_then(|configuration| configuration.values.values.get(key))
                .or_else(|| self.config.provider_configuration.values.values.get(key))
                .and_then(|value| value.as_str())
                .unwrap_or("default")
                .to_owned()
        };
        let context = SlashCommandContext {
            stores: Some(&stores),
            model: request
                .model
                .as_ref()
                .map(|model| model.model_id.as_str().to_owned())
                .unwrap_or_else(|| self.config.model.model_id.as_str().to_owned()),
            endpoint_plan: config_value("zai:endpoint-plan"),
            reasoning_mode: config_value("zai:reasoning-mode"),
            permission_mode: match request.permission_mode {
                vesper_domain::SessionPermissionMode::Ask => "ask".to_owned(),
                vesper_domain::SessionPermissionMode::Bypass => "bypass".to_owned(),
                vesper_domain::SessionPermissionMode::ReadOnly => "read-only".to_owned(),
            },
            operating_mode: request.operating_mode,
            quota_available: false,
            visible_messages,
            context_window: 0,
            tokens_used: 0,
        };
        match execute_slash_command(name, argument, &context) {
            SlashCommandOutcome::Text(body) => slash_result(body),
            SlashCommandOutcome::Override { overrides, text } => {
                let mut map = self.overrides.lock().await;
                let session_overrides = map.entry(request.session_id.clone()).or_default();
                if overrides.max_tool_iterations.is_some() {
                    session_overrides.max_tool_iterations = overrides.max_tool_iterations;
                }
                for (source, target) in [
                    (overrides.model, &mut session_overrides.model),
                    (
                        overrides.endpoint_plan,
                        &mut session_overrides.endpoint_plan,
                    ),
                    (
                        overrides.reasoning_mode,
                        &mut session_overrides.reasoning_mode,
                    ),
                    (
                        overrides.generation_profile,
                        &mut session_overrides.generation_profile,
                    ),
                    (
                        overrides.auxiliary_model,
                        &mut session_overrides.auxiliary_model,
                    ),
                    (overrides.mixture_mode, &mut session_overrides.mixture_mode),
                ] {
                    if source.is_some() {
                        *target = source;
                    }
                }
                drop(map);
                slash_result(text)
            }
            SlashCommandOutcome::Host(argument) => {
                self.host_owned_command(name, &argument, request).await
            }
            SlashCommandOutcome::Unknown(_) => slash_result(unknown_command_text(trimmed)),
        }
    }

    /// Serves one host-owned catalog command with full TUI parity:
    /// store-backed commands (`/checkpoint`, `/rollback`, `/undo`,
    /// `/export`, `/sessions`, `/lineage`, `/ci`, `/plugins`, `/mcp`) run on
    /// the shared `vesper-harness` host executor against the durable
    /// checkpoint/MCP roots; conversation-state commands (`/compact`,
    /// `/clear-history`, `/clear-plan`) mutate this engine's per-session
    /// history and plan maps; `/usage` queries the live provider quota
    /// endpoint; `/diff` and `/release` become workflow prompts for a real
    /// agent turn.
    async fn host_owned_command(
        &self,
        name: &str,
        argument: &str,
        request: &AcpPromptRequest,
    ) -> SlashFlow {
        let respond = |body: String| {
            SlashFlow::Respond(AcpPromptResult {
                text: body,
                cancelled: false,
                persist_turn: false,
            })
        };
        match name {
            "compact" => {
                let keep = parse_compact_keep(argument);
                let mut histories = self.histories.lock().await;
                match histories.get_mut(&request.session_id) {
                    Some(history) => {
                        let dropped = history.len().saturating_sub(keep);
                        if keep == 0 {
                            history.clear();
                        } else if history.len() > keep {
                            let drain_from = history.len() - keep;
                            history.drain(0..drain_from);
                        }
                        respond(format!(
                            "compact: dropped {dropped} older message(s); kept {} recent.",
                            history.len()
                        ))
                    }
                    None => respond("compact: no conversation history yet.".to_owned()),
                }
            }
            "clear-history" => {
                let removed = self
                    .histories
                    .lock()
                    .await
                    .remove(&request.session_id)
                    .map_or(0, |history: Vec<ConversationMessage>| history.len());
                respond(format!(
                    "clear-history: cleared {removed} message(s). Model and plan settings are kept."
                ))
            }
            "clear-plan" => {
                if let Ok(mut plans) = self.plans.lock() {
                    plans.remove(&request.session_id);
                }
                if let Some(sink) = request.event_sink.as_ref() {
                    sink.event(vesper_acp::AcpEngineEvent::PlanUpdated {
                        markdown: String::new(),
                    });
                }
                respond("plan: cleared (back to NORMAL).".to_owned())
            }
            "usage" => respond(self.usage_text().await),
            "diff" => SlashFlow::Workflow(
                "Run `git diff` (and `git diff --staged` if there are staged changes) \
                 and summarize the working-tree changes: files touched, lines added / \
                 removed, and a one-paragraph summary of what the changes do."
                    .to_owned(),
            ),
            "release" => SlashFlow::Workflow(format!(
                "Cut a {} release from this workspace. Bump the version, update the \
                 changelog, run the full verification gate, commit, tag, and push.",
                release_bump(argument)
            )),
            _ => {
                let session_id = request.session_id.as_str().to_owned();
                let workspace_root = workspace_root_path(&request.workspace_roots);
                let transcript = self.transcript_lines(&request.session_id).await;
                let hosted = Arc::clone(&self.hosted);
                let name = name.to_owned();
                let name_for_error = name.clone();
                let argument = argument.to_owned();
                let body = tokio::task::spawn_blocking(move || {
                    hosted.execute_host_command(
                        &name,
                        &argument,
                        &session_id,
                        &workspace_root,
                        &transcript,
                    )
                })
                .await
                .unwrap_or_else(|error| {
                    format!("/{name_for_error} failed — host executor panicked: {error}")
                });
                respond(body)
            }
        }
    }

    /// Queries the live provider plan-quota endpoint (TUI `/usage` parity).
    /// Only the installed GLM adapter registers a quota integration; every
    /// other provider reports that truthfully without a network call.
    async fn usage_text(&self) -> String {
        if self.config.model.provider_id != provider_id() {
            return "usage: The active provider has no registered quota integration.".to_owned();
        }
        let glm_config = match vesper_provider_glm::GlmConfig::from_provider_configuration(
            &self.config.provider_configuration,
        ) {
            Ok(config) => config,
            Err(error) => return format!("usage: quota configuration failed: {error}"),
        };
        let credential = match vesper_provider_glm::resolve_credential(
            &vesper_provider_glm::EnvironmentCredentialSource,
        ) {
            Ok(credential) => credential,
            Err(error) => return format!("usage: quota authentication failed: {error}"),
        };
        let session =
            match vesper_provider_glm::GlmSession::from_config(glm_config, credential.secret) {
                Ok(session) => session,
                Err(error) => return format!("usage: quota session failed: {error}"),
            };
        match session
            .query_plan_usage(Arc::new(RuntimeCancellation::new()))
            .await
        {
            Ok(usage) => format_glm_usage(&usage),
            Err(error) => format!("usage: quota query failed: {error}"),
        }
    }

    /// Renders the bounded history as `role: text` lines for `/export`.
    async fn transcript_lines(&self, session_id: &vesper_domain::SessionId) -> Vec<String> {
        self.histories
            .lock()
            .await
            .get(session_id)
            .map(|history| {
                history
                    .iter()
                    .map(|message| {
                        let role = match &message.role {
                            MessageRole::User => "user",
                            MessageRole::Assistant => "assistant",
                            MessageRole::Tool => "tool",
                            MessageRole::ProviderOpaque(_) => "provider",
                        };
                        let text = message
                            .content
                            .iter()
                            .filter_map(|part| match part {
                                ContentPart::Text(text) => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!("{role}: {text}")
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn plans_shared(&self) -> Arc<std::sync::Mutex<BTreeMap<vesper_domain::SessionId, String>>> {
        Arc::clone(&self.plans)
    }
}

/// Bridges bounded `AgentProgressEvent`s from the agent loop into ACP
/// session updates through the adapter's event sink. Constructed once per
/// turn; cheap to clone. `AgentProgressEvent` carries no tool-call ids, so
/// started ids are synthesized per name and finished events pair with the
/// most recent outstanding id of the same name (the agent loop executes
/// tool calls strictly sequentially).
struct AcpEngineProgressPort {
    sink: Option<Arc<dyn vesper_acp::AcpEventSink>>,
    tool_seq: std::sync::atomic::AtomicU64,
    /// Outstanding started tool-call ids by tool name, most recent last.
    outstanding: std::sync::Mutex<BTreeMap<String, Vec<String>>>,
    /// Session the turn belongs to (plan bookkeeping).
    session_id: vesper_domain::SessionId,
    /// Shared latest-plan map on the engine (`/clear-plan` resets it).
    plans: Arc<std::sync::Mutex<BTreeMap<vesper_domain::SessionId, String>>>,
}

impl vesper_agent::AgentProgressPort for AcpEngineProgressPort {
    fn emit(&self, event: vesper_agent::AgentProgressEvent) {
        use vesper_acp::AcpEngineEvent;
        // Plan bookkeeping happens even without a sink so `/clear-plan`
        // always reflects the latest engine-tracked plan.
        if let vesper_agent::AgentProgressEvent::PlanUpdated { markdown } = &event
            && let Ok(mut plans) = self.plans.lock()
        {
            plans.insert(self.session_id.clone(), markdown.clone());
        }
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        match event {
            vesper_agent::AgentProgressEvent::ReasoningDelta { text } => {
                sink.event(AcpEngineEvent::ReasoningDelta {
                    text: text.as_str().to_owned(),
                });
            }
            vesper_agent::AgentProgressEvent::ContentDelta { text } => {
                sink.event(AcpEngineEvent::ContentDelta {
                    text: text.as_str().to_owned(),
                });
            }
            vesper_agent::AgentProgressEvent::ToolStarted { name, hint } => {
                let seq = self
                    .tool_seq
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let tool_call_id = format!("acp-tool-{seq}");
                if let Ok(mut outstanding) = self.outstanding.lock() {
                    outstanding
                        .entry(name.clone())
                        .or_default()
                        .push(tool_call_id.clone());
                }
                sink.event(AcpEngineEvent::ToolStarted {
                    tool_call_id,
                    name,
                    hint,
                    arguments: serde_json::Value::Null,
                });
            }
            vesper_agent::AgentProgressEvent::ToolFinished {
                name,
                success,
                note,
            } => {
                let paired = self
                    .outstanding
                    .lock()
                    .ok()
                    .and_then(|mut outstanding| outstanding.get_mut(&name).and_then(Vec::pop))
                    .unwrap_or_else(|| format!("acp-tool-{name}"));
                sink.event(AcpEngineEvent::ToolFinished {
                    tool_call_id: paired,
                    name,
                    success,
                    note,
                });
            }
            vesper_agent::AgentProgressEvent::PlanUpdated { markdown } => {
                sink.event(AcpEngineEvent::PlanUpdated { markdown });
            }
            vesper_agent::AgentProgressEvent::UsageUpdated { usage } => {
                sink.event(AcpEngineEvent::Usage { usage: *usage });
            }
            vesper_agent::AgentProgressEvent::TurnStarted
            | vesper_agent::AgentProgressEvent::ProviderTurnStarted { .. } => {}
        }
    }
}

impl AcpPromptEngine for AcpHarnessEngine {
    fn run<'a>(
        &'a self,
        request: AcpPromptRequest,
    ) -> AcpPromptFuture<'a, Result<AcpPromptResult, String>> {
        Box::pin(self.run_inner(request))
    }

    fn cancel<'a>(&'a self, session_id: &'a vesper_domain::SessionId) -> AcpPromptFuture<'a, bool> {
        Box::pin(async move {
            let Some(cancellation) = self.cancellations.lock().await.get(session_id).cloned()
            else {
                return false;
            };
            cancellation.cancel();
            true
        })
    }
}

/// Oracle-parity unknown-command response for un-catalog `/` text. The list
/// is byte-stable against the frozen oracle's `_handle_command` fallback
/// (pinned commit bf4d4287).
fn unknown_command_text(command: &str) -> String {
    format!(
        "Unknown command: {command}\nAvailable commands: /compact, /help, /clear-plan, \
         /clear-history, /diff, /export, /status, /usage, /max-iterations, /memory, \
         /awareness, /metacognition, /deliberation, /repository, /meta-learning, \
         /skills, /profile, /curator, /sessions, /lineage, /goal, /subgoal, \
         /checkpoint, /rollback, /plugins, /version, /release, /ci, /mcp"
    )
}

/// What one prompt's slash analysis decided the engine should do.
enum SlashFlow {
    /// Not a slash command — run the ordinary multi-turn loop.
    Ordinary,
    /// Answer now with this result (never dispatched, never persisted).
    Respond(AcpPromptResult),
    /// Replace the prompt with this workflow text and run one real agent
    /// turn (TUI `/diff` and `/release` parity).
    Workflow(String),
}

/// Parses the optional keep-count for `/compact [N]` (TUI parity). Defaults
/// to 20; bounded to `[0, 1000]`.
fn parse_compact_keep(argument: &str) -> usize {
    if argument.trim().is_empty() {
        return 20;
    }
    match argument.trim().parse::<usize>() {
        Ok(n) => n.min(1000),
        Err(_) => 20,
    }
}

/// Resolves the bump level for `/release [patch|minor|major]` (TUI parity).
fn release_bump(argument: &str) -> &'static str {
    match argument.trim().to_ascii_lowercase().as_str() {
        "minor" => "minor",
        "major" => "major",
        _ => "patch",
    }
}

/// Resolves the workspace root for checkpoint confinement: the primary ACP
/// workspace root when supplied, else the first root, else the process
/// working directory.
fn workspace_root_path(roots: &[vesper_domain::WorkspaceRoot]) -> PathBuf {
    roots
        .iter()
        .find(|root| root.primary)
        .or_else(|| roots.first())
        .map(|root| PathBuf::from(root.path.as_str()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Renders one live plan-usage report (TUI `format_glm_usage` parity).
fn format_glm_usage(usage: &vesper_provider_glm::GlmPlanUsage) -> String {
    let windows = usage
        .quotas
        .iter()
        .map(|quota| {
            format!(
                "{}: used {}, remaining {}, limit {}{}",
                quota.kind,
                quota
                    .used
                    .map_or_else(|| "unknown".into(), |value| value.to_string()),
                quota
                    .remaining
                    .map_or_else(|| "unknown".into(), |value| value.to_string()),
                quota
                    .limit
                    .map_or_else(|| "unknown".into(), |value| value.to_string()),
                quota
                    .percentage
                    .map_or_else(String::new, |value| format!(" ({value:.1}%)")),
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("{} quota — {windows}", usage.platform)
}

fn outcome_text(outcome: &vesper_agent::AgentTurnOutcome) -> String {
    match outcome {
        vesper_agent::AgentTurnOutcome::Completed {
            assistant_content, ..
        } => assistant_content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        vesper_agent::AgentTurnOutcome::MaxIterationsReached { iterations } => {
            format!("agent reached the bounded tool-iteration limit ({iterations})")
        }
    }
}

fn next_engine_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static IDS: AtomicU64 = AtomicU64::new(1);
    IDS.fetch_add(1, Ordering::Relaxed)
}

fn runtime_model(model: &ModelId, provider: &ProviderId) -> QualifiedModelId {
    QualifiedModelId {
        provider_id: provider.clone(),
        model_id: model.clone(),
    }
}

/// Resolved provider configuration, model, and default endpoint for one boot.
///
/// The runtime is provider-neutral; the composition boundary supplies the
/// concrete provider configuration, qualified model, and default endpoint so
/// freshly created sessions carry a stable, persistable endpoint identity.
/// GLM credentials and endpoint overrides are consulted only when the GLM
/// adapter is selected. The feature-gated synthetic test adapter never touches
/// GLM credential resolution and is absent from normal production dispatch.
struct ProviderProfile {
    provider_configuration: ProviderConfiguration,
    model: ModelId,
    endpoint: EndpointId,
}

impl ProviderProfile {
    /// Resolves the profile for a concrete provider identity, failing closed
    /// for any unrecognised adapter.
    fn for_identity(provider: &ProviderId) -> Result<Self, ()> {
        if provider == &provider_id() {
            // GLM (zai): the production adapter. Endpoint and credential
            // overrides are only consulted here.
            let mut provider_configuration = GlmFactory::default_configuration();
            if let Ok(base_url) = std::env::var("AGENT_VESPER_GLM_BASE_URL") {
                let allow_insecure =
                    std::env::var_os("AGENT_VESPER_ALLOW_INSECURE_LOOPBACK").is_some();
                provider_configuration
                    .values
                    .values
                    .insert("zai:endpoint-plan", serde_json::json!("custom"))
                    .map_err(|_| ())?;
                provider_configuration
                    .values
                    .values
                    .insert("zai:base-url", serde_json::json!(base_url))
                    .map_err(|_| ())?;
                provider_configuration
                    .values
                    .values
                    .insert("zai:allow-insecure-http", serde_json::json!(allow_insecure))
                    .map_err(|_| ())?;
                provider_configuration
                    .values
                    .values
                    .insert("zai:attach-inference-auth", serde_json::json!(true))
                    .map_err(|_| ())?;
            }
            let model = ModelId::new(
                std::env::var("AGENT_VESPER_GLM_MODEL").unwrap_or_else(|_| "glm-5.3".into()),
            )
            .map_err(|_| ())?;
            let endpoint = EndpointId::new("zai-coding").map_err(|_| ())?;
            Ok(Self {
                provider_configuration,
                model,
                endpoint,
            })
        } else {
            #[cfg(feature = "integration-test-harness")]
            if provider == &vesper_provider_synthetic::provider_id() {
                // Synthetic: deterministic in-process reference adapter. No
                // credential, no endpoint override, no network dependency.
                return Ok(Self {
                    provider_configuration: SyntheticFactory::default_configuration(),
                    model: ModelId::new("synthetic-1").map_err(|_| ())?,
                    endpoint: EndpointId::new("synthetic").map_err(|_| ())?,
                });
            }
            if provider.as_str() == "lmstudio" {
                // LM Studio: local/LAN OpenAI-compatible server. The optional
                // API key comes from LMSTUDIO_API_KEY; no credential gate.
                return Ok(Self {
                    provider_configuration: crate::lmstudio_provider::LmStudioFactory::default_configuration(),
                    model: ModelId::new("local-model").map_err(|_| ())?,
                    endpoint: EndpointId::new("lmstudio-local").map_err(|_| ())?,
                });
            }
            Err(())
        }
    }
}

fn session_reads_from_environment(
    model: &QualifiedModelId,
) -> Result<Option<RuntimeSessionReads>, ()> {
    let settings = SessionReadSettings::from_environment()?;
    if !settings.enable_vesper && !settings.enable_legacy {
        return Ok(None);
    }
    settings.build(model)
}

/// Resolves the optional transactional session writer from environment
/// configuration. The writer is only constructed when persistence is opted in
/// via `AGENT_VESPER_ENABLE_SESSION_WRITES`. All filesystem mutation stays
/// owned by `VesperSessionWriter` inside `vesper-sessions`; the composition
/// binary constructs and injects it without performing any I/O itself.
///
/// Write root resolution order: explicit `AGENT_VESPER_SESSION_WRITE_ROOT`,
/// then the shared read root `AGENT_VESPER_SESSION_ROOT`, then the platform
/// Agent Vesper data root. The writer requires an absolute root whose parent
/// exists; deployment is responsible for that parent when the default is used.
fn session_writes_from_environment() -> Result<Option<Arc<RuntimeSessionWrites>>, ()> {
    if !enabled("AGENT_VESPER_ENABLE_SESSION_WRITES") {
        return Ok(None);
    }
    let home = std::env::var_os(home_variable()).map(PathBuf::from);
    let root = match std::env::var_os("AGENT_VESPER_SESSION_WRITE_ROOT").map(PathBuf::from) {
        Some(root) => root,
        None => match std::env::var_os("AGENT_VESPER_SESSION_ROOT").map(PathBuf::from) {
            Some(root) => root,
            None => default_vesper_root(home.as_deref())?,
        },
    };
    let max_session_bytes =
        bounded_number::<u64>("AGENT_VESPER_SESSION_WRITE_MAX_BYTES", 16 * 1024 * 1024)?;
    let max_session_bytes = usize::try_from(max_session_bytes).map_err(|_| ())?;
    let bounds = WriteBounds {
        max_session_bytes,
        ..WriteBounds::default()
    };
    let writer =
        VesperSessionWriter::new(root, SessionSource::AgentVesper, bounds).map_err(|_| ())?;
    Ok(Some(Arc::new(RuntimeSessionWrites::new(Arc::new(writer)))))
}

#[derive(Debug, Clone)]
struct SessionReadSettings {
    enable_vesper: bool,
    enable_legacy: bool,
    vesper_root: Option<PathBuf>,
    legacy_root: Option<PathBuf>,
    legacy_profile: Option<ProfileName>,
    home: Option<PathBuf>,
    max_session_bytes: u64,
    max_entries: usize,
}

impl SessionReadSettings {
    fn from_environment() -> Result<Self, ()> {
        Ok(Self {
            enable_vesper: enabled("AGENT_VESPER_ENABLE_SESSION_READS")
                || enabled("AGENT_VESPER_ENABLE_VESPER_SESSION_READS"),
            enable_legacy: enabled("AGENT_VESPER_ENABLE_LEGACY_SESSION_READS"),
            vesper_root: std::env::var_os("AGENT_VESPER_SESSION_ROOT").map(PathBuf::from),
            legacy_root: std::env::var_os("AGENT_VESPER_LEGACY_SESSION_ROOT").map(PathBuf::from),
            legacy_profile: std::env::var("AGENT_VESPER_LEGACY_PROFILE")
                .ok()
                .map(ProfileName::new)
                .transpose()
                .map_err(|_| ())?,
            home: std::env::var_os(home_variable()).map(PathBuf::from),
            max_session_bytes: bounded_number("AGENT_VESPER_SESSION_MAX_BYTES", 16 * 1024 * 1024)?,
            max_entries: bounded_number("AGENT_VESPER_SESSION_MAX_ENTRIES", 10_000)?,
        })
    }

    fn build(self, model: &QualifiedModelId) -> Result<Option<RuntimeSessionReads>, ()> {
        let bounds = DiscoveryBounds {
            max_entries: self.max_entries,
            max_session_bytes: self.max_session_bytes,
            ..DiscoveryBounds::default()
        };
        let memory: Arc<dyn SessionRepository> =
            Arc::new(EmptySessionRepository::new(SessionSource::InMemory).map_err(|_| ())?);
        let agent: Arc<dyn SessionRepository> = if self.enable_vesper {
            let root = match self.vesper_root {
                Some(root) => root,
                None => default_vesper_root(self.home.as_deref())?,
            };
            Arc::new(
                FilesystemSessionStore::new(root, SessionSource::AgentVesper, bounds)
                    .map_err(|_| ())?,
            )
        } else {
            Arc::new(EmptySessionRepository::new(SessionSource::AgentVesper).map_err(|_| ())?)
        };
        let legacy_source = SessionSource::LegacyNativeGlm {
            profile: self
                .legacy_profile
                .as_ref()
                .map(|profile| profile.as_str().to_owned()),
        };
        let legacy: Arc<dyn SessionRepository> = if self.enable_legacy {
            let root = match self.legacy_root {
                Some(root) => root,
                None => default_legacy_root(
                    self.home.as_deref().ok_or(())?,
                    self.legacy_profile.as_ref(),
                ),
            };
            Arc::new(
                FilesystemSessionStore::new(root, legacy_source.clone(), bounds).map_err(|_| ())?,
            )
        } else {
            Arc::new(EmptySessionRepository::new(legacy_source).map_err(|_| ())?)
        };
        let repository =
            Arc::new(CompositeSessionRepository::new(memory, agent, legacy).map_err(|_| ())?);
        let endpoint = EndpointId::new("zai-coding").map_err(|_| ())?;
        let mut availability = CompatibilityAvailability::default()
            .with_provider(model.provider_id.clone())
            .with_model(model.clone());
        for endpoint in [
            endpoint,
            EndpointId::new("zai-standard").map_err(|_| ())?,
            EndpointId::new("zai-bigmodel-cn").map_err(|_| ())?,
            EndpointId::new("zai-custom").map_err(|_| ())?,
        ] {
            availability = availability.with_endpoint(model.provider_id.clone(), endpoint);
        }
        Ok(Some(RuntimeSessionReads::new(
            repository,
            availability,
            LegacyDecodeBounds {
                max_file_bytes: usize::try_from(self.max_session_bytes).map_err(|_| ())?,
                ..LegacyDecodeBounds::default()
            },
            VesperDecodeBounds {
                max_file_bytes: usize::try_from(self.max_session_bytes).map_err(|_| ())?,
                ..VesperDecodeBounds::default()
            },
        )))
    }
}

fn enabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

fn bounded_number<T>(name: &str, default: T) -> Result<T, ()>
where
    T: std::str::FromStr + PartialEq + Default,
{
    let value = std::env::var(name)
        .ok()
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| ())?
        .unwrap_or(default);
    if value == T::default() {
        return Err(());
    }
    Ok(value)
}

fn default_vesper_root(home: Option<&std::path::Path>) -> Result<PathBuf, ()> {
    let environment = PathEnvironment {
        home: home.map(std::path::Path::to_path_buf),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        app_data: std::env::var_os("APPDATA").map(PathBuf::from),
        local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        ..PathEnvironment::default()
    };
    let paths = VesperPaths::resolve(current_platform(), &environment).map_err(|_| ())?;
    Ok(AgentVesperSessionLayout::from_paths(&paths)
        .root()
        .to_path_buf())
}

fn default_legacy_root(home: &std::path::Path, profile: Option<&ProfileName>) -> PathBuf {
    profile
        .map_or_else(
            || LegacySessionLayout::default_profile(home),
            |profile| LegacySessionLayout::named_profile(home, profile.clone()),
        )
        .root()
        .to_path_buf()
}

const fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

const fn home_variable() -> &'static str {
    if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    }
}

/// Runs the normal release composition with the provider selected by
/// `AGENT_VESPER_PROVIDER` (default `glm`).
///
/// Both production adapters (Z.ai GLM + LM Studio) are registered so the
/// ACP `provider` footer picker (TUI `/provider` parity) can switch between
/// them mid-session. The selected provider is the initial acting provider.
pub async fn run() -> Result<(), ()> {
    run_multi_provider(&selected_provider_token()).await
}

/// Boots the multi-provider composition: registers every production adapter,
/// then resolves the initial acting provider from the token.
///
/// TUI parity: the provider registry matches the TUI's
/// `register_default_providers` surface (GLM with full superpowers/credentials/
/// policy + LM Studio as the local/LAN adapter), and the ACP footer exposes a
/// `provider` dropdown with per-provider auth status descriptions. Switching
/// providers takes effect on the next turn; unauthenticated providers are
/// still selectable but each turn fails fast with the credential error until
/// the user authenticates (`--setup` for GLM, `LMSTUDIO_API_KEY` is optional).
pub async fn run_multi_provider(initial: &str) -> Result<(), ()> {
    let providers = Arc::new(ProviderRegistry::new());

    // Z.ai GLM (production default): full superpowers + credentials + policy.
    let glm = GlmFactory::default();
    let glm_superpowers = GlmFactory::default();
    let glm_credentials = GlmFactory::default();
    let glm_policy = vesper_provider_glm::GlmSuperpowerPolicy;
    providers
        .register_with_all(glm, glm_superpowers, glm_credentials, glm_policy)
        .await
        .map_err(|_| ())?;

    // LM Studio (local/LAN): registered always so the picker lists it.
    let lmstudio = lmstudio_provider::factory_from_settings();
    let lmstudio_superpowers = lmstudio_provider::factory_from_settings();
    let lmstudio_credentials = lmstudio_provider::factory_from_settings();
    providers
        .register_with_all(
            lmstudio,
            lmstudio_superpowers,
            lmstudio_credentials,
            vesper_provider::PermissiveSuperpowerPolicy,
        )
        .await
        .map_err(|_| ())?;

    // Feature-gated synthetic reference adapter (test-only).
    #[cfg(feature = "integration-test-harness")]
    {
        let synthetic = SyntheticFactory::default();
        let synthetic_superpowers = SyntheticFactory::default();
        providers
            .register_with_superpowers(synthetic, synthetic_superpowers)
            .await
            .map_err(|_| ())?;
    }

    // Resolve the initial acting provider (fail closed on unknown tokens).
    let initial_id = match initial {
        "glm" | "zai" => ProviderId::new("zai").map_err(|_| ())?,
        "lmstudio" => ProviderId::new("lmstudio").map_err(|_| ())?,
        #[cfg(feature = "integration-test-harness")]
        "synthetic" => vesper_provider_synthetic::provider_id(),
        _ => return Err(()),
    };

    let profile = ProviderProfile::for_identity(&initial_id)?;
    let qualified_model = runtime_model(&profile.model, &initial_id);
    let session_reads = session_reads_from_environment(&qualified_model).map_err(|_| ())?;
    let session_writes = session_writes_from_environment().map_err(|_| ())?;
    let runtime = RuntimeSupervisor::new(
        Arc::clone(&providers),
        RuntimeDefaults {
            provider_configuration: profile.provider_configuration.clone(),
            model: qualified_model.clone(),
            endpoint: profile.endpoint,
            system_instructions: Vec::new(),
            reasoning: None,
            sampling: None,
            maximum_output_tokens: None,
        },
    );
    let runtime = match session_reads {
        Some(reads) => runtime.with_session_reads(Arc::new(reads)),
        None => runtime,
    };
    let runtime = match session_writes {
        Some(writes) => runtime.with_session_writes(writes),
        None => runtime,
    };
    let runtime = Arc::new(runtime);

    // Build the picker's provider list with live auth status (TUI parity:
    // the TUI auth hub gates missing credentials; the ACP picker surfaces
    // the status in descriptions so the user knows to run --setup).
    let mut registered: Vec<(String, String, bool)> = Vec::new();
    for id in providers.provider_ids().await {
        let display = providers
            .descriptor(&id)
            .await
            .map(|d| d.display_name.as_str().to_owned())
            .unwrap_or_else(|| id.as_str().to_owned());
        let authenticated = providers
            .credential_present(&id)
            .await
            .unwrap_or(false);
        registered.push((id.as_str().to_owned(), display, authenticated));
    }

    let adapter = AcpAdapter::new(
        runtime,
        AcpAdapterConfig {
            context_window: controls::glm_context_window(&profile.provider_configuration),
            controls: Some(controls::multi_provider_control_surface(
                &profile.provider_configuration,
                &registered,
            )),
        },
    );
    let adapter = if full_harness_enabled() {
        let agent_config = vesper_agent::AgentLoopConfig {
            provider_id: initial_id.clone(),
            provider_configuration: profile.provider_configuration.clone(),
            model: qualified_model,
            system_instructions: Vec::new(),
            workspace_roots: Vec::new(),
            max_tool_iterations: vesper_agent::DEFAULT_MAX_TOOL_ITERATIONS,
        };
        let worker_factory = Arc::new(WorkerFactory::new(
            Arc::clone(&providers),
            agent_config.clone(),
        ));
        let hosted = Arc::new(HarnessToolService::new(
            Arc::new(MemoryStores::open_default()),
            checkpoint_root_path(),
            mcp_root_path(),
            Some(worker_factory),
        ));
        let engine = Arc::new(AcpHarnessEngine::new(
            Arc::clone(&providers),
            agent_config,
            hosted,
        ));
        adapter.with_prompt_engine(engine)
    } else {
        adapter
    };
    adapter.run_stdio().await.map_err(|_| ())
}

/// Boots the composition with an explicitly resolved provider token.
///
/// The composition boundary keeps the runtime provider-neutral: it maps a
/// provider token to the initial acting factory. `glm`/`zai` boot the Z.ai
/// GLM adapter (the production default), `lmstudio` boots the local/LAN
/// adapter. Under `integration-test-harness` only, `synthetic` boots the
/// deterministic reference adapter. Unknown production tokens fail closed
/// with a startup error rather than an ambiguous default. Every production
/// boot registers ALL adapters so the ACP `provider` picker can switch
/// between them mid-session (TUI `/provider` parity).
pub async fn boot(provider: &str) -> Result<(), ()> {
    match provider {
        "glm" | "zai" | "lmstudio" => run_multi_provider(provider).await,
        #[cfg(feature = "integration-test-harness")]
        "synthetic" => run_multi_provider(provider).await,
        _ => Err(()),
    }
}

/// Resolves the provider token from `AGENT_VESPER_PROVIDER`, defaulting to
/// `glm` so the production adapter remains the default when unset.
fn selected_provider_token() -> String {
    std::env::var("AGENT_VESPER_PROVIDER").unwrap_or_else(|_| String::from("glm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FakePermissionRequester(AcpPermissionDecision);

    impl AcpPermissionRequester for FakePermissionRequester {
        fn request<'a>(
            &'a self,
            _request: AcpPermissionRequest,
        ) -> AcpPromptFuture<'a, AcpPermissionDecision> {
            let decision = self.0;
            Box::pin(async move { decision })
        }
    }

    fn model() -> QualifiedModelId {
        runtime_model(&ModelId::new("glm-5.2").unwrap(), &provider_id())
    }

    #[derive(Debug)]
    struct RecordingEventSink(std::sync::Mutex<Vec<String>>);

    impl vesper_acp::AcpEventSink for RecordingEventSink {
        fn event(&self, event: vesper_acp::AcpEngineEvent) {
            use vesper_acp::AcpEngineEvent;
            let rendered = match event {
                AcpEngineEvent::ToolStarted { tool_call_id, .. } => {
                    format!("started:{tool_call_id}")
                }
                AcpEngineEvent::ToolFinished {
                    tool_call_id,
                    success,
                    ..
                } => format!("finished:{tool_call_id}:{}", success),
                AcpEngineEvent::ReasoningDelta { .. }
                | AcpEngineEvent::ContentDelta { .. }
                | AcpEngineEvent::Usage { .. }
                | AcpEngineEvent::PlanUpdated { .. } => String::new(),
            };
            self.0.lock().unwrap().push(rendered);
        }
    }

    #[test]
    fn tool_started_and_finished_pair_by_outstanding_id() {
        let recording = Arc::new(RecordingEventSink(std::sync::Mutex::new(Vec::new())));
        let port = AcpEngineProgressPort {
            sink: Some(recording.clone()),
            tool_seq: std::sync::atomic::AtomicU64::new(0),
            outstanding: std::sync::Mutex::new(BTreeMap::new()),
            session_id: vesper_domain::SessionId::new("sess-test").unwrap(),
            plans: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
        };
        vesper_agent::AgentProgressPort::emit(
            &port,
            vesper_agent::AgentProgressEvent::ToolStarted {
                name: "read_file".to_owned(),
                hint: "path=src/main.rs".to_owned(),
            },
        );
        vesper_agent::AgentProgressPort::emit(
            &port,
            vesper_agent::AgentProgressEvent::ToolStarted {
                name: "read_file".to_owned(),
                hint: "path=src/lib.rs".to_owned(),
            },
        );
        vesper_agent::AgentProgressPort::emit(
            &port,
            vesper_agent::AgentProgressEvent::ToolFinished {
                name: "read_file".to_owned(),
                success: true,
                note: "43 lines".to_owned(),
            },
        );
        let events = recording.0.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![
                "started:acp-tool-0".to_owned(),
                "started:acp-tool-1".to_owned(),
                "finished:acp-tool-1:true".to_owned(),
            ]
        );
    }

    #[test]
    fn unknown_command_text_matches_oracle_fallback_format() {
        let text = unknown_command_text("/future-command");
        assert!(text.starts_with("Unknown command: /future-command\n"));
        assert!(text.contains("/max-iterations, /memory"));
        assert!(text.ends_with("/version, /release, /ci, /mcp"));
    }

    #[test]
    fn plan_updated_events_are_recorded_per_session() {
        let plans: Arc<std::sync::Mutex<BTreeMap<vesper_domain::SessionId, String>>> =
            Arc::new(std::sync::Mutex::new(BTreeMap::new()));
        let port = AcpEngineProgressPort {
            sink: None,
            tool_seq: std::sync::atomic::AtomicU64::new(0),
            outstanding: std::sync::Mutex::new(BTreeMap::new()),
            session_id: vesper_domain::SessionId::new("sess-plan").unwrap(),
            plans: Arc::clone(&plans),
        };
        vesper_agent::AgentProgressPort::emit(
            &port,
            vesper_agent::AgentProgressEvent::PlanUpdated {
                markdown: "## Step 1".to_owned(),
            },
        );
        assert_eq!(
            plans
                .lock()
                .unwrap()
                .get(&vesper_domain::SessionId::new("sess-plan").unwrap()),
            Some(&"## Step 1".to_owned())
        );
    }

    #[test]
    fn compact_and_release_helpers_match_tui_semantics() {
        assert_eq!(parse_compact_keep(""), 20);
        assert_eq!(parse_compact_keep("5"), 5);
        assert_eq!(parse_compact_keep("9999"), 1000);
        assert_eq!(parse_compact_keep("not-a-number"), 20);
        assert_eq!(release_bump(""), "patch");
        assert_eq!(release_bump("MINOR"), "minor");
        assert_eq!(release_bump("major"), "major");
    }

    #[test]
    fn workspace_root_prefers_the_primary_root() {
        use vesper_domain::WorkspaceRoot;
        let name = |text: &str| vesper_domain::BoundedString::<256>::new(text).unwrap();
        let path = |text: &str| vesper_domain::BoundedString::<32768>::new(text).unwrap();
        let roots = vec![
            WorkspaceRoot {
                name: name("secondary"),
                path: path("/tmp/secondary"),
                primary: false,
            },
            WorkspaceRoot {
                name: name("primary"),
                path: path("/tmp/primary"),
                primary: true,
            },
        ];
        assert_eq!(workspace_root_path(&roots), PathBuf::from("/tmp/primary"));
        let only = vec![WorkspaceRoot {
            name: name("only"),
            path: path("/tmp/only"),
            primary: false,
        }];
        assert_eq!(workspace_root_path(&only), PathBuf::from("/tmp/only"));
    }

    #[test]
    fn explicit_missing_roots_configure_readers_without_creating_directories() {
        let base = std::env::temp_dir().join(format!(
            "agent-vesper-session-config-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let vesper_root = base.join("vesper");
        let legacy_root = base.join("legacy");
        let reads = SessionReadSettings {
            enable_vesper: true,
            enable_legacy: true,
            vesper_root: Some(vesper_root.clone()),
            legacy_root: Some(legacy_root.clone()),
            legacy_profile: None,
            home: None,
            max_session_bytes: 4096,
            max_entries: 10,
        }
        .build(&model())
        .unwrap();
        assert!(reads.is_some());
        assert!(!vesper_root.exists());
        assert!(!legacy_root.exists());
    }

    #[test]
    fn unsafe_relative_roots_fail_closed() {
        let result = SessionReadSettings {
            enable_vesper: true,
            enable_legacy: false,
            vesper_root: Some(PathBuf::from("relative/sessions")),
            legacy_root: None,
            legacy_profile: None,
            home: None,
            max_session_bytes: 4096,
            max_entries: 10,
        }
        .build(&model());
        assert!(result.is_err());
        assert!(!PathBuf::from("relative/sessions").exists());
    }

    #[tokio::test]
    async fn acp_permission_port_maps_client_decisions_and_preserves_arguments() {
        let requester = Arc::new(FakePermissionRequester(AcpPermissionDecision::Allow));
        let port = AcpHarnessPermissionPort {
            requester,
            session_id: vesper_domain::SessionId::new("permission-session").unwrap(),
        };
        let call = vesper_domain::ToolCall {
            id: vesper_domain::ToolCallId::new("permission-call").unwrap(),
            tool_id: vesper_domain::ToolId::new("write_file").unwrap(),
            arguments: serde_json::json!({"path":"notes.txt","content":"bounded"}),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        let definition = vesper_agent::schema_definition(
            "write_file",
            "Write a file",
            vesper_domain::ToolExecutionClass::Mutating,
            &[("path", "string", true), ("content", "string", true)],
        );
        let context = vesper_agent::ToolContext {
            workspace_roots: Vec::new(),
            operating_mode: vesper_domain::SessionOperatingMode::Code,
            permission_mode: vesper_domain::SessionPermissionMode::Ask,
            conversation: Vec::new(),
            cancellation: Arc::new(RuntimeCancellation::new()),
        };
        assert_eq!(
            vesper_agent::PermissionPort::authorize(&port, &call, &definition, &context).await,
            vesper_agent::PermissionDecision::Allow
        );
    }
}
