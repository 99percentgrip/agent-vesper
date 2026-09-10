//! Explicit admission retention policy; no cross-scope eviction.
use super::store::{LedgerError, MemoryScope};
use serde::{Deserialize, Serialize};

/// Persisted scope admission caps. Existing constructors retain strict global
/// capacity refusal; callers opt into automatic eviction with `with_retention`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum LedgerRetention {
    /// No automatic eviction. Explicit pruning remains available.
    #[default]
    Disabled,
    /// Per-scope caps: shared confidence/age eviction and private FIFO eviction.
    Limited {
        /// Maximum shared entries.
        swarm: usize,
        /// Maximum entries in each individual worker scope.
        worker: usize,
        /// Maximum entries in each individual task scope.
        task: usize,
    },
}
impl LedgerRetention {
    pub(crate) fn validate(self, global: usize) -> Result<(), LedgerError> {
        if let Self::Limited {
            swarm,
            worker,
            task,
        } = self
            && [swarm, worker, task]
                .into_iter()
                .any(|cap| cap == 0 || cap > global)
        {
            return Err(LedgerError::InvalidRetention);
        }
        Ok(())
    }
    pub(crate) fn cap(self, scope: &MemoryScope) -> Option<usize> {
        match self {
            Self::Disabled => None,
            Self::Limited {
                swarm,
                worker,
                task,
            } => Some(match scope {
                MemoryScope::Swarm => swarm,
                MemoryScope::Worker(_) => worker,
                MemoryScope::Task(_) => task,
            }),
        }
    }
}
