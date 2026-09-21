//! In-memory, owner-scoped stdio connections. Never a process-global cache.
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::mcp::{MAX_COMMAND_CHARS, McpProcess};
use crate::{McpClient, McpError, McpServerConfig, McpToolDescriptor, McpTransport};

/// Maximum retained stdio connections in one host conversation.
pub const MAX_SESSION_SERVERS: usize = 16;

struct Connection {
    config: McpServerConfig,
    process: Option<McpProcess>,
    next_id: u64,
}

/// One conversation's serialized MCP stdio lifecycle. Independent owners never
/// share processes, even with identical server configuration and workspace.
/// Calls are bounded, never replayed, and transport failure quarantines a server
/// until explicit close. HTTP continues to use the existing per-call transport.
pub struct McpSession {
    connections: Mutex<BTreeMap<String, Connection>>,
    timeout: Duration,
}

impl Default for McpSession {
    fn default() -> Self {
        Self::with_timeout(Duration::from_secs(60))
    }
}

impl McpSession {
    /// Creates an empty owner with a bounded operation deadline (at most 60 s).
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            connections: Mutex::new(BTreeMap::new()),
            timeout: timeout.clamp(Duration::from_millis(1), Duration::from_secs(60)),
        }
    }

    /// Discovers tools without replacing an already-running stdio connection.
    pub fn tools(&self, config: &McpServerConfig) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.tools_cancellable(config, &|| false)
    }

    /// Discovery with the caller's cancellation signal.
    pub fn tools_cancellable(
        &self,
        config: &McpServerConfig,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        if config.transport == McpTransport::Http {
            if cancelled() {
                return Err(McpError::Subprocess("cancelled before dispatch"));
            }
            return McpClient::tools(config);
        }
        let result = self.request(config, "tools/list", json!({}), cancelled)?;
        serde_json::from_value(
            result
                .get("tools")
                .cloned()
                .ok_or(McpError::Subprocess("missing tools"))?,
        )
        .map_err(|_| McpError::Subprocess("invalid tools"))
    }

    /// Calls a tool on the same initialized stdio process as prior calls.
    pub fn call_tool(
        &self,
        config: &McpServerConfig,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, McpError> {
        self.call_tool_cancellable(config, tool, arguments, &|| false)
    }

    /// As above, but checks cancellation before dispatch and during stdio I/O.
    /// Cancellation after dispatch means effects may be unknown, never success.
    pub fn call_tool_cancellable(
        &self,
        config: &McpServerConfig,
        tool: &str,
        arguments: Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, McpError> {
        if tool.is_empty() || tool.len() > MAX_COMMAND_CHARS {
            return Err(McpError::BoundsViolated("tool name length"));
        }
        if !arguments.is_object() {
            return Err(McpError::BoundsViolated("tool arguments must be an object"));
        }
        if cancelled() {
            return Err(McpError::Subprocess("cancelled before dispatch"));
        }
        if config.transport == McpTransport::Http {
            return McpClient::call_tool(config, tool, arguments);
        }
        self.request(
            config,
            "tools/call",
            json!({"name": tool, "arguments": arguments}),
            cancelled,
        )
    }

    /// Ends all connections when the owning host switches conversations.
    /// Refuses a concurrent request rather than moving its browser to a new owner.
    pub fn reset(&self) -> Result<(), McpError> {
        self.connections
            .try_lock()
            .map_err(|_| McpError::Subprocess("MCP session busy"))?
            .clear();
        Ok(())
    }

    /// Explicitly releases a retained or quarantined connection; does not replay
    /// any request or certify previously ambiguous external effects.
    pub fn close(&self, server: &str) -> Result<(), McpError> {
        let mut connections = self
            .connections
            .try_lock()
            .map_err(|_| McpError::Subprocess("MCP session busy"))?;
        connections.remove(server);
        Ok(())
    }

    fn request(
        &self,
        config: &McpServerConfig,
        method: &str,
        params: Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, McpError> {
        config.validate()?;
        if cancelled() {
            return Err(McpError::Subprocess("cancelled before dispatch"));
        }
        let mut connections = self
            .connections
            .try_lock()
            .map_err(|_| McpError::Subprocess("MCP session busy"))?;
        let closing = method == "tools/call"
            && params.get("name").and_then(Value::as_str) == Some("browser_close");
        if closing && !connections.contains_key(&config.id) {
            return Ok(
                json!({"content":[{"type":"text", "text":"No retained MCP session to close."}], "isError":false}),
            );
        }
        if closing
            && connections
                .get(&config.id)
                .is_some_and(|c| c.process.is_none() || c.config != *config)
        {
            connections.remove(&config.id);
            return Err(McpError::Subprocess(
                "old or lost MCP session cleared; prior effects remain unknown",
            ));
        }
        let deadline = Instant::now() + self.timeout;
        if !connections.contains_key(&config.id) {
            if connections.len() >= MAX_SESSION_SERVERS {
                return Err(McpError::BoundsViolated("session server count"));
            }
            let mut process = McpProcess::spawn(config)?;
            process.initialize_cancellable(deadline, cancelled)?;
            connections.insert(
                config.id.clone(),
                Connection {
                    config: config.clone(),
                    process: Some(process),
                    next_id: 2,
                },
            );
        }
        let connection = connections
            .get_mut(&config.id)
            .expect("inserted connection");
        if connection.config != *config {
            return Err(McpError::Subprocess(
                "MCP configuration changed; close the session before reconnecting",
            ));
        }
        let process = connection.process.as_mut().ok_or(McpError::Subprocess("MCP session lost; close before starting a new session; do not replay uncertain actions"))?;
        let id = connection.next_id;
        connection.next_id = id
            .checked_add(1)
            .ok_or(McpError::BoundsViolated("request id"))?;
        let result = process
            .request_cancellable(id, method, params, deadline, cancelled)
            .and_then(|response| {
                if response.get("error").is_some() {
                    return Err(McpError::Subprocess("JSON-RPC error"));
                }
                response
                    .get("result")
                    .cloned()
                    .ok_or(McpError::Subprocess("missing result"))
            });
        if result.is_err() {
            connection.process.take();
        }
        if closing
            && result
                .as_ref()
                .is_ok_and(|value| value.get("isError") != Some(&Value::Bool(true)))
        {
            connections.remove(&config.id);
        }
        result
    }
}
