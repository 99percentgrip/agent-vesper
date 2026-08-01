//! Secret-safe error type for the memory subsystem.
//!
//! Errors never leak raw file contents, full paths, or payloads that may
//! carry secrets. Each variant carries a stable, sanitised message suitable
//! for surfacing in the TUI transcript without redaction post-processing.

use thiserror::Error;

/// All errors raised by [`crate::MemoryStore`], [`crate::SkillStore`],
/// [`crate::UserProfile`], and [`crate::AwarenessLedger`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MemoryError {
    /// The supplied root was not absolute, or its parent does not exist.
    /// The confinement rule mirrors the Stage 6 session writer.
    #[error("memory root is not absolute or its parent does not exist")]
    InvalidRoot,
    /// An input exceeded a configured bound (size, count, length).
    #[error("input exceeded a configured bound: {0}")]
    BoundsViolated(&'static str),
    /// A record id was not found.
    #[error("record not found: {0}")]
    NotFound(String),
    /// A supplied identifier failed validation.
    #[error("invalid identifier: {0}")]
    InvalidIdentifier(String),
    /// A filesystem operation failed. The path is not included; only the
    /// high-level kind is reported so secret-laden paths never surface.
    #[error("filesystem operation failed: {kind}")]
    Io {
        /// High-level kind (`"read"`, `"write"`, `"rename"`, `"create"`).
        kind: &'static str,
    },
    /// A serialisation or deserialisation failure on a memory artefact.
    /// The payload is deliberately omitted.
    #[error("memory artefact could not be (de)serialised")]
    Serde,
    /// The persistent store has reached its configured entry cap.
    #[error("memory store is full (entry cap reached)")]
    StoreFull,
}

impl From<std::io::Error> for MemoryError {
    /// Maps any `io::Error` to [`MemoryError::Io`] without leaking the
    /// underlying path or payload. The caller supplies the high-level kind
    /// when raising the error via [`MemoryError::io`].
    fn from(error: std::io::Error) -> Self {
        // Touch the error so the compiler keeps the relationship explicit
        // even though we never surface its message.
        let _ = error.kind();
        Self::Io { kind: "io" }
    }
}

impl From<serde_json::Error> for MemoryError {
    fn from(_error: serde_json::Error) -> Self {
        Self::Serde
    }
}

impl MemoryError {
    /// Constructs an [`MemoryError::Io`] with a caller-supplied high-level
    /// kind. Use this instead of `?` on `io` results so the diagnostic stays
    /// secret-safe while still being actionable.
    #[must_use]
    pub fn io(kind: &'static str) -> Self {
        Self::Io { kind }
    }
}
