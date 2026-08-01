//! Atomic-write helpers shared by every store in this crate.
//!
//! The pattern mirrors the Stage 6 session writer: write the payload to a
//! sibling temp file inside the target directory, `fsync` it, then rename
//! over the target. POSIX guarantees the rename is atomic because the temp
//! file and the target share one directory and therefore one filesystem.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::error::MemoryError;

/// Writes `payload` to `target` atomically. The temp file is created as a
/// sibling of `target` (so the rename stays intra-directory), `fsync`ed,
/// then renamed. Returns `Ok(())` only after the rename has succeeded.
pub(crate) fn write_atomic(target: &Path, payload: &[u8]) -> Result<(), MemoryError> {
    let parent = target.parent().ok_or(MemoryError::InvalidRoot)?;
    write_atomic_into(parent, target, payload)
}

/// Same as [`write_atomic`] but the caller controls the parent directory
/// (used by the JSONL store when the temp file must live in a specific dir).
fn write_atomic_into(parent: &Path, target: &Path, payload: &[u8]) -> Result<(), MemoryError> {
    let temp = parent.join(format!(
        ".{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("memory")
    ));
    {
        let mut file = File::create(&temp).map_err(|_| MemoryError::io("create"))?;
        file.write_all(payload)
            .map_err(|_| MemoryError::io("write"))?;
        file.sync_all().map_err(|_| MemoryError::io("fsync"))?;
    }
    fs::rename(&temp, target).map_err(|_| MemoryError::io("rename"))?;
    Ok(())
}

/// Appends `line` (followed by `\n`) to the JSONL log at `target`. If the
/// file does not yet exist it is created. Appends are NOT atomic across
/// concurrent writers; the store wraps every mutation in a process-local
/// `Mutex` to serialize them.
pub(crate) fn append_line(target: &Path, line: &str) -> Result<(), MemoryError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(target)
        .map_err(|_| MemoryError::io("open"))?;
    writeln!(file, "{line}").map_err(|_| MemoryError::io("write"))?;
    file.sync_all().map_err(|_| MemoryError::io("fsync"))?;
    Ok(())
}

/// Reads and parses every JSONL line from `target`. Lines that fail to
/// parse are skipped (treated as torn writes from a previous crash), which
/// keeps the log self-healing under partial writes.
pub(crate) fn read_all_jsonl<T: serde::de::DeserializeOwned>(
    target: &Path,
) -> Result<Vec<T>, MemoryError> {
    let bytes = match fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(MemoryError::io("read")),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| MemoryError::Serde)?;
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
/// vector when the file does not exist (used by the user-profile store,
/// which treats absence as "empty profile").
pub(crate) fn read_all_lines(target: &Path) -> Result<Vec<String>, MemoryError> {
    let bytes = match fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(MemoryError::io("read")),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| MemoryError::Serde)?;
    Ok(text.lines().map(String::from).collect())
}
