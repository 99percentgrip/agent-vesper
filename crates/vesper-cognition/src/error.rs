//! Sanitized error type. Error messages never include file contents, API
//! keys, full paths, or extracted memory text — only sanitized categories
//! and bounded context.

use thiserror::Error;

/// Cognitive-memory error. Secret-safe by construction.
#[derive(Debug, Error)]
pub enum CognitionError {
    /// Configuration or input violates a structural rule.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),

    /// The configured root was not absolute or its parent was missing.
    #[error("cognition root is not absolute or parent missing")]
    InvalidRoot,

    /// The caller provided no session identifier (user_id / agent_id / run_id).
    #[error("scope requires at least one of user_id, agent_id, or run_id")]
    MissingScope,

    /// SQLite failed to open or run a statement.
    #[error("storage failure: {0}")]
    Storage(String),

    /// The embedding port returned a vector of the wrong dimension.
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    EmbeddingDimension { expected: usize, actual: usize },

    /// The embedding port failed.
    #[error("embedding port failure")]
    Embedding,

    /// The extraction LLM port failed.
    #[error("extraction LLM port failure")]
    Extraction,

    /// The LLM returned a response that could not be parsed as JSON.
    #[error("extraction response was not valid JSON")]
    ExtractionParse,

    /// Serialization/deserialization failed on a payload.
    #[error("payload (de)serialization failure: {0}")]
    Payload(String),

    /// An I/O error outside SQLite (root probe, etc.).
    #[error("io failure: {0}")]
    Io(String),
}

impl From<rusqlite::Error> for CognitionError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Storage(value.to_string())
    }
}

impl From<std::io::Error> for CognitionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for CognitionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Payload(value.to_string())
    }
}

/// Result alias.
pub type Result<T, E = CognitionError> = std::result::Result<T, E>;
