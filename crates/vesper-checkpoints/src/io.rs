//! Atomic-write helpers and JSONL reader shared by every store in this crate.
//!
//! The pattern mirrors `vesper-memory` and `vesper-sessions`: write the
//! payload to a sibling temp file inside the target directory, `fsync` it,
//! then rename over the target. POSIX guarantees the rename is atomic
//! because the temp file and the target share one directory and therefore
//! one filesystem.
//!
//! ## RAII discipline (Errno 24 prevention)
//!
//! The original Python oracle suffered an uncontrolled file-descriptor leak
//! because unmanaged SQLite transactions left `.wal` / `.shm` files hanging.
//! This crate bypasses SQLite entirely. Every helper here opens a `File`,
//! operates, and drops inside the function body — no `File` is ever stored
//! in a long-lived struct. When the function returns, Rust's `Drop` runs
//! immediately, the descriptor is returned to the OS, and the descriptor
//! count never grows unbounded.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::error::CheckpointError;

/// Writes `payload` to `target` atomically. The temp file is created as a
/// sibling of `target` (so the rename stays intra-directory), `fsync`ed,
/// then renamed. Returns `Ok(())` only after the rename has succeeded.
pub(crate) fn write_atomic(target: &Path, payload: &[u8]) -> Result<(), CheckpointError> {
    let parent = target.parent().ok_or(CheckpointError::InvalidRoot)?;
    let temp = parent.join(format!(
        ".{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("checkpoint")
    ));
    {
        // Scoped: the `File` is dropped at the closing brace, freeing the
        // descriptor before the rename. This is the RAII contract that
        // prevents the Errno 24 leak.
        let mut file = File::create(&temp).map_err(|_| CheckpointError::io("create"))?;
        file.write_all(payload)
            .map_err(|_| CheckpointError::io("write"))?;
        file.sync_all().map_err(|_| CheckpointError::io("fsync"))?;
    }
    fs::rename(&temp, target).map_err(|_| CheckpointError::io("rename"))?;
    Ok(())
}

/// Appends `line` (followed by `\n`) to the JSONL log at `target`. If the
/// file does not yet exist it is created. Appends are NOT atomic across
/// concurrent writers; the store wraps every mutation in a process-local
/// `Mutex` to serialize them.
pub(crate) fn append_line(target: &Path, line: &str) -> Result<(), CheckpointError> {
    use std::io::Write;
    // Scoped: the `File` is dropped at the closing brace.
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(target)
        .map_err(|_| CheckpointError::io("open"))?;
    writeln!(file, "{line}").map_err(|_| CheckpointError::io("write"))?;
    file.sync_all().map_err(|_| CheckpointError::io("fsync"))?;
    Ok(())
}

/// Reads and parses every JSONL line from `target`. Lines that fail to
/// parse are skipped (treated as torn writes from a previous crash), which
/// keeps the log self-healing under partial writes.
pub(crate) fn read_all_jsonl<T: serde::de::DeserializeOwned>(
    target: &Path,
) -> Result<Vec<T>, CheckpointError> {
    let bytes = match fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(CheckpointError::io("read")),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| CheckpointError::Serde)?;
    let mut records = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<T>(line) {
            records.push(record);
        }
        // Torn / unparseable lines are deliberately skipped so a crashed
        // append never poisons the log.
    }
    Ok(records)
}

/// Reads every line from `target` as an owned `String`. Returns an empty
/// vector when the file does not exist.
#[allow(dead_code)] // reserved for future ad-hoc text reads
pub(crate) fn read_all_lines(target: &Path) -> Result<Vec<String>, CheckpointError> {
    let bytes = match fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(CheckpointError::io("read")),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| CheckpointError::Serde)?;
    Ok(text.lines().map(String::from).collect())
}
