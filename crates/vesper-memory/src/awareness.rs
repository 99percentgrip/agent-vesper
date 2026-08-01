//! Bounded in-process epistemic ledger.
//!
//! Mirrors the oracle's `awareness.EpistemicLedger`: an in-memory map of
//! bounded epistemic records keyed by id, with explicit upsert / resolve
//! / invalidate operations. Persistence is opt-in via [`save`] / [`load`]
//! to a single JSON file under the configured root; the harness (not this
//! crate) is responsible for keeping the live state coherent with
//! provider evidence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::MemoryError;
use crate::io::{read_all_lines, write_atomic};
use crate::types::{EpistemicRecord, MemoryKind, RecordStatus};

/// Maximum number of records the ledger will hold before rejecting an
/// upsert (mirrors the oracle's `awareness.MAX_RECORDS`).
pub const MAX_RECORDS: usize = 100;

/// File name for the persisted awareness ledger.
pub const AWARENESS_FILENAME: &str = "awareness.json";

/// Wrapper that owns a `BTreeMap<id, EpistemicRecord>` plus a monotonic
/// record counter. The map is wrapped in a `Mutex` so the composition
/// boundary can share one `Arc<AwarenessLedger>` across threads.
pub struct AwarenessLedger {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    records: BTreeMap<String, EpistemicRecord>,
    next_id: u64,
}

impl std::fmt::Debug for AwarenessLedger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AwarenessLedger")
            .field("root", &self.root)
            .field(
                "records",
                &self.state.lock().map(|s| s.records.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl AwarenessLedger {
    /// Opens a ledger rooted at `root`, loading any persisted state from
    /// `<root>/awareness.json`. The root must be absolute with an existing
    /// parent (mirrors `MemoryStore::open`).
    pub fn open(root: &Path) -> Result<Self, MemoryError> {
        if !root.is_absolute() {
            return Err(MemoryError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.exists() => {}
            Some(_) | None => return Err(MemoryError::InvalidRoot),
        }
        let mut state = State::default();
        if let Some(loaded) = Self::load_from(root)? {
            state = loaded.into_state();
        }
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(state),
        })
    }

    fn path(root: &Path) -> PathBuf {
        root.join(AWARENESS_FILENAME)
    }

    /// Returns the current record count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("awareness mutex poisoned")
            .records
            .len()
    }

    /// Returns true when the ledger holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists records, optionally filtered by kind.
    #[must_use]
    pub fn list(&self, kind: Option<MemoryKind>) -> Vec<EpistemicRecord> {
        let state = self.state.lock().expect("awareness mutex poisoned");
        state
            .records
            .values()
            .filter(|record| kind.is_none_or(|k| record.kind == k))
            .cloned()
            .collect()
    }

    /// Returns a clone of the record with the given id, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<EpistemicRecord> {
        self.state
            .lock()
            .expect("awareness mutex poisoned")
            .records
            .get(id)
            .cloned()
    }

    /// Inserts or replaces a record. If `record.id` is empty the ledger
    /// mints one of the form `rec-<counter>`; otherwise the supplied id is
    /// used. Returns the persisted record.
    pub fn upsert(&self, mut record: EpistemicRecord) -> Result<EpistemicRecord, MemoryError> {
        record.validate()?;
        let mut state = self.state.lock().expect("awareness mutex poisoned");
        let now = SystemTime::now();
        if record.id.is_empty() {
            record.id = format!("rec-{}", state.next_id);
            state.next_id = state.next_id.saturating_add(1);
            record.created_at = now;
        } else if !state.records.contains_key(&record.id) {
            // New explicit id: advance the counter past it if it matches
            // our scheme so future auto-mints do not collide.
            if let Some(suffix) = record.id.strip_prefix("rec-")
                && let Ok(n) = suffix.parse::<u64>()
            {
                state.next_id = state.next_id.max(n.saturating_add(1));
            }
            record.created_at = now;
        }
        // Cap by rejecting brand-new records once full (replacing an
        // existing id is always allowed).
        if !state.records.contains_key(&record.id) && state.records.len() >= MAX_RECORDS {
            return Err(MemoryError::StoreFull);
        }
        record.updated_at = now;
        state.records.insert(record.id.clone(), record.clone());
        Ok(record)
    }

    /// Marks the record with the given id as `Resolved`. Returns
    /// `Ok(false)` when no record matched.
    pub fn resolve(&self, id: &str) -> Result<bool, MemoryError> {
        self.set_status(id, RecordStatus::Resolved)
    }

    /// Marks the record with the given id as `Invalidated`. Returns
    /// `Ok(false)` when no record matched.
    pub fn invalidate(&self, id: &str) -> Result<bool, MemoryError> {
        self.set_status(id, RecordStatus::Invalidated)
    }

    fn set_status(&self, id: &str, status: RecordStatus) -> Result<bool, MemoryError> {
        let mut state = self.state.lock().expect("awareness mutex poisoned");
        let Some(record) = state.records.get_mut(id) else {
            return Ok(false);
        };
        record.status = status;
        record.updated_at = SystemTime::now();
        Ok(true)
    }

    /// Removes the record with the given id. Idempotent.
    pub fn remove(&self, id: &str) -> Result<bool, MemoryError> {
        let mut state = self.state.lock().expect("awareness mutex poisoned");
        Ok(state.records.remove(id).is_some())
    }

    /// Persists the entire ledger to `<root>/awareness.json` as a single
    /// atomic rename.
    pub fn save(&self) -> Result<(), MemoryError> {
        let state = self.state.lock().expect("awareness mutex poisoned");
        let snapshot = PersistedLedger {
            records: state.records.values().cloned().collect::<Vec<_>>(),
            next_id: state.next_id,
        };
        let body = serde_json::to_vec(&snapshot)?;
        write_atomic(&Self::path(&self.root), &body)
    }

    fn load_from(root: &Path) -> Result<Option<PersistedLedger>, MemoryError> {
        let lines = read_all_lines(&Self::path(root))?;
        if lines.is_empty() {
            return Ok(None);
        }
        let body = lines.join("\n");
        let snapshot: PersistedLedger = serde_json::from_str(&body)?;
        Ok(Some(snapshot))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedLedger {
    records: Vec<EpistemicRecord>,
    next_id: u64,
}

impl PersistedLedger {
    /// Rehydrates the in-memory `State` from this snapshot.
    fn into_state(self) -> State {
        let mut records = BTreeMap::new();
        for record in self.records {
            records.insert(record.id.clone(), record);
        }
        State {
            records,
            next_id: self.next_id,
        }
    }
}

// `State::Default` needs `PersistedLedger::into_state` only in `open`,
// but `load_from` returns `Option<PersistedLedger>` and `open` then calls
// `into_state`. The intermediate plumbing keeps the lock surface small.

#[cfg(test)]
mod tests {
    //! Awareness ledger: open, upsert, resolve, invalidate, save/load.

    use super::*;
    use crate::types::{Confidence, EvidenceEvent, EvidenceSource, MAX_EVIDENCE};
    use std::time::UNIX_EPOCH;
    use tempfile::TempDir;

    fn ledger_under(temp: &TempDir) -> (PathBuf, AwarenessLedger) {
        let root = temp.path().join("memory-root");
        std::fs::create_dir_all(&root).unwrap();
        let ledger = AwarenessLedger::open(&root).unwrap();
        (root, ledger)
    }

    fn observation(summary: &str) -> EpistemicRecord {
        EpistemicRecord {
            id: String::new(),
            kind: MemoryKind::Observation,
            summary: summary.into(),
            scopes: Vec::new(),
            evidence: vec![EvidenceEvent {
                id: "ev1".into(),
                source: EvidenceSource::User,
                summary: "evidence".into(),
            }],
            supports: Vec::new(),
            confidence: Confidence::High,
            status: RecordStatus::Active,
            created_at: UNIX_EPOCH,
            updated_at: UNIX_EPOCH,
        }
    }

    #[test]
    fn upsert_mints_an_id_when_none_supplied() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let record = ledger.upsert(observation("test")).unwrap();
        assert_eq!(record.id, "rec-0");
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn upsert_with_explicit_id_advances_the_counter() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let mut record = observation("test");
        record.id = "rec-99".into();
        ledger.upsert(record).unwrap();
        // Next auto-mint must not collide with rec-99.
        let next = ledger.upsert(observation("auto")).unwrap();
        assert_eq!(next.id, "rec-100");
    }

    #[test]
    fn resolve_marks_active_record_resolved() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let record = ledger.upsert(observation("to resolve")).unwrap();
        assert!(ledger.resolve(&record.id).unwrap());
        assert_eq!(
            ledger.get(&record.id).unwrap().status,
            RecordStatus::Resolved
        );
    }

    #[test]
    fn save_then_reopen_round_trips() {
        let temp = TempDir::new().unwrap();
        let (root, ledger) = ledger_under(&temp);
        ledger.upsert(observation("persisted")).unwrap();
        ledger.upsert(observation("also persisted")).unwrap();
        ledger.save().unwrap();
        // Reopen from the same root: the records must still be there.
        let reopened = AwarenessLedger::open(&root).unwrap();
        assert_eq!(reopened.len(), 2);
    }

    #[test]
    fn list_filters_by_kind() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let mut obs = observation("an observation");
        obs.kind = MemoryKind::Observation;
        let mut hyp = observation("a hypothesis");
        hyp.kind = MemoryKind::Hypothesis;
        ledger.upsert(obs).unwrap();
        ledger.upsert(hyp).unwrap();
        assert_eq!(ledger.list(Some(MemoryKind::Observation)).len(), 1);
        assert_eq!(ledger.list(Some(MemoryKind::Hypothesis)).len(), 1);
        assert_eq!(ledger.list(None).len(), 2);
    }

    #[test]
    fn bounds_violations_are_rejected() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let mut record = observation("test");
        record.summary = "x".repeat(crate::types::MAX_SUMMARY_CHARS + 1);
        let err = ledger.upsert(record).unwrap_err();
        assert_eq!(err, MemoryError::BoundsViolated("summary length"));
    }

    #[test]
    fn evidence_count_cap_is_enforced() {
        let temp = TempDir::new().unwrap();
        let (_root, ledger) = ledger_under(&temp);
        let mut record = observation("test");
        record.evidence = (0..=MAX_EVIDENCE)
            .map(|i| EvidenceEvent {
                id: format!("ev{i}"),
                source: EvidenceSource::User,
                summary: "e".into(),
            })
            .collect();
        let err = ledger.upsert(record).unwrap_err();
        assert_eq!(err, MemoryError::BoundsViolated("evidence count"));
    }
}
