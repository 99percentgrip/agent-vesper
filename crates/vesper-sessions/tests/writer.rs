//! Stage 6 transactional writer integration tests: atomicity, crash-resilient
//! temp cleanup, sidecar generation, per-session write isolation, bounds, and
//! full write -> list -> load round-trip.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serde_json::Value;
use vesper_domain::{
    BoundedString, EndpointId, ExtensionMap, ExtensionNamespace, ModelId, NormalizedUsage,
    ProviderId, QualifiedModelId, Revision, SchemaVersion, SessionId, SessionLineage,
    SessionOperatingMode, SessionPermissionMode, UsageMode, VersionedExtensionEnvelope,
    WorkspaceRoot,
};
use vesper_sessions::{
    CompatibilityAvailability, DiscoveryBounds, FilesystemSessionStore, MetadataOrigin,
    PersistedProviderConfiguration, SessionCapability, SessionLister, SessionSource,
    SessionStoreError, SessionWriter, VesperDecodeBounds, VesperSessionDecoder, VesperSessionV1,
    VesperSessionWriter, WriteBounds,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "agent-vesper-writer-{label}-{}-{sequence}",
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

fn envelope(namespace: &str) -> VersionedExtensionEnvelope {
    VersionedExtensionEnvelope {
        namespace: ExtensionNamespace::new(namespace).unwrap(),
        version: SchemaVersion::new(1).unwrap(),
        values: ExtensionMap::default(),
    }
}

fn record(id: &str, revision: u64, parent: Option<&str>) -> VesperSessionV1 {
    let provider = ProviderId::new("zai").unwrap();
    let session_id = SessionId::new(id).unwrap();
    let lineage = SessionLineage {
        root_session_id: SessionId::new(parent.unwrap_or(id)).unwrap(),
        parent_session_id: parent.map(SessionId::new).transpose().unwrap(),
    };
    VesperSessionV1 {
        format: BoundedString::new(VesperSessionV1::format_name()).unwrap(),
        version: VesperSessionV1::current_version(),
        session_id: session_id.clone(),
        title: Some(BoundedString::new("Stage 6 round trip").unwrap()),
        updated_at: Some(BoundedString::new("00000000000000000009").unwrap()),
        lineage,
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
        provider_configuration: PersistedProviderConfiguration {
            provider_id: provider,
            values: envelope("provider.zai"),
        },
        operating_mode: SessionOperatingMode::Code,
        permission_mode: SessionPermissionMode::Ask,
        history: Vec::new(),
        cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
        revision: Revision::new(revision),
        plan: Vec::new(),
        extensions: envelope("compat.agent-vesper"),
    }
}

fn writer(root: PathBuf) -> VesperSessionWriter {
    VesperSessionWriter::new(root, SessionSource::AgentVesper, WriteBounds::default()).unwrap()
}

fn reader(root: PathBuf) -> FilesystemSessionStore {
    FilesystemSessionStore::new(root, SessionSource::AgentVesper, DiscoveryBounds::default())
        .unwrap()
}

fn decoder() -> VesperSessionDecoder {
    let provider = ProviderId::new("zai").unwrap();
    let model = QualifiedModelId {
        provider_id: provider.clone(),
        model_id: ModelId::new("glm-5.2").unwrap(),
    };
    VesperSessionDecoder::new(
        VesperDecodeBounds::default(),
        CompatibilityAvailability::default()
            .with_provider(provider.clone())
            .with_model(model)
            .with_endpoint(provider, EndpointId::new("zai-coding").unwrap()),
    )
}

#[test]
fn writer_advertises_supported_write_capability() {
    // Use the platform temp dir so the root is absolute on every OS:
    // `/tmp/vesper-cap` is not absolute on Windows (no drive letter) and
    // would surface as RootNotAbsolute from VesperSessionWriter::new.
    let capabilities = writer(std::env::temp_dir().join("vesper-cap")).capabilities();
    assert_eq!(capabilities.write, SessionCapability::Supported);
    assert_eq!(capabilities.list, SessionCapability::Supported);
    assert_eq!(capabilities.delete, SessionCapability::Unsupported);
}

#[tokio::test]
async fn atomic_write_produces_session_and_sidecar_files() {
    let temp = TempDirectory::new("atomic");
    let root = temp.path().join("sessions");
    let writer = writer(root.clone());

    let outcome = writer.store(&record("alpha", 1, None)).await.unwrap();
    assert!(outcome.session_bytes > 0);
    assert!(outcome.sidecar_bytes > 0);

    let session = std::fs::read(root.join("alpha.json")).unwrap();
    let value: Value = serde_json::from_slice(&session).unwrap();
    assert_eq!(value["format"], "agent-vesper-session");
    assert_eq!(value["version"], 1);
    assert_eq!(value["session_id"], "alpha");
    assert_eq!(value["revision"], 1);

    let sidecar = std::fs::read(root.join("alpha.meta")).unwrap();
    let sidecar: Value = serde_json::from_slice(&sidecar).unwrap();
    assert_eq!(sidecar["session_id"], "alpha");
    assert_eq!(sidecar["schema_version"], 1);
    assert_eq!(sidecar["branch_root_id"], "alpha");

    // No temp files linger after a clean write.
    let leftovers = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".tmp"))
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[tokio::test]
async fn written_session_round_trips_through_list_and_decode() {
    let temp = TempDirectory::new("roundtrip");
    let root = temp.path().join("sessions");
    let writer = writer(root.clone());

    writer
        .store(&record("forked", 7, Some("root")))
        .await
        .unwrap();

    // Listing uses the sidecar without parsing the record body.
    let store = reader(root.clone());
    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id.as_str(), "forked");
    assert_eq!(listed[0].origin, MetadataOrigin::Sidecar);
    assert_eq!(listed[0].cwd, "/fixture");
    assert_eq!(listed[0].title.as_deref(), Some("Stage 6 round trip"));
    assert_eq!(listed[0].parent_session_id.as_deref(), Some("root"));
    assert_eq!(listed[0].branch_root_id.as_deref(), Some("root"));
    assert_eq!(listed[0].model.as_deref(), Some("glm-5.2"));
    assert_eq!(listed[0].provider.as_deref(), Some("zai"));

    // Decoding reproduces the persisted state including lineage and revision.
    let loaded = decoder()
        .load(&store, &SessionId::new("forked").unwrap())
        .await;
    let vesper_sessions::VesperLoadOutcome::Loaded(state) = loaded else {
        panic!("expected loaded outcome, got {loaded:?}")
    };
    assert_eq!(state.session_id.as_str(), "forked");
    assert_eq!(state.revision.get(), 7);
    assert_eq!(state.lineage.parent_session_id.unwrap().as_str(), "root");
}

#[tokio::test]
async fn orphaned_temp_files_from_a_crash_are_swept_before_write() {
    let temp = TempDirectory::new("crash");
    let root = temp.path().join("sessions");
    fs::create_dir_all(&root).unwrap();

    // Simulate a crash that left orphan temp files for this stem.
    fs::write(root.join(".alpha.json.dead-pid.tmp"), b"partial").unwrap();
    fs::write(root.join(".alpha.meta.dead-pid.tmp"), b"partial-meta").unwrap();

    let writer = writer(root.clone());
    writer.store(&record("alpha", 2, None)).await.unwrap();

    let names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    assert!(
        !names.iter().any(|name| name.ends_with(".tmp")),
        "orphan temp files remained: {names:?}"
    );
    // The authoritative record replaced any partial state.
    let body: Value =
        serde_json::from_slice(&std::fs::read(root.join("alpha.json")).unwrap()).unwrap();
    assert_eq!(body["revision"], 2);
}

#[tokio::test]
async fn revision_updates_are_overwritten_atomically() {
    let temp = TempDirectory::new("revision");
    let root = temp.path().join("sessions");
    let writer = writer(root.clone());

    writer.store(&record("alpha", 1, None)).await.unwrap();
    writer.store(&record("alpha", 2, None)).await.unwrap();
    writer.store(&record("alpha", 3, None)).await.unwrap();

    let body: Value =
        serde_json::from_slice(&std::fs::read(root.join("alpha.json")).unwrap()).unwrap();
    assert_eq!(body["revision"], 3);
}

#[tokio::test]
async fn concurrent_writes_to_the_same_id_never_corrupt() {
    let temp = TempDirectory::new("concurrent-same");
    let root = temp.path().join("sessions");
    let writer = Arc::new(writer(root.clone()));

    let mut handles = Vec::new();
    for revision in 1..=16 {
        let writer = Arc::clone(&writer);
        handles.push(tokio::spawn(async move {
            writer.store(&record("shared", revision, None)).await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // The file must always be valid JSON and a well-formed session record.
    let store = reader(root.clone());
    let loaded = decoder()
        .load(&store, &SessionId::new("shared").unwrap())
        .await;
    assert!(
        matches!(loaded, vesper_sessions::VesperLoadOutcome::Loaded(_)),
        "expected a valid loaded record, got {loaded:?}"
    );
}

#[tokio::test]
async fn writes_to_distinct_ids_run_concurrently() {
    let temp = TempDirectory::new("concurrent-distinct");
    let root = temp.path().join("sessions");
    let writer = Arc::new(writer(root.clone()));

    let mut handles = Vec::new();
    for index in 0..8 {
        let writer = Arc::clone(&writer);
        handles.push(tokio::spawn(async move {
            writer
                .store(&record(&format!("session-{index}"), 1, None))
                .await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let store = reader(root.clone());
    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 8);
}

#[tokio::test]
async fn oversized_record_is_rejected_before_any_write() {
    let temp = TempDirectory::new("oversize");
    let root = temp.path().join("sessions");
    let writer = VesperSessionWriter::new(
        root.clone(),
        SessionSource::AgentVesper,
        WriteBounds {
            max_session_bytes: 8,
            ..WriteBounds::default()
        },
    )
    .unwrap();

    let result = writer.store(&record("alpha", 1, None)).await;
    assert!(matches!(
        result,
        Err(SessionStoreError::RecordLimitExceeded { .. })
    ));
    // No file and no root directory are created for a rejected write.
    assert!(!root.exists());
}

#[tokio::test]
async fn writer_creates_the_root_directory_on_first_write_only() {
    let temp = TempDirectory::new("mkdir");
    // The writer creates only its configured root (one level); it never invents
    // arbitrary nested parent paths, so the root's parent must already exist.
    let root = temp.path().join("sessions");
    let writer = writer(root.clone());

    assert!(!root.exists());
    writer.store(&record("alpha", 1, None)).await.unwrap();
    assert!(root.is_dir());

    // A second write must not error when the directory already exists.
    writer.store(&record("alpha", 2, None)).await.unwrap();
    assert!(root.join("alpha.json").is_file());
}

#[tokio::test]
async fn requested_id_is_sanitized_and_never_used_as_a_path() {
    let temp = TempDirectory::new("sanitize");
    let root = temp.path().join("sessions");
    let writer = writer(root.clone());

    writer
        .store(&record("../../escape", 1, None))
        .await
        .unwrap();

    // The traversal characters are replaced with underscores by the filename
    // policy, and the file lands inside the root.
    assert!(root.join("______escape.json").is_file());
    assert!(std::fs::read_dir(temp.path()).unwrap().count() <= 2);
}
