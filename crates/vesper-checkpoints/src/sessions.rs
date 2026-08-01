//! Session lineage tracking — `/sessions-new`, `/sessions`, `/lineage`,
//! `/branch`, `/rename`.
//!
//! Mirrors `vesper-memory::MemoryStore` discipline: append-only JSONL log,
//! in-memory mirror behind a `Mutex`, atomic rewrites for renames.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::CheckpointError;
use crate::io::{append_line, read_all_jsonl, write_atomic};
use crate::types::{MAX_LABEL_CHARS, MAX_LINEAGE_DEPTH, SessionRecord, SessionStatus};

/// In-memory cache + on-disk JSONL store of [`SessionRecord`]s.
pub struct SessionLineage {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    records: Vec<SessionRecord>,
    next_id: u64,
}

impl std::fmt::Debug for SessionLineage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionLineage")
            .field("root", &self.root)
            .field(
                "sessions",
                &self.state.lock().map(|s| s.records.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl SessionLineage {
    /// Opens (or creates) a session lineage rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let records = read_all_jsonl::<SessionRecord>(&log_path)?;
        let next_id = records
            .iter()
            .filter_map(|record| record.id.strip_prefix("sess-"))
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
        root.join("sessions.jsonl")
    }

    /// Returns the current session count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("session mutex poisoned")
            .records
            .len()
    }

    /// Returns true when the lineage holds no sessions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists every session record in append order.
    #[must_use]
    pub fn list(&self) -> Vec<SessionRecord> {
        self.state
            .lock()
            .expect("session mutex poisoned")
            .records
            .clone()
    }

    /// Returns the session with the given id, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<SessionRecord> {
        self.state
            .lock()
            .expect("session mutex poisoned")
            .records
            .iter()
            .find(|record| record.id == id)
            .cloned()
    }

    /// Creates a new session. If `name` is `None` the session id is used.
    /// The session is rooted at `workspace_root` (must be absolute). The
    /// parent session (if any) is marked `Superseded`. Returns the new
    /// session record.
    pub fn create(
        &self,
        parent_id: Option<&str>,
        name: Option<&str>,
        workspace_root: &Path,
    ) -> Result<SessionRecord, CheckpointError> {
        if !workspace_root.is_absolute() {
            return Err(CheckpointError::InvalidWorkspaceRoot);
        }
        let mut state = self.state.lock().expect("session mutex poisoned");
        // Lineage depth guard.
        if state.records.len() >= MAX_LINEAGE_DEPTH {
            return Err(CheckpointError::RetentionCapReached);
        }
        let id = format!("sess-{}", state.next_id);
        state.next_id = state.next_id.saturating_add(1);
        // Validate the parent exists, if supplied.
        if let Some(parent) = parent_id
            && !state.records.iter().any(|r| r.id == parent)
        {
            return Err(CheckpointError::SessionNotFound(parent.to_string()));
        }
        let now = SystemTime::now();
        let record = SessionRecord {
            id: id.clone(),
            parent_id: parent_id.map(str::to_string),
            name: name.map(str::to_string).unwrap_or_else(|| id.clone()),
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
        };
        record.validate()?;
        // Mark the parent superseded.
        if let Some(parent) = parent_id
            && let Some(existing) = state.records.iter_mut().find(|r| r.id == parent)
        {
            existing.status = SessionStatus::Superseded;
            existing.updated_at = now;
        }
        let serialized = serde_json::to_string(&record)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        // Rewrite to persist the parent's status change atomically.
        Self::rewrite_log(&self.root, &state.records)?;
        state.records.push(record.clone());
        Ok(record)
    }

    /// Branches from `parent_id` into a new session with the given name.
    /// The parent keeps its `Active` status (a branch does not supersede).
    pub fn branch(
        &self,
        parent_id: &str,
        name: Option<&str>,
        workspace_root: &Path,
    ) -> Result<SessionRecord, CheckpointError> {
        let mut state = self.state.lock().expect("session mutex poisoned");
        if state.records.len() >= MAX_LINEAGE_DEPTH {
            return Err(CheckpointError::RetentionCapReached);
        }
        if !state.records.iter().any(|r| r.id == parent_id) {
            return Err(CheckpointError::SessionNotFound(parent_id.to_string()));
        }
        let id = format!("sess-{}", state.next_id);
        state.next_id = state.next_id.saturating_add(1);
        let now = SystemTime::now();
        let record = SessionRecord {
            id: id.clone(),
            parent_id: Some(parent_id.to_string()),
            name: name.map(str::to_string).unwrap_or_else(|| id.clone()),
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
        };
        record.validate()?;
        let serialized = serde_json::to_string(&record)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.records.push(record.clone());
        Ok(record)
    }

    /// Renames the session with the given id. Idempotent on the name.
    pub fn rename(&self, id: &str, new_name: &str) -> Result<SessionRecord, CheckpointError> {
        if new_name.chars().count() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("name length"));
        }
        let mut state = self.state.lock().expect("session mutex poisoned");
        let Some(record) = state.records.iter_mut().find(|r| r.id == id) else {
            return Err(CheckpointError::SessionNotFound(id.to_string()));
        };
        record.name = new_name.to_string();
        record.updated_at = SystemTime::now();
        let updated = record.clone();
        Self::rewrite_log(&self.root, &state.records)?;
        Ok(updated)
    }

    /// Returns the lineage chain from the root down to `id` (inclusive).
    /// Returns an empty vector when the id is unknown.
    #[must_use]
    pub fn lineage(&self, id: &str) -> Vec<SessionRecord> {
        let state = self.state.lock().expect("session mutex poisoned");
        let by_id: HashMap<&str, &SessionRecord> = state
            .records
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect();
        let mut chain: Vec<SessionRecord> = Vec::new();
        let mut current = id;
        while let Some(record) = by_id.get(current) {
            chain.push((*record).clone());
            match &record.parent_id {
                Some(parent) => current = parent.as_str(),
                None => break,
            }
            if chain.len() >= MAX_LINEAGE_DEPTH {
                break;
            }
        }
        chain.reverse();
        chain
    }

    /// Rewrites the JSONL log from `records` in a single atomic rename.
    fn rewrite_log(root: &Path, records: &[SessionRecord]) -> Result<(), CheckpointError> {
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
    //! Session lineage: create, branch, rename, lineage chain.

    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn lineage_under(temp: &TempDir) -> (PathBuf, SessionLineage) {
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let lineage = SessionLineage::open(&root).unwrap();
        (root, lineage)
    }

    #[test]
    fn create_mints_sequential_ids() {
        let temp = TempDir::new().unwrap();
        let (_root, lineage) = lineage_under(&temp);
        let workspace = temp.path().join("workspace");
        let a = lineage.create(None, Some("alpha"), &workspace).unwrap();
        let b = lineage.create(None, Some("beta"), &workspace).unwrap();
        assert_eq!(a.id, "sess-1");
        assert_eq!(b.id, "sess-2");
        assert_eq!(a.name, "alpha");
        assert_eq!(b.name, "beta");
    }

    #[test]
    fn create_marks_parent_superseded() {
        let temp = TempDir::new().unwrap();
        let (_root, lineage) = lineage_under(&temp);
        let workspace = temp.path().join("workspace");
        let parent = lineage.create(None, None, &workspace).unwrap();
        assert_eq!(parent.status, SessionStatus::Active);
        let _child = lineage.create(Some(&parent.id), None, &workspace).unwrap();
        let parent_after = lineage.get(&parent.id).unwrap();
        assert_eq!(parent_after.status, SessionStatus::Superseded);
    }

    #[test]
    fn branch_keeps_parent_active() {
        let temp = TempDir::new().unwrap();
        let (_root, lineage) = lineage_under(&temp);
        let workspace = temp.path().join("workspace");
        let parent = lineage.create(None, None, &workspace).unwrap();
        let _child = lineage
            .branch(&parent.id, Some("experiment"), &workspace)
            .unwrap();
        let parent_after = lineage.get(&parent.id).unwrap();
        assert_eq!(parent_after.status, SessionStatus::Active);
    }

    #[test]
    fn rename_persists_across_reopen() {
        let temp = TempDir::new().unwrap();
        let (root, lineage) = lineage_under(&temp);
        let workspace = temp.path().join("workspace");
        let record = lineage.create(None, Some("old"), &workspace).unwrap();
        lineage.rename(&record.id, "new").unwrap();
        let reopened = SessionLineage::open(&root).unwrap();
        assert_eq!(reopened.get(&record.id).unwrap().name, "new");
    }

    #[test]
    fn lineage_returns_root_to_id_chain() {
        let temp = TempDir::new().unwrap();
        let (_root, lineage) = lineage_under(&temp);
        let workspace = temp.path().join("workspace");
        let a = lineage.create(None, Some("a"), &workspace).unwrap();
        let b = lineage.create(Some(&a.id), Some("b"), &workspace).unwrap();
        let c = lineage.create(Some(&b.id), Some("c"), &workspace).unwrap();
        let chain = lineage.lineage(&c.id);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].id, a.id);
        assert_eq!(chain[1].id, b.id);
        assert_eq!(chain[2].id, c.id);
    }

    #[test]
    fn create_unknown_parent_returns_not_found() {
        let temp = TempDir::new().unwrap();
        let (_root, lineage) = lineage_under(&temp);
        let workspace = temp.path().join("workspace");
        let err = lineage
            .create(Some("sess-does-not-exist"), None, &workspace)
            .unwrap_err();
        assert_eq!(
            err,
            CheckpointError::SessionNotFound("sess-does-not-exist".into())
        );
    }
}
