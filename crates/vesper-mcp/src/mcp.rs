//! MCP server registry + stdio JSON-RPC client.
//!
//! [`McpRegistry`] is the persistent backing for `/mcp`: a config-driven
//! list of MCP servers (stdio command or future HTTP URL). [`McpClient`]
//! is a minimal JSON-RPC 2.0 over stdio client that spawns the
//! configured subprocess, sends `initialize` + `tools/list`, and returns
//! the advertised tools. The subprocess is scoped (`Child` drops → RAII
//! reaps the process), so no descriptors leak.

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
    /// Connect to a Streamable HTTP endpoint (NOT yet implemented — the
    /// oracle's HTTP path requires live provider credentials, which
    /// foundation verification forbids).
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
    /// For HTTP: the endpoint URL (not yet implemented).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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
            }
        }
        Ok(())
    }
}

/// One advertised MCP tool (a subset of the MCP `Tool` schema — enough
/// for `/mcp tools <name>` to render a useful list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    /// Tool name as the MCP server advertises it.
    pub name: String,
    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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

    /// Adds a configured server. Idempotent on the id.
    pub fn add(&self, mut config: McpServerConfig) -> Result<McpServerConfig, McpError> {
        config.validate()?;
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

/// Minimal MCP stdio client. Spawns the configured subprocess, sends
/// `initialize` + `tools/list`, parses the JSON-RPC responses, and
/// returns the advertised tools. The subprocess is scoped: when
/// [`McpClient::tools`] returns, the `Child` has been dropped and the
/// process reaped (RAII).
pub struct McpClient;

impl McpClient {
    /// Connects to the configured stdio server, performs the MCP
    /// handshake, and lists the advertised tools. Returns the tool
    /// descriptors.
    pub fn tools(config: &McpServerConfig) -> Result<Vec<McpToolDescriptor>, McpError> {
        if config.transport != McpTransport::Stdio {
            return Err(McpError::Subprocess("non-stdio transport"));
        }
        let command = config
            .command
            .as_ref()
            .ok_or(McpError::Subprocess("missing command"))?;
        // Spawn the subprocess with piped stdin/stdout. Scoped: `child`
        // drops at the end of this function, reaping the process.
        let mut child = Command::new(command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| McpError::Subprocess("spawn"))?;
        // 1. initialize handshake.
        let init_request = jsonrpc_request(
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
        let _init_response = round_trip(&mut child, &init_request)?;
        // Best-effort: send initialized notification (no response expected).
        let _ = write_line(
            child.stdin.as_mut(),
            &jsonrpc_notification("notifications/initialized", serde_json::json!({})),
        );
        // 2. tools/list.
        let tools_request = jsonrpc_request(2, "tools/list", serde_json::json!({}));
        let tools_response = round_trip(&mut child, &tools_request)?;
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
                tools.push(McpToolDescriptor { name, description });
            }
        }
        // `child` drops here → subprocess reaped.
        let _ = child.kill();
        Ok(tools)
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
    stdout: Option<&mut std::process::ChildStdout>,
    expected_id: serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    use std::io::BufRead;
    let Some(stdout) = stdout else {
        return Err(McpError::Subprocess("no stdout"));
    };
    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.map_err(|_| McpError::Subprocess("read"))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(&line).map_err(|_| McpError::Subprocess("parse"))?;
        // Skip notifications (no id) and mismatched ids.
        if value.get("id") == Some(&expected_id) {
            return Ok(value);
        }
    }
    Err(McpError::Subprocess("no response"))
}

/// Sends a request and reads its response.
fn round_trip(
    child: &mut std::process::Child,
    request: &str,
) -> Result<serde_json::Value, McpError> {
    let parsed: serde_json::Value = serde_json::from_str(request).map_err(|_| McpError::Serde)?;
    let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
    write_line(child.stdin.as_mut(), request)?;
    read_response(child.stdout.as_mut(), id)
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
                label: None,
                created_at: SystemTime::UNIX_EPOCH,
            })
            .unwrap_err();
        assert_eq!(err, McpError::BoundsViolated("stdio command missing"));
    }
}
