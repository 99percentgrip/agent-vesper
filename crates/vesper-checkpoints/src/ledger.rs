//! Checkpoints ledger — append-only JSONL lineage log with prune().
//!
//! [`CheckpointsLedger`] is the source of truth for `/checkpoint`,
//! `/rollback`, `/rewind`, `/undo`. It mirrors `vesper-memory::MemoryStore`:
//! append-only log, atomic forget/curate via rewrite, in-memory mirror
//! behind a process-local `Mutex`. The payload files (the actual workspace
//! copies) live under `<root>/checkpoints/<id>/` and are unlinked by
//! [`prune`](Self::prune) when the retention cap is exceeded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::CheckpointError;
use crate::io::{append_line, read_all_jsonl, write_atomic};
use crate::snapshot::{self, SnapshotConfig, SnapshotOutcome};
use crate::types::{CheckpointKind, CheckpointRecord, FileSnapshot, MAX_RETENTION_COUNT};

/// In-memory cache + on-disk JSONL store of [`CheckpointRecord`]s.
pub struct CheckpointsLedger {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    records: Vec<CheckpointRecord>,
    next_id: u64,
}

impl std::fmt::Debug for CheckpointsLedger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckpointsLedger")
            .field("root", &self.root)
            .field(
                "records",
                &self.state.lock().map(|s| s.records.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl CheckpointsLedger {
    /// Opens (or creates) a checkpoints ledger rooted at `root`. The root
    /// must be absolute with an existing parent; the store does NOT create
    /// the root itself.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let records = read_all_jsonl::<CheckpointRecord>(&log_path)?;
        let next_id = records
            .iter()
            .filter_map(|record| record.id.strip_prefix("ckpt-"))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(State { records, next_id }),
        })
    }

    fn validate_root(root: &Path) -> Result<(), CheckpointError> {
        if !root.is_absolute() {
            return Err(CheckpointError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.as_os_str().is_empty() => Ok(()),
            Some(parent) if parent.exists() => Ok(()),
            _ => Err(CheckpointError::InvalidRoot),
        }
    }

    fn log_path(root: &Path) -> PathBuf {
        root.join("checkpoints.jsonl")
    }

    /// Returns the directory holding the payload files for `checkpoint_id`.
    fn payload_dir(&self, checkpoint_id: &str) -> PathBuf {
        self.root.join("checkpoints").join(checkpoint_id)
    }

    /// Returns the configured root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the current record count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("checkpoints mutex poisoned")
            .records
            .len()
    }

    /// Returns true when the ledger holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists records in append order (oldest first).
    #[must_use]
    pub fn list(&self) -> Vec<CheckpointRecord> {
        self.state
            .lock()
            .expect("checkpoints mutex poisoned")
            .records
            .clone()
    }

    /// Returns the record with the given id, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<CheckpointRecord> {
        self.state
            .lock()
            .expect("checkpoints mutex poisoned")
            .records
            .iter()
            .find(|record| record.id == id)
            .cloned()
    }

    /// Returns the N most recent records (newest first).
    #[must_use]
    pub fn recent(&self, count: usize) -> Vec<CheckpointRecord> {
        let state = self.state.lock().expect("checkpoints mutex poisoned");
        state.records.iter().rev().take(count).cloned().collect()
    }

    /// Creates an explicit checkpoint. Walks `workspace_root`, copies every
    /// eligible file into `<root>/checkpoints/<new-id>/`, appends the
    /// record to the JSONL log, then runs `prune()` to enforce
    /// [`MAX_RETENTION_COUNT`]. Returns the persisted record on success.
    ///
    /// This is the **only** path that produces a checkpoint — there is no
    /// auto-snapshotting on agent turns or file mutations, per the
    /// architect's directive.
    pub fn create(
        &self,
        session_id: &str,
        parent_id: Option<&str>,
        kind: CheckpointKind,
        label: Option<&str>,
        workspace_root: &Path,
    ) -> Result<CheckpointRecord, CheckpointError> {
        let mut state = self.state.lock().expect("checkpoints mutex poisoned");
        let id = format!("ckpt-{}", state.next_id);
        state.next_id = state.next_id.saturating_add(1);
        // Snapshot first — if it fails we leave the ledger untouched.
        let destination = self.payload_dir(&id);
        let config = SnapshotConfig::new(workspace_root.to_path_buf(), destination.clone());
        let outcome: SnapshotOutcome = snapshot::snapshot(&config)?;
        let record = CheckpointRecord {
            id: id.clone(),
            session_id: session_id.to_string(),
            parent_id: parent_id.map(str::to_string),
            kind,
            label: label.map(str::to_string),
            files: outcome.files,
            total_bytes: outcome.total_bytes,
            created_at: SystemTime::now(),
        };
        record.validate()?;
        // Append to the log.
        let serialized = serde_json::to_string(&record)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.records.push(record.clone());
        // Prune immediately so storage stays bounded.
        let pruned_ids = Self::prune_locked(&self.root, &mut state)?;
        drop(state);
        // Unlink pruned payload directories (outside the lock — disk I/O).
        for pruned_id in pruned_ids {
            let _ = snapshot::remove_payload(&self.payload_dir(&pruned_id));
        }
        Ok(record)
    }

    /// Restores the workspace from the checkpoint with the given id.
    /// Returns the number of files restored. Returns
    /// [`CheckpointError::CheckpointNotFound`] when the id is unknown.
    pub fn restore(&self, id: &str, workspace_root: &Path) -> Result<usize, CheckpointError> {
        let state = self.state.lock().expect("checkpoints mutex poisoned");
        let Some(record) = state.records.iter().find(|r| r.id == id) else {
            return Err(CheckpointError::CheckpointNotFound(id.to_string()));
        };
        let files: Vec<FileSnapshot> = record.files.clone();
        let source_dir = self.payload_dir(id);
        drop(state);
        // Restore outside the lock — disk I/O.
        snapshot::restore(workspace_root, &source_dir, &files)
    }

    /// Removes the records with the given ids AND unlinks their payload
    /// directories. Returns the number of records removed.
    pub fn forget(&self, ids: &[&str]) -> Result<usize, CheckpointError> {
        let mut state = self.state.lock().expect("checkpoints mutex poisoned");
        let id_set: HashSet<&str> = ids.iter().copied().collect();
        let before = state.records.len();
        state
            .records
            .retain(|record| !id_set.contains(record.id.as_str()));
        let removed = before - state.records.len();
        if removed == 0 {
            return Ok(0);
        }
        Self::rewrite_log(&self.root, &state.records)?;
        let pruned_ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        drop(state);
        for pruned_id in pruned_ids {
            let _ = snapshot::remove_payload(&self.payload_dir(&pruned_id));
        }
        Ok(removed)
    }

    /// Convenience: removes the record with the given id. Idempotent.
    pub fn forget_one(&self, id: &str) -> Result<bool, CheckpointError> {
        Ok(self.forget(std::slice::from_ref(&id))? > 0)
    }

    /// Enforces [`MAX_RETENTION_COUNT`]: if the record count exceeds the
    /// cap, the oldest records are removed from the log AND their payload
    /// directories are unlinked from disk. Returns the ids that were pruned
    /// so the caller can sweep the on-disk payloads (the ledger itself
    /// only rewrites the log here; payload unlinking is the caller's job
    /// to keep the lock window tight).
    pub fn prune(&self) -> Result<Vec<String>, CheckpointError> {
        let mut state = self.state.lock().expect("checkpoints mutex poisoned");
        let pruned_ids = Self::prune_locked(&self.root, &mut state)?;
        drop(state);
        for pruned_id in &pruned_ids {
            let _ = snapshot::remove_payload(&self.payload_dir(pruned_id));
        }
        Ok(pruned_ids)
    }

    /// Locked helper: trims `state.records` to `MAX_RETENTION_COUNT` (keeping
    /// the most recent), rewrites the log atomically, and returns the ids
    /// that were dropped. Does NOT touch the payload directories — the
    /// caller sweeps those outside the lock.
    fn prune_locked(_root: &Path, state: &mut State) -> Result<Vec<String>, CheckpointError> {
        if state.records.len() <= MAX_RETENTION_COUNT {
            return Ok(Vec::new());
        }
        let overflow = state.records.len() - MAX_RETENTION_COUNT;
        let pruned_ids: Vec<String> = state.records.drain(0..overflow).map(|r| r.id).collect();
        Self::rewrite_log(_root, &state.records)?;
        Ok(pruned_ids)
    }

    /// Rewrites the JSONL log from `records` in a single atomic rename.
    fn rewrite_log(root: &Path, records: &[CheckpointRecord]) -> Result<(), CheckpointError> {
        let mut buffer = String::new();
        for record in records {
            buffer.push_str(&serde_json::to_string(record)?);
            buffer.push('\n');
        }
        write_atomic(&Self::log_path(root), buffer.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    //! Checkpoints ledger: create, restore, prune, forget, persistence.

    use super::*;
    use crate::types::CheckpointKind;
    use std::fs;
    use std::time::UNIX_EPOCH;
    use tempfile::TempDir;

    fn ledger_under(temp: &TempDir) -> (PathBuf, CheckpointsLedger) {
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let ledger = CheckpointsLedger::open(&root).unwrap();
        (root, ledger)
    }

    fn workspace_with_files(temp: &TempDir) -> PathBuf {
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(workspace.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(workspace.join("README.md"), "# project").unwrap();
        workspace
    }

    #[test]
    fn rejects_non_absolute_root() {
        let err = CheckpointsLedger::open(Path::new("relative/path")).unwrap_err();
        assert_eq!(err, CheckpointError::InvalidRoot);
    }

    #[test]
    fn create_persists_across_reopen() {
        let temp = TempDir::new().unwrap();
        let (root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        let record = ledger
            .create(
                "sess-1",
                None,
                CheckpointKind::Manual,
                Some("initial"),
                &workspace,
            )
            .unwrap();
        assert_eq!(record.id, "ckpt-1");
        assert_eq!(record.label.as_deref(), Some("initial"));
        assert!(!record.files.is_empty());
        // Reopen from the same root: the record must still be there.
        let reopened = CheckpointsLedger::open(&root).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.list()[0].id, "ckpt-1");
    }

    #[test]
    fn restore_round_trips_workspace_state() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        let record = ledger
            .create("sess-1", None, CheckpointKind::Manual, None, &workspace)
            .unwrap();
        // Mutate the workspace.
        fs::write(workspace.join("src/main.rs"), "MODIFIED").unwrap();
        fs::write(workspace.join("README.md"), "MODIFIED").unwrap();
        // Restore.
        let restored = ledger.restore(&record.id, &workspace).unwrap();
        assert!(restored > 0);
        assert_eq!(
            fs::read_to_string(workspace.join("src/main.rs")).unwrap(),
            "fn main() {}"
        );
    }

    #[test]
    fn restore_unknown_id_returns_not_found() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        let err = ledger
            .restore("ckpt-does-not-exist", &workspace)
            .unwrap_err();
        assert_eq!(
            err,
            CheckpointError::CheckpointNotFound("ckpt-does-not-exist".into())
        );
    }

    #[test]
    fn prune_unlinks_payload_directories_when_retention_cap_exceeded() {
        // THE crucial test the lead architect demanded: prove that
        // MAX_RETENTION_COUNT actually unlinks old checkpoint files from
        // disk (preventing the storage bloat that paired with the Errno 24
        // leak in the Python oracle).
        let temp = TempDir::new().unwrap();
        let (root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        // Create MAX_RETENTION_COUNT + 5 checkpoints. The first 5 should
        // be pruned from both the ledger AND the disk.
        let overage = 5;
        let total = MAX_RETENTION_COUNT + overage;
        let mut all_ids = Vec::new();
        for index in 1..=total {
            let record = ledger
                .create(
                    "sess-1",
                    None,
                    CheckpointKind::Manual,
                    Some(&format!("ckpt-{index}")),
                    &workspace,
                )
                .unwrap();
            all_ids.push(record.id);
        }
        // The ledger must hold exactly MAX_RETENTION_COUNT records.
        assert_eq!(ledger.len(), MAX_RETENTION_COUNT);
        // The first `overage` ids must be gone from the ledger...
        for pruned_id in &all_ids[0..overage] {
            assert!(
                ledger.get(pruned_id).is_none(),
                "{pruned_id} should have been pruned from the ledger"
            );
        }
        // ... AND their payload directories must be unlinked from disk.
        for pruned_id in &all_ids[0..overage] {
            let payload = root.join("checkpoints").join(pruned_id);
            assert!(
                !payload.exists(),
                "{pruned_id} payload directory should have been unlinked"
            );
        }
        // The most recent `MAX_RETENTION_COUNT` ids must still be present
        // both in the ledger AND on disk.
        for kept_id in &all_ids[overage..] {
            assert!(ledger.get(kept_id).is_some());
            let payload = root.join("checkpoints").join(kept_id);
            assert!(payload.exists(), "{kept_id} payload should still exist");
        }
    }

    #[test]
    fn forget_unlinks_payload_directory() {
        let temp = TempDir::new().unwrap();
        let (root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        let record = ledger
            .create("sess-1", None, CheckpointKind::Manual, None, &workspace)
            .unwrap();
        let payload = root.join("checkpoints").join(&record.id);
        assert!(payload.exists());
        assert!(ledger.forget_one(&record.id).unwrap());
        assert!(
            !payload.exists(),
            "forget must unlink the payload directory"
        );
        assert_eq!(ledger.len(), 0);
    }

    #[test]
    fn recent_returns_newest_first() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        for index in 1..=3 {
            ledger
                .create(
                    "sess-1",
                    None,
                    CheckpointKind::Manual,
                    Some(&format!("v{index}")),
                    &workspace,
                )
                .unwrap();
        }
        let recent = ledger.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].label.as_deref(), Some("v3"));
        assert_eq!(recent[1].label.as_deref(), Some("v2"));
    }

    #[test]
    fn create_with_unix_epoch_timestamp_is_replaced_with_now() {
        // Verify the created_at field is populated (the record.validate
        // path doesn't check timestamps but the field must not be EPOCH).
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let workspace = workspace_with_files(&temp);
        let record = ledger
            .create("sess-1", None, CheckpointKind::Manual, None, &workspace)
            .unwrap();
        assert!(record.created_at > UNIX_EPOCH);
    }
}
