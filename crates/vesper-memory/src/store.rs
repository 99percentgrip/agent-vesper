//! Durable memory entry store (append-only JSONL with atomic forget).
//!
//! [`MemoryStore`] is the source of truth for `/memory`, `/goal`,
//! `/subgoal`, and the write side of every other Tier C Phase 8 command.
//! It mirrors the oracle's `memory.append_memory` / `forget_memory` model:
//! appends go to an append-only log (durable, cheap), and forgets rewrite
//! the file by filtering lines (single atomic rename).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::MemoryError;
use crate::io::{append_line, read_all_jsonl, write_atomic};
use crate::types::{MemoryEntry, MemoryKind};

/// In-memory cache + on-disk JSONL store of [`MemoryEntry`] records.
///
/// All public methods take `&self` and serialize through an internal
/// `Mutex`, so the composition boundary can share one
/// `Arc<MemoryStore>` across the TUI event loop and the agent loop without
/// external locking.
pub struct MemoryStore {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// In-memory mirror of the JSONL log, kept in append order.
    entries: Vec<MemoryEntry>,
    /// Monotonic counter used to mint stable ids when the caller does not
    /// supply one.
    next_id: u64,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryStore")
            .field("root", &self.root)
            .field(
                "entries",
                &self.state.lock().map(|s| s.entries.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl MemoryStore {
    /// Opens (or creates) a memory store rooted at `root`. The root must be
    /// absolute and its parent must exist; the store does NOT create the
    /// root directory itself (the composition boundary owns that).
    pub fn open(root: &Path) -> Result<Self, MemoryError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let entries = read_all_jsonl::<MemoryEntry>(&log_path)?;
        let next_id = entries
            .iter()
            .filter_map(|entry| entry.id.strip_prefix("mem-"))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(State { entries, next_id }),
        })
    }

    fn validate_root(root: &Path) -> Result<(), MemoryError> {
        if !root.is_absolute() {
            return Err(MemoryError::InvalidRoot);
        }
        // The parent of the root must exist so we never create intermediate
        // directories. We do not require the root itself to exist yet —
        // `append` will create it lazily.
        match root.parent() {
            Some(parent) if parent.as_os_str().is_empty() => {
                // Root's parent is the filesystem root — accept it.
                Ok(())
            }
            Some(parent) if parent.exists() => Ok(()),
            _ => Err(MemoryError::InvalidRoot),
        }
    }

    fn log_path(root: &Path) -> PathBuf {
        root.join("memory.jsonl")
    }

    /// Returns the configured root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the current entry count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }

    /// Returns true when the store holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists entries, optionally filtered by kind. The returned slice
    /// borrows from an internal clone so the caller can iterate without
    /// holding the lock.
    #[must_use]
    pub fn list(&self, kind: Option<MemoryKind>) -> Vec<MemoryEntry> {
        let state = self.state.lock().expect("memory store mutex poisoned");
        match kind {
            Some(filter) => state
                .entries
                .iter()
                .filter(|entry| entry.kind == filter)
                .cloned()
                .collect(),
            None => state.entries.clone(),
        }
    }

    /// Returns entries whose `summary`, `scopes`, or `id` contain
    /// `needle` (case-insensitive substring match).
    #[must_use]
    pub fn query(&self, needle: &str) -> Vec<MemoryEntry> {
        let needle = needle.to_ascii_lowercase();
        let state = self.state.lock().expect("memory store mutex poisoned");
        state
            .entries
            .iter()
            .filter(|entry| {
                if entry.summary.to_ascii_lowercase().contains(&needle) {
                    return true;
                }
                if entry.id.to_ascii_lowercase().contains(&needle) {
                    return true;
                }
                entry
                    .scopes
                    .iter()
                    .any(|scope| scope.to_ascii_lowercase().contains(&needle))
            })
            .cloned()
            .collect()
    }

    /// Appends a new entry. If `entry.id` is empty the store mints one of
    /// the form `mem-<counter>`; otherwise the supplied id is used (and
    /// must be unique). Returns the persisted entry on success.
    pub fn append(&self, mut entry: MemoryEntry) -> Result<MemoryEntry, MemoryError> {
        entry.validate()?;
        let mut state = self.state.lock().expect("memory store mutex poisoned");
        if state.entries.len() >= crate::types::MAX_ENTRIES {
            return Err(MemoryError::StoreFull);
        }
        if entry.id.is_empty() {
            entry.id = format!("mem-{}", state.next_id);
            state.next_id = state.next_id.saturating_add(1);
        } else {
            // Reject duplicate ids so the log stays a clean set.
            if state.entries.iter().any(|existing| existing.id == entry.id) {
                return Err(MemoryError::InvalidIdentifier("duplicate id".into()));
            }
        }
        let now = SystemTime::now();
        if entry.created_at == UNIX_EPOCH {
            entry.created_at = now;
        }
        entry.updated_at = now;
        let serialized = serde_json::to_string(&entry)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.entries.push(entry.clone());
        Ok(entry)
    }

    /// Removes every entry whose id matches one of `ids`. The JSONL log
    /// is rewritten in a single atomic rename. Returns the number of
    /// entries removed.
    pub fn forget(&self, ids: &[&str]) -> Result<usize, MemoryError> {
        let mut state = self.state.lock().expect("memory store mutex poisoned");
        let id_set: HashSet<&str> = ids.iter().copied().collect();
        let before = state.entries.len();
        state
            .entries
            .retain(|entry| !id_set.contains(entry.id.as_str()));
        let removed = before - state.entries.len();
        if removed == 0 {
            return Ok(0);
        }
        Self::rewrite_log(&self.root, &state.entries)?;
        Ok(removed)
    }

    /// Convenience: removes the entry with the given id. Returns
    /// `Ok(false)` when no entry matched (idempotent).
    pub fn forget_one(&self, id: &str) -> Result<bool, MemoryError> {
        Ok(self.forget(std::slice::from_ref(&id))? > 0)
    }

    /// Rewrites the JSONL log from `entries` in a single atomic rename.
    /// Used by [`forget`](Self::forget) and by [`curate`](Self::curate).
    fn rewrite_log(root: &Path, entries: &[MemoryEntry]) -> Result<(), MemoryError> {
        let mut buffer = String::new();
        for entry in entries {
            buffer.push_str(&serde_json::to_string(entry)?);
            buffer.push('\n');
        }
        write_atomic(&Self::log_path(root), buffer.as_bytes())
    }

    /// Curation pass: deduplicates entries with identical (kind, summary)
    /// pairs (keeping the earliest) and trims the log to the most recent
    /// [`MAX_ENTRIES`](crate::types::MAX_ENTRIES) entries when over the
    /// cap. Returns `(duplicates_removed, overflow_trimmed)`.
    pub fn curate(&self) -> Result<(usize, usize), MemoryError> {
        let mut state = self.state.lock().expect("memory store mutex poisoned");
        let mut seen: HashSet<(MemoryKind, String)> = HashSet::new();
        let before = state.entries.len();
        state.entries.retain(|entry| {
            let key = (entry.kind, entry.summary.clone());
            seen.insert(key)
        });
        let duplicates_removed = before - state.entries.len();
        let overflow_trimmed = state
            .entries
            .len()
            .saturating_sub(crate::types::MAX_ENTRIES);
        if overflow_trimmed > 0 {
            // Drop the oldest entries (append order = chronological).
            state.entries.drain(0..overflow_trimmed);
        }
        Self::rewrite_log(&self.root, &state.entries)?;
        Ok((duplicates_removed, overflow_trimmed))
    }
}

#[cfg(test)]
mod tests {
    //! Memory store: confinement, append, query, forget, curate.

    use super::*;
    use crate::types::MemoryKind;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn store_under(temp: &TempDir) -> (PathBuf, MemoryStore) {
        let root = temp.path().join("memory-root");
        std::fs::create_dir_all(&root).unwrap();
        let store = MemoryStore::open(&root).unwrap();
        (root, store)
    }

    #[test]
    fn rejects_non_absolute_root() {
        let err = MemoryStore::open(Path::new("relative/path")).unwrap_err();
        assert_eq!(err, MemoryError::InvalidRoot);
    }

    #[test]
    fn rejects_root_whose_parent_does_not_exist() {
        let err = MemoryStore::open(Path::new("/this/does/not/exist/memory")).unwrap_err();
        assert_eq!(err, MemoryError::InvalidRoot);
    }

    #[test]
    fn append_mints_an_id_when_none_supplied() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let entry = store
            .append(MemoryEntry {
                id: String::new(),
                kind: MemoryKind::Memory,
                summary: "first entry".into(),
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        assert_eq!(entry.id, "mem-1");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn append_persists_across_reopen() {
        let temp = TempDir::new().unwrap();
        let (root, store) = store_under(&temp);
        store
            .append(MemoryEntry {
                id: String::new(),
                kind: MemoryKind::Goal,
                summary: "ship stage 12".into(),
                scopes: vec!["apps/agent-vesper-tui".into()],
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        // Reopen from the same root: the entry must still be there.
        let reopened = MemoryStore::open(&root).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.list(None)[0].summary, "ship stage 12");
    }

    #[test]
    fn query_matches_summary_scope_or_id_case_insensitively() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        store
            .append(MemoryEntry {
                id: "goal-alpha".into(),
                kind: MemoryKind::Goal,
                summary: "Ship Stage 12".into(),
                scopes: vec!["apps/agent-vesper-tui".into()],
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        assert_eq!(store.query("STAGE 12").len(), 1);
        assert_eq!(store.query("vesper-tui").len(), 1);
        assert_eq!(store.query("goal-alpha").len(), 1);
        assert!(store.query("nonexistent").is_empty());
    }

    #[test]
    fn forget_rewrites_the_log_atomically_and_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let (root, store) = store_under(&temp);
        let a = store
            .append(MemoryEntry {
                id: String::new(),
                kind: MemoryKind::Memory,
                summary: "a".into(),
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        store
            .append(MemoryEntry {
                id: String::new(),
                kind: MemoryKind::Memory,
                summary: "b".into(),
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        assert_eq!(store.len(), 2);
        // Forget a: should return 1 and leave 1.
        assert!(store.forget_one(&a.id).unwrap());
        assert_eq!(store.len(), 1);
        // Reopen: still 1.
        let reopened = MemoryStore::open(&root).unwrap();
        assert_eq!(reopened.len(), 1);
        // Idempotent forget of the same id returns false.
        assert!(!store.forget_one(&a.id).unwrap());
    }

    #[test]
    fn curate_deduplicates_identical_kind_summary_pairs() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        for _ in 0..3 {
            store
                .append(MemoryEntry {
                    id: String::new(),
                    kind: MemoryKind::Memory,
                    summary: "duplicate".into(),
                    scopes: Vec::new(),
                    evidence: Vec::new(),
                    created_at: UNIX_EPOCH,
                    updated_at: UNIX_EPOCH,
                })
                .unwrap();
        }
        store
            .append(MemoryEntry {
                id: String::new(),
                kind: MemoryKind::Memory,
                summary: "unique".into(),
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        let (duplicates_removed, overflow_trimmed) = store.curate().unwrap();
        assert_eq!(duplicates_removed, 2);
        assert_eq!(overflow_trimmed, 0);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn bounds_violations_are_rejected() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let long_summary = "x".repeat(crate::types::MAX_SUMMARY_CHARS + 1);
        let err = store
            .append(MemoryEntry {
                id: String::new(),
                kind: MemoryKind::Memory,
                summary: long_summary,
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap_err();
        assert_eq!(err, MemoryError::BoundsViolated("summary length"));
    }

    #[test]
    fn duplicate_supplied_id_is_rejected() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        store
            .append(MemoryEntry {
                id: "fixed-id".into(),
                kind: MemoryKind::Memory,
                summary: "first".into(),
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap();
        let err = store
            .append(MemoryEntry {
                id: "fixed-id".into(),
                kind: MemoryKind::Memory,
                summary: "second".into(),
                scopes: Vec::new(),
                evidence: Vec::new(),
                created_at: UNIX_EPOCH,
                updated_at: UNIX_EPOCH,
            })
            .unwrap_err();
        assert_eq!(err, MemoryError::InvalidIdentifier("duplicate id".into()));
    }
}
