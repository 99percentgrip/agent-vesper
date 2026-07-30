use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{FixtureCorpus, FixtureError, ScenarioFixture, fixture_root};

static TEMPORARY_STORE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Loads authoritative session fixtures without embedding fixture payloads in Rust.
#[derive(Debug)]
pub struct SessionFixtureLoader {
    corpus: FixtureCorpus,
}

impl SessionFixtureLoader {
    /// Loads and validates the complete authoritative corpus.
    pub fn load() -> Result<Self, FixtureError> {
        Ok(Self {
            corpus: FixtureCorpus::load(fixture_root())?,
        })
    }

    /// Returns a cloned session fixture by stable scenario identity.
    #[must_use]
    pub fn scenario(&self, scenario_id: &str) -> Option<ScenarioFixture> {
        let fixture = self.corpus.scenario(scenario_id)?;
        (fixture.manifest.category == "sessions/v1").then(|| fixture.clone())
    }

    /// Returns all seven authoritative session fixtures in stable ID order.
    #[must_use]
    pub fn all(&self) -> Vec<ScenarioFixture> {
        self.corpus
            .scenarios
            .iter()
            .filter(|fixture| fixture.manifest.category == "sessions/v1")
            .cloned()
            .collect()
    }
}

/// Test-owned temporary read store. It is removed when dropped.
#[derive(Debug)]
pub struct TemporaryReadStore {
    root: PathBuf,
    session_root: PathBuf,
}

impl TemporaryReadStore {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn session_root(&self) -> &Path {
        &self.session_root
    }
}

impl Drop for TemporaryReadStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Builder for a synthetic Native GLM ACP store under a unique temporary home.
#[derive(Debug, Default)]
pub struct LegacyStoreBuilder {
    profile: Option<String>,
    records: Vec<PendingFile>,
}

impl LegacyStoreBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the named-profile layout. Profile syntax remains deliberately narrow.
    pub fn profile(mut self, profile: &str) -> Result<Self, TestStoreError> {
        validate_component(profile)?;
        self.profile = Some(profile.to_owned());
        Ok(self)
    }

    pub fn json_record(mut self, session_id: &str, value: &Value) -> Result<Self, TestStoreError> {
        self.records.push(PendingFile::json(session_id, value)?);
        Ok(self)
    }

    pub fn metadata_sidecar(
        mut self,
        session_id: &str,
        value: &Value,
    ) -> Result<Self, TestStoreError> {
        self.records.push(PendingFile::metadata(session_id, value)?);
        Ok(self)
    }

    pub fn corrupt_record(
        mut self,
        session_id: &str,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, TestStoreError> {
        self.records
            .push(PendingFile::record(session_id, bytes.into())?);
        Ok(self)
    }

    pub fn truncated_record(
        mut self,
        session_id: &str,
        source: &[u8],
        retained_bytes: usize,
    ) -> Result<Self, TestStoreError> {
        let retained = retained_bytes.min(source.len());
        self.records.push(PendingFile::record(
            session_id,
            source[..retained].to_vec(),
        )?);
        Ok(self)
    }

    /// Materializes only the synthetic test store.
    pub fn build(self) -> Result<TemporaryReadStore, TestStoreError> {
        let root = unique_temporary_root("legacy")?;
        let session_root = match self.profile {
            Some(profile) => root
                .join(".glm-acp/profiles")
                .join(profile)
                .join("sessions"),
            None => root.join(".glm-acp/sessions"),
        };
        materialize(root, session_root, self.records)
    }
}

/// Builder for a synthetic future Agent Vesper read store.
#[derive(Debug, Default)]
pub struct AgentVesperReadStoreBuilder {
    records: Vec<PendingFile>,
}

impl AgentVesperReadStoreBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn json_record(mut self, session_id: &str, value: &Value) -> Result<Self, TestStoreError> {
        self.records.push(PendingFile::json(session_id, value)?);
        Ok(self)
    }

    pub fn corrupt_record(
        mut self,
        session_id: &str,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, TestStoreError> {
        self.records
            .push(PendingFile::record(session_id, bytes.into())?);
        Ok(self)
    }

    pub fn truncated_record(
        mut self,
        session_id: &str,
        source: &[u8],
        retained_bytes: usize,
    ) -> Result<Self, TestStoreError> {
        let retained = retained_bytes.min(source.len());
        self.records.push(PendingFile::record(
            session_id,
            source[..retained].to_vec(),
        )?);
        Ok(self)
    }

    /// Materializes the platform-neutral data-root shape used by injected tests.
    pub fn build(self) -> Result<TemporaryReadStore, TestStoreError> {
        let root = unique_temporary_root("vesper")?;
        let session_root = root.join("agent-vesper/sessions");
        materialize(root, session_root, self.records)
    }
}

#[derive(Debug)]
struct PendingFile {
    name: String,
    bytes: Vec<u8>,
}

impl PendingFile {
    fn json(session_id: &str, value: &Value) -> Result<Self, TestStoreError> {
        let bytes = serde_json::to_vec(value).map_err(TestStoreError::Json)?;
        Self::record(session_id, bytes)
    }

    fn metadata(session_id: &str, value: &Value) -> Result<Self, TestStoreError> {
        validate_component(session_id)?;
        let bytes = serde_json::to_vec(value).map_err(TestStoreError::Json)?;
        Ok(Self {
            name: format!("{session_id}.meta"),
            bytes,
        })
    }

    fn record(session_id: &str, bytes: Vec<u8>) -> Result<Self, TestStoreError> {
        validate_component(session_id)?;
        Ok(Self {
            name: format!("{session_id}.json"),
            bytes,
        })
    }
}

fn materialize(
    root: PathBuf,
    session_root: PathBuf,
    records: Vec<PendingFile>,
) -> Result<TemporaryReadStore, TestStoreError> {
    fs::create_dir_all(&session_root).map_err(TestStoreError::Io)?;
    for record in records {
        fs::write(session_root.join(record.name), record.bytes).map_err(TestStoreError::Io)?;
    }
    Ok(TemporaryReadStore { root, session_root })
}

fn validate_component(value: &str) -> Result<(), TestStoreError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(TestStoreError::UnsafeComponent);
    }
    Ok(())
}

fn unique_temporary_root(kind: &str) -> Result<PathBuf, TestStoreError> {
    for _ in 0..128 {
        let sequence = TEMPORARY_STORE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = std::env::temp_dir().join(format!(
            "agent-vesper-testkit-{kind}-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(TestStoreError::Io(error)),
        }
    }
    Err(TestStoreError::TemporaryRootExhausted)
}

/// One canonical file-tree entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTreeEntry {
    pub kind: String,
    pub length: Option<u64>,
    pub sha256: Option<String>,
    pub symlink_target: Option<String>,
}

/// Deterministic recursive manifest used to prove a synthetic store was unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTreeHashManifest {
    pub entries: BTreeMap<String, FileTreeEntry>,
}

impl FileTreeHashManifest {
    pub fn capture(root: &Path) -> Result<Self, TestStoreError> {
        let metadata = fs::symlink_metadata(root).map_err(TestStoreError::Io)?;
        if !metadata.is_dir() {
            return Err(TestStoreError::RootNotDirectory);
        }
        let mut entries = BTreeMap::new();
        capture_directory(root, root, &mut entries)?;
        Ok(Self { entries })
    }
}

fn capture_directory(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, FileTreeEntry>,
) -> Result<(), TestStoreError> {
    let mut children = fs::read_dir(directory)
        .map_err(TestStoreError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(TestStoreError::Io)?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| TestStoreError::EscapedRoot)?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let metadata = fs::symlink_metadata(&path).map_err(TestStoreError::Io)?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(TestStoreError::Io)?;
            entries.insert(
                relative,
                FileTreeEntry {
                    kind: "symlink".into(),
                    length: None,
                    sha256: None,
                    symlink_target: Some(target.to_string_lossy().into_owned()),
                },
            );
        } else if metadata.is_dir() {
            entries.insert(
                relative,
                FileTreeEntry {
                    kind: "directory".into(),
                    length: None,
                    sha256: None,
                    symlink_target: None,
                },
            );
            capture_directory(root, &path, entries)?;
        } else if metadata.is_file() {
            let bytes = fs::read(&path).map_err(TestStoreError::Io)?;
            entries.insert(
                relative,
                FileTreeEntry {
                    kind: "file".into(),
                    length: Some(metadata.len()),
                    sha256: Some(
                        Sha256::digest(bytes)
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect(),
                    ),
                    symlink_target: None,
                },
            );
        }
    }
    Ok(())
}

/// Captures a before-manifest and proves the complete tree remains identical later.
#[derive(Debug)]
pub struct NoWriteAssertion {
    root: PathBuf,
    before: FileTreeHashManifest,
}

impl NoWriteAssertion {
    pub fn capture(root: &Path) -> Result<Self, TestStoreError> {
        Ok(Self {
            root: root.to_path_buf(),
            before: FileTreeHashManifest::capture(root)?,
        })
    }

    pub fn assert_unchanged(&self) -> Result<(), TestStoreError> {
        let after = FileTreeHashManifest::capture(&self.root)?;
        if self.before == after {
            Ok(())
        } else {
            Err(TestStoreError::TreeChanged)
        }
    }

    #[must_use]
    pub fn before(&self) -> &FileTreeHashManifest {
        &self.before
    }
}

/// Synthetic-store helper failure.
#[derive(Debug, Error)]
pub enum TestStoreError {
    #[error("temporary store I/O failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("temporary store JSON encoding failed: {0}")]
    Json(#[source] serde_json::Error),
    #[error("store component violates the safe test alphabet")]
    UnsafeComponent,
    #[error("temporary root allocation was exhausted")]
    TemporaryRootExhausted,
    #[error("manifest root is not a directory")]
    RootNotDirectory,
    #[error("manifest traversal escaped its root")]
    EscapedRoot,
    #[error("synthetic persistent tree changed")]
    TreeChanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_create_isolated_legacy_and_vesper_read_stores() {
        let legacy = LegacyStoreBuilder::new()
            .json_record("legacy", &serde_json::json!({"schema_version": 1}))
            .unwrap()
            .metadata_sidecar("legacy", &serde_json::json!({"title": "safe"}))
            .unwrap()
            .corrupt_record("corrupt", b"{broken".to_vec())
            .unwrap()
            .truncated_record("truncated", br#"{"complete":true}"#, 5)
            .unwrap()
            .build()
            .unwrap();
        assert!(legacy.session_root().ends_with(".glm-acp/sessions"));
        assert!(legacy.session_root().join("legacy.json").is_file());
        assert!(legacy.session_root().join("legacy.meta").is_file());
        assert_eq!(
            fs::read(legacy.session_root().join("truncated.json")).unwrap(),
            br#"{"com"#
        );

        let vesper = AgentVesperReadStoreBuilder::new()
            .json_record(
                "vesper",
                &serde_json::json!({"format": "agent-vesper-session", "version": 1}),
            )
            .unwrap()
            .build()
            .unwrap();
        assert!(vesper.session_root().ends_with("agent-vesper/sessions"));
        assert!(vesper.session_root().join("vesper.json").is_file());
    }

    #[test]
    fn named_profile_and_unsafe_components_are_explicit() {
        let named = LegacyStoreBuilder::new()
            .profile("work_2")
            .unwrap()
            .build()
            .unwrap();
        assert!(
            named
                .session_root()
                .ends_with(".glm-acp/profiles/work_2/sessions")
        );
        assert!(LegacyStoreBuilder::new().profile("../escape").is_err());
        assert!(
            AgentVesperReadStoreBuilder::new()
                .corrupt_record("/absolute", b"{}".to_vec())
                .is_err()
        );
    }

    #[test]
    fn hash_manifest_and_no_write_assertion_detect_every_tree_change() {
        let store = LegacyStoreBuilder::new()
            .json_record("stable", &serde_json::json!({"schema_version": 1}))
            .unwrap()
            .build()
            .unwrap();
        let assertion = NoWriteAssertion::capture(store.root()).unwrap();
        assertion.assert_unchanged().unwrap();
        fs::write(store.session_root().join("stable.json"), b"changed").unwrap();
        assert!(matches!(
            assertion.assert_unchanged(),
            Err(TestStoreError::TreeChanged)
        ));
    }

    #[test]
    fn loader_returns_exactly_the_seven_session_scenarios() {
        let loader = SessionFixtureLoader::load().unwrap();
        let fixtures = loader.all();
        assert_eq!(fixtures.len(), 7);
        assert!(loader.scenario("session.schema1-complete").is_some());
        assert!(loader.scenario("acp.initialization").is_none());
    }

    #[test]
    fn file_tree_manifest_records_symlinks_without_following_them() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let store = LegacyStoreBuilder::new().build().unwrap();
            symlink(
                std::ffi::OsStr::new("/outside"),
                store.session_root().join("outside-link"),
            )
            .unwrap();
            let manifest = FileTreeHashManifest::capture(store.root()).unwrap();
            let link = manifest
                .entries
                .values()
                .find(|entry| entry.kind == "symlink")
                .unwrap();
            assert_eq!(link.symlink_target.as_deref(), Some("/outside"));
        }
    }
}
