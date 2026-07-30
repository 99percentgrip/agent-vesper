use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use vesper_domain::{
    BoundedString, ExtensionMap, ExtensionNamespace, SchemaVersion, SessionId,
    VersionedExtensionEnvelope,
};
use vesper_sessions::{
    CompatibilityAvailability, LegacyDecodeBounds, LegacyLoadOutcome, LegacyRuntimeConverter,
    LegacySessionDecoder, PersistedProviderConfiguration, PersistedSessionState, SessionReadIntent,
    SessionRepository, SessionSource, SessionWriter, VesperDecodeBounds, VesperLoadOutcome,
    VesperSessionDecoder, VesperSessionV1, WriteOutcome,
};

use crate::{RuntimeError, SessionSnapshot};

/// Zero-padded unix-second timestamp string used for sortable `updated_at`.
/// Lexical ordering of equal-width decimal strings matches chronological
/// ordering, so the listing path needs no date parser.
fn sortable_timestamp() -> BoundedString<128> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    BoundedString::new(format!("{seconds:020}")).expect("20-digit seconds fit in 128 bytes")
}

/// Converts a runtime session snapshot into a version-1 Agent Vesper record
/// ready for transactional persistence.
///
/// The runtime is provider-neutral: it stores the provider's own configuration
/// envelope verbatim and never interprets provider values. Plan entries are not
/// yet retained across the snapshot boundary (Stage 6 limitation) and are
/// persisted as empty; the read path rebuilds replay from history.
fn snapshot_to_record(snapshot: &SessionSnapshot) -> Result<VesperSessionV1, RuntimeError> {
    let endpoint_id = snapshot
        .endpoint_id
        .clone()
        .ok_or(RuntimeError::PersistentSessionWriteFailed)?;
    let provider_configuration = PersistedProviderConfiguration {
        provider_id: snapshot.provider_configuration.provider_id.clone(),
        values: snapshot.provider_configuration.values.clone(),
    };
    let extensions = match &snapshot.compatibility {
        Some(vesper_sessions::SessionCompatibilityData::AgentVesper(envelope)) => envelope.clone(),
        _ => default_agent_vesper_envelope(),
    };
    Ok(VesperSessionV1 {
        format: BoundedString::new(VesperSessionV1::format_name())
            .expect("static format discriminator"),
        version: VesperSessionV1::current_version(),
        session_id: snapshot.session_id.clone(),
        title: None,
        updated_at: Some(sortable_timestamp()),
        lineage: snapshot.lineage.clone(),
        workspace_roots: snapshot.workspace_roots.clone(),
        provider_id: snapshot.provider_id.clone(),
        model: snapshot.model.clone(),
        endpoint_id,
        provider_configuration,
        operating_mode: snapshot.operating_mode,
        permission_mode: snapshot.permission_mode,
        history: snapshot.history.clone(),
        cumulative_usage: snapshot.cumulative_usage.clone(),
        revision: snapshot.revision,
        plan: Vec::new(),
        extensions,
    })
}

fn default_agent_vesper_envelope() -> VersionedExtensionEnvelope {
    VersionedExtensionEnvelope {
        namespace: ExtensionNamespace::new("compat.agent-vesper")
            .expect("static compatibility namespace"),
        version: SchemaVersion::new(1).expect("static schema version"),
        values: ExtensionMap::default(),
    }
}

/// Injected, read-only persistent session boundary.
pub struct RuntimeSessionReads {
    repository: Arc<dyn SessionRepository>,
    legacy_decoder: LegacySessionDecoder,
    legacy_converter: LegacyRuntimeConverter,
    vesper_decoder: VesperSessionDecoder,
}

impl std::fmt::Debug for RuntimeSessionReads {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeSessionReads")
            .finish_non_exhaustive()
    }
}

impl RuntimeSessionReads {
    /// Creates the read path. No directory or file is created.
    #[must_use]
    pub fn new(
        repository: Arc<dyn SessionRepository>,
        availability: CompatibilityAvailability,
        legacy_bounds: LegacyDecodeBounds,
        vesper_bounds: VesperDecodeBounds,
    ) -> Self {
        Self {
            repository,
            legacy_decoder: LegacySessionDecoder::new(legacy_bounds),
            legacy_converter: LegacyRuntimeConverter::new(availability.clone()),
            vesper_decoder: VesperSessionDecoder::new(vesper_bounds, availability),
        }
    }

    pub(crate) fn repository(&self) -> &dyn SessionRepository {
        self.repository.as_ref()
    }

    pub(crate) async fn load(
        &self,
        session_id: &SessionId,
        intent: SessionReadIntent,
    ) -> Result<Option<PersistedSessionState>, RuntimeError> {
        let record = self
            .repository
            .read(session_id, intent)
            .await
            .map_err(RuntimeError::from_session_store)?;
        let Some(record) = record else {
            return Ok(None);
        };
        match record.metadata.source {
            SessionSource::LegacyNativeGlm { .. } => {
                match self
                    .legacy_decoder
                    .decode_record(record.metadata, &record.bytes)
                {
                    LegacyLoadOutcome::Loaded(decoded) => self
                        .legacy_converter
                        .convert(*decoded)
                        .map(Some)
                        .map_err(|_| RuntimeError::PersistentSessionCorrupt),
                    LegacyLoadOutcome::Missing => Ok(None),
                    LegacyLoadOutcome::Corrupt(_) => Err(RuntimeError::PersistentSessionCorrupt),
                    LegacyLoadOutcome::UnsupportedVersion(_) => {
                        Err(RuntimeError::PersistentSessionUnsupportedVersion)
                    }
                    LegacyLoadOutcome::RejectedByBounds(_) => {
                        Err(RuntimeError::PersistentSessionRejectedByBounds)
                    }
                    LegacyLoadOutcome::PermissionDenied => {
                        Err(RuntimeError::PersistentSessionPermissionDenied)
                    }
                    LegacyLoadOutcome::UnsafePath => Err(RuntimeError::PersistentSessionUnsafePath),
                }
            }
            SessionSource::AgentVesper => {
                match self
                    .vesper_decoder
                    .decode_record(record.metadata, &record.bytes)
                {
                    VesperLoadOutcome::Loaded(state) => Ok(Some(*state)),
                    VesperLoadOutcome::Missing => Ok(None),
                    VesperLoadOutcome::Corrupt(_) => Err(RuntimeError::PersistentSessionCorrupt),
                    VesperLoadOutcome::UnsupportedVersion(_) => {
                        Err(RuntimeError::PersistentSessionUnsupportedVersion)
                    }
                    VesperLoadOutcome::RejectedByBounds(_) => {
                        Err(RuntimeError::PersistentSessionRejectedByBounds)
                    }
                    VesperLoadOutcome::PermissionDenied => {
                        Err(RuntimeError::PersistentSessionPermissionDenied)
                    }
                    VesperLoadOutcome::UnsafePath => Err(RuntimeError::PersistentSessionUnsafePath),
                }
            }
            SessionSource::InMemory | SessionSource::Composite => {
                Err(RuntimeError::PersistentSessionCorrupt)
            }
        }
    }
}

/// Injected, transactional persistent session write boundary.
pub struct RuntimeSessionWrites {
    writer: Arc<dyn SessionWriter>,
}

impl std::fmt::Debug for RuntimeSessionWrites {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeSessionWrites")
            .finish_non_exhaustive()
    }
}

impl RuntimeSessionWrites {
    /// Creates the write path. No directory or file is created.
    #[must_use]
    pub fn new(writer: Arc<dyn SessionWriter>) -> Self {
        Self { writer }
    }

    /// Atomically persists a session snapshot as a version-1 Agent Vesper record.
    pub async fn store_snapshot(
        &self,
        snapshot: &SessionSnapshot,
    ) -> Result<WriteOutcome, RuntimeError> {
        let record = snapshot_to_record(snapshot)?;
        self.writer
            .store(&record)
            .await
            .map_err(RuntimeError::from_session_write)
    }
}
