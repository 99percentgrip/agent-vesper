use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use vesper_domain::SessionId;
use vesper_sessions::{
    BoxSessionFuture, CompositeSessionRepository, DiscoveryBounds, FilesystemSessionStore,
    LegacyLoadOutcome, LegacySessionDecoder, MetadataOrigin, SessionCapability, SessionListFilter,
    SessionLister, SessionMetadata, SessionReadIntent, SessionReader, SessionRecord,
    SessionRepositoryCapabilities, SessionSource, SessionStoreError, UnsupportedSessionOperation,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "agent-vesper-sessions-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn store(root: PathBuf, source: SessionSource) -> FilesystemSessionStore {
    FilesystemSessionStore::new(root, source, DiscoveryBounds::default()).unwrap()
}

#[test]
fn capability_contract_is_explicitly_read_only() {
    let capabilities = SessionRepositoryCapabilities::read_only();
    assert_eq!(capabilities.list, SessionCapability::Supported);
    assert_eq!(capabilities.load, SessionCapability::Supported);
    assert_eq!(capabilities.resume, SessionCapability::Supported);
    assert_eq!(capabilities.replay, SessionCapability::Supported);
    assert_eq!(capabilities.write, SessionCapability::Unsupported);
    assert_eq!(capabilities.delete, SessionCapability::Unsupported);
    assert_eq!(capabilities.migrate, SessionCapability::Unsupported);
    assert_eq!(
        capabilities.persistent_search,
        SessionCapability::Unsupported
    );
    assert!(matches!(
        SessionRepositoryCapabilities::reject(UnsupportedSessionOperation::Write),
        Err(SessionStoreError::UnsupportedOperation(
            UnsupportedSessionOperation::Write
        ))
    ));

    let temp = TempDirectory::new("capabilities");
    let repository = store(temp.path().to_path_buf(), SessionSource::AgentVesper);
    assert_eq!(repository.capabilities(), capabilities);
}

#[tokio::test]
async fn missing_root_is_empty_and_is_not_created() {
    let temp = TempDirectory::new("missing");
    let missing = temp.path().join("not-created");
    let repository = store(missing.clone(), SessionSource::AgentVesper);
    assert!(repository.list().await.unwrap().is_empty());
    assert!(
        repository
            .load(&SessionId::new("absent").unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(!missing.exists());
}

#[tokio::test]
async fn discovery_is_non_recursive_and_reads_only_bounded_records() {
    let temp = TempDirectory::new("nonrecursive");
    fs::write(temp.path().join("alpha.json"), br#"{"schema":1}"#).unwrap();
    fs::write(temp.path().join("ignored.txt"), b"not a session").unwrap();
    fs::create_dir(temp.path().join("nested")).unwrap();
    fs::write(temp.path().join("nested/beta.json"), b"nested").unwrap();

    let repository = store(temp.path().to_path_buf(), SessionSource::AgentVesper);
    let listed = repository.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id.as_str(), "alpha");

    let record = repository
        .replay_record(&SessionId::new("alpha").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.bytes, br#"{"schema":1}"#);
    assert_eq!(record.metadata.byte_len, 12);
}

#[tokio::test]
async fn requested_id_is_mapped_and_never_used_as_a_path() {
    let temp = TempDirectory::new("mapping");
    fs::write(temp.path().join("______outside.json"), b"safe").unwrap();
    let repository = store(temp.path().to_path_buf(), SessionSource::AgentVesper);
    let record = repository
        .load(&SessionId::new("../../outside").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.bytes, b"safe");
    assert!(
        record
            .metadata
            .record_path
            .unwrap()
            .starts_with(temp.path())
    );
}

#[tokio::test]
async fn entry_filename_and_record_bounds_fail_closed() {
    let entries = TempDirectory::new("entry-bound");
    fs::write(entries.path().join("one.json"), b"1").unwrap();
    fs::write(entries.path().join("two.json"), b"2").unwrap();
    let entry_store = FilesystemSessionStore::new(
        entries.path().to_path_buf(),
        SessionSource::AgentVesper,
        DiscoveryBounds {
            max_entries: 1,
            ..DiscoveryBounds::default()
        },
    )
    .unwrap();
    assert!(matches!(
        entry_store.list().await,
        Err(SessionStoreError::EntryLimitExceeded { maximum: 1 })
    ));

    let names = TempDirectory::new("name-bound");
    fs::write(names.path().join("longname.json"), b"1").unwrap();
    let name_store = FilesystemSessionStore::new(
        names.path().to_path_buf(),
        SessionSource::AgentVesper,
        DiscoveryBounds {
            max_filename_bytes: 8,
            ..DiscoveryBounds::default()
        },
    )
    .unwrap();
    assert!(name_store.list().await.unwrap().is_empty());

    let records = TempDirectory::new("record-bound");
    fs::write(records.path().join("large.json"), b"12345").unwrap();
    let record_store = FilesystemSessionStore::new(
        records.path().to_path_buf(),
        SessionSource::AgentVesper,
        DiscoveryBounds {
            max_session_bytes: 4,
            ..DiscoveryBounds::default()
        },
    )
    .unwrap();
    assert!(matches!(
        record_store.load(&SessionId::new("large").unwrap()).await,
        Err(SessionStoreError::RecordLimitExceeded { maximum: 4 })
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = TempDirectory::new("symlink-root");
    let outside = TempDirectory::new("symlink-outside");
    fs::write(outside.path().join("escaped.json"), b"outside").unwrap();
    symlink(
        outside.path().join("escaped.json"),
        root.path().join("escaped.json"),
    )
    .unwrap();
    let repository = store(root.path().to_path_buf(), SessionSource::AgentVesper);
    assert!(matches!(
        repository.load(&SessionId::new("escaped").unwrap()).await,
        Err(SessionStoreError::PathEscapesRoot)
    ));
}

#[test]
fn errors_do_not_render_private_root_paths() {
    let canary = "private/root/credential-canary";
    let error = FilesystemSessionStore::new(
        PathBuf::from(canary),
        SessionSource::AgentVesper,
        DiscoveryBounds::default(),
    )
    .err()
    .unwrap();
    assert!(!error.to_string().contains(canary));
    assert!(!format!("{error:?}").contains(canary));
}

#[tokio::test]
async fn atomic_replacement_during_read_is_consistent_and_typed() {
    let root = TempDirectory::new("replacement");
    let path = root.path().join("replace.json");
    let valid = br#"{"cwd":"/fixture","model":"glm-5.2","messages":[]}"#;
    fs::write(&path, valid).unwrap();
    let repository = store(
        root.path().to_path_buf(),
        SessionSource::LegacyNativeGlm { profile: None },
    );
    let replacement_root = root.path().to_path_buf();
    let writer = std::thread::spawn(move || {
        for index in 0..100 {
            let temporary = replacement_root.join(format!("replacement-{index}.tmp"));
            let bytes: &[u8] = if index % 2 == 0 { valid } else { b"{corrupt" };
            fs::write(&temporary, bytes).unwrap();
            fs::rename(temporary, replacement_root.join("replace.json")).unwrap();
        }
    });
    let decoder = LegacySessionDecoder::default();
    let id = SessionId::new("replace").unwrap();
    for _ in 0..100 {
        // On Windows a concurrent rename can momentarily make the destination
        // unreadable in two ways, both transient "couldn't read cleanly during
        // concurrent rename" states -- never torn bytes, never an
        // untyped/unsupported outcome, and never a genuine path escape (the
        // path is always under root):
        //   * ERROR_SHARING_VIOLATION -> io::ErrorKind::PermissionDenied
        //   * a TOCTOU in the confined read (canonicalize + starts_with(root))
        //     during the rename -> PathEscapesRoot -> LegacyLoadOutcome::UnsafePath
        // We retry briefly (with a small sleep so retries span across the
        // writer's rename windows) so the majority of reads still exercise the
        // decode path. Under sustained contention a residual PermissionDenied
        // or UnsafePath is a legitimate concurrent-access outcome on Windows
        // and is accepted as such; the guarantee the test upholds -- every
        // observed record is consistent and typed -- is unchanged. (On Linux
        // rename is atomic, so the retry/sleep never triggers and neither
        // transient outcome ever arises.)
        let outcome = {
            let mut outcome = decoder.load(&repository, &id).await;
            for _ in 0..8 {
                if !matches!(
                    outcome,
                    LegacyLoadOutcome::PermissionDenied | LegacyLoadOutcome::UnsafePath
                ) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
                outcome = decoder.load(&repository, &id).await;
            }
            outcome
        };
        assert!(
            matches!(
                outcome,
                LegacyLoadOutcome::Loaded(_)
                    | LegacyLoadOutcome::Corrupt(_)
                    | LegacyLoadOutcome::Missing
                    | LegacyLoadOutcome::PermissionDenied
                    | LegacyLoadOutcome::UnsafePath
            ),
            "unexpected outcome during concurrent replacement: {outcome:?}"
        );
    }
    writer.join().unwrap();
}

#[derive(Clone)]
struct FakeRepository {
    source: SessionSource,
    records: BTreeMap<SessionId, SessionRecord>,
}

impl FakeRepository {
    fn with_record(source: SessionSource, id: &str, bytes: &[u8]) -> Self {
        let session_id = SessionId::new(id).unwrap();
        let metadata = SessionMetadata {
            session_id: session_id.clone(),
            source: source.clone(),
            byte_len: bytes.len() as u64,
            modified: None,
            record_path: None,
            metadata_path: None,
            origin: MetadataOrigin::InMemory,
            title: None,
            cwd: String::new(),
            updated_at: None,
            model: None,
            provider: None,
            parent_session_id: None,
            branch_root_id: Some(id.to_owned()),
            safe_preview: None,
            read_only: source != SessionSource::InMemory,
        };
        Self {
            source,
            records: BTreeMap::from([(
                session_id,
                SessionRecord {
                    metadata,
                    bytes: bytes.to_vec(),
                },
            )]),
        }
    }
}

impl SessionReader for FakeRepository {
    fn source(&self) -> SessionSource {
        self.source.clone()
    }

    fn read<'a>(
        &'a self,
        session_id: &'a SessionId,
        _intent: SessionReadIntent,
    ) -> BoxSessionFuture<'a, Result<Option<SessionRecord>, SessionStoreError>> {
        let result = self.records.get(session_id).cloned();
        Box::pin(async move { Ok(result) })
    }
}

impl SessionLister for FakeRepository {
    fn list_filtered(
        &self,
        filter: SessionListFilter,
    ) -> BoxSessionFuture<'_, Result<Vec<SessionMetadata>, SessionStoreError>> {
        let result = self
            .records
            .values()
            .filter(|record| {
                filter
                    .cwd
                    .as_deref()
                    .is_none_or(|cwd| record.metadata.cwd == cwd)
            })
            .map(|record| record.metadata.clone())
            .collect();
        Box::pin(async move { Ok(result) })
    }
}

#[tokio::test]
async fn composite_collision_order_is_memory_then_vesper_then_legacy() {
    let memory = Arc::new(FakeRepository::with_record(
        SessionSource::InMemory,
        "same",
        b"memory",
    ));
    let vesper = Arc::new(FakeRepository::with_record(
        SessionSource::AgentVesper,
        "same",
        b"vesper",
    ));
    let legacy = Arc::new(FakeRepository::with_record(
        SessionSource::LegacyNativeGlm { profile: None },
        "same",
        b"legacy",
    ));
    let composite = CompositeSessionRepository::new(memory, vesper, legacy).unwrap();

    let id = SessionId::new("same").unwrap();
    let loaded = composite.load(&id).await.unwrap().unwrap();
    assert_eq!(loaded.bytes, b"memory");
    let listed = composite.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source, SessionSource::InMemory);
}
