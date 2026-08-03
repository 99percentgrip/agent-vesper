//! Value types and bound constants for the checkpoints subsystem.
//!
//! Bounds mirror the oracle's `checkpoints.py` defaults where practical:
//! `DEFAULT_MAX_FILE_MIB = 25`, `HARD_PROJECT_HISTORY = 100`,
//! `DEFAULT_AUTO_CHECKPOINT = False` (we go further: auto-checkpointing is
//! not implemented at all — every checkpoint is explicit).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::CheckpointError;

// === Bounds (defaults; the composition boundary may lower them) ===

/// Maximum size of any single file that a snapshot will copy.
/// Default 1 MiB (oracle `DEFAULT_MAX_FILE_MIB = 25` is the hard cap; we
/// use a conservative default that the binary can raise).
pub const MAX_FILE_SIZE_BYTES: usize = 1024 * 1024;
/// Hard cap on the per-file size — no caller may raise a limit past this.
pub const HARD_MAX_FILE_SIZE_BYTES: usize = 25 * 1024 * 1024;

/// Maximum number of files a single checkpoint may capture.
pub const MAX_FILES_PER_CHECKPOINT: usize = 1000;
/// Hard cap on per-checkpoint file count.
pub const HARD_MAX_FILES_PER_CHECKPOINT: usize = 20_000;

/// Maximum total payload size of a single checkpoint (sum of copied files).
pub const MAX_CHECKPOINT_SIZE_BYTES: usize = 10 * 1024 * 1024;
/// Hard cap on per-checkpoint total payload.
pub const HARD_MAX_CHECKPOINT_SIZE_BYTES: usize = 250 * 1024 * 1024;

/// Maximum number of checkpoints kept on disk before pruning kicks in.
/// The oracle's `HARD_PROJECT_HISTORY = 100`; we default lower (50) so
/// aggressive pruning keeps storage bounded even on long-running projects.
pub const MAX_RETENTION_COUNT: usize = 50;
/// Hard cap on retention — no caller may raise past this.
pub const HARD_MAX_RETENTION_COUNT: usize = 100;

/// Maximum depth of a session lineage chain (parent → child → ... ).
pub const MAX_LINEAGE_DEPTH: usize = 100;

/// Maximum number of cron jobs the registry will hold.
pub const MAX_CRON_JOBS: usize = 500;

/// Maximum characters in a checkpoint or session label / id.
pub const MAX_LABEL_CHARS: usize = 120;
/// Maximum characters in a cron prompt.
pub const MAX_CRON_PROMPT_CHARS: usize = 32_000;
/// Maximum retained scheduler output per run.
pub const MAX_CRON_OUTPUT_CHARS: usize = 64_000;
/// Claim lease duration. Expired claims are recoverable by the next tick.
pub const CRON_CLAIM_TTL_SECONDS: u64 = 1_800;

/// One row of the workspace snapshot: a single captured file's metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    /// Workspace-relative path using forward slashes (POSIX-normalized).
    pub relative_path: String,
    /// Captured file size in bytes.
    pub size_bytes: usize,
    /// Lowercase hex SHA-256 of the captured bytes (for verification).
    pub sha256: String,
}

/// Why a checkpoint was created. `Auto` is reserved for future harness
/// transitions; the TUI never emits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointKind {
    /// Explicit `/checkpoint` from the driver.
    Manual,
    /// Harness-created on a major session transition (not yet wired).
    Auto,
    /// Created by `/branch` to mark a fork point.
    Branchpoint,
    /// Created by `/undo` to mark the restore target.
    Undo,
}

/// One row of the JSONL checkpoint ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    /// Stable opaque id, e.g. `ckpt-42`.
    pub id: String,
    /// Owning session id (links to `SessionRecord`).
    pub session_id: String,
    /// Parent checkpoint id in the lineage chain (None for the first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Why this checkpoint exists.
    pub kind: CheckpointKind,
    /// Optional human-readable label (the `/checkpoint <label>` argument).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Captured file metadata (paths + sizes + hashes; the bytes live on
    /// disk under `<root>/checkpoints/<id>/`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileSnapshot>,
    /// Total bytes captured.
    #[serde(default)]
    pub total_bytes: usize,
    /// Creation timestamp.
    pub created_at: SystemTime,
}

impl CheckpointRecord {
    /// Validates the record against the bounded contract.
    pub fn validate(&self) -> Result<(), CheckpointError> {
        if self.id.len() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("id length"));
        }
        if self.session_id.len() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("session_id length"));
        }
        if self.files.len() > HARD_MAX_FILES_PER_CHECKPOINT {
            return Err(CheckpointError::BoundsViolated("file count"));
        }
        if self.total_bytes > HARD_MAX_CHECKPOINT_SIZE_BYTES {
            return Err(CheckpointError::BoundsViolated("total bytes"));
        }
        if let Some(label) = &self.label
            && label.chars().count() > MAX_LABEL_CHARS
        {
            return Err(CheckpointError::BoundsViolated("label length"));
        }
        Ok(())
    }
}

/// Status of a session in the lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Active session (most recent in its chain).
    Active,
    /// Superseded by a child session.
    Superseded,
    /// Closed by the driver.
    Closed,
}

/// One row of the JSONL session lineage log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Stable opaque id, e.g. `sess-7`.
    pub id: String,
    /// Parent session id (None for the root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Branch label / human-readable name (defaults to the id).
    pub name: String,
    /// Workspace root the session was created against (absolute, stored as
    /// a string so it round-trips through JSON).
    pub workspace_root: String,
    pub status: SessionStatus,
    /// Creation timestamp.
    pub created_at: SystemTime,
    /// Last-update timestamp (renames, branch operations, status changes).
    pub updated_at: SystemTime,
}

impl SessionRecord {
    /// Validates the record against the bounded contract.
    pub fn validate(&self) -> Result<(), CheckpointError> {
        if self.id.len() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("id length"));
        }
        if self.name.chars().count() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("name length"));
        }
        if !std::path::Path::new(&self.workspace_root).is_absolute() {
            return Err(CheckpointError::InvalidWorkspaceRoot);
        }
        Ok(())
    }
}

/// One row of the JSONL cron registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronEntry {
    /// Stable opaque id, e.g. `cron-3`.
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Prompt to run on each scheduled tick.
    pub prompt: String,
    /// Schedule expression in the oracle's bounded format (`every 30m`,
    /// `every 1h`, `daily 09:00`, or `hourly`).
    pub schedule: String,
    /// ISO-8601 timestamp of the last registered update.
    pub created_at: SystemTime,
    /// Whether a scheduler may dispatch this job. Defaults to enabled when
    /// reading older JSONL records.
    #[serde(default = "default_cron_enabled")]
    pub enabled: bool,
    /// Next eligible execution time. Older registry rows may omit this and
    /// are treated as immediately due on their first scheduler tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<SystemTime>,
    /// Cross-process claim lease owned by one runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<CronClaim>,
    /// Number of completed attempts.
    #[serde(default)]
    pub run_count: u64,
    /// Last bounded run status (`ok`, `error`, `cancelled`, or `silent`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// Last bounded output/error projection; full artifacts remain host-owned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output: Option<String>,
}

/// One bounded scheduler claim lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronClaim {
    pub token: String,
    pub claimed_at: SystemTime,
    pub expires_at: SystemTime,
}

fn default_cron_enabled() -> bool {
    true
}

impl CronEntry {
    /// Validates the entry against the bounded contract.
    pub fn validate(&self) -> Result<(), CheckpointError> {
        if self.id.len() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("id length"));
        }
        if self.name.chars().count() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("name length"));
        }
        if self.prompt.chars().count() > MAX_CRON_PROMPT_CHARS {
            return Err(CheckpointError::BoundsViolated("prompt length"));
        }
        if self.schedule.chars().count() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("schedule length"));
        }
        if let Some(output) = &self.last_output
            && output.chars().count() > MAX_CRON_OUTPUT_CHARS
        {
            return Err(CheckpointError::BoundsViolated("cron output length"));
        }
        Ok(())
    }
}
