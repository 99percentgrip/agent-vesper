use thiserror::Error;
use vesper_domain::{Revision, SessionId, TurnId};
use vesper_sessions::SessionStoreError;

/// Safe runtime failure independent of ACP or a concrete provider.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeError {
    /// Provider has not been registered.
    #[error("provider is not registered")]
    UnknownProvider,
    /// Provider identity was registered twice.
    #[error("provider is already registered")]
    DuplicateProvider,
    /// Ephemeral session is not present in this process.
    #[error("ephemeral session {0} was not found")]
    SessionNotFound(SessionId),
    /// A persisted record was unreadable or invalid.
    #[error("persistent session is corrupt")]
    PersistentSessionCorrupt,
    /// A persisted record uses an unsupported version.
    #[error("persistent session version is unsupported")]
    PersistentSessionUnsupportedVersion,
    /// A persisted record exceeded configured safety bounds.
    #[error("persistent session exceeds configured safety bounds")]
    PersistentSessionRejectedByBounds,
    /// Persistent session access was denied.
    #[error("persistent session access was denied")]
    PersistentSessionPermissionDenied,
    /// A persistent record path failed containment checks.
    #[error("persistent session path is unsafe")]
    PersistentSessionUnsafePath,
    /// A transactional session write failed.
    #[error("persistent session write failed")]
    PersistentSessionWriteFailed,
    /// Caller workspace does not match the persisted primary root.
    #[error("persistent session belongs to a different workspace")]
    PersistentSessionWorkspaceMismatch,
    /// Requested session identity already exists.
    #[error("session {0} already exists")]
    DuplicateSession(SessionId),
    /// Session no longer accepts commands.
    #[error("session {0} is closed")]
    SessionClosed(SessionId),
    /// Another provider turn is active.
    #[error("session already has an active turn")]
    TurnAlreadyActive,
    /// Restored compatibility configuration is inspectable but unavailable.
    #[error("session provider configuration must be selected before a new turn")]
    ConfigurationRequired,
    /// Requested cancellation does not match the active turn.
    #[error("turn {0} is not active")]
    TurnNotActive(TurnId),
    /// Optimistic revision check failed.
    #[error("session revision mismatch: expected {expected:?}, actual {actual:?}")]
    RevisionConflict {
        /// Requested revision.
        expected: Revision,
        /// Current revision.
        actual: Revision,
    },
    /// Provider operation failed safely.
    #[error("provider operation failed")]
    Provider,
    /// Provider stream violated its terminal contract.
    #[error("provider stream contract failed")]
    ProviderStream,
    /// Usage arithmetic overflowed or was inconsistent.
    #[error("usage aggregation failed")]
    Usage,
    /// Runtime channel closed unexpectedly.
    #[error("runtime channel closed")]
    ChannelClosed,
    /// Command is owned by a later runtime stage.
    #[error("command is unsupported by the minimal runtime")]
    UnsupportedCommand,
    /// Runtime has begun shutdown.
    #[error("runtime is shutting down")]
    ShuttingDown,
}

impl RuntimeError {
    pub(crate) fn from_session_store(error: SessionStoreError) -> Self {
        match error {
            SessionStoreError::PathEscapesRoot | SessionStoreError::InvalidFileName(_) => {
                Self::PersistentSessionUnsafePath
            }
            SessionStoreError::Io(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                Self::PersistentSessionPermissionDenied
            }
            SessionStoreError::RecordLimitExceeded { .. } => {
                Self::PersistentSessionRejectedByBounds
            }
            _ => Self::PersistentSessionCorrupt,
        }
    }

    /// Maps a session-store write failure into the safest runtime classification.
    pub(crate) fn from_session_write(error: SessionStoreError) -> Self {
        match error {
            SessionStoreError::PathEscapesRoot | SessionStoreError::InvalidFileName(_) => {
                Self::PersistentSessionUnsafePath
            }
            SessionStoreError::Io(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                Self::PersistentSessionPermissionDenied
            }
            SessionStoreError::RecordLimitExceeded { .. } => {
                Self::PersistentSessionRejectedByBounds
            }
            // Serialization, blocking-gate, root-creation, and generic I/O
            // failures all surface as a bounded transactional write failure.
            _ => Self::PersistentSessionWriteFailed,
        }
    }
}
