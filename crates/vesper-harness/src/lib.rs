#![forbid(unsafe_code)]
//! Shared hosted tool services for the production ACP and TUI compositions.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use vesper_agent::{AgentLoop, AgentTurnOutcome, ToolRegistry, ToolService};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ConversationMessage, ExtensionMap, MessageId,
    MessageRole, SessionOperatingMode, SessionPermissionMode, WorkspaceRoot,
};

fn session_root_path() -> PathBuf {
    let root = match std::env::var("AGENT_VESPER_SESSION_ROOT") {
        Ok(value) => PathBuf::from(value),
        Err(_) => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".agent-vesper")
            .join("sessions"),
    };
    if root.is_absolute() {
        root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(root)
    }
}

fn build_user_message(text: &str) -> ConversationMessage {
    static SEQ: AtomicU64 = AtomicU64::new(10_000);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    ConversationMessage {
        id: MessageId::new(format!("harness-worker-{n}")).expect("bounded worker message id"),
        role: MessageRole::User,
        content: vec![ContentPart::Text(ContentText::new(text).unwrap_or_else(
            |_| ContentText::new("[prompt too large]").expect("bounded"),
        ))],
        extensions: ExtensionMap::default(),
    }
}

fn load_trusted_publishers(root: &Path) -> vesper_mcp::TrustedPublishers {
    let path = root.join("publishers.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return vesper_mcp::TrustedPublishers::new();
    };
    let entries = text
        .lines()
        .filter_map(|line| serde_json::from_str::<vesper_mcp::TrustedPublisher>(line.trim()).ok())
        .collect::<Vec<_>>();
    vesper_mcp::TrustedPublishers::from_records(entries)
}

fn tui_tool_failure(name: &str, error: impl std::fmt::Display) -> vesper_agent::ToolError {
    vesper_agent::ToolError::Failed(format!("{name} failed: {error}"))
}

/// Prefix the gateway executor and `mcp_list_tools` injection use to name
/// dynamically-discovered MCP tools so the agent loop's gateway routing
/// recognizes them. Format: `mcp__<server>__<tool>`.
const MCP_GATEWAY_PREFIX: &str = "mcp__";

/// Parses an `mcp__<server>__<tool>` gateway name into `(server, tool)`.
/// Returns `None` when the name does not match the expected shape so the
/// gateway can return a clear error rather than dispatching with a
/// malformed server/tool pair.
fn parse_mcp_gateway_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix(MCP_GATEWAY_PREFIX)?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

/// Translates one advertised MCP tool descriptor into a provider-neutral
/// [`ToolDefinition`] whose `harness_name` matches the gateway prefix so
/// the agent loop can both advertise and execute it after a `mcp_list_tools`
/// discovery call.
///
/// `defer_loading` is `false` because these definitions are surfaced when
/// the model has actively requested them via `mcp_list_tools` — they should
/// be live in the next turn's advertisement, not hidden behind another
/// discovery step.
fn mcp_descriptor_to_tool_definition(
    server_id: &str,
    descriptor: &vesper_mcp::McpToolDescriptor,
) -> vesper_domain::ToolDefinition {
    let harness_name_value = format!("mcp__{server_id}__{}", descriptor.name);
    let description = descriptor
        .description
        .clone()
        .unwrap_or_else(|| format!("MCP tool `{}` from server `{}`", descriptor.name, server_id));
    let input_schema = descriptor
        .input_schema
        .clone()
        .unwrap_or_else(|| serde_json::json!({"type": "object"}));
    vesper_domain::ToolDefinition {
        id: vesper_domain::ToolId::new(&harness_name_value).unwrap_or_else(|_| {
            vesper_domain::ToolId::new("mcp_tool").expect("static fallback id")
        }),
        harness_name: vesper_domain::HarnessToolName::new(&harness_name_value)
            .expect("bounded harness name"),
        provider_name: None,
        description,
        input_schema,
        execution_class: vesper_domain::ToolExecutionClass::NestedWorkflow,
        extensions: vesper_domain::ExtensionMap::default(),
        defer_loading: false,
    }
}

/// Gateway executor for `mcp__<server>__<tool>` tool names. The harness
/// registers ONE of these under the `mcp__` prefix; the agent-loop registry
/// routes any call whose name starts with `mcp__` to it. This executor
/// parses out the server id and tool name, then dispatches via
/// [`vesper_mcp::McpClient::call_tool`].
pub struct McpGatewayExecutor {
    plugin_root: std::path::PathBuf,
}

impl std::fmt::Debug for McpGatewayExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpGatewayExecutor")
            .field("plugin_root", &self.plugin_root)
            .finish_non_exhaustive()
    }
}

impl McpGatewayExecutor {
    /// Creates a gateway executor rooted at `plugin_root`, the same root the
    /// `McpRegistry` lives in.
    #[must_use]
    pub fn new(plugin_root: std::path::PathBuf) -> Self {
        Self { plugin_root }
    }
}

impl vesper_agent::ToolExecutor for McpGatewayExecutor {
    fn definition(&self) -> vesper_domain::ToolDefinition {
        // The gateway has no single definition — it dispatches by name. The
        // stub definition here is informational only; the loop never
        // advertises it because gateways are registered via `with_gateway`,
        // not `with_service`, and `definitions_for` consults `entries`.
        vesper_agent::schema_definition(
            "mcp_gateway",
            "Gateway executor for dynamically-discovered MCP tools (mcp__<server>__<tool>).",
            vesper_domain::ToolExecutionClass::NestedWorkflow,
            &[],
        )
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        _context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        let name = call.tool_id.as_str().to_owned();
        let arguments = call.arguments.clone();
        let plugin_root = self.plugin_root.clone();
        Box::pin(async move {
            let (server, tool) = parse_mcp_gateway_name(&name).ok_or_else(|| {
                vesper_agent::ToolError::Failed(format!(
                    "MCP gateway name `{name}` is not `{MCP_GATEWAY_PREFIX}<server>__<tool>`"
                ))
            })?;
            let server_owned = server.to_owned();
            let tool_owned = tool.to_owned();
            let result = tokio::task::spawn_blocking(move || {
                let registry = vesper_mcp::McpRegistry::open(&plugin_root)
                    .map_err(|error| tui_tool_failure("mcp_gateway", error))?;
                let server = registry.get_with_builtins(&server_owned).ok_or_else(|| {
                    vesper_agent::ToolError::Failed(format!("MCP server not found: {server_owned}"))
                })?;
                vesper_mcp::McpClient::call_tool(&server, &tool_owned, arguments)
                    .map_err(|error| tui_tool_failure("mcp_gateway", error))
            })
            .await
            .map_err(|_| vesper_agent::ToolError::Failed("mcp gateway task failed".into()))??;
            vesper_agent::ToolResult::new(truncate_text(
                &serde_json::to_string(&result)
                    .map_err(|error| tui_tool_failure("mcp_gateway", error))?,
                16_000,
            ))
        })
    }
}

fn mcp_result(
    name: &str,
    registry_root: &std::path::Path,
    server_id: String,
    tool: String,
    arguments: serde_json::Value,
) -> impl std::future::Future<Output = Result<vesper_agent::ToolResult, vesper_agent::ToolError>> + Send
{
    let registry_root = registry_root.to_path_buf();
    let error_name = name.to_owned();
    async move {
        let task_error_name = error_name.clone();
        let result = tokio::task::spawn_blocking(move || {
            let registry = vesper_mcp::McpRegistry::open(&registry_root)
                .map_err(|error| tui_tool_failure(&task_error_name, error))?;
            let server = registry.get_with_builtins(&server_id).ok_or_else(|| {
                vesper_agent::ToolError::Failed(format!("MCP server not found: {server_id}"))
            })?;
            vesper_mcp::McpClient::call_tool(&server, &tool, arguments)
                .map_err(|error| tui_tool_failure(&task_error_name, error))
        })
        .await
        .map_err(|_| vesper_agent::ToolError::Failed("MCP call task failed".into()))??;
        vesper_agent::ToolResult::new(truncate_text(
            &serde_json::to_string(&result)
                .map_err(|error| tui_tool_failure(&error_name, error))?,
            16_000,
        ))
    }
}

/// Executes the tool families that need a workspace/session boundary in
/// addition to the durable memory stores. Keeping this at the composition
/// boundary prevents the provider-neutral agent crate from depending on
/// checkpoints, plugins, or frontend state.
#[allow(clippy::too_many_arguments)]
async fn execute_extended_tui_tool(
    name: &str,
    arguments: &serde_json::Value,
    context: &vesper_agent::ToolContext,
    core: &ToolRegistry,
    cron_root: &std::path::Path,
    plugin_loader: Option<&vesper_mcp::PluginLoader>,
    trusted_publishers: &vesper_mcp::TrustedPublishers,
    plugin_root: &std::path::Path,
    session_root: &std::path::Path,
    worker_factory: Option<&WorkerFactory>,
    worker_service: Arc<HarnessToolService>,
) -> Result<vesper_agent::ToolResult, vesper_agent::ToolError> {
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use vesper_agent::confinement::{confine, primary_root};
    use vesper_domain::{ContentPart, MessageRole, ToolCall, ToolCallId, ToolId};

    let required_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                tool: name.to_owned(),
                reason: format!("missing string argument `{key}`"),
            })
    };
    let optional_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let root = primary_root(context)?;

    match name {
        "search_tools" => {
            let intent = required_string("intent")?;
            if intent.chars().count() > 500 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "intent exceeds 500 characters".into(),
                });
            }
            let mode = optional_string("mode").unwrap_or_else(|| "bm25".into());
            if !matches!(mode.as_str(), "bm25" | "regex") {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "mode must be `bm25` or `regex`".into(),
                });
            }
            let definitions = worker_service.definitions();
            let mut matches = if mode == "regex" {
                if intent.chars().count() > 200 {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "regex intent exceeds 200 characters".into(),
                    });
                }
                let pattern = regex::RegexBuilder::new(&intent)
                    .case_insensitive(true)
                    .build()
                    .map_err(|error| tui_tool_failure(name, error))?;
                definitions
                    .into_iter()
                    .filter(|definition| {
                        pattern.is_match(definition.harness_name.as_str())
                            || pattern.is_match(&definition.description)
                    })
                    .map(|definition| (1usize, definition))
                    .collect::<Vec<_>>()
            } else {
                let terms = intent
                    .split_whitespace()
                    .map(str::to_ascii_lowercase)
                    .filter(|term| !term.is_empty())
                    .collect::<Vec<_>>();
                definitions
                    .into_iter()
                    .filter_map(|definition| {
                        let haystack = format!(
                            "{} {}",
                            definition.harness_name.as_str().to_ascii_lowercase(),
                            definition.description.to_ascii_lowercase()
                        );
                        let score = terms
                            .iter()
                            .filter(|term| haystack.contains(term.as_str()))
                            .count();
                        (score > 0).then_some((score, definition))
                    })
                    .collect::<Vec<_>>()
            };
            matches.sort_by(|left, right| {
                right.0.cmp(&left.0).then_with(|| {
                    left.1
                        .harness_name
                        .as_str()
                        .cmp(right.1.harness_name.as_str())
                })
            });
            matches.truncate(5);
            let output = matches
                .into_iter()
                .map(|(_, definition)| {
                    serde_json::json!({
                        "name": definition.harness_name.as_str(),
                        "description": definition.description,
                        "input_schema": definition.input_schema,
                    })
                })
                .collect::<Vec<_>>();
            vesper_agent::ToolResult::new(
                serde_json::to_string(&output).map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "web_search" => {
            let query = required_string("query")?;
            if query.chars().count() > 2_000 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "query exceeds 2000 characters".into(),
                });
            }
            mcp_result(
                name,
                plugin_root,
                "zai_search".into(),
                "web_search_prime".into(),
                serde_json::json!({"search_query": query}),
            )
            .await
        }
        "web_reader" => {
            let url = required_string("url")?;
            if url.chars().count() > 2_000
                || !(url.starts_with("https://") || url.starts_with("http://"))
            {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "url must be an http(s) URL of at most 2000 characters".into(),
                });
            }
            mcp_result(
                name,
                plugin_root,
                "zai_reader".into(),
                "webReader".into(),
                serde_json::json!({"url": url}),
            )
            .await
        }
        "vision_analyze" => {
            let path = required_string("path")?;
            let prompt = required_string("prompt")?;
            if prompt.chars().count() > 2_000 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "prompt exceeds 2000 characters".into(),
                });
            }
            let path = confine(root, &path)?;
            if !path.is_file() {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "image path must refer to a workspace file".into(),
                });
            }
            mcp_result(
                name,
                plugin_root,
                "zai_vision".into(),
                "image_analysis".into(),
                serde_json::json!({
                    "image_path": path.to_string_lossy(),
                    "prompt": prompt,
                }),
            )
            .await
        }
        "browser_ui" => {
            let action = required_string("action")?;
            let tool = match action.as_str() {
                "navigate" => "browser_navigate",
                "snapshot" => "browser_snapshot",
                "console" => "browser_console_messages",
                "network" => "browser_network_requests",
                "screenshot" => "browser_take_screenshot",
                "click" => "browser_click",
                "type" => "browser_type",
                "fill_form" => "browser_fill_form",
                "press_key" => "browser_press_key",
                "wait" => "browser_wait_for",
                "close" => "browser_close",
                _ => {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "unsupported browser action".into(),
                    });
                }
            };
            let call_arguments = arguments.get("arguments").cloned().ok_or_else(|| {
                vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "missing object argument `arguments`".into(),
                }
            })?;
            if !call_arguments.is_object() {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "`arguments` must be a JSON object".into(),
                });
            }
            mcp_result(
                name,
                plugin_root,
                optional_string("server").unwrap_or_else(|| "playwright".into()),
                tool.into(),
                call_arguments,
            )
            .await
        }
        "mcp_search" => {
            let requested_server = optional_string("server");
            let registry_root = plugin_root.to_path_buf();
            let descriptors = tokio::task::spawn_blocking(move || {
                let registry = vesper_mcp::McpRegistry::open(&registry_root)
                    .map_err(|error| tui_tool_failure("mcp_search", error))?;
                let mut output = Vec::new();
                for server in registry.list_with_builtins() {
                    if requested_server
                        .as_deref()
                        .is_some_and(|requested| requested != server.id)
                    {
                        continue;
                    }
                    let tools = vesper_mcp::McpClient::tools(&server)
                        .map_err(|error| tui_tool_failure("mcp_search", error))?;
                    output.push(serde_json::json!({
                        "server": server.id,
                        "tools": tools,
                    }));
                }
                Ok::<_, vesper_agent::ToolError>(output)
            })
            .await
            .map_err(|_| vesper_agent::ToolError::Failed("mcp discovery task failed".into()))??;
            vesper_agent::ToolResult::new(
                serde_json::to_string(&descriptors)
                    .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "mcp_list_tools" => {
            let server_id = required_string("server")?;
            let server_id_for_injection = server_id.clone();
            let registry_root = plugin_root.to_path_buf();
            let descriptors = tokio::task::spawn_blocking(move || {
                let registry = vesper_mcp::McpRegistry::open(&registry_root)
                    .map_err(|error| tui_tool_failure("mcp_list_tools", error))?;
                let server = registry.get_with_builtins(&server_id).ok_or_else(|| {
                    vesper_agent::ToolError::Failed(format!("MCP server not found: {server_id}"))
                })?;
                vesper_mcp::McpClient::tools(&server)
                    .map_err(|error| tui_tool_failure("mcp_list_tools", error))
            })
            .await
            .map_err(|_| vesper_agent::ToolError::Failed("MCP discovery task failed".into()))??;
            let injected: Vec<vesper_domain::ToolDefinition> = descriptors
                .iter()
                .map(|descriptor| {
                    mcp_descriptor_to_tool_definition(&server_id_for_injection, descriptor)
                })
                .collect();
            let summary = format!(
                "discovered {} tool(s) from `{server_id_for_injection}`; they are now advertised and callable as `mcp__{server_id_for_injection}__<tool>`",
                injected.len(),
            );
            Ok(vesper_agent::ToolResult::new(summary)
                .map_err(|error| tui_tool_failure("mcp_list_tools", error))?
                .with_injected_tools(injected))
        }
        "mcp_call" => {
            let server_id = required_string("server")?;
            let tool = required_string("tool")?;
            let arguments = arguments.get("arguments").cloned().ok_or_else(|| {
                vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "missing object argument `arguments`".into(),
                }
            })?;
            if !arguments.is_object() {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "`arguments` must be a JSON object".into(),
                });
            }
            let registry_root = plugin_root.to_path_buf();
            let result = tokio::task::spawn_blocking(move || {
                let registry = vesper_mcp::McpRegistry::open(&registry_root)
                    .map_err(|error| tui_tool_failure("mcp_call", error))?;
                let server = registry.get_with_builtins(&server_id).ok_or_else(|| {
                    vesper_agent::ToolError::Failed(format!("MCP server not found: {server_id}"))
                })?;
                vesper_mcp::McpClient::call_tool(&server, &tool, arguments)
                    .map_err(|error| tui_tool_failure("mcp_call", error))
            })
            .await
            .map_err(|_| vesper_agent::ToolError::Failed("mcp call task failed".into()))??;
            vesper_agent::ToolResult::new(truncate_text(
                &serde_json::to_string(&result).map_err(|error| tui_tool_failure(name, error))?,
                16_000,
            ))
        }
        "cronjob" => {
            let registry = vesper_checkpoints::CronRegistry::open(cron_root)
                .map_err(|error| tui_tool_failure(name, error))?;
            match required_string("action")?.as_str() {
                "list" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&registry.list())
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "create" => {
                    let entry = registry
                        .register(
                            optional_string("name").as_deref().unwrap_or("vesper-job"),
                            &required_string("prompt")?,
                            &required_string("schedule")?,
                        )
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&entry)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "update" => {
                    let id = required_string("job_id")?;
                    let entry = registry
                        .update(
                            &id,
                            optional_string("name").as_deref(),
                            optional_string("prompt").as_deref(),
                            optional_string("schedule").as_deref(),
                        )
                        .map_err(|error| tui_tool_failure(name, error))?
                        .ok_or_else(|| {
                            vesper_agent::ToolError::Failed(format!("cron job not found: {id}"))
                        })?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&entry)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "pause" | "resume" => {
                    let id = required_string("job_id")?;
                    let enabled = required_string("action")? == "resume";
                    let entry = registry
                        .set_enabled(&id, enabled)
                        .map_err(|error| tui_tool_failure(name, error))?
                        .ok_or_else(|| {
                            vesper_agent::ToolError::Failed(format!("cron job not found: {id}"))
                        })?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&entry)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "run" => {
                    let Some(factory) = worker_factory else {
                        return Err(vesper_agent::ToolError::Failed(
                            "cron run requires a provider-backed worker factory".into(),
                        ));
                    };
                    let id = required_string("job_id")?;
                    let Some(entry) = registry.get(&id) else {
                        return Err(vesper_agent::ToolError::Failed(format!(
                            "cron job not found: {id}"
                        )));
                    };
                    if !entry.enabled {
                        return Err(vesper_agent::ToolError::Failed("cron job is paused".into()));
                    }
                    let report = run_provider_worker(
                        factory,
                        None,
                        entry.prompt,
                        SessionOperatingMode::Code,
                        SessionPermissionMode::Ask,
                        None,
                    )
                    .await?;
                    vesper_agent::ToolResult::new(report)
                }
                "remove" => {
                    let id = required_string("job_id")?;
                    let removed = registry
                        .forget(&id)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("cron job {id} removed: {removed}"))
                }
                action => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: format!("unsupported cronjob action `{action}`"),
                }),
            }
        }
        "session_search" => {
            let query = optional_string("query").unwrap_or_default();
            let limit = arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 20) as usize;
            let persisted_store = vesper_sessions::FilesystemSessionStore::new(
                session_root.to_path_buf(),
                vesper_sessions::SessionSource::AgentVesper,
                vesper_sessions::DiscoveryBounds::default(),
            )
            .map_err(|error| tui_tool_failure(name, error))?;
            let persisted = vesper_sessions::search_sessions(
                &persisted_store,
                vesper_sessions::SessionSearchRequest {
                    query: query.clone(),
                    session_id: None,
                    around_ordinal: None,
                    limit,
                    window: 5,
                },
            )
            .await
            .map_err(|error| tui_tool_failure(name, error))?;
            let mut matches = persisted
                .into_iter()
                .map(|hit| {
                    serde_json::json!({
                        "source": "persisted",
                        "session_id": hit.session_id,
                        "ordinal": hit.ordinal,
                        "role": hit.role,
                        "snippet": hit.snippet,
                        "context": hit
                            .context
                            .into_iter()
                            .map(|message| {
                                serde_json::json!({
                                    "ordinal": message.ordinal,
                                    "role": message.role,
                                    "text": message.text,
                                })
                            })
                            .collect::<Vec<_>>(),
                        "score": hit.score,
                    })
                })
                .collect::<Vec<_>>();
            let query_lower = query.to_lowercase();
            for (ordinal, message) in context.conversation.iter().enumerate() {
                let text = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if query_lower.is_empty() || text.to_lowercase().contains(&query_lower) {
                    let role = match &message.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::Tool => "tool",
                        MessageRole::ProviderOpaque(_) => "provider",
                    };
                    matches.push(serde_json::json!({
                        "source": "current",
                        "session_id": "current",
                        "ordinal": ordinal,
                        "role": role,
                        "text": truncate_text(&text, 4000),
                    }));
                    if matches.len() >= limit {
                        break;
                    }
                }
            }
            matches.truncate(limit);
            vesper_agent::ToolResult::new(
                serde_json::to_string(&matches).map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "semantic_code" => {
            let action = required_string("action")?;
            let requested = required_string("path")?;
            let path = confine(root, &requested)?;
            if action == "workspace_symbols" {
                let query = optional_string("query").unwrap_or_default().to_lowercase();
                let mut symbols = Vec::new();
                collect_source_symbols(&path, &query, &mut symbols, 200)?;
                return vesper_agent::ToolResult::new(
                    serde_json::to_string(&symbols)
                        .map_err(|error| tui_tool_failure(name, error))?,
                );
            }
            let source =
                fs::read_to_string(&path).map_err(|error| tui_tool_failure(name, error))?;
            let query = optional_string("query").unwrap_or_default();
            let symbols = source_symbols(&source, &query);
            match action.as_str() {
                "document_symbols" | "definition" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&symbols)
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "references" => {
                    let needle = if query.is_empty() {
                        required_string("query")?
                    } else {
                        query
                    };
                    let references = source
                        .lines()
                        .enumerate()
                        .filter(|(_, line)| line.contains(&needle))
                        .take(200)
                        .map(|(line, text)| {
                            serde_json::json!({
                                "path": requested,
                                "line": line + 1,
                                "text": truncate_text(text, 400),
                            })
                        })
                        .collect::<Vec<_>>();
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&references)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "hover" => {
                    let line = arguments
                        .get("line")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(1) as usize;
                    let text = source.lines().nth(line.saturating_sub(1)).unwrap_or("");
                    vesper_agent::ToolResult::new(
                        serde_json::json!({
                            "path": requested,
                            "line": line,
                            "text": text,
                            "symbols": symbols,
                        })
                        .to_string(),
                    )
                }
                _ => Err(vesper_agent::ToolError::Failed(format!(
                    "semantic_code action `{action}` requires a language-server host; bounded source inspection supports document_symbols, workspace_symbols, definition, references, and hover"
                ))),
            }
        }
        "apply_patch_set" => {
            let Some(entries) = arguments.get("patches").and_then(|value| value.as_array()) else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "patches must be an array".into(),
                });
            };
            if entries.is_empty() || entries.len() > 20 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "patches must contain between 1 and 20 entries".into(),
                });
            }
            let mut seen = BTreeSet::new();
            let mut candidates = Vec::new();
            for entry in entries {
                let path_text = entry
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "each patch needs path".into(),
                    })?;
                let path = confine(root, path_text)?;
                if !seen.insert(path.clone()) {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: format!("duplicate patch target `{path_text}`"),
                    });
                }
                let old_bytes = fs::read(&path).map_err(|error| tui_tool_failure(name, error))?;
                let expected = entry
                    .get("expected_sha256")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                let actual = Sha256::digest(&old_bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                if expected.len() != 64 || expected != actual {
                    return Err(vesper_agent::ToolError::Failed(format!(
                        "content hash mismatch for `{path_text}`"
                    )));
                }
                let old_text = String::from_utf8(old_bytes.clone()).map_err(|_| {
                    vesper_agent::ToolError::Failed(format!("`{path_text}` is not UTF-8"))
                })?;
                let patch = entry
                    .get("patch")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "each patch needs patch text".into(),
                    })?;
                let new_text = vesper_agent::tools::apply_unified_diff(&old_text, patch)?;
                candidates.push((path, old_bytes, new_text.into_bytes()));
            }
            let mut committed = Vec::new();
            for (path, old_bytes, new_bytes) in &candidates {
                if let Err(error) = fs::write(path, new_bytes) {
                    for (rollback_path, rollback_bytes) in committed.iter().rev() {
                        let _ = fs::write(rollback_path, rollback_bytes);
                    }
                    return Err(tui_tool_failure(name, error));
                }
                committed.push((path, old_bytes));
            }
            vesper_agent::ToolResult::new(format!(
                "transactionally applied {} patch(es)",
                candidates.len()
            ))
        }
        "batch_read" => {
            let Some(operations) = arguments
                .get("operations")
                .and_then(|value| value.as_array())
            else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "operations must be an array".into(),
                });
            };
            if operations.is_empty() || operations.len() > 20 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "operations must contain between 1 and 20 entries".into(),
                });
            }
            let per_result = arguments
                .get("max_chars_per_result")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(4000)
                .clamp(200, 16_000) as usize;
            let mut results = Vec::new();
            let mut ids = BTreeSet::new();
            for (index, operation) in operations.iter().enumerate() {
                let id = operation
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| (index + 1).to_string());
                if !ids.insert(id.clone()) {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: format!("duplicate batch operation id `{id}`"),
                    });
                }
                let tool = operation
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if !matches!(
                    tool,
                    "read_file" | "list_directory" | "search_files" | "grep"
                ) {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: format!("unsupported batch operation `{tool}`"),
                    });
                }
                let call = ToolCall {
                    id: ToolCallId::new(format!("batch-{index}"))
                        .map_err(|error| tui_tool_failure(name, error))?,
                    tool_id: ToolId::new(tool).map_err(|error| tui_tool_failure(name, error))?,
                    arguments: operation
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                    extensions: vesper_domain::ExtensionMap::default(),
                };
                let result = core.execute(&call, context).await;
                match result {
                    Ok(result) => {
                        let text = result.text.as_str().to_owned();
                        results.push(serde_json::json!({
                            "id": id,
                            "tool": tool,
                            "ok": true,
                            "output": truncate_text(&text, per_result),
                            "truncated": text.chars().count() > per_result,
                        }));
                    }
                    Err(error) => results.push(serde_json::json!({
                        "id": id,
                        "tool": tool,
                        "ok": false,
                        "error": truncate_text(&error.to_string(), 1000),
                    })),
                }
            }
            vesper_agent::ToolResult::new(serde_json::json!({"results": results}).to_string())
        }
        "run_workflow" => {
            let Some(steps) = arguments.get("steps").and_then(|value| value.as_array()) else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "steps must be an array".into(),
                });
            };
            if steps.is_empty() || steps.len() > 12 {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "steps must contain between 1 and 12 entries".into(),
                });
            }
            let mut ids = BTreeMap::new();
            for (index, step) in steps.iter().enumerate() {
                let id = step
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("step-{index}"));
                if ids.insert(id, index).is_some() {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "workflow step ids must be unique".into(),
                    });
                }
            }
            let mut completed = BTreeSet::new();
            let mut order = Vec::new();
            while order.len() < steps.len() {
                let mut progressed = false;
                for (index, step) in steps.iter().enumerate() {
                    let id = ids
                        .iter()
                        .find_map(|(id, candidate)| (*candidate == index).then_some(id.clone()))
                        .expect("workflow id");
                    if completed.contains(&id) {
                        continue;
                    }
                    let needs = step
                        .get("needs")
                        .and_then(|value| value.as_array())
                        .map(|needs| {
                            needs
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if needs.iter().any(|need| !ids.contains_key(*need)) {
                        return Err(vesper_agent::ToolError::InvalidArguments {
                            tool: name.to_owned(),
                            reason: format!(
                                "workflow step `{id}` references an unknown dependency"
                            ),
                        });
                    }
                    if needs.iter().all(|need| completed.contains(*need)) {
                        completed.insert(id);
                        order.push(index);
                        progressed = true;
                    }
                }
                if !progressed {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "workflow dependencies contain a cycle".into(),
                    });
                }
            }
            let mut results = Vec::new();
            for index in order {
                let step = &steps[index];
                let id = step
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let tool = step
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if core.definition(tool).is_none() {
                    return Err(vesper_agent::ToolError::Failed(format!(
                        "workflow tool `{tool}` is not a registered core tool"
                    )));
                }
                let call = ToolCall {
                    id: ToolCallId::new(format!("workflow-{id}"))
                        .map_err(|error| tui_tool_failure(name, error))?,
                    tool_id: ToolId::new(tool).map_err(|error| tui_tool_failure(name, error))?,
                    arguments: step
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                    extensions: vesper_domain::ExtensionMap::default(),
                };
                match core.execute(&call, context).await {
                    Ok(result) => results.push(serde_json::json!({
                        "id": id,
                        "tool": tool,
                        "ok": true,
                        "output": truncate_text(result.text.as_str(), 8000),
                    })),
                    Err(error) => {
                        results.push(serde_json::json!({"id": id, "tool": tool, "ok": false, "error": error.to_string()}));
                        break;
                    }
                }
            }
            vesper_agent::ToolResult::new(serde_json::json!({"steps": results}).to_string())
        }
        "plugin_package" => {
            let Some(loader) = plugin_loader else {
                return Err(tui_tool_failure(name, "plugin loader unavailable"));
            };
            match required_string("action")?.as_str() {
                "list" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&loader.list())
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "publishers" => vesper_agent::ToolResult::new(
                    serde_json::to_string(&trusted_publishers.list())
                        .map_err(|error| tui_tool_failure(name, error))?,
                ),
                "verify" => {
                    let package = confine(root, &required_string("manifest_path")?)?;
                    let manifest = loader
                        .verify(&package)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&manifest)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "install" => {
                    let package = confine(root, &required_string("manifest_path")?)?;
                    let record = loader
                        .load(&package)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(
                        serde_json::to_string(&record)
                            .map_err(|error| tui_tool_failure(name, error))?,
                    )
                }
                "trust" => {
                    let publisher = required_string("publisher")?;
                    let key_path = confine(root, &required_string("public_key_path")?)?;
                    let key = fs::read_to_string(&key_path)
                        .map_err(|error| tui_tool_failure(name, error))?
                        .trim()
                        .to_owned();
                    let entry = vesper_mcp::TrustedPublisher {
                        publisher,
                        public_key_hex: key,
                    };
                    trusted_publishers
                        .trust(entry.clone())
                        .map_err(|error| tui_tool_failure(name, error))?;
                    persist_trusted_publishers(plugin_root, trusted_publishers)?;
                    vesper_agent::ToolResult::new("publisher trusted")
                }
                "untrust" => {
                    let publisher = required_string("publisher")?;
                    let removed = trusted_publishers.revoke(&publisher);
                    persist_trusted_publishers(plugin_root, trusted_publishers)?;
                    vesper_agent::ToolResult::new(format!("publisher removed: {removed}"))
                }
                action => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: format!("unsupported plugin action `{action}`"),
                }),
            }
        }
        "delegate_task" => {
            let Some(factory) = worker_factory else {
                return Err(vesper_agent::ToolError::Failed(
                    "delegate_task requires a provider-backed worker factory".into(),
                ));
            };
            if arguments
                .get("background")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                return Err(vesper_agent::ToolError::Failed(
                    "background delegate delivery is unavailable in the foreground TUI".into(),
                ));
            }
            let goal = required_string("goal")?;
            let context_text = optional_string("context").unwrap_or_default();
            let prompt = if context_text.is_empty() {
                goal
            } else {
                format!("{goal}\n\nAdditional read-only context:\n{context_text}")
            };
            let report = run_provider_worker(
                factory,
                Some(worker_service),
                prompt,
                SessionOperatingMode::Plan,
                SessionPermissionMode::ReadOnly,
                None,
            )
            .await?;
            vesper_agent::ToolResult::new(report)
        }
        "worktree_worker" => execute_worktree_worker(name, arguments, root, worker_factory).await,
        _ => Err(vesper_agent::ToolError::UnknownTool(name.to_owned())),
    }
}

async fn run_provider_worker(
    factory: &WorkerFactory,
    service: Option<Arc<HarnessToolService>>,
    prompt: String,
    mode: SessionOperatingMode,
    permission: SessionPermissionMode,
    workspace: Option<std::path::PathBuf>,
) -> Result<String, vesper_agent::ToolError> {
    let mut config = factory.config.clone();
    if let Some(workspace) = workspace {
        config.workspace_roots = vec![WorkspaceRoot {
            name: BoundedString::new("worker").expect("bounded worker root name"),
            path: BoundedString::new(workspace.display().to_string())
                .map_err(|error| tui_tool_failure("worker", error))?,
            primary: true,
        }];
    }
    let tools = if let Some(service) = service {
        service.build_default_registry()
    } else {
        ToolRegistry::parity_default()
    };
    let worker = AgentLoop::new(Arc::clone(&factory.registry), tools, config);
    let outcome = worker
        .run_prompt(build_user_message(&prompt), mode, permission)
        .await
        .map_err(|error| tui_tool_failure("worker", error))?;
    Ok(outcome_text(&outcome))
}

async fn execute_worktree_worker(
    name: &str,
    arguments: &serde_json::Value,
    root: &std::path::Path,
    worker_factory: Option<&WorkerFactory>,
) -> Result<vesper_agent::ToolResult, vesper_agent::ToolError> {
    let required_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                tool: name.to_owned(),
                reason: format!("missing string argument `{key}`"),
            })
    };
    let optional_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let action = required_string("action")?;
    let confine_path = |value: &str| vesper_agent::confinement::confine(root, value);
    match action.as_str() {
        "run" => {
            let Some(factory) = worker_factory else {
                return Err(vesper_agent::ToolError::Failed(
                    "worktree_worker requires a provider-backed worker factory".into(),
                ));
            };
            let task = required_string("task")?;
            let base_ref = optional_string("base_ref").unwrap_or_else(|| "HEAD".into());
            if base_ref.starts_with('-') {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "base_ref must not begin with '-'".into(),
                });
            }
            let path_text = optional_string("worker_path").unwrap_or_else(|| {
                format!(".agent-vesper/worktrees/worker-{}", std::process::id())
            });
            if let Some(parent) = std::path::Path::new(&path_text).parent() {
                let parent = if parent.is_absolute() {
                    parent.to_path_buf()
                } else {
                    root.join(parent)
                };
                std::fs::create_dir_all(parent).map_err(|error| tui_tool_failure(name, error))?;
            }
            let path = confine_path(&path_text)?;
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(["worktree", "add", "--detach"])
                .arg(&path)
                .arg(&base_ref)
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            if !output.status.success() {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "git worktree add failed: {}",
                    truncate_text(&String::from_utf8_lossy(&output.stderr), 2000)
                )));
            }
            let report = run_provider_worker(
                factory,
                None,
                task,
                SessionOperatingMode::Code,
                SessionPermissionMode::Ask,
                Some(path.clone()),
            )
            .await?;
            vesper_agent::ToolResult::new(format!(
                "worker completed in {}\n{}",
                path.display(),
                report
            ))
        }
        "inspect" | "verify" => {
            let path = confine_path(&required_string("worker_path")?)?;
            let args = if action == "inspect" {
                vec!["diff", "--stat"]
            } else {
                if optional_string("verification_command").as_deref() != Some("git diff --check")
                    && arguments.get("verification_command").is_some()
                {
                    return Err(vesper_agent::ToolError::Failed(
                        "verification_command is restricted to `git diff --check`".into(),
                    ));
                }
                vec!["diff", "--check"]
            };
            let output = std::process::Command::new("git")
                .current_dir(&path)
                .args(args)
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            let body = if output.stdout.is_empty() {
                &output.stderr
            } else {
                &output.stdout
            };
            vesper_agent::ToolResult::new(format!(
                "{} (exit {})\n{}",
                action,
                output.status.code().unwrap_or(1),
                truncate_text(&String::from_utf8_lossy(body), 8000)
            ))
        }
        "discard" => {
            let path = confine_path(&required_string("worker_path")?)?;
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            if !output.status.success() {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "git worktree remove failed: {}",
                    truncate_text(&String::from_utf8_lossy(&output.stderr), 2000)
                )));
            }
            vesper_agent::ToolResult::new(format!("worker discarded: {}", path.display()))
        }
        "promote" => {
            let path = confine_path(&required_string("worker_path")?)?;
            let diff = std::process::Command::new("git")
                .current_dir(&path)
                .args(["diff", "--binary"])
                .output()
                .map_err(|error| tui_tool_failure(name, error))?;
            let expected = required_string("diff_sha256")?.to_lowercase();
            let actual = sha256_hex(&diff.stdout);
            if expected != actual {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "worker diff hash mismatch: expected {}, found {}",
                    truncate_text(&expected, 16),
                    truncate_text(&actual, 16)
                )));
            }
            let mut apply = std::process::Command::new("git")
                .current_dir(root)
                .args(["apply", "--binary"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|error| tui_tool_failure(name, error))?;
            if let Some(stdin) = apply.stdin.as_mut() {
                use std::io::Write;
                stdin
                    .write_all(&diff.stdout)
                    .map_err(|error| tui_tool_failure(name, error))?;
            }
            let applied = apply
                .wait_with_output()
                .map_err(|error| tui_tool_failure(name, error))?;
            if !applied.status.success() {
                return Err(vesper_agent::ToolError::Failed(format!(
                    "git apply failed: {}",
                    truncate_text(&String::from_utf8_lossy(&applied.stderr), 2000)
                )));
            }
            let _ = std::process::Command::new("git")
                .current_dir(root)
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .output();
            vesper_agent::ToolResult::new(format!("worker diff promoted from {}", path.display()))
        }
        other => Err(vesper_agent::ToolError::InvalidArguments {
            tool: name.to_owned(),
            reason: format!("unsupported worktree action `{other}`"),
        }),
    }
}

fn outcome_text(outcome: &AgentTurnOutcome) -> String {
    match outcome {
        AgentTurnOutcome::Completed {
            assistant_content, ..
        } => assistant_content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        AgentTurnOutcome::MaxIterationsReached { iterations } => {
            format!("worker reached the {iterations}-iteration safety cap")
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn truncate_text(value: &str, limit: usize) -> String {
    let mut output = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        output.push_str("… [truncated]");
    }
    output
}

fn source_symbols(source: &str, query: &str) -> Vec<serde_json::Value> {
    source
        .lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let trimmed = text.trim_start();
            let (kind, name) = if let Some(rest) = trimmed.strip_prefix("fn ") {
                ("function", rest.split(['(', '<', ' ', ':']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                ("function", rest.split(['(', '<', ' ', ':']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("struct ") {
                ("struct", rest.split(['{', '<', ' ']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("pub struct ") {
                ("struct", rest.split(['{', '<', ' ']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("class ") {
                ("class", rest.split(['(', ':', ' ']).next()?)
            } else if let Some(rest) = trimmed.strip_prefix("def ") {
                ("function", rest.split(['(', ' ']).next()?)
            } else {
                return None;
            };
            if !query.is_empty() && !name.to_lowercase().contains(&query.to_lowercase()) {
                return None;
            }
            Some(serde_json::json!({"name": name, "kind": kind, "line": line + 1}))
        })
        .collect()
}

fn collect_source_symbols(
    path: &std::path::Path,
    query: &str,
    output: &mut Vec<serde_json::Value>,
    limit: usize,
) -> Result<(), vesper_agent::ToolError> {
    if output.len() >= limit {
        return Ok(());
    }
    if path.is_file() {
        let source = std::fs::read_to_string(path)
            .map_err(|error| tui_tool_failure("semantic_code", error))?;
        for mut symbol in source_symbols(&source, query) {
            symbol["path"] = serde_json::Value::String(path.display().to_string());
            output.push(symbol);
            if output.len() >= limit {
                break;
            }
        }
        return Ok(());
    }
    let entries =
        std::fs::read_dir(path).map_err(|error| tui_tool_failure("semantic_code", error))?;
    for entry in entries.flatten() {
        let child = entry.path();
        if child.file_name().and_then(|name| name.to_str()) == Some(".git") {
            continue;
        }
        if child.is_dir()
            || matches!(
                child.extension().and_then(|ext| ext.to_str()),
                Some("rs" | "py" | "js" | "ts")
            )
        {
            collect_source_symbols(&child, query, output, limit)?;
        }
        if output.len() >= limit {
            break;
        }
    }
    Ok(())
}

fn persist_trusted_publishers(
    root: &std::path::Path,
    publishers: &vesper_mcp::TrustedPublishers,
) -> Result<(), vesper_agent::ToolError> {
    let body = publishers
        .list()
        .into_iter()
        .map(|entry| serde_json::to_string(&entry))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| tui_tool_failure("plugin_package", error))?
        .join("\n");
    std::fs::write(root.join("publishers.jsonl"), format!("{body}\n"))
        .map_err(|error| tui_tool_failure("plugin_package", error))
}

fn execute_tui_tool(
    name: &str,
    arguments: &serde_json::Value,
    stores: &MemoryStores,
) -> Result<vesper_agent::ToolResult, vesper_agent::ToolError> {
    use std::time::UNIX_EPOCH;
    use vesper_memory::{
        Confidence, EpistemicRecord, MemoryEntry, MemoryKind, RecordStatus, SkillBundle, SkillSlug,
    };

    let string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                tool: name.to_owned(),
                reason: format!("missing string argument `{key}`"),
            })
    };
    let optional_string = |key: &str| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let result = match name {
        "recall_memory" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            let entries = optional_string("query")
                .map_or_else(|| store.list(None), |query| store.query(&query));
            let body =
                serde_json::to_string(&entries).map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(body)
        }
        "store_memory" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            let entry = store
                .append(MemoryEntry {
                    id: String::new(),
                    kind: MemoryKind::Memory,
                    summary: string("entry")?,
                    scopes: Vec::new(),
                    evidence: Vec::new(),
                    created_at: UNIX_EPOCH,
                    updated_at: UNIX_EPOCH,
                })
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("stored memory {}", entry.id))
        }
        "recall_user_profile" => {
            let Some(profile) = stores.profile.as_ref() else {
                return Err(tui_tool_failure(name, "user profile unavailable"));
            };
            vesper_agent::ToolResult::new(profile.read())
        }
        "store_user_profile" => {
            let Some(profile) = stores.profile.as_ref() else {
                return Err(tui_tool_failure(name, "user profile unavailable"));
            };
            let category = string("category")?;
            let entry = string("entry")?;
            let size = profile
                .append(&category, &entry)
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("stored profile entry ({size} bytes)"))
        }
        "forget_memory" => {
            let scope = string("scope")?;
            let entry = string("entry")?;
            match scope.as_str() {
                "project" => {
                    let Some(store) = stores.memory.as_ref() else {
                        return Err(tui_tool_failure(name, "memory store unavailable"));
                    };
                    let ids: Vec<String> = store
                        .query(&entry)
                        .into_iter()
                        .filter(|candidate| candidate.summary == entry)
                        .map(|candidate| candidate.id)
                        .collect();
                    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                    let removed = store
                        .forget(&refs)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("removed {removed} project fact(s)"))
                }
                "user" => {
                    let Some(profile) = stores.profile.as_ref() else {
                        return Err(tui_tool_failure(name, "user profile unavailable"));
                    };
                    let removed = profile
                        .forget(&entry)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("removed {removed} profile line(s)"))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "scope must be project or user".into(),
                }),
            }
        }
        "update_memory" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            let Some(operations) = arguments.get("operations").and_then(|v| v.as_array()) else {
                return Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "operations must be an array".into(),
                });
            };
            let mut changed = 0usize;
            for operation in operations.iter().take(20) {
                let op = operation.get("op").and_then(|v| v.as_str()).unwrap_or("");
                let entry = operation
                    .get("entry")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "each operation needs an entry".into(),
                    })?;
                match op {
                    "add" => {
                        store
                            .append(MemoryEntry {
                                id: String::new(),
                                kind: MemoryKind::Memory,
                                summary: entry.to_owned(),
                                scopes: Vec::new(),
                                evidence: Vec::new(),
                                created_at: UNIX_EPOCH,
                                updated_at: UNIX_EPOCH,
                            })
                            .map_err(|error| tui_tool_failure(name, error))?;
                        changed += 1;
                    }
                    "remove" => {
                        let ids: Vec<String> = store
                            .query(entry)
                            .into_iter()
                            .filter(|candidate| candidate.summary == entry)
                            .map(|candidate| candidate.id)
                            .collect();
                        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                        changed += store
                            .forget(&refs)
                            .map_err(|error| tui_tool_failure(name, error))?;
                    }
                    _ => {
                        return Err(vesper_agent::ToolError::InvalidArguments {
                            tool: name.to_owned(),
                            reason: "operation must be add or remove".into(),
                        });
                    }
                }
            }
            vesper_agent::ToolResult::new(format!("applied {changed} memory change(s)"))
        }
        "update_awareness" | "update_deliberation" => {
            let Some(ledger) = stores.awareness.as_ref() else {
                return Err(tui_tool_failure(name, "awareness ledger unavailable"));
            };
            let action = string("action")?;
            let id = optional_string("record_id").unwrap_or_default();
            let changed = match action.as_str() {
                "resolve" => ledger
                    .resolve(&id)
                    .map_err(|error| tui_tool_failure(name, error))?,
                "invalidate" => ledger
                    .invalidate(&id)
                    .map_err(|error| tui_tool_failure(name, error))?,
                "upsert" => {
                    let kind = match optional_string("kind").as_deref() {
                        Some("assumption") => MemoryKind::Assumption,
                        Some("hypothesis") => MemoryKind::Hypothesis,
                        Some("contradiction") => MemoryKind::Contradiction,
                        Some("unknown") => MemoryKind::Unknown,
                        Some("capability") => MemoryKind::Capability,
                        _ if name == "update_deliberation" => MemoryKind::Deliberation,
                        _ => MemoryKind::Observation,
                    };
                    let confidence = match optional_string("confidence").as_deref() {
                        Some("low") => Confidence::Low,
                        Some("high") => Confidence::High,
                        _ => Confidence::Medium,
                    };
                    ledger
                        .upsert(EpistemicRecord {
                            id,
                            kind,
                            summary: string("summary")?,
                            scopes: Vec::new(),
                            evidence: Vec::new(),
                            supports: Vec::new(),
                            confidence,
                            status: RecordStatus::Active,
                            created_at: UNIX_EPOCH,
                            updated_at: UNIX_EPOCH,
                        })
                        .map_err(|error| tui_tool_failure(name, error))?;
                    true
                }
                _ => {
                    return Err(vesper_agent::ToolError::InvalidArguments {
                        tool: name.to_owned(),
                        reason: "action must be upsert, resolve, or invalidate".into(),
                    });
                }
            };
            ledger
                .save()
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("awareness update accepted: {changed}"))
        }
        "list_skills" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            vesper_agent::ToolResult::new(
                serde_json::to_string(
                    &skills
                        .list()
                        .into_iter()
                        .map(|summary| {
                            serde_json::json!({
                                "name": summary.slug,
                                "headline": summary.headline,
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "read_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(
                skills
                    .read(&slug)
                    .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "learn_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let body = format!(
                "# {}\n\n{}\n\n{}\n",
                slug.as_str(),
                string("description")?,
                string("instructions")?
            );
            skills
                .write(&slug, &body)
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("learned skill {}", slug.as_str()))
        }
        "forget_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let removed = skills
                .forget(&slug)
                .map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(format!("skill removed: {removed}"))
        }
        "manage_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let action = string("action")?;
            let mut body = skills
                .read(&slug)
                .map_err(|error| tui_tool_failure(name, error))?;
            let marker = format!("<!-- vesper:{action} -->");
            match action.as_str() {
                "pin" | "unpin" | "archive" | "restore" => {
                    for previous in ["pin", "unpin", "archive", "restore"] {
                        body = body.replace(&format!("<!-- vesper:{previous} -->\n"), "");
                    }
                    body = format!("{marker}\n{body}");
                    skills
                        .write(&slug, &body)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("skill {}: {}", slug.as_str(), action))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "action must be pin, unpin, archive, or restore".into(),
                }),
            }
        }
        "curate_skills" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let count = skills.list().len();
            vesper_agent::ToolResult::new(format!("skill curation scanned {count} skill(s)"))
        }
        "list_skill_bundles" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let bundles = skills.list_bundles();
            vesper_agent::ToolResult::new(
                serde_json::to_string(&bundles).map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "read_skill_bundle" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            vesper_agent::ToolResult::new(
                serde_json::to_string(
                    &skills
                        .read_bundle(&slug)
                        .map_err(|error| tui_tool_failure(name, error))?,
                )
                .map_err(|error| tui_tool_failure(name, error))?,
            )
        }
        "manage_skill_bundle" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let action = string("action")?;
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            match action.as_str() {
                "delete" => {
                    let removed = skills
                        .forget_bundle(&slug)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("bundle removed: {removed}"))
                }
                "create" => {
                    let skills_value = arguments
                        .get("skills")
                        .and_then(|value| value.as_array())
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|value| value.as_str().map(str::to_owned))
                                .take(32)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    skills
                        .write_bundle(SkillBundle {
                            name: slug.as_str().to_owned(),
                            description: optional_string("description").unwrap_or_default(),
                            skills: skills_value,
                            instruction: optional_string("instruction").unwrap_or_default(),
                        })
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("bundle created: {}", slug.as_str()))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "action must be create or delete".into(),
                }),
            }
        }
        "evolve_skill" => {
            let Some(skills) = stores.skills.as_ref() else {
                return Err(tui_tool_failure(name, "skill store unavailable"));
            };
            let action = string("action")?;
            let slug =
                SkillSlug::new(&string("name")?).map_err(|error| tui_tool_failure(name, error))?;
            let candidate = SkillSlug::new(&format!("candidate-{}", slug.as_str()))
                .map_err(|error| tui_tool_failure(name, error))?;
            match action.as_str() {
                "draft" | "propose" => {
                    let body = format!(
                        "# {}\n\n{}\n\n{}\n",
                        slug.as_str(),
                        optional_string("description").unwrap_or_default(),
                        optional_string("instructions").unwrap_or_default()
                    );
                    skills
                        .write(&candidate, &body)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!(
                        "candidate staged: {}",
                        candidate.as_str()
                    ))
                }
                "promote" => {
                    let body = skills
                        .read(&candidate)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    skills
                        .write(&slug, &body)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    skills
                        .forget(&candidate)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("candidate promoted: {}", slug.as_str()))
                }
                "discard" => {
                    let removed = skills
                        .forget(&candidate)
                        .map_err(|error| tui_tool_failure(name, error))?;
                    vesper_agent::ToolResult::new(format!("candidate discarded: {removed}"))
                }
                _ => Err(vesper_agent::ToolError::InvalidArguments {
                    tool: name.to_owned(),
                    reason: "action must be draft, propose, promote, or discard".into(),
                }),
            }
        }
        "failure_corpus" => {
            let Some(store) = stores.memory.as_ref() else {
                return Err(tui_tool_failure(name, "memory store unavailable"));
            };
            if optional_string("action").as_deref() == Some("recall") {
                let entries = store.list(Some(MemoryKind::MetaLearning));
                vesper_agent::ToolResult::new(
                    serde_json::to_string(&entries)
                        .map_err(|error| tui_tool_failure(name, error))?,
                )
            } else {
                let entry = store
                    .append(MemoryEntry {
                        id: String::new(),
                        kind: MemoryKind::MetaLearning,
                        summary: string("summary")?,
                        scopes: Vec::new(),
                        evidence: Vec::new(),
                        created_at: UNIX_EPOCH,
                        updated_at: UNIX_EPOCH,
                    })
                    .map_err(|error| tui_tool_failure(name, error))?;
                vesper_agent::ToolResult::new(format!(
                    "failure observation recorded as {}",
                    entry.id
                ))
            }
        }
        _ => {
            return Err(vesper_agent::ToolError::UnknownTool(name.to_owned()));
        }
    }?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_agent::ToolService;

    #[test]
    fn shared_service_advertises_all_hosted_python_tools() {
        let service = HarnessToolService {
            stores: Arc::new(MemoryStores {
                memory: None,
                skills: None,
                profile: None,
                awareness: None,
            }),
            core: Arc::new(ToolRegistry::parity_default()),
            cron_root: PathBuf::new(),
            plugin_loader: None,
            trusted_publishers: vesper_mcp::TrustedPublishers::new(),
            plugin_root: PathBuf::new(),
            session_root: PathBuf::new(),
            worker_factory: None,
            cron_abort: None,
        };
        let names = service
            .definitions()
            .into_iter()
            .map(|definition| definition.harness_name.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), 36);
        for name in [
            "update_awareness",
            "recall_memory",
            "store_memory",
            "list_skills",
            "cronjob",
            "session_search",
            "delegate_task",
            "semantic_code",
            "apply_patch_set",
            "batch_read",
            "run_workflow",
            "plugin_package",
            "worktree_worker",
            "mcp_search",
            "mcp_list_tools",
            "mcp_call",
            "search_tools",
            "web_search",
            "web_reader",
            "vision_analyze",
            "browser_ui",
        ] {
            assert!(names.contains(name), "missing shared hosted tool {name}");
        }
    }

    // ------------------- Phase 3: MCP gateway & translation -------------------

    #[test]
    fn parse_mcp_gateway_name_extracts_server_and_tool() {
        assert_eq!(
            parse_mcp_gateway_name("mcp__playwright__navigate"),
            Some(("playwright", "navigate"))
        );
        assert_eq!(
            parse_mcp_gateway_name("mcp__zai-search__web_search_prime"),
            Some(("zai-search", "web_search_prime"))
        );
    }

    #[test]
    fn parse_mcp_gateway_name_rejects_malformed_names() {
        // Missing prefix.
        assert_eq!(parse_mcp_gateway_name("playwright__navigate"), None);
        // Missing the inner __ separator after the prefix.
        assert_eq!(parse_mcp_gateway_name("mcp__playwright"), None);
        // Empty server or tool.
        assert_eq!(parse_mcp_gateway_name("mcp____navigate"), None);
        assert_eq!(parse_mcp_gateway_name("mcp__playwright__"), None);
        // Not an mcp__ name at all.
        assert_eq!(parse_mcp_gateway_name("read_file"), None);
        assert_eq!(parse_mcp_gateway_name(""), None);
    }

    #[test]
    fn mcp_descriptor_translates_into_a_gateway_routed_tool_definition() {
        let descriptor = vesper_mcp::McpToolDescriptor {
            name: "navigate".to_owned(),
            description: Some("Navigate to a URL.".to_owned()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {"url": {"type": "string"}},
                "required": ["url"],
            })),
        };
        let definition = mcp_descriptor_to_tool_definition("playwright", &descriptor);
        assert_eq!(
            definition.harness_name.as_str(),
            "mcp__playwright__navigate"
        );
        assert_eq!(definition.description, "Navigate to a URL.");
        assert_eq!(
            definition.execution_class,
            vesper_domain::ToolExecutionClass::NestedWorkflow
        );
        assert!(
            !definition.defer_loading,
            "injected tools must NOT be deferred"
        );
        assert_eq!(
            definition.input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {"url": {"type": "string"}},
                "required": ["url"],
            })
        );
    }

    #[test]
    fn mcp_descriptor_falls_back_to_default_description_and_schema() {
        let descriptor = vesper_mcp::McpToolDescriptor {
            name: "ping".to_owned(),
            description: None,
            input_schema: None,
        };
        let definition = mcp_descriptor_to_tool_definition("echo", &descriptor);
        assert_eq!(definition.harness_name.as_str(), "mcp__echo__ping");
        assert!(
            definition.description.contains("echo"),
            "default description must mention the server id: {}",
            definition.description
        );
        assert_eq!(
            definition.input_schema,
            serde_json::json!({"type": "object"}),
            "missing inputSchema must default to a permissive object schema"
        );
    }

    #[test]
    fn build_default_registry_wires_the_mcp_gateway() {
        // Use a real tempdir so the HarnessToolService constructor (which
        // creates roots and reads publishers.jsonl) does not error.
        let root = tempfile::tempdir().unwrap();
        let stores = Arc::new(MemoryStores::open_default());
        let service = Arc::new(HarnessToolService::new(
            stores,
            root.path().join("cron"),
            root.path().join("mcp"),
            None,
        ));
        let registry = service.build_default_registry();
        // The 9 parity tools + 36 hosted tools are still advertised.
        assert_eq!(
            registry
                .definitions_for(vesper_domain::SessionOperatingMode::Code)
                .len(),
            9 + 36
        );
        // The gateway is registered under the mcp__ prefix.
        assert!(registry.has_gateway(MCP_GATEWAY_PREFIX));
        // A call to a name matching the prefix is considered registered.
        assert!(registry.contains("mcp__anything__here"));
    }
}

/// Drains one [`MemoryOp`] against the durable stores, pushing the result
/// into the transcript. Pure-with-side-effects: no async, no terminal I/O,
/// only local filesystem reads/writes via `vesper_memory`.
pub struct MemoryStores {
    memory: Option<Arc<vesper_memory::MemoryStore>>,
    skills: Option<Arc<vesper_memory::SkillStore>>,
    profile: Option<Arc<vesper_memory::UserProfile>>,
    awareness: Option<Arc<vesper_memory::AwarenessLedger>>,
}

impl MemoryStores {
    /// Opens the bundle at `AGENT_VESPER_MEMORY_ROOT` (falling back to
    /// `.agent-vesper/memory/` under the current directory). If opening any
    /// store fails the bundle stays `None` for that store and memory
    /// commands surface a clear error rather than crashing the TUI.
    pub fn open_default() -> Self {
        let root = match std::env::var("AGENT_VESPER_MEMORY_ROOT") {
            Ok(value) => std::path::PathBuf::from(value),
            Err(_) => std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".agent-vesper")
                .join("memory"),
        };
        // Ensure the root directory exists so the stores can open it.
        let _ = std::fs::create_dir_all(&root);
        let memory = vesper_memory::MemoryStore::open(&root).ok().map(Arc::new);
        let skills = vesper_memory::SkillStore::open(&root).ok().map(Arc::new);
        let profile = vesper_memory::UserProfile::open(&root).ok().map(Arc::new);
        let awareness = vesper_memory::AwarenessLedger::open(&root)
            .ok()
            .map(Arc::new);
        Self {
            memory,
            skills,
            profile,
            awareness,
        }
    }
}

/// Memory/skills tool service injected into the agent loop.
///
/// These tools deliberately use the same durable stores as slash-command
/// dispatch. The agent loop only sees the provider-neutral `ToolService`
/// contract; storage ownership and secret-safe bounds remain in
/// `vesper-memory`.
#[derive(Clone)]
pub struct WorkerFactory {
    registry: Arc<vesper_runtime::ProviderRegistry>,
    config: vesper_agent::AgentLoopConfig,
}

impl WorkerFactory {
    #[must_use]
    pub fn new(
        registry: Arc<vesper_runtime::ProviderRegistry>,
        config: vesper_agent::AgentLoopConfig,
    ) -> Self {
        Self { registry, config }
    }
}

pub struct HarnessToolService {
    stores: Arc<MemoryStores>,
    /// Core read/write tools used by `batch_read` and `run_workflow`.
    core: Arc<ToolRegistry>,
    cron_root: std::path::PathBuf,
    plugin_loader: Option<Arc<vesper_mcp::PluginLoader>>,
    trusted_publishers: vesper_mcp::TrustedPublishers,
    plugin_root: std::path::PathBuf,
    session_root: std::path::PathBuf,
    worker_factory: Option<Arc<WorkerFactory>>,
    cron_abort: Option<tokio::task::AbortHandle>,
}

impl Drop for HarnessToolService {
    fn drop(&mut self) {
        if let Some(handle) = self.cron_abort.take() {
            handle.abort();
        }
    }
}

impl HarnessToolService {
    pub fn new(
        stores: Arc<MemoryStores>,
        cron_root: std::path::PathBuf,
        plugin_root: std::path::PathBuf,
        worker_factory: Option<Arc<WorkerFactory>>,
    ) -> Self {
        let _ = std::fs::create_dir_all(&cron_root);
        let _ = std::fs::create_dir_all(&plugin_root);
        let session_root = session_root_path();
        let _ = std::fs::create_dir_all(&session_root);
        let trusted_publishers = load_trusted_publishers(&plugin_root);
        let plugin_loader =
            vesper_mcp::PluginLoader::open(&plugin_root, trusted_publishers.clone())
                .ok()
                .map(Arc::new);
        let cron_abort = worker_factory.as_ref().and_then(|factory| {
            tokio::runtime::Handle::try_current().ok().map(|handle| {
                handle
                    .spawn(run_cron_scheduler(cron_root.clone(), (**factory).clone()))
                    .abort_handle()
            })
        });
        Self {
            stores,
            core: Arc::new(ToolRegistry::parity_default()),
            cron_root,
            plugin_loader,
            trusted_publishers,
            plugin_root,
            session_root,
            worker_factory,
            cron_abort,
        }
    }

    /// Composes the parity-default registry with this hosted service and the
    /// MCP gateway executor. Composition boundaries (TUI, ACP, internal
    /// workers) call this so dynamically-discovered MCP tools advertised via
    /// `mcp_list_tools` can actually be executed when the model calls them
    /// by their `mcp__<server>__<tool>` name on a later turn.
    #[must_use]
    pub fn build_default_registry(self: Arc<Self>) -> ToolRegistry {
        let plugin_root = self.plugin_root.clone();
        ToolRegistry::parity_default()
            .with_service(self)
            .with_gateway(
                MCP_GATEWAY_PREFIX,
                Arc::new(McpGatewayExecutor::new(plugin_root)),
            )
    }

    fn read_only_worker_service(&self) -> Arc<Self> {
        Arc::new(Self {
            stores: Arc::clone(&self.stores),
            core: Arc::clone(&self.core),
            cron_root: self.cron_root.clone(),
            plugin_loader: self.plugin_loader.clone(),
            trusted_publishers: self.trusted_publishers.clone(),
            plugin_root: self.plugin_root.clone(),
            session_root: self.session_root.clone(),
            worker_factory: None,
            cron_abort: None,
        })
    }
}

async fn run_cron_scheduler(root: PathBuf, factory: WorkerFactory) {
    loop {
        if let Ok(registry) = vesper_checkpoints::CronRegistry::open(&root)
            && let Ok(runs) = registry.claim_due(std::time::SystemTime::now(), false)
        {
            for run in runs {
                let result = run_provider_worker(
                    &factory,
                    None,
                    run.entry.prompt.clone(),
                    SessionOperatingMode::Code,
                    SessionPermissionMode::Ask,
                    None,
                )
                .await;
                let (status, output) = match result {
                    Ok(output) => ("ok", output),
                    Err(error) => ("error", error.to_string()),
                };
                let _ = registry.finish_claim(&run.entry.id, &run.token, status, &output);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

impl vesper_agent::ToolService for HarnessToolService {
    fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
        use vesper_domain::ToolExecutionClass;
        type DefinitionRow = (
            &'static str,
            &'static str,
            ToolExecutionClass,
            &'static [(&'static str, &'static str, bool)],
        );
        let definitions: [DefinitionRow; 36] = [
            (
                "update_awareness",
                "Upsert, resolve, or invalidate bounded evidence-backed awareness records.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("record_id", "string", false),
                    ("summary", "string", false),
                    ("kind", "string", false),
                    ("confidence", "string", false),
                ],
            ),
            (
                "recall_memory",
                "Read opt-in durable project knowledge.",
                ToolExecutionClass::ReadOnly,
                &[("query", "string", false)],
            ),
            (
                "store_memory",
                "Store one stable reusable project fact.",
                ToolExecutionClass::Mutating,
                &[("entry", "string", true)],
            ),
            (
                "recall_user_profile",
                "Read approved cross-project user preferences.",
                ToolExecutionClass::ReadOnly,
                &[],
            ),
            (
                "store_user_profile",
                "Store one explicit durable user preference or environment fact.",
                ToolExecutionClass::Mutating,
                &[("entry", "string", true), ("category", "string", true)],
            ),
            (
                "forget_memory",
                "Remove one exact durable project or user fact.",
                ToolExecutionClass::Mutating,
                &[("scope", "string", true), ("entry", "string", true)],
            ),
            (
                "update_memory",
                "Apply bounded project-memory additions and removals.",
                ToolExecutionClass::Mutating,
                &[("operations", "array", true)],
            ),
            (
                "list_skills",
                "List reusable learned project skills.",
                ToolExecutionClass::ReadOnly,
                &[],
            ),
            (
                "read_skill",
                "Read one learned project skill.",
                ToolExecutionClass::ReadOnly,
                &[("name", "string", true)],
            ),
            (
                "learn_skill",
                "Create or refine a reusable project skill after verification.",
                ToolExecutionClass::Mutating,
                &[
                    ("name", "string", true),
                    ("description", "string", true),
                    ("instructions", "string", true),
                ],
            ),
            (
                "forget_skill",
                "Remove one agent-learned project skill.",
                ToolExecutionClass::Mutating,
                &[("name", "string", true)],
            ),
            (
                "manage_skill",
                "Pin, unpin, archive, or restore one learned skill.",
                ToolExecutionClass::Mutating,
                &[("name", "string", true), ("action", "string", true)],
            ),
            (
                "curate_skills",
                "Run deterministic bounded skill maintenance.",
                ToolExecutionClass::Mutating,
                &[],
            ),
            (
                "list_skill_bundles",
                "List project-local groups of learned skills.",
                ToolExecutionClass::ReadOnly,
                &[],
            ),
            (
                "read_skill_bundle",
                "Read one learned skill bundle.",
                ToolExecutionClass::ReadOnly,
                &[("name", "string", true)],
            ),
            (
                "manage_skill_bundle",
                "Create or delete a project-local skill bundle.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("name", "string", true),
                    ("description", "string", false),
                    ("skills", "array", false),
                    ("instruction", "string", false),
                ],
            ),
            (
                "evolve_skill",
                "Draft, promote, or discard a bounded candidate skill.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("name", "string", true),
                    ("description", "string", false),
                    ("instructions", "string", false),
                ],
            ),
            (
                "update_deliberation",
                "Record a bounded deliberation state update.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("record_id", "string", false),
                    ("summary", "string", false),
                ],
            ),
            (
                "failure_corpus",
                "Record or recall bounded failure-corpus observations.",
                ToolExecutionClass::Mutating,
                &[("action", "string", true), ("summary", "string", false)],
            ),
            (
                "cronjob",
                "Create, list, or remove persistent local scheduled-task definitions.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("name", "string", false),
                    ("prompt", "string", false),
                    ("schedule", "string", false),
                    ("job_id", "string", false),
                ],
            ),
            (
                "session_search",
                "Search the bounded conversation currently visible to this session.",
                ToolExecutionClass::ReadOnly,
                &[("query", "string", false), ("limit", "integer", false)],
            ),
            (
                "delegate_task",
                "Delegate a bounded read-only investigation to an injected worker port.",
                ToolExecutionClass::Mutating,
                &[
                    ("goal", "string", true),
                    ("context", "string", false),
                    ("role", "string", false),
                    ("background", "boolean", false),
                ],
            ),
            (
                "semantic_code",
                "Inspect bounded source symbols and references without editing files.",
                ToolExecutionClass::ReadOnly,
                &[
                    ("action", "string", true),
                    ("path", "string", true),
                    ("query", "string", false),
                    ("line", "integer", false),
                    ("column", "integer", false),
                ],
            ),
            (
                "apply_patch_set",
                "Transactionally apply hash-checked unified diffs to multiple files.",
                ToolExecutionClass::Mutating,
                &[("patches", "array", true)],
            ),
            (
                "batch_read",
                "Run up to twenty bounded read-only core operations and return JSON results.",
                ToolExecutionClass::ReadOnly,
                &[
                    ("operations", "array", true),
                    ("max_chars_per_result", "integer", false),
                ],
            ),
            (
                "run_workflow",
                "Run a validated bounded dependency graph of registered core tools.",
                ToolExecutionClass::Mutating,
                &[("steps", "array", true)],
            ),
            (
                "plugin_package",
                "Verify, install, list, or manage signed declarative plugin packages.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("manifest_path", "string", false),
                    ("publisher", "string", false),
                    ("public_key_path", "string", false),
                ],
            ),
            (
                "worktree_worker",
                "Inspect or manage the lifecycle of a bounded isolated Git worker.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("task", "string", false),
                    ("worker_path", "string", false),
                    ("base_ref", "string", false),
                    ("diff_sha256", "string", false),
                ],
            ),
            (
                "mcp_search",
                "Discover bounded tool descriptors from configured MCP servers.",
                ToolExecutionClass::ReadOnly,
                &[("server", "string", false)],
            ),
            (
                "mcp_list_tools",
                "List tools exposed by one configured MCP server.",
                ToolExecutionClass::Mutating,
                &[("server", "string", true)],
            ),
            (
                "mcp_call",
                "Call one explicitly selected MCP tool through the configured gateway.",
                ToolExecutionClass::Mutating,
                &[
                    ("server", "string", true),
                    ("tool", "string", true),
                    ("arguments", "object", true),
                ],
            ),
            (
                "search_tools",
                "Find up to five bounded hosted tools by capability intent.",
                ToolExecutionClass::ReadOnly,
                &[("intent", "string", true), ("mode", "string", false)],
            ),
            (
                "web_search",
                "Search the current web through the first-party Z.ai Web Search MCP.",
                ToolExecutionClass::ReadOnly,
                &[("query", "string", true)],
            ),
            (
                "web_reader",
                "Read and extract a public web page through the first-party Z.ai Reader MCP.",
                ToolExecutionClass::ReadOnly,
                &[("url", "string", true)],
            ),
            (
                "vision_analyze",
                "Analyze a workspace image through the first-party Z.ai Vision MCP.",
                ToolExecutionClass::Mutating,
                &[("path", "string", true), ("prompt", "string", true)],
            ),
            (
                "browser_ui",
                "Permission-gated isolated Playwright browser interaction without arbitrary eval.",
                ToolExecutionClass::Mutating,
                &[
                    ("action", "string", true),
                    ("arguments", "object", true),
                    ("server", "string", false),
                ],
            ),
        ];
        definitions
            .into_iter()
            .map(|(name, description, class, properties)| {
                vesper_agent::schema_definition(name, description, class, properties)
            })
            .collect()
    }

    fn execute<'a>(
        &'a self,
        call: &'a vesper_domain::ToolCall,
        context: &'a vesper_agent::ToolContext,
    ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
    {
        let stores = Arc::clone(&self.stores);
        let core = Arc::clone(&self.core);
        let cron_root = self.cron_root.clone();
        let plugin_loader = self.plugin_loader.clone();
        let trusted_publishers = self.trusted_publishers.clone();
        let plugin_root = self.plugin_root.clone();
        let session_root = self.session_root.clone();
        let worker_factory = self.worker_factory.clone();
        let worker_service = self.read_only_worker_service();
        let name = call.tool_id.as_str().to_owned();
        let arguments = call.arguments.clone();
        Box::pin(async move {
            match name.as_str() {
                "cronjob" | "session_search" | "delegate_task" | "semantic_code"
                | "apply_patch_set" | "batch_read" | "run_workflow" | "plugin_package"
                | "worktree_worker" | "mcp_search" | "mcp_list_tools" | "mcp_call"
                | "search_tools" | "web_search" | "web_reader" | "vision_analyze"
                | "browser_ui" => {
                    execute_extended_tui_tool(
                        &name,
                        &arguments,
                        context,
                        &core,
                        &cron_root,
                        plugin_loader.as_deref(),
                        &trusted_publishers,
                        &plugin_root,
                        &session_root,
                        worker_factory.as_deref(),
                        worker_service,
                    )
                    .await
                }
                _ => execute_tui_tool(&name, &arguments, &stores),
            }
        })
    }
}
