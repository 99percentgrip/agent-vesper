//! Cron registry — `/loop`.
//!
//! Records scheduled prompts in a JSONL log. The TUI binary is NOT a
//! daemon — it does not actually run the scheduler. A future long-running
//! daemon process (or a separate cron entry the user adds to their system
//! crontab) reads this registry and fires the prompts. Recording here
//! makes `/loop` a real, persistent operation rather than a deferred one.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::CheckpointError;
use crate::io::{append_line, read_all_jsonl, write_atomic};
use crate::types::{CronEntry, MAX_CRON_JOBS};

/// In-memory cache + on-disk JSONL store of [`CronEntry`]s.
pub struct CronRegistry {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    entries: Vec<CronEntry>,
    next_id: u64,
}

impl std::fmt::Debug for CronRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CronRegistry")
            .field("root", &self.root)
            .field(
                "entries",
                &self.state.lock().map(|s| s.entries.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl CronRegistry {
    /// Opens (or creates) a cron registry rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let entries = read_all_jsonl::<CronEntry>(&log_path)?;
        let next_id = entries
            .iter()
            .filter_map(|entry| entry.id.strip_prefix("cron-"))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(State { entries, next_id }),
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
        root.join("cron.jsonl")
    }

    /// Returns the current job count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("cron mutex poisoned")
            .entries
            .len()
    }

    /// Returns true when the registry holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists every cron entry in append order.
    #[must_use]
    pub fn list(&self) -> Vec<CronEntry> {
        self.state
            .lock()
            .expect("cron mutex poisoned")
            .entries
            .clone()
    }

    /// Registers a new cron job. Returns the persisted entry.
    pub fn register(
        &self,
        name: &str,
        prompt: &str,
        schedule: &str,
    ) -> Result<CronEntry, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        if state.entries.len() >= MAX_CRON_JOBS {
            return Err(CheckpointError::RetentionCapReached);
        }
        let id = format!("cron-{}", state.next_id);
        state.next_id = state.next_id.saturating_add(1);
        let entry = CronEntry {
            id,
            name: name.to_string(),
            prompt: prompt.to_string(),
            schedule: schedule.to_string(),
            created_at: SystemTime::now(),
        };
        entry.validate()?;
        let serialized = serde_json::to_string(&entry)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.entries.push(entry.clone());
        Ok(entry)
    }

    /// Removes the entry with the given id. Idempotent.
    pub fn forget(&self, id: &str) -> Result<bool, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let before = state.entries.len();
        state.entries.retain(|entry| entry.id != id);
        let removed = before - state.entries.len();
        if removed == 0 {
            return Ok(false);
        }
        let mut buffer = String::new();
        for entry in &state.entries {
            buffer.push_str(&serde_json::to_string(entry)?);
            buffer.push('\n');
        }
        write_atomic(&Self::log_path(&self.root), buffer.as_bytes())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn register_persists_across_reopen() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let registry = CronRegistry::open(&root).unwrap();
        let entry = registry
            .register("nightly", "run cargo xtask verify", "daily 02:00")
            .unwrap();
        assert_eq!(entry.id, "cron-1");
        let reopened = CronRegistry::open(&root).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.list()[0].name, "nightly");
    }

    #[test]
    fn forget_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let registry = CronRegistry::open(&root).unwrap();
        let entry = registry.register("ephemeral", "noop", "every 1m").unwrap();
        assert!(registry.forget(&entry.id).unwrap());
        assert!(!registry.forget(&entry.id).unwrap());
    }
}
