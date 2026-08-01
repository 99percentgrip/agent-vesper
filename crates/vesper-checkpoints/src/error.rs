//! Secret-safe error type for the checkpoints subsystem.
//!
//! Errors never leak raw file contents, full paths, or payloads that may
//! carry secrets. Each variant carries a stable, sanitised message suitable
//! for surfacing in the TUI transcript without redaction post-processing.

use thiserror::Error;

/// All errors raised by [`crate::CheckpointsLedger`],
/// [`crate::SessionLineage`], [`crate::CronRegistry`],
/// [`crate::SessionExporter`], [`crate::ClipboardPort`], and
/// [`crate::CiStatusReader`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CheckpointError {
    /// The supplied root was not absolute, or its parent does not exist.
    /// Mirrors the confinement rule of `vesper-sessions` and `vesper-memory`.
    #[error("checkpoint root is not absolute or its parent does not exist")]
    InvalidRoot,
    /// The workspace root supplied to a snapshot operation was not absolute.
    #[error("workspace root is not absolute")]
    InvalidWorkspaceRoot,
    /// An input exceeded a configured bound (size, count, length).
    #[error("input exceeded a configured bound: {0}")]
    BoundsViolated(&'static str),
    /// A checkpoint id was not found in the ledger.
    #[error("checkpoint not found: {0}")]
    CheckpointNotFound(String),
    /// A session id was not found in the lineage.
    #[error("session not found: {0}")]
    SessionNotFound(String),
    /// A workspace path escaped the configured workspace root.
    #[error("workspace path escapes the root")]
    PathEscape,
    /// A workspace file was too large to snapshot.
    #[error("workspace file exceeds MAX_FILE_SIZE_BYTES")]
    FileTooLarge,
    /// A workspace file matched the sensitive-file guard and was refused.
    #[error("workspace file matched the sensitive-file guard")]
    SensitiveFile,
    /// A filesystem operation failed. The path is not included; only the
    /// high-level kind is reported so secret-laden paths never surface.
    #[error("filesystem operation failed: {kind}")]
    Io {
        /// High-level kind (`"read"`, `"write"`, `"rename"`, `"create"`,
        /// `"remove"`, `"copy"`).
        kind: &'static str,
    },
    /// A serialisation or deserialisation failure on a checkpoint artefact.
    /// The payload is deliberately omitted.
    #[error("checkpoint artefact could not be (de)serialised")]
    Serde,
    /// The persistent ledger has reached its configured retention cap and
    /// the caller refused to prune (e.g. a session-scoped rollback that
    /// must keep history).
    #[error("checkpoint retention cap reached")]
    RetentionCapReached,
    /// A subprocess invocation failed (used by the CI status reader).
    #[error("subprocess invocation failed")]
    Subprocess,
    /// The requested capability is not available in this environment (used
    /// by the clipboard port when no clipboard is reachable).
    #[error("capability not available: {0}")]
    Unavailable(&'static str),
}

impl From<std::io::Error> for CheckpointError {
    /// Maps any `io::Error` to [`CheckpointError::Io`] without leaking the
    /// underlying path or payload.
    fn from(error: std::io::Error) -> Self {
        let _ = error.kind();
        Self::Io { kind: "io" }
    }
}

impl From<serde_json::Error> for CheckpointError {
    fn from(_error: serde_json::Error) -> Self {
        Self::Serde
    }
}

impl CheckpointError {
    /// Constructs a [`CheckpointError::Io`] with a caller-supplied
    /// high-level kind. Use this instead of `?` on `io` results so the
    /// diagnostic stays secret-safe while still being actionable.
    #[must_use]
    pub fn io(kind: &'static str) -> Self {
        Self::Io { kind }
    }
}
