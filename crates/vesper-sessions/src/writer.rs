//! Transactional Agent Vesper session writer.
//!
//! The writer persists `VesperSessionV1` records and their derived `.meta`
//! sidecars using bounded write-to-temp, `fsync`, and atomic rename. Writes
//! are confined to the configured absolute root, isolated per session ID, and
//! bounded by configured byte limits. Orphaned temp files from a previous
//! crash are swept on the next write of the same session.
//!
//! Atomicity guarantee: each file is written to a sibling temp file inside the
//! canonical session directory, `fsync`ed, then renamed over its target. On
//! POSIX this is atomic because the temp file and the target share one
//! directory and therefore one filesystem. The session record is committed
//! before its sidecar; the sidecar is a derived cache, so a crash between the
//! two leaves a valid session that the reader regenerates metadata for from
//! the JSON body.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::sync::{Mutex, Semaphore};
use vesper_domain::SessionId;

use crate::{
    BoxSessionFuture, SessionFileName, SessionRepositoryCapabilities, SessionSource,
    SessionStoreError, VesperSessionV1,
};

/// Bounded limits for a transactional Agent Vesper writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteBounds {
    /// Maximum serialized session record size in bytes.
    pub max_session_bytes: usize,
    /// Maximum serialized sidecar size in bytes.
    pub max_sidecar_bytes: usize,
    /// Maximum concurrent blocking write tasks.
    pub max_blocking_operations: usize,
    /// Maximum orphaned temp files swept per write.
    pub max_orphan_temps: usize,
}

impl Default for WriteBounds {
    fn default() -> Self {
        Self {
            max_session_bytes: 16 * 1024 * 1024,
            max_sidecar_bytes: 64 * 1024,
            max_blocking_operations: 4,
            max_orphan_temps: 1_024,
        }
    }
}

impl WriteBounds {
    const fn validate(self) -> Result<Self, SessionStoreError> {
        if self.max_session_bytes == 0
            || self.max_sidecar_bytes == 0
            || self.max_blocking_operations == 0
            || self.max_orphan_temps == 0
        {
            return Err(SessionStoreError::InvalidBounds);
        }
        Ok(self)
    }
}

/// Accepted write result for one session record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOutcome {
    /// Persisted session record byte length.
    pub session_bytes: usize,
    /// Persisted sidecar byte length.
    pub sidecar_bytes: usize,
}

/// Object-safe port for transactional Agent Vesper session writes.
pub trait SessionWriter: Send + Sync {
    fn source(&self) -> SessionSource;

    /// Advertises the writer's repository capabilities.
    fn capabilities(&self) -> SessionRepositoryCapabilities {
        SessionRepositoryCapabilities::read_write()
    }

    /// Atomically persists a version-1 Agent Vesper session and its sidecar.
    fn store<'a>(
        &'a self,
        record: &'a VesperSessionV1,
    ) -> BoxSessionFuture<'a, Result<WriteOutcome, SessionStoreError>>;
}

/// Concrete transactional Agent Vesper filesystem writer.
#[derive(Clone)]
pub struct VesperSessionWriter {
    root: PathBuf,
    source: SessionSource,
    bounds: WriteBounds,
    blocking_gate: Arc<Semaphore>,
    write_locks: Arc<Mutex<BTreeMap<SessionId, Arc<Mutex<()>>>>>,
    temp_counter: Arc<AtomicU64>,
}

impl std::fmt::Debug for VesperSessionWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VesperSessionWriter")
            .field("root", &self.root)
            .field("source", &self.source)
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

impl VesperSessionWriter {
    /// Creates a writer for an absolute Agent Vesper session root.
    /// Construction performs no filesystem I/O.
    pub fn new(
        root: PathBuf,
        source: SessionSource,
        bounds: WriteBounds,
    ) -> Result<Self, SessionStoreError> {
        if !root.is_absolute() {
            return Err(SessionStoreError::RootNotAbsolute);
        }
        let bounds = bounds.validate()?;
        Ok(Self {
            root,
            source,
            bounds,
            blocking_gate: Arc::new(Semaphore::new(bounds.max_blocking_operations)),
            write_locks: Arc::new(Mutex::new(BTreeMap::new())),
            temp_counter: Arc::new(AtomicU64::new(0)),
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn bounds(&self) -> WriteBounds {
        self.bounds
    }

    async fn write_lock(&self, session_id: &SessionId) -> Arc<Mutex<()>> {
        let mut locks = self.write_locks.lock().await;
        Arc::clone(
            locks
                .entry(session_id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }
}

impl SessionWriter for VesperSessionWriter {
    fn source(&self) -> SessionSource {
        self.source.clone()
    }

    fn store<'a>(
        &'a self,
        record: &'a VesperSessionV1,
    ) -> BoxSessionFuture<'a, Result<WriteOutcome, SessionStoreError>> {
        let writer = self.clone();
        Box::pin(async move {
            // Validate the filename mapping before any I/O. The session record
            // owns its identity; the stored stem is derived from it.
            let stem = SessionFileName::from_requested_id(record.session_id.as_str())?
                .session_id_text()
                .to_owned();

            // Serialize and bound before acquiring locks so oversized or
            // unserializable records never block concurrent writers.
            let session_bytes =
                serde_json::to_vec(record).map_err(|_| SessionStoreError::SerializationFailed)?;
            if session_bytes.len() > writer.bounds.max_session_bytes {
                return Err(SessionStoreError::RecordLimitExceeded {
                    maximum: u64::try_from(writer.bounds.max_session_bytes).unwrap_or(u64::MAX),
                });
            }
            let sidecar = build_sidecar(record);
            let sidecar_bytes =
                serde_json::to_vec(&sidecar).map_err(|_| SessionStoreError::SerializationFailed)?;
            if sidecar_bytes.len() > writer.bounds.max_sidecar_bytes {
                return Err(SessionStoreError::RecordLimitExceeded {
                    maximum: u64::try_from(writer.bounds.max_sidecar_bytes).unwrap_or(u64::MAX),
                });
            }

            // Per-session mutual exclusion: distinct IDs stay concurrent.
            let lock = writer.write_lock(&record.session_id).await;
            let _guard = lock.lock().await;

            let permit = writer
                .blocking_gate
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| SessionStoreError::BlockingGateClosed)?;

            let root = writer.root.clone();
            let bounds = writer.bounds;
            let pid = std::process::id();
            let sequence = writer.temp_counter.fetch_add(1, Ordering::Relaxed);

            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                store_sync(
                    &root,
                    &stem,
                    &session_bytes,
                    &sidecar_bytes,
                    pid,
                    sequence,
                    bounds,
                )
            })
            .await
            .map_err(|_| SessionStoreError::BlockingTaskFailed)?
        })
    }
}

/// Blocking half of the transactional write, isolated from the reactor.
fn store_sync(
    root: &Path,
    stem: &str,
    session_bytes: &[u8],
    sidecar_bytes: &[u8],
    pid: u32,
    sequence: u64,
    bounds: WriteBounds,
) -> Result<WriteOutcome, SessionStoreError> {
    // The configured root is the only directory this writer may create, and it
    // is created non-recursively so arbitrary nested paths are never invented.
    match fs::create_dir(root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let canonical_root = fs::canonicalize(root)?;

    let session_target = canonical_root.join(format!("{stem}.json"));
    let sidecar_target = canonical_root.join(format!("{stem}.meta"));
    ensure_contained(&canonical_root, &session_target)?;
    ensure_contained(&canonical_root, &sidecar_target)?;

    // Remove orphaned temp files left by a previous crash for this stem.
    sweep_orphan_temps(&canonical_root, stem, bounds.max_orphan_temps)?;

    // Commit the authoritative session record first.
    let session_temp = format!(".{stem}.json.{pid}-{sequence}.tmp");
    write_atomic(
        &canonical_root,
        &session_target,
        &session_temp,
        session_bytes,
    )?;

    // Then commit the derived sidecar.
    let sidecar_temp = format!(".{stem}.meta.{pid}-{sequence}.tmp");
    write_atomic(
        &canonical_root,
        &sidecar_target,
        &sidecar_temp,
        sidecar_bytes,
    )?;

    Ok(WriteOutcome {
        session_bytes: session_bytes.len(),
        sidecar_bytes: sidecar_bytes.len(),
    })
}

/// Writes `bytes` to a sibling temp file, fsyncs, and atomically renames it
/// over `target`. The temp file lives inside `canonical_root` so the rename
/// never crosses a filesystem boundary on POSIX.
fn write_atomic(
    canonical_root: &Path,
    target: &Path,
    temp_name: &str,
    bytes: &[u8],
) -> Result<(), SessionStoreError> {
    let temp_path = canonical_root.join(temp_name);
    ensure_contained(canonical_root, &temp_path)?;

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(bytes)?;
        // Durability: flush page cache to disk before the atomic publish.
        file.sync_all()?;
    }
    fs::rename(&temp_path, target)?;
    Ok(())
}

/// Defensive containment check for files derived from sanitized names.
fn ensure_contained(root: &Path, path: &Path) -> Result<(), SessionStoreError> {
    if !path.starts_with(root) {
        return Err(SessionStoreError::PathEscapesRoot);
    }
    Ok(())
}

/// Removes orphaned `.tmp` files for `stem` produced by prior crashes. Bounded
/// by `max_temps` and best-effort: per-file errors never abort the write.
fn sweep_orphan_temps(
    canonical_root: &Path,
    stem: &str,
    max_temps: usize,
) -> Result<(), SessionStoreError> {
    let read_dir = match fs::read_dir(canonical_root) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let json_prefix = format!(".{stem}.json.");
    let meta_prefix = format!(".{stem}.meta.");
    let mut removed = 0_usize;
    for entry in read_dir {
        let Ok(entry) = entry else {
            continue;
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !(name.starts_with(&json_prefix) || name.starts_with(&meta_prefix))
            || !name.ends_with(".tmp")
        {
            continue;
        }
        let _ = fs::remove_file(entry.path());
        removed += 1;
        if removed >= max_temps {
            break;
        }
    }
    Ok(())
}

/// Builds the derived sidecar JSON from a session record. Fields mirror what
/// `metadata::decode_sidecar` reads so the listing path never needs to parse
/// the full record body.
fn build_sidecar(record: &VesperSessionV1) -> serde_json::Value {
    let cwd = record
        .workspace_roots
        .iter()
        .find(|root| root.primary)
        .map(|root| root.path.as_str().to_owned())
        .unwrap_or_default();
    serde_json::json!({
        "session_id": record.session_id.as_str(),
        "schema_version": record.version,
        "format": record.format.as_str(),
        "cwd": cwd,
        "title": record.title.as_ref().map(|title| title.as_str()),
        "updated_at": record.updated_at.as_ref().map(|stamp| stamp.as_str()),
        "parent_session_id": record.lineage.parent_session_id.as_ref().map(|id| id.as_str()),
        "branch_root_id": record.lineage.root_session_id.as_str(),
        "model": record.model.model_id.as_str(),
        "provider": record.provider_id.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::{
        BoundedString, EndpointId, ExtensionMap, ExtensionNamespace, ModelId, NormalizedUsage,
        ProviderId, QualifiedModelId, Revision, SchemaVersion, SessionId, SessionLineage,
        SessionOperatingMode, SessionPermissionMode, UsageMode, VersionedExtensionEnvelope,
        WorkspaceRoot,
    };

    fn record(id: &str, revision: u64) -> VesperSessionV1 {
        let provider = ProviderId::new("zai").unwrap();
        VesperSessionV1 {
            format: BoundedString::new(VesperSessionV1::format_name()).unwrap(),
            version: VesperSessionV1::current_version(),
            session_id: SessionId::new(id).unwrap(),
            title: Some(BoundedString::new("Stage 6 write").unwrap()),
            updated_at: Some(BoundedString::new("00000000000000000007").unwrap()),
            lineage: SessionLineage {
                root_session_id: SessionId::new(id).unwrap(),
                parent_session_id: None,
            },
            workspace_roots: vec![WorkspaceRoot {
                name: BoundedString::new("workspace").unwrap(),
                path: BoundedString::new("/fixture").unwrap(),
                primary: true,
            }],
            provider_id: provider.clone(),
            model: QualifiedModelId {
                provider_id: provider.clone(),
                model_id: ModelId::new("glm-5.2").unwrap(),
            },
            endpoint_id: EndpointId::new("zai-coding").unwrap(),
            provider_configuration: crate::PersistedProviderConfiguration {
                provider_id: provider,
                values: default_envelope(),
            },
            operating_mode: SessionOperatingMode::Code,
            permission_mode: SessionPermissionMode::Ask,
            history: Vec::new(),
            cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
            revision: Revision::new(revision),
            plan: Vec::new(),
            extensions: default_envelope(),
        }
    }

    fn default_envelope() -> VersionedExtensionEnvelope {
        VersionedExtensionEnvelope {
            namespace: ExtensionNamespace::new("compat.agent-vesper").unwrap(),
            version: SchemaVersion::new(1).unwrap(),
            values: ExtensionMap::default(),
        }
    }

    #[test]
    fn bounds_must_be_nonzero() {
        let tmp = std::env::temp_dir().join("vesper-writer-bounds-test");
        let bad = WriteBounds {
            max_session_bytes: 0,
            ..WriteBounds::default()
        };
        assert!(matches!(
            VesperSessionWriter::new(tmp, SessionSource::AgentVesper, bad),
            Err(SessionStoreError::InvalidBounds)
        ));
    }

    #[test]
    fn root_must_be_absolute() {
        assert!(matches!(
            VesperSessionWriter::new(
                PathBuf::from("relative/root"),
                SessionSource::AgentVesper,
                WriteBounds::default()
            ),
            Err(SessionStoreError::RootNotAbsolute)
        ));
    }

    #[test]
    fn build_sidecar_matches_decoder_shape() {
        let sidecar = build_sidecar(&record("alpha", 1));
        assert_eq!(sidecar["session_id"], "alpha");
        assert_eq!(sidecar["schema_version"], 1);
        assert_eq!(sidecar["cwd"], "/fixture");
        assert_eq!(sidecar["title"], "Stage 6 write");
        assert_eq!(sidecar["branch_root_id"], "alpha");
        assert_eq!(sidecar["model"], "glm-5.2");
        assert_eq!(sidecar["provider"], "zai");
        assert!(sidecar["parent_session_id"].is_null());
    }
}
