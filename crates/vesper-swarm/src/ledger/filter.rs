//! Bounded structured predicates; scopes remain a separate mandatory boundary.
use super::store::{EntryKind, LedgerEntry, LedgerError};

/// Conjunctive provenance/category/range filters. Ranges are inclusive original
/// producer sequence numbers, not fabricated wall-clock timestamps or copy IDs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LedgerFilter {
    /// Required original worker, when present.
    pub worker_id: Option<String>,
    /// Required original task, when present.
    pub task_id: Option<String>,
    /// Required original role, when present.
    pub role: Option<String>,
    /// Required entry category, when present.
    pub kind: Option<EntryKind>,
    /// Inclusive minimum original sequence.
    pub sequence_min: Option<u64>,
    /// Inclusive maximum original sequence.
    pub sequence_max: Option<u64>,
    /// Minimum confidence, in 0..=1.
    pub confidence_min: Option<f32>,
}

impl LedgerFilter {
    pub(crate) fn validate(&self) -> Result<(), LedgerError> {
        if self
            .sequence_min
            .zip(self.sequence_max)
            .is_some_and(|(min, max)| min > max)
        {
            return Err(LedgerError::InvalidFilter("reversed sequence range"));
        }
        if self
            .confidence_min
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err(LedgerError::InvalidFilter("invalid confidence floor"));
        }
        if [&self.worker_id, &self.task_id, &self.role]
            .into_iter()
            .flatten()
            .any(|value| value.is_empty() || value.len() > 256)
        {
            return Err(LedgerError::InvalidFilter(
                "identity must contain 1..=256 bytes",
            ));
        }
        Ok(())
    }

    pub(crate) fn matches(&self, entry: &LedgerEntry) -> bool {
        self.worker_id
            .as_ref()
            .is_none_or(|value| *value == entry.provenance.worker_id)
            && self
                .task_id
                .as_ref()
                .is_none_or(|value| *value == entry.provenance.task_id)
            && self
                .role
                .as_ref()
                .is_none_or(|value| *value == entry.provenance.role)
            && self.kind.is_none_or(|value| value == entry.kind)
            && self
                .sequence_min
                .is_none_or(|value| entry.provenance.sequence >= value)
            && self
                .sequence_max
                .is_none_or(|value| entry.provenance.sequence <= value)
            && self
                .confidence_min
                .is_none_or(|value| entry.confidence >= value)
    }
}
