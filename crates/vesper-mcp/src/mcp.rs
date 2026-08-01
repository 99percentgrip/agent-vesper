//! MCP server registry + stdio JSON-RPC client.
//!
//! [`McpRegistry`] is the persistent backing for `/mcp`: a config-driven
//! list of MCP servers (stdio command or future HTTP URL). [`McpClient`]
//! is a bounded JSON-RPC 2.0 over stdio client that performs the MCP
//! handshake and supports discovery plus `tools/call`. The subprocess is
//! scoped and killed/reaped on every path, so no child process leaks.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::McpError;
use crate::plugins::{append_line, read_all_jsonl};

/// MCP protocol version this client advertises (mirrors the oracle's
/// `MCP_PROTOCOL_VERSION = "2025-06-18"`).
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
/// Maximum bytes of a single MCP server id.
pub const MAX_SERVER_ID_CHARS: usize = 64;
/// Maximum bytes of a configured command string.
pub const MAX_COMMAND_CHARS: usize = 1024;
/// Maximum number of MCP servers the registry will hold.
pub const MAX_SERVERS: usize = 100;
/// Maximum bytes of a single JSON-RPC response we will read.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Transport for an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// Spawn a subprocess and speak JSON-RPC over its stdin/stdout.
    Stdio,
    /// Connect to a bounded Streamable HTTP endpoint.
    Http,
}

/// One configured MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Stable opaque id (e.g. `playwright`, `zai-search`).
    pub id: String,
    /// Transport.
    pub transport: McpTransport,
    /// For stdio: the executable to spawn (e.g. `npx`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// For stdio: the argv (after the command).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// For HTTP: the endpoint URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional environment variable name containing a bearer token. The
    /// secret itself is never persisted in the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_env: Option<String>,
    /// Optional human-readable label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Creation timestamp.
    pub created_at: SystemTime,
}

impl McpServerConfig {
    /// Validates the config against the bounded contract.
    pub fn validate(&self) -> Result<(), McpError> {
        if self.id.is_empty() || self.id.len() > MAX_SERVER_ID_CHARS {
            return Err(McpError::BoundsViolated("server id length"));
        }
        if !self
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(McpError::BoundsViolated("server id charset"));
        }
        match self.transport {
            McpTransport::Stdio => {
                let Some(command) = &self.command else {
                    return Err(McpError::BoundsViolated("stdio command missing"));
                };
                if command.len() > MAX_COMMAND_CHARS {
                    return Err(McpError::BoundsViolated("command length"));
                }
            }
            McpTransport::Http => {
                if self.url.is_none() {
                    return Err(McpError::BoundsViolated("http url missing"));
                }
                if let Some(name) = &self.auth_env
                    && (name.is_empty()
                        || !name
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric() || character == '_'))
                {
                    return Err(McpError::BoundsViolated("auth environment name"));
                }
            }
        }
        Ok(())
    }
}

/// Returns the first-party MCP presets exposed by the Python oracle.
///
/// Presets are intentionally kept out of the persisted registry: users may
/// add custom servers, but cannot shadow the stable web, vision, or browser
/// routes. Credentials are resolved from the named environment variable only
/// and are never written to this configuration.
#[must_use]
pub fn builtin_servers() -> Vec<McpServerConfig> {
    vec![
        McpServerConfig {
            id: "zai_search".into(),
            transport: McpTransport::Http,
            command: None,
            args: Vec::new(),
            url: Some("https://api.z.ai/api/mcp/web_search_prime/mcp".into()),
            auth_env: Some("ZAI_API_KEY".into()),
            label: Some("Z.ai Web Search".into()),
            created_at: SystemTime::UNIX_EPOCH,
        },
        McpServerConfig {
            id: "zai_reader".into(),
            transport: McpTransport::Http,
            command: None,
            args: Vec::new(),
            url: Some("https://api.z.ai/api/mcp/web_reader/mcp".into()),
            auth_env: Some("ZAI_API_KEY".into()),
            label: Some("Z.ai Web Reader".into()),
            created_at: SystemTime::UNIX_EPOCH,
        },
        McpServerConfig {
            id: "zai_vision".into(),
            transport: McpTransport::Stdio,
            command: Some("npx".into()),
            args: vec!["-y".into(), "@z_ai/mcp-server@latest".into()],
            url: None,
            auth_env: Some("ZAI_API_KEY".into()),
            label: Some("Z.ai Vision".into()),
            created_at: SystemTime::UNIX_EPOCH,
        },
        McpServerConfig {
            id: "playwright".into(),
            transport: McpTransport::Stdio,
            command: Some("npx".into()),
            args: vec![
                "-y".into(),
                "@playwright/mcp@latest".into(),
                "--headless".into(),
                "--isolated".into(),
            ],
            url: None,
            auth_env: None,
            label: Some("Playwright Browser".into()),
            created_at: SystemTime::UNIX_EPOCH,
        },
    ]
}

/// One advertised MCP tool (a subset of the MCP `Tool` schema — enough
/// for `/mcp tools <name>` to render a useful list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    /// Tool name as the MCP server advertises it.
    pub name: String,
    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the tool arguments, when advertised by the server.
    #[serde(
        rename = "inputSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_schema: Option<serde_json::Value>,
}

/// In-memory cache + on-disk JSONL store of [`McpServerConfig`]s.
pub struct McpRegistry {
    root: PathBuf,
    state: Mutex<Vec<McpServerConfig>>,
}

impl std::fmt::Debug for McpRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpRegistry")
            .field("root", &self.root)
            .field("servers", &self.state.lock().map(|s| s.len()).unwrap_or(0))
            .finish_non_exhaustive()
    }
}

impl McpRegistry {
    /// Opens (or creates) an MCP registry rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, McpError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let state = read_all_jsonl::<McpServerConfig>(&log_path)?;
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(state),
        })
    }

    fn validate_root(root: &Path) -> Result<(), McpError> {
        if !root.is_absolute() {
            return Err(McpError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.as_os_str().is_empty() => Ok(()),
            Some(parent) if parent.exists() => Ok(()),
            _ => Err(McpError::InvalidRoot),
        }
    }

    fn log_path(root: &Path) -> PathBuf {
        root.join("mcp.jsonl")
    }

    /// Returns the current server count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("mcp registry mutex poisoned")
            .len()
    }

    /// Returns true when the registry holds no servers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists every configured server.
    #[must_use]
    pub fn list(&self) -> Vec<McpServerConfig> {
        self.state
            .lock()
            .expect("mcp registry mutex poisoned")
            .clone()
    }

    /// Returns the server with the given id, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<McpServerConfig> {
        self.state
            .lock()
            .expect("mcp registry mutex poisoned")
            .iter()
            .find(|server| server.id == id)
            .cloned()
    }

    /// Returns a custom server or one of the protected first-party presets.
    #[must_use]
    pub fn get_with_builtins(&self, id: &str) -> Option<McpServerConfig> {
        builtin_servers()
            .into_iter()
            .find(|server| server.id == id)
            .or_else(|| self.get(id))
    }

    /// Lists custom servers together with the protected first-party presets.
    #[must_use]
    pub fn list_with_builtins(&self) -> Vec<McpServerConfig> {
        let custom = self.list();
        let builtins = builtin_servers();
        let builtin_ids = builtins
            .iter()
            .map(|server| server.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut servers = builtins;
        servers.extend(
            custom
                .into_iter()
                .filter(|server| !builtin_ids.contains(&server.id)),
        );
        servers
    }

    /// Adds a configured server. Idempotent on the id.
    pub fn add(&self, mut config: McpServerConfig) -> Result<McpServerConfig, McpError> {
        config.validate()?;
        if builtin_servers()
            .iter()
            .any(|builtin| builtin.id == config.id)
        {
            return Err(McpError::BuiltinServerProtected(config.id));
        }
        let mut state = self.state.lock().expect("mcp registry mutex poisoned");
        if state.len() >= MAX_SERVERS {
            return Err(McpError::BoundsViolated("server count"));
        }
        config.created_at = SystemTime::now();
        // Replace if the id already exists.
        state.retain(|existing| existing.id != config.id);
        let serialized = serde_json::to_string(&config)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.push(config.clone());
        Ok(config)
    }

    /// Removes the server with the given id. Idempotent.
    pub fn remove(&self, id: &str) -> Result<bool, McpError> {
        if builtin_servers().iter().any(|builtin| builtin.id == id) {
            return Err(McpError::BuiltinServerProtected(id.to_owned()));
        }
        let mut state = self.state.lock().expect("mcp registry mutex poisoned");
        let before = state.len();
        state.retain(|server| server.id != id);
        let removed = before != state.len();
        if removed {
            // Rewrite the log atomically.
            let mut buffer = String::new();
            for server in state.iter() {
                buffer.push_str(&serde_json::to_string(server)?);
                buffer.push('\n');
            }
            atomic_write(&Self::log_path(&self.root), buffer.as_bytes())?;
        }
        Ok(removed)
    }
}

/// Minimal bounded MCP client. Each operation uses a fresh scoped stdio
/// subprocess, performs the handshake, and then performs discovery or one
/// tool call. The client does not retain processes or credentials between
/// operations.
pub struct McpClient;

impl McpClient {
    /// Connects to the configured stdio server, performs the MCP
    /// handshake, and lists the advertised tools. Returns the tool
    /// descriptors.
    pub fn tools(config: &McpServerConfig) -> Result<Vec<McpToolDescriptor>, McpError> {
        if config.transport == McpTransport::Http {
            return http_tools(config);
        }
        let mut process = McpProcess::spawn(config)?;
        process.initialize()?;
        let tools_response = process.request(2, "tools/list", serde_json::json!({}))?;
        // Parse the result.tools array.
        let tools_value = tools_response
            .get("result")
            .and_then(|result| result.get("tools"))
            .cloned()
            .unwrap_or(serde_json::Value::Array(Vec::new()));
        let mut tools = Vec::new();
        if let Some(array) = tools_value.as_array() {
            for entry in array {
                let name = entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unnamed)")
                    .to_string();
                let description = entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                tools.push(McpToolDescriptor {
                    name,
                    description,
                    input_schema: entry.get("inputSchema").cloned(),
                });
            }
        }
        Ok(tools)
    }

    /// Calls one advertised tool and returns the MCP `result` object. The
    /// result remains JSON so hosts can preserve structured content while
    /// applying their own output rendering and bounds.
    pub fn call_tool(
        config: &McpServerConfig,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        if tool.is_empty() || tool.len() > MAX_COMMAND_CHARS {
            return Err(McpError::BoundsViolated("tool name length"));
        }
        if !arguments.is_object() {
            return Err(McpError::BoundsViolated("tool arguments must be an object"));
        }
        if config.transport == McpTransport::Http {
            let (_, session) = http_request_with_session(
                config,
                1,
                "initialize",
                serde_json::json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "agent-vesper-tui", "version": env!("CARGO_PKG_VERSION")}
                }),
                None,
            )?;
            http_notification(config, "notifications/initialized", session.as_deref())?;
            return http_request_with_session(
                config,
                2,
                "tools/call",
                serde_json::json!({"name": tool, "arguments": arguments}),
                session.as_deref(),
            )?
            .0
            .get("result")
            .cloned()
            .ok_or(McpError::Http("tool call returned no result"));
        }
        let mut process = McpProcess::spawn(config)?;
        process.initialize()?;
        let response = process.request(
            2,
            "tools/call",
            serde_json::json!({"name": tool, "arguments": arguments}),
        )?;
        response
            .get("result")
            .cloned()
            .ok_or(McpError::Subprocess("tool call returned no result"))
    }
}

fn http_tools(config: &McpServerConfig) -> Result<Vec<McpToolDescriptor>, McpError> {
    let (_, session) = http_request_with_session(
        config,
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "agent-vesper-tui", "version": env!("CARGO_PKG_VERSION")}
        }),
        None,
    )?;
    http_notification(config, "notifications/initialized", session.as_deref())?;
    let response = http_request_with_session(
        config,
        2,
        "tools/list",
        serde_json::json!({}),
        session.as_deref(),
    )?
    .0;
    let mut tools = Vec::new();
    if let Some(array) = response
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in array {
            let name = entry
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(unnamed)")
                .to_owned();
            tools.push(McpToolDescriptor {
                name,
                description: entry
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                input_schema: entry.get("inputSchema").cloned(),
            });
        }
    }
    Ok(tools)
}

fn http_request_with_session(
    config: &McpServerConfig,
    id: u64,
    method: &str,
    params: serde_json::Value,
    session: Option<&str>,
) -> Result<(serde_json::Value, Option<String>), McpError> {
    let url = config.url.as_deref().ok_or(McpError::Http("url missing"))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| McpError::Http("client"))?;
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", method)
        .json(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
    if let Some(session) = session {
        request = request.header("Mcp-Session-Id", session);
    }
    if method == "tools/call"
        && let Some(name) = params.get("name").and_then(serde_json::Value::as_str)
    {
        request = request.header("Mcp-Name", name);
    }
    if let Some(environment) = &config.auth_env {
        let token = std::env::var(environment)
            .or_else(|_| {
                if environment == "ZAI_API_KEY" {
                    std::env::var("Z_AI_API_KEY")
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .map_err(|_| McpError::Http("auth unavailable"))?;
        if token.len() > 8 * 1024 {
            return Err(McpError::BoundsViolated("auth token size"));
        }
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|_| McpError::Http("send"))?;
    if !response.status().is_success() {
        return Err(McpError::Http("non-success response"));
    }
    let session_id = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = response.bytes().map_err(|_| McpError::Http("body"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(McpError::BoundsViolated("response size"));
    }
    let value = decode_http_payload(&bytes, &content_type)?;
    if value.get("error").is_some() {
        return Err(McpError::Http("remote error"));
    }
    Ok((value, session_id))
}

fn http_notification(
    config: &McpServerConfig,
    method: &str,
    session: Option<&str>,
) -> Result<(), McpError> {
    let url = config.url.as_deref().ok_or(McpError::Http("url missing"))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| McpError::Http("client"))?;
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", method)
        .json(&serde_json::json!({"jsonrpc":"2.0","method":method}));
    if let Some(session) = session {
        request = request.header("Mcp-Session-Id", session);
    }
    if let Some(environment) = &config.auth_env {
        let token = std::env::var(environment)
            .or_else(|_| {
                if environment == "ZAI_API_KEY" {
                    std::env::var("Z_AI_API_KEY")
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .map_err(|_| McpError::Http("auth unavailable"))?;
        if token.len() > 8 * 1024 {
            return Err(McpError::BoundsViolated("auth token size"));
        }
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|_| McpError::Http("send"))?;
    if !response.status().is_success() {
        return Err(McpError::Http("notification failed"));
    }
    Ok(())
}

fn decode_http_payload(bytes: &[u8], content_type: &str) -> Result<serde_json::Value, McpError> {
    if content_type.contains("text/event-stream") {
        let text = std::str::from_utf8(bytes).map_err(|_| McpError::Http("parse"))?;
        let mut last = None;
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if !data.is_empty() {
                    last = Some(serde_json::from_str(data).map_err(|_| McpError::Http("parse"))?);
                }
            }
        }
        return last.ok_or(McpError::Http("empty event stream"));
    }
    serde_json::from_slice(bytes).map_err(|_| McpError::Http("parse"))
}

/// A single scoped MCP subprocess. The reader is retained across requests;
/// creating a fresh `BufReader` per request could discard bytes a server
/// wrote ahead of the response being awaited.
struct McpProcess {
    child: std::process::Child,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl McpProcess {
    fn spawn(config: &McpServerConfig) -> Result<Self, McpError> {
        if config.transport != McpTransport::Stdio {
            return Err(McpError::Subprocess("non-stdio transport"));
        }
        let command = config
            .command
            .as_ref()
            .ok_or(McpError::Subprocess("missing command"))?;
        let mut process = Command::new(command);
        process
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if config.id == "playwright" {
            for (key, _) in std::env::vars() {
                let uppercase = key.to_ascii_uppercase();
                if uppercase.ends_with("_API_KEY")
                    || uppercase.ends_with("_TOKEN")
                    || uppercase.ends_with("_SECRET")
                    || uppercase.ends_with("_PASSWORD")
                    || uppercase.ends_with("_CREDENTIAL")
                    || uppercase.ends_with("_PRIVATE_KEY")
                    || uppercase.ends_with("_ACCESS_KEY")
                    || uppercase == "SSH_AUTH_SOCK"
                {
                    process.env_remove(key);
                }
            }
        }
        if config.id == "zai_vision"
            && let Some(environment) = &config.auth_env
        {
            let token = std::env::var(environment)
                .or_else(|_| {
                    if environment == "ZAI_API_KEY" {
                        std::env::var("Z_AI_API_KEY")
                    } else {
                        Err(std::env::VarError::NotPresent)
                    }
                })
                .map_err(|_| McpError::Http("auth unavailable"))?;
            if token.len() > 8 * 1024 {
                return Err(McpError::BoundsViolated("auth token size"));
            }
            process.env("Z_AI_API_KEY", token);
            process.env("Z_AI_MODE", "ZAI");
        }
        let mut child = process.spawn().map_err(|_| McpError::Subprocess("spawn"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(McpError::Subprocess("no stdout"))?;
        Ok(Self {
            child,
            stdout: std::io::BufReader::new(stdout),
        })
    }

    fn initialize(&mut self) -> Result<(), McpError> {
        let request = jsonrpc_request(
            1,
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "agent-vesper-tui",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        );
        let _ = self.request_raw(&request)?;
        write_line(
            self.child.stdin.as_mut(),
            &jsonrpc_notification("notifications/initialized", serde_json::json!({})),
        )
    }

    fn request(
        &mut self,
        id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        self.request_raw(&jsonrpc_request(id, method, params))
    }

    fn request_raw(&mut self, request: &str) -> Result<serde_json::Value, McpError> {
        let parsed: serde_json::Value =
            serde_json::from_str(request).map_err(|_| McpError::Serde)?;
        let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
        write_line(self.child.stdin.as_mut(), request)?;
        read_response(&mut self.stdout, id)
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Writes one JSON-RPC line to the child's stdin.
fn write_line(stdin: Option<&mut std::process::ChildStdin>, line: &str) -> Result<(), McpError> {
    use std::io::Write;
    let Some(stdin) = stdin else {
        return Err(McpError::Subprocess("no stdin"));
    };
    writeln!(stdin, "{line}").map_err(|_| McpError::Subprocess("write"))?;
    stdin.flush().map_err(|_| McpError::Subprocess("flush"))?;
    Ok(())
}

/// Reads one JSON-RPC line from the child's stdout, skipping
/// non-JSON-RPC notifications until a `result` for our id arrives.
fn read_response(
    stdout: &mut std::io::BufReader<std::process::ChildStdout>,
    expected_id: serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    use std::io::BufRead;
    let mut total_bytes = 0usize;
    let mut buffer = String::new();
    loop {
        buffer.clear();
        let read = stdout
            .read_line(&mut buffer)
            .map_err(|_| McpError::Subprocess("read"))?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read);
        if total_bytes > MAX_RESPONSE_BYTES || buffer.len() > MAX_RESPONSE_BYTES {
            return Err(McpError::BoundsViolated("response size"));
        }
        let line = buffer.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|_| McpError::Subprocess("parse"))?;
        // Skip notifications (no id) and mismatched ids.
        if value.get("id") == Some(&expected_id) {
            return Ok(value);
        }
    }
    Err(McpError::Subprocess("no response"))
}

/// Builds a JSON-RPC 2.0 request string.
fn jsonrpc_request(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// Builds a JSON-RPC 2.0 notification string (no id).
fn jsonrpc_notification(method: &str, params: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
    .to_string()
}

/// Atomic write helper.
fn atomic_write(target: &Path, payload: &[u8]) -> Result<(), McpError> {
    use std::io::Write;
    let parent = target.parent().ok_or(McpError::InvalidRoot)?;
    let temp = parent.join(format!(
        ".{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("mcp")
    ));
    {
        let mut file = std::fs::File::create(&temp).map_err(|_| McpError::io("create"))?;
        file.write_all(payload).map_err(|_| McpError::io("write"))?;
        file.sync_all().map_err(|_| McpError::io("fsync"))?;
    }
    std::fs::rename(&temp, target).map_err(|_| McpError::io("rename"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! MCP registry: add, remove, persistence; client validation.

    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn registry_under(temp: &TempDir) -> (PathBuf, McpRegistry) {
        let root = temp.path().join("mcp-root");
        fs::create_dir_all(&root).unwrap();
        let registry = McpRegistry::open(&root).unwrap();
        (root, registry)
    }

    #[test]
    fn add_persists_across_reopen() {
        let temp = TempDir::new().unwrap();
        let (root, registry) = registry_under(&temp);
        let config = registry
            .add(McpServerConfig {
                id: "demo".into(),
                transport: McpTransport::Stdio,
                command: Some("echo".into()),
                args: vec!["hello".into()],
                url: None,
                auth_env: None,
                label: Some("Demo server".into()),
                created_at: SystemTime::UNIX_EPOCH,
            })
            .unwrap();
        assert_eq!(config.id, "demo");
        // Reopen from the same root.
        let reopened = McpRegistry::open(&root).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.list()[0].id, "demo");
    }

    #[test]
    fn remove_unlinks_from_log() {
        let temp = TempDir::new().unwrap();
        let (_root, registry) = registry_under(&temp);
        registry
            .add(McpServerConfig {
                id: "ephemeral".into(),
                transport: McpTransport::Stdio,
                command: Some("echo".into()),
                args: Vec::new(),
                url: None,
                auth_env: None,
                label: None,
                created_at: SystemTime::UNIX_EPOCH,
            })
            .unwrap();
        assert!(registry.remove("ephemeral").unwrap());
        assert!(!registry.remove("ephemeral").unwrap());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn rejects_invalid_server_id() {
        let temp = TempDir::new().unwrap();
        let (_root, registry) = registry_under(&temp);
        let err = registry
            .add(McpServerConfig {
                id: "bad id with spaces".into(),
                transport: McpTransport::Stdio,
                command: Some("echo".into()),
                args: Vec::new(),
                url: None,
                auth_env: None,
                label: None,
                created_at: SystemTime::UNIX_EPOCH,
            })
            .unwrap_err();
        assert_eq!(err, McpError::BoundsViolated("server id charset"));
    }

    #[test]
    fn rejects_stdio_config_without_command() {
        let temp = TempDir::new().unwrap();
        let (_root, registry) = registry_under(&temp);
        let err = registry
            .add(McpServerConfig {
                id: "incomplete".into(),
                transport: McpTransport::Stdio,
                command: None,
                args: Vec::new(),
                url: None,
                auth_env: None,
                label: None,
                created_at: SystemTime::UNIX_EPOCH,
            })
            .unwrap_err();
        assert_eq!(err, McpError::BoundsViolated("stdio command missing"));
    }

    #[test]
    fn stdio_client_discovers_and_calls_a_tool() {
        let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"id":1'*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{}}' ;;
    *'"method":"tools/list"'*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"echo input","inputSchema":{"type":"object"}}]}}' ;;
    *'"method":"tools/call"'*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"ok"}],"isError":false}}' ;;
  esac
done
"#;
        let config = McpServerConfig {
            id: "demo".into(),
            transport: McpTransport::Stdio,
            command: Some("sh".into()),
            args: vec!["-c".into(), script.into()],
            url: None,
            auth_env: None,
            label: None,
            created_at: SystemTime::UNIX_EPOCH,
        };
        let tools = McpClient::tools(&config).unwrap();
        assert_eq!(tools[0].name, "echo");
        assert_eq!(
            tools[0].input_schema,
            Some(serde_json::json!({"type": "object"}))
        );
        let result =
            McpClient::call_tool(&config, "echo", serde_json::json!({"value": "ok"})).unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["text"], "ok");
    }

    #[test]
    fn first_party_mcp_presets_are_present_and_protected() {
        let presets = builtin_servers();
        assert_eq!(presets.len(), 4);
        assert!(presets.iter().any(|server| server.id == "zai_search"));
        assert!(presets.iter().any(|server| server.id == "zai_reader"));
        assert!(presets.iter().any(|server| server.id == "zai_vision"));
        assert!(presets.iter().any(|server| server.id == "playwright"));

        let temp = TempDir::new().unwrap();
        let (_root, registry) = registry_under(&temp);
        let error = registry
            .add(McpServerConfig {
                id: "playwright".into(),
                transport: McpTransport::Stdio,
                command: Some("echo".into()),
                args: Vec::new(),
                url: None,
                auth_env: None,
                label: None,
                created_at: SystemTime::UNIX_EPOCH,
            })
            .unwrap_err();
        assert_eq!(error, McpError::BuiltinServerProtected("playwright".into()));
    }

    #[test]
    fn streamable_http_event_payload_is_bounded_and_decoded() {
        let value = decode_http_payload(
            b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n",
            "text/event-stream",
        )
        .unwrap();
        assert_eq!(value["id"], 2);
    }
}
