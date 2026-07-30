use std::{future::Future, path::PathBuf, pin::Pin, time::SystemTime};

use vesper_domain::SessionId;

use crate::SessionStoreError;

/// Boxed future used by object-safe read-only repository ports.
pub type BoxSessionFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Origin of a session record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionSource {
    /// Logical composite; records always retain their selected leaf source.
    Composite,
    /// Current-process volatile state.
    InMemory,
    /// Agent Vesper's independent application data root.
    AgentVesper,
    /// Frozen Native GLM ACP compatibility state.
    LegacyNativeGlm {
        /// Named profile, or `None` for the default profile.
        profile: Option<String>,
    },
}

/// Availability of an operation at this repository stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCapability {
    Supported,
    Unsupported,
}

/// Operations that remain unavailable in Stage 5 Part 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedSessionOperation {
    Write,
    Delete,
    Migrate,
    PersistentSearch,
}

/// Explicit read-only repository capability declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRepositoryCapabilities {
    pub list: SessionCapability,
    pub load: SessionCapability,
    pub resume: SessionCapability,
    /// Supplies the raw record required by a future replay engine.
    pub replay: SessionCapability,
    pub write: SessionCapability,
    pub delete: SessionCapability,
    pub migrate: SessionCapability,
    pub persistent_search: SessionCapability,
}

impl SessionRepositoryCapabilities {
    /// Capabilities of every Stage 5 Part 1 repository.
    #[must_use]
    pub const fn read_only() -> Self {
        Self {
            list: SessionCapability::Supported,
            load: SessionCapability::Supported,
            resume: SessionCapability::Supported,
            replay: SessionCapability::Supported,
            write: SessionCapability::Unsupported,
            delete: SessionCapability::Unsupported,
            migrate: SessionCapability::Unsupported,
            persistent_search: SessionCapability::Unsupported,
        }
    }

    /// Capabilities of a Stage 6 transactional Agent Vesper writer.
    ///
    /// Read, write, and replay remain available; delete, migrate, and search
    /// stay unavailable until their owning stages.
    #[must_use]
    pub const fn read_write() -> Self {
        Self {
            list: SessionCapability::Supported,
            load: SessionCapability::Supported,
            resume: SessionCapability::Supported,
            replay: SessionCapability::Supported,
            write: SessionCapability::Supported,
            delete: SessionCapability::Unsupported,
            migrate: SessionCapability::Unsupported,
            persistent_search: SessionCapability::Unsupported,
        }
    }

    /// Returns a typed failure for a deliberately unsupported mutation.
    pub const fn reject(operation: UnsupportedSessionOperation) -> Result<(), SessionStoreError> {
        Err(SessionStoreError::UnsupportedOperation(operation))
    }
}

/// Why a caller requests the raw record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionReadIntent {
    Load,
    Resume,
    /// Read for a future replay engine; this crate does not execute replay.
    Replay,
}

/// Metadata available without decoding session JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMetadata {
    pub session_id: SessionId,
    pub source: SessionSource,
    pub byte_len: u64,
    pub modified: Option<SystemTime>,
    /// Resolved record path for filesystem sources; absent for memory sources.
    pub record_path: Option<PathBuf>,
    /// Sidecar path used for listing, when any.
    pub metadata_path: Option<PathBuf>,
    pub origin: MetadataOrigin,
    pub title: Option<String>,
    pub cwd: String,
    /// Source-compatible timestamp string.
    pub updated_at: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub parent_session_id: Option<String>,
    pub branch_root_id: Option<String>,
    /// Always absent unless a future explicitly safe preview format is defined.
    pub safe_preview: Option<String>,
    pub read_only: bool,
}

/// How listing metadata was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataOrigin {
    InMemory,
    Sidecar,
    JsonFallback,
    FilesystemEntry,
}

/// Exact optional listing filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionListFilter {
    pub cwd: Option<String>,
}

/// Raw, bounded session record. Decoding belongs to the next persistence part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub metadata: SessionMetadata,
    pub bytes: Vec<u8>,
}

/// Read a raw record for a declared compatibility intent.
pub trait SessionReader: Send + Sync {
    fn source(&self) -> SessionSource;

    /// Advertises the deliberately read-only repository surface.
    fn capabilities(&self) -> SessionRepositoryCapabilities {
        SessionRepositoryCapabilities::read_only()
    }

    fn read<'a>(
        &'a self,
        session_id: &'a SessionId,
        intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>>;

    fn load<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        self.read(session_id, SessionReadIntent::Load)
    }

    fn resume<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        self.read(session_id, SessionReadIntent::Resume)
    }

    fn replay_record<'a>(
        &'a self,
        session_id: &'a SessionId,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        self.read(session_id, SessionReadIntent::Replay)
    }
}

/// List bounded metadata without reading record bodies.
pub trait SessionLister: Send + Sync {
    fn list_filtered(
        &self,
        filter: SessionListFilter,
    ) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>>;

    fn list(&self) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>> {
        self.list_filtered(SessionListFilter::default())
    }
}

/// Cohesive object-safe port for a readable session source.
pub trait SessionRepository: SessionReader + SessionLister {}

impl<T> SessionRepository for T where T: SessionReader + SessionLister + ?Sized {}
