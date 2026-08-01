//! Secret-safe error type for the MCP + plugins subsystem.

use thiserror::Error;

/// All errors raised by [`crate::McpRegistry`], [`crate::McpClient`],
/// [`crate::PluginLoader`], and [`crate::TrustedPublishers`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum McpError {
    /// The supplied root was not absolute, or its parent does not exist.
    #[error("mcp root is not absolute or its parent does not exist")]
    InvalidRoot,
    /// An input exceeded a configured bound (size, count, length).
    #[error("input exceeded a configured bound: {0}")]
    BoundsViolated(&'static str),
    /// A plugin signature was missing, malformed, or did not verify
    /// against the trusted publisher's public key. This is the security-
    /// critical rejection path — it fires for ANY unsigned or tampered
    /// plugin under [`crate::PluginLoader::load`].
    #[error("plugin signature verification failed: {0}")]
    SignatureVerificationFailed(&'static str),
    /// The publisher who signed the plugin is not in the trusted
    /// publishers registry.
    #[error("publisher not trusted: {0}")]
    PublisherNotTrusted(String),
    /// A plugin manifest failed validation (bad id, bad permissions, etc.).
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(&'static str),
    /// A configured MCP server id was not found in the registry.
    #[error("mcp server not found: {0}")]
    ServerNotFound(String),
    /// A protected first-party server cannot be replaced or removed.
    #[error("built-in MCP server is protected: {0}")]
    BuiltinServerProtected(String),
    /// A filesystem operation failed. The path is not included.
    #[error("filesystem operation failed: {kind}")]
    Io { kind: &'static str },
    /// A serialisation or deserialisation failure.
    #[error("artefact could not be (de)serialised")]
    Serde,
    /// An MCP stdio subprocess failed to spawn, exit cleanly, or produce
    /// parseable JSON-RPC output.
    #[error("mcp subprocess failed: {0}")]
    Subprocess(&'static str),
    /// An HTTP MCP request failed without exposing the endpoint or body.
    #[error("mcp http request failed: {0}")]
    Http(&'static str),
    /// The plugin loader was asked to load an unsigned plugin in a
    /// `--release` build. This is the runtime mirror of the compile-time
    /// `#[cfg(debug_assertions)]` gate: even if a caller somehow reached
    /// this point, the loader hard-rejects.
    #[error("unsigned plugin loading is forbidden in release builds")]
    UnsignedForbidden,
}

impl From<std::io::Error> for McpError {
    fn from(error: std::io::Error) -> Self {
        let _ = error.kind();
        Self::Io { kind: "io" }
    }
}

impl From<serde_json::Error> for McpError {
    fn from(_error: serde_json::Error) -> Self {
        Self::Serde
    }
}

impl McpError {
    /// Constructs an [`McpError::Io`] with a caller-supplied high-level kind.
    #[must_use]
    pub fn io(kind: &'static str) -> Self {
        Self::Io { kind }
    }
}
