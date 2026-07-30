use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::json;
use vesper_sessions::{
    DiscoveryBounds, FilesystemSessionStore, MetadataOrigin, SessionListFilter, SessionLister,
    SessionSource,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "agent-vesper-metadata-{label}-{}-{sequence}",
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

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn store(root: &Path) -> FilesystemSessionStore {
    FilesystemSessionStore::new(
        root.to_path_buf(),
        SessionSource::LegacyNativeGlm { profile: None },
        DiscoveryBounds::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn valid_sidecar_wins_without_reading_corrupt_history() {
    let temp = TempDirectory::new("sidecar");
    fs::write(temp.path().join("indexed.json"), b"corrupt history").unwrap();
    write_json(
        &temp.path().join("indexed.meta"),
        &json!({
            "session_id": "indexed",
            "cwd": "/workspace",
            "title": "Indexed",
            "updated_at": "2026-07-30T12:00:00+00:00",
            "parent_session_id": null,
            "branch_root_id": "indexed"
        }),
    );

    let listed = store(temp.path()).list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].origin, MetadataOrigin::Sidecar);
    assert_eq!(listed[0].title.as_deref(), Some("Indexed"));
    assert_eq!(listed[0].cwd, "/workspace");
    assert!(listed[0].read_only);
    assert!(listed[0].safe_preview.is_none());
}

#[tokio::test]
async fn corrupt_or_mismatched_sidecar_falls_back_to_bounded_json() {
    let temp = TempDirectory::new("fallback");
    write_json(
        &temp.path().join("fallback.json"),
        &json!({
            "version": 1,
            "cwd": "/workspace",
            "title": "From JSON",
            "saved_at": "2026-07-30T11:00:00+00:00",
            "model": "glm-5.2",
            "parent_session_id": "parent",
            "messages": [{
                "role": "assistant",
                "content": "private body",
                "reasoning_content": "private reasoning",
                "tool_calls": [{"function": {"arguments": "{\"secret\":true}"}}]
            }]
        }),
    );
    fs::write(temp.path().join("fallback.meta"), b"{broken").unwrap();

    write_json(
        &temp.path().join("mismatch.json"),
        &json!({"cwd": "/workspace", "title": "Mismatch fallback"}),
    );
    write_json(
        &temp.path().join("mismatch.meta"),
        &json!({"session_id": "different", "cwd": "/wrong"}),
    );

    let listed = store(temp.path()).list().await.unwrap();
    assert_eq!(listed.len(), 2);
    let fallback = listed
        .iter()
        .find(|entry| entry.session_id.as_str() == "fallback")
        .unwrap();
    assert_eq!(fallback.origin, MetadataOrigin::JsonFallback);
    assert_eq!(fallback.model.as_deref(), Some("glm-5.2"));
    assert_eq!(fallback.parent_session_id.as_deref(), Some("parent"));
    assert_eq!(fallback.branch_root_id.as_deref(), Some("fallback"));
    assert!(fallback.safe_preview.is_none());
    let mismatch = listed
        .iter()
        .find(|entry| entry.session_id.as_str() == "mismatch")
        .unwrap();
    assert_eq!(mismatch.title.as_deref(), Some("Mismatch fallback"));
}

#[tokio::test]
async fn unusable_entries_are_skipped_without_failing_complete_listing() {
    let temp = TempDirectory::new("fail-soft");
    fs::write(temp.path().join("broken.json"), b"not json").unwrap();
    fs::write(temp.path().join("broken.meta"), b"also not json").unwrap();
    write_json(
        &temp.path().join("good.json"),
        &json!({"cwd": "/workspace", "title": "Good"}),
    );
    write_json(
        &temp.path().join("wrong-type.meta"),
        &json!({"session_id": 7, "cwd": "/workspace"}),
    );

    let listed = store(temp.path()).list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id.as_str(), "good");
}

#[tokio::test]
async fn listing_is_newest_first_with_stable_ties_and_exact_cwd_filtering() {
    let temp = TempDirectory::new("order-filter");
    for (id, cwd, updated_at) in [
        ("b", "/work", "2026-07-30T12:00:00+00:00"),
        ("a", "/work", "2026-07-30T12:00:00+00:00"),
        ("newest", "/work/sub", "2026-07-30T13:00:00+00:00"),
        ("old", "/work", "2026-07-30T10:00:00+00:00"),
    ] {
        write_json(
            &temp.path().join(format!("{id}.meta")),
            &json!({
                "session_id": id,
                "cwd": cwd,
                "updated_at": updated_at
            }),
        );
    }
    let repository = store(temp.path());
    let all = repository.list().await.unwrap();
    assert_eq!(
        all.iter()
            .map(|entry| entry.session_id.as_str())
            .collect::<Vec<_>>(),
        ["newest", "a", "b", "old"]
    );

    let filtered = repository
        .list_filtered(SessionListFilter {
            cwd: Some("/work".into()),
        })
        .await
        .unwrap();
    assert_eq!(
        filtered
            .iter()
            .map(|entry| entry.session_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "old"]
    );
}

#[tokio::test]
async fn oversized_sidecar_is_ignored_and_json_fallback_remains_available() {
    let temp = TempDirectory::new("sidecar-bound");
    write_json(
        &temp.path().join("bounded.json"),
        &json!({"cwd": "/workspace", "title": "Fallback"}),
    );
    fs::write(temp.path().join("bounded.meta"), vec![b'x'; 65]).unwrap();
    let repository = FilesystemSessionStore::new(
        temp.path().to_path_buf(),
        SessionSource::LegacyNativeGlm { profile: None },
        DiscoveryBounds {
            max_sidecar_bytes: 64,
            ..DiscoveryBounds::default()
        },
    )
    .unwrap();
    let listed = repository.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].origin, MetadataOrigin::JsonFallback);
    assert_eq!(listed[0].title.as_deref(), Some("Fallback"));
}
