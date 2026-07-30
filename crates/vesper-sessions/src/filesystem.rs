use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use tokio::sync::Semaphore;
use vesper_domain::SessionId;

use crate::metadata::{MetadataContext, decode_json_fallback, decode_sidecar};
use crate::{
    BoxSessionFuture, MetadataOrigin, SessionFileName, SessionListFilter, SessionLister,
    SessionMetadata, SessionReadIntent, SessionReader, SessionRecord, SessionSource,
    SessionStoreError, sort_session_metadata,
};

/// Explicit limits for every filesystem operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryBounds {
    pub max_entries: usize,
    pub max_filename_bytes: usize,
    pub max_session_bytes: u64,
    pub max_sidecar_bytes: u64,
    pub max_blocking_operations: usize,
}

impl Default for DiscoveryBounds {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_filename_bytes: 255,
            max_session_bytes: 16 * 1024 * 1024,
            max_sidecar_bytes: 64 * 1024,
            max_blocking_operations: 4,
        }
    }
}

impl DiscoveryBounds {
    fn validate(self) -> Result<Self, SessionStoreError> {
        if self.max_entries == 0
            || self.max_filename_bytes == 0
            || self.max_session_bytes == 0
            || self.max_sidecar_bytes == 0
            || self.max_blocking_operations == 0
        {
            return Err(SessionStoreError::InvalidBounds);
        }
        Ok(self)
    }
}

/// Read-only bounded filesystem session source.
#[derive(Clone)]
pub struct FilesystemSessionStore {
    root: PathBuf,
    source: SessionSource,
    bounds: DiscoveryBounds,
    blocking_gate: Arc<Semaphore>,
}

impl FilesystemSessionStore {
    pub fn new(
        root: PathBuf,
        source: SessionSource,
        bounds: DiscoveryBounds,
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
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn list_sync(
        &self,
        filter: &SessionListFilter,
    ) -> Result<Vec<SessionMetadata>, SessionStoreError> {
        let root_metadata = match fs::metadata(&self.root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        if !root_metadata.is_dir() {
            return Err(SessionStoreError::NotRegularFile);
        }
        let canonical_root = fs::canonicalize(&self.root)?;
        let mut candidates = BTreeMap::<String, ListingCandidates>::new();
        for (index, entry) in fs::read_dir(&self.root)?.enumerate() {
            if index >= self.bounds.max_entries {
                return Err(SessionStoreError::EntryLimitExceeded {
                    maximum: self.bounds.max_entries,
                });
            }
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name();
            if name.as_encoded_bytes().len() > self.bounds.max_filename_bytes {
                continue;
            }
            let Some((stem, kind)) = stored_entry(&name) else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = confined_regular_metadata(&canonical_root, &path) else {
                continue;
            };
            let file = ListingFile {
                path,
                byte_len: metadata.len(),
                modified: metadata.modified().ok(),
            };
            let candidate = candidates.entry(stem).or_default();
            match kind {
                ListingKind::Json => candidate.json = Some(file),
                ListingKind::Sidecar => candidate.sidecar = Some(file),
            }
        }

        let mut results = Vec::new();
        for (stem, candidate) in candidates {
            let Ok(session_id) = SessionId::new(stem) else {
                continue;
            };
            let mut listed = candidate.sidecar.as_ref().and_then(|sidecar| {
                self.list_from_sidecar(&canonical_root, &session_id, sidecar, &candidate)
            });
            if listed.is_none() {
                listed = candidate.json.as_ref().and_then(|json| {
                    self.list_from_json(
                        &canonical_root,
                        &session_id,
                        json,
                        candidate.sidecar.as_ref(),
                    )
                });
            }
            let Some(metadata) = listed else {
                continue;
            };
            if filter.cwd.as_deref().is_some_and(|cwd| metadata.cwd != cwd) {
                continue;
            }
            results.push(metadata);
        }
        sort_session_metadata(&mut results);
        Ok(results)
    }

    fn list_from_sidecar(
        &self,
        canonical_root: &Path,
        session_id: &SessionId,
        sidecar: &ListingFile,
        candidate: &ListingCandidates,
    ) -> Option<SessionMetadata> {
        if sidecar.byte_len > self.bounds.max_sidecar_bytes {
            return None;
        }
        let bytes =
            read_confined_bounded(canonical_root, &sidecar.path, self.bounds.max_sidecar_bytes)
                .ok()?;
        let json = candidate.json.as_ref();
        decode_sidecar(
            &bytes,
            MetadataContext {
                session_id: session_id.clone(),
                source: self.source.clone(),
                byte_len: json.map_or(0, |file| file.byte_len),
                modified: sidecar.modified,
                record_path: json.map(|file| file.path.clone()),
                metadata_path: Some(sidecar.path.clone()),
            },
        )
    }

    fn list_from_json(
        &self,
        canonical_root: &Path,
        session_id: &SessionId,
        json: &ListingFile,
        sidecar: Option<&ListingFile>,
    ) -> Option<SessionMetadata> {
        if json.byte_len > self.bounds.max_session_bytes {
            return None;
        }
        let bytes =
            read_confined_bounded(canonical_root, &json.path, self.bounds.max_session_bytes)
                .ok()?;
        decode_json_fallback(
            &bytes,
            MetadataContext {
                session_id: session_id.clone(),
                source: self.source.clone(),
                byte_len: json.byte_len,
                modified: json.modified,
                record_path: Some(json.path.clone()),
                metadata_path: sidecar.map(|file| file.path.clone()),
            },
        )
    }

    fn read_sync(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionRecord>, SessionStoreError> {
        match fs::metadata(&self.root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(SessionStoreError::NotRegularFile),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let canonical_root = fs::canonicalize(&self.root)?;
        let file_name = SessionFileName::from_requested_id(session_id.as_str())?;
        let path = self.root.join(file_name.as_str());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Err(SessionStoreError::PathEscapesRoot);
        }
        let metadata = confined_regular_metadata(&canonical_root, &path)?;
        if metadata.len() > self.bounds.max_session_bytes {
            return Err(SessionStoreError::RecordLimitExceeded {
                maximum: self.bounds.max_session_bytes,
            });
        }
        let bytes = read_confined_bounded(&canonical_root, &path, self.bounds.max_session_bytes)?;
        Ok(Some(SessionRecord {
            metadata: SessionMetadata {
                session_id: session_id.clone(),
                source: self.source.clone(),
                byte_len: bytes.len() as u64,
                modified: metadata.modified().ok(),
                record_path: Some(path),
                metadata_path: None,
                origin: MetadataOrigin::FilesystemEntry,
                title: None,
                cwd: String::new(),
                updated_at: None,
                model: None,
                provider: None,
                parent_session_id: None,
                branch_root_id: None,
                safe_preview: None,
                read_only: self.source != SessionSource::InMemory,
            },
            bytes,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
enum ListingKind {
    Json,
    Sidecar,
}

#[derive(Debug, Default)]
struct ListingCandidates {
    json: Option<ListingFile>,
    sidecar: Option<ListingFile>,
}

#[derive(Debug)]
struct ListingFile {
    path: PathBuf,
    byte_len: u64,
    modified: Option<SystemTime>,
}

fn stored_entry(name: &OsStr) -> Option<(String, ListingKind)> {
    let name = name.to_str()?;
    let (stem, kind) = if let Some(stem) = name.strip_suffix(".json") {
        (stem, ListingKind::Json)
    } else if let Some(stem) = name.strip_suffix(".meta") {
        (stem, ListingKind::Sidecar)
    } else {
        return None;
    };
    let synthetic_json = format!("{stem}.json");
    let validated = SessionFileName::from_stored_name(OsStr::new(&synthetic_json)).ok()?;
    Some((validated.session_id_text().to_owned(), kind))
}

fn read_confined_bounded(
    canonical_root: &Path,
    path: &Path,
    maximum: u64,
) -> Result<Vec<u8>, SessionStoreError> {
    let metadata = confined_regular_metadata(canonical_root, path)?;
    if metadata.len() > maximum {
        return Err(SessionStoreError::RecordLimitExceeded { maximum });
    }
    let capacity =
        usize::try_from(metadata.len()).unwrap_or(usize::try_from(maximum).unwrap_or(usize::MAX));
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(SessionStoreError::RecordLimitExceeded { maximum });
    }
    Ok(bytes)
}

fn confined_regular_metadata(
    canonical_root: &Path,
    path: &Path,
) -> Result<fs::Metadata, SessionStoreError> {
    let link_metadata = fs::symlink_metadata(path)?;
    if link_metadata.file_type().is_symlink() {
        return Err(SessionStoreError::PathEscapesRoot);
    }
    let canonical_path = fs::canonicalize(path)?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(SessionStoreError::PathEscapesRoot);
    }
    let metadata = fs::metadata(canonical_path)?;
    if !metadata.is_file() {
        return Err(SessionStoreError::NotRegularFile);
    }
    Ok(metadata)
}

impl SessionReader for FilesystemSessionStore {
    fn source(&self) -> SessionSource {
        self.source.clone()
    }

    fn read<'a>(
        &'a self,
        session_id: &'a SessionId,
        _intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        let store = self.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let permit = store
                .blocking_gate
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| SessionStoreError::BlockingGateClosed)?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                store.read_sync(&session_id)
            })
            .await
            .map_err(|_| SessionStoreError::BlockingTaskFailed)?
        })
    }
}

impl SessionLister for FilesystemSessionStore {
    fn list_filtered(
        &self,
        filter: SessionListFilter,
    ) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>> {
        let store = self.clone();
        Box::pin(async move {
            let permit = store
                .blocking_gate
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| SessionStoreError::BlockingGateClosed)?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                store.list_sync(&filter)
            })
            .await
            .map_err(|_| SessionStoreError::BlockingTaskFailed)?
        })
    }
}
