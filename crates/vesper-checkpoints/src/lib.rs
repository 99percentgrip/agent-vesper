#![forbid(unsafe_code)]
//! Workspace snapshot, rollback, session lineage, and bounded
//! cron / export / clipboard / CI surface for Agent Vesper
//! (ADR 0012 — Stage 14).
//!
//! This crate backs the Tier C Phase 9 un-stubbed TUI commands:
//! `/sessions-new`, `/sessions`, `/lineage`, `/branch`, `/rename`,
//! `/checkpoint`, `/rollback`, `/rewind`, `/undo`, `/loop`, `/export`,
//! `/copy`, `/ci`. It mirrors the Python oracle's
//! `glm_acp/checkpoints.py` + `cron.py` data models, adapted to Rust's
//! RAII discipline so the Errno-24 file-descriptor leak that plagued the
//! Python oracle (unmanaged SQLite transactions leaving `.wal`/`.shm`
//! files hanging) cannot recur.
//!
//! ## Storage layout
//!
//! All artefacts live under one configurable root directory (the
//! composition boundary chooses the path; this crate never creates the
//! root itself):
//!
//! - `checkpoints.jsonl` — append-only [`CheckpointRecord`] log.
//! - `checkpoints/<id>/` — payload directory holding the copied files
//!   for checkpoint `<id>`. Unlinked by [`CheckpointsLedger::prune`]
//!   when [`MAX_RETENTION_COUNT`] is exceeded.
//! - `sessions.jsonl` — append-only [`SessionRecord`] lineage log.
//! - `cron.jsonl` — append-only [`CronEntry`] registry.
//! - `exports/<timestamp>.md` — bounded markdown session exports.
//! - `clipboard.log` — append-only log of clipboard targets.
//!
//! All writes are atomic (write-to-temp + `fsync` + rename), confined to
//! the absolute root, and bounded by configured byte limits — the same
//! discipline as the Stage 6 session writer and the Stage 12 memory
//! writer.
//!
//! ## RAII discipline (Errno 24 prevention)
//!
//! The crucial historical mandate: every `File` opened in this crate is
//! scoped to a function body and dropped at the closing brace. No `File`
//! is stored in a long-lived struct; there are no `lazy_static` /
//! `once_cell` file handles; there are no background loops holding
//! descriptors. The OS reclaims every descriptor the moment its scope
//! exits, regardless of how many snapshots are taken.
//!
//! ## Architecture
//!
//! Depends only on `vesper-domain` and `vesper-security`. No provider,
//! runtime, ACP, sessions, agent, testkit, SQLite, HTTP, or TUI
//! dependency. The CI reader shells out to `gh` (a bounded subprocess
//! invocation, never a direct API call).

pub mod ci;
pub mod clipboard;
pub mod cron;
pub mod error;
pub mod export;
pub mod io;
pub mod ledger;
pub mod sessions;
pub mod snapshot;
pub mod types;

pub use ci::{CiStatus, CiStatusReader};
pub use clipboard::{ClipboardOutcome, ClipboardPort, MAX_CLIPBOARD_BYTES};
pub use cron::{CronRegistry, CronRun};
pub use error::CheckpointError;
pub use export::{MAX_EXPORT_BYTES, SessionExporter};
pub use ledger::CheckpointsLedger;
pub use sessions::SessionLineage;
pub use snapshot::{SnapshotConfig, SnapshotOutcome};
pub use types::{
    CRON_CLAIM_TTL_SECONDS, CheckpointKind, CheckpointRecord, CronClaim, CronEntry, FileSnapshot,
    HARD_MAX_CHECKPOINT_SIZE_BYTES, HARD_MAX_FILE_SIZE_BYTES, HARD_MAX_FILES_PER_CHECKPOINT,
    HARD_MAX_RETENTION_COUNT, MAX_CHECKPOINT_SIZE_BYTES, MAX_CRON_JOBS, MAX_CRON_OUTPUT_CHARS,
    MAX_CRON_PROMPT_CHARS, MAX_FILE_SIZE_BYTES, MAX_FILES_PER_CHECKPOINT, MAX_LABEL_CHARS,
    MAX_LINEAGE_DEPTH, MAX_RETENTION_COUNT, SessionRecord, SessionStatus,
};
