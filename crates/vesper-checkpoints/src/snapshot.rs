//! Workspace snapshot: copy files into `checkpoints/<id>/` and restore them.
//!
//! This is the heart of the Errno-24 prevention story. Every helper here
//! opens a `File` (or DirBuilder / fs::copy), operates, and drops inside
//! the function body. No `File` is stored in a long-lived struct; the
//! descriptor count therefore cannot grow unbounded no matter how many
//! snapshots are taken.
//!
//! ## Guards
//!
//! Two guards keep snapshots safe and bounded:
//! - **Sensitive-file guard**: refuses `.env`, `credentials.json`,
//!   `id_rsa`, `id_ed25519`, and any `*.key` / `*.pem` / `*.p12` /
//!   `*.pfx`. Mirrors the oracle's `checkpoints._SENSITIVE_*`.
//! - **Ignored-tree guard**: refuses to descend into `.git`, `.venv`,
//!   `venv`, `node_modules`, `dist`, `build`, `__pycache__`, `target`,
//!   `.agent-vesper`. Mirrors the oracle's `checkpoints._IGNORED`.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::CheckpointError;
use crate::types::{
    FileSnapshot, HARD_MAX_CHECKPOINT_SIZE_BYTES, HARD_MAX_FILE_SIZE_BYTES,
    HARD_MAX_FILES_PER_CHECKPOINT, MAX_CHECKPOINT_SIZE_BYTES, MAX_FILE_SIZE_BYTES,
    MAX_FILES_PER_CHECKPOINT,
};

/// Names that trigger the sensitive-file guard.
const SENSITIVE_NAMES: &[&str] = &[".env", "credentials.json", "id_rsa", "id_ed25519"];

/// Suffixes that trigger the sensitive-file guard.
const SENSITIVE_SUFFIXES: &[&str] = &[".key", ".pem", ".p12", ".pfx"];

/// Directory names that the ignored-tree guard refuses to descend into.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    "target",
    ".agent-vesper",
    ".glm-acp",
];

/// Returns true when `path` matches the sensitive-file guard.
fn is_sensitive(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && SENSITIVE_NAMES.contains(&name)
    {
        return true;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let dotted = format!(".{ext}");
        if SENSITIVE_SUFFIXES.contains(&dotted.as_str()) {
            return true;
        }
    }
    // Also catch suffix-style names like `id_rsa.key` (no extension separator).
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_ascii_lowercase();
        if SENSITIVE_SUFFIXES
            .iter()
            .any(|suffix| lower.ends_with(suffix))
        {
            return true;
        }
    }
    // SSH directory.
    if path
        .components()
        .any(|component| component.as_os_str() == ".ssh")
    {
        return true;
    }
    false
}

/// Returns true when `dir_name` matches the ignored-tree guard.
fn is_ignored_dir(dir_name: &str) -> bool {
    IGNORED_DIRS.contains(&dir_name)
}

/// Configuration for a single snapshot operation. The composition boundary
/// supplies the workspace root + the destination directory; the snapshot
/// walks the workspace and copies each eligible file.
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Absolute workspace root. Files outside this root are never copied.
    pub workspace_root: PathBuf,
    /// Absolute destination directory, typically `<root>/checkpoints/<id>/`.
    pub destination: PathBuf,
    /// Per-file size cap (default [`MAX_FILE_SIZE_BYTES`]).
    pub max_file_size_bytes: usize,
    /// Per-checkpoint file count cap (default [`MAX_FILES_PER_CHECKPOINT`]).
    pub max_files_per_checkpoint: usize,
    /// Per-checkpoint total size cap (default [`MAX_CHECKPOINT_SIZE_BYTES`]).
    pub max_checkpoint_size_bytes: usize,
}

impl SnapshotConfig {
    /// Default configuration clamped to the soft bounds. Callers may lower
    /// any field but may not raise past the `HARD_*` constants.
    #[must_use]
    pub fn new(workspace_root: PathBuf, destination: PathBuf) -> Self {
        Self {
            workspace_root,
            destination,
            max_file_size_bytes: MAX_FILE_SIZE_BYTES,
            max_files_per_checkpoint: MAX_FILES_PER_CHECKPOINT,
            max_checkpoint_size_bytes: MAX_CHECKPOINT_SIZE_BYTES,
        }
    }

    /// Validates that every bound is at or below its hard cap and that the
    /// workspace root is absolute.
    pub fn validate(&self) -> Result<(), CheckpointError> {
        if !self.workspace_root.is_absolute() {
            return Err(CheckpointError::InvalidWorkspaceRoot);
        }
        if !self.destination.is_absolute() {
            return Err(CheckpointError::InvalidRoot);
        }
        if self.max_file_size_bytes > HARD_MAX_FILE_SIZE_BYTES {
            return Err(CheckpointError::BoundsViolated("max_file_size_bytes"));
        }
        if self.max_files_per_checkpoint > HARD_MAX_FILES_PER_CHECKPOINT {
            return Err(CheckpointError::BoundsViolated("max_files_per_checkpoint"));
        }
        if self.max_checkpoint_size_bytes > HARD_MAX_CHECKPOINT_SIZE_BYTES {
            return Err(CheckpointError::BoundsViolated("max_checkpoint_size_bytes"));
        }
        Ok(())
    }
}

/// Outcome of one snapshot operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotOutcome {
    /// Metadata for every file that was copied.
    pub files: Vec<FileSnapshot>,
    /// Total bytes copied.
    pub total_bytes: usize,
    /// Number of files skipped because they exceeded the per-file size cap.
    pub skipped_oversized: usize,
    /// Number of files skipped because they matched the sensitive guard.
    pub skipped_sensitive: usize,
    /// Number of files skipped because the per-checkpoint file-count cap
    /// was reached.
    pub skipped_count_cap: usize,
    /// Number of files skipped because the per-checkpoint total-size cap
    /// was reached.
    pub skipped_size_cap: usize,
}

/// Walks the workspace and copies every eligible file into the destination.
/// Returns the captured metadata. The destination directory is created if
/// it does not exist; it is NOT cleared first (callers must arrange that).
pub fn snapshot(config: &SnapshotConfig) -> Result<SnapshotOutcome, CheckpointError> {
    config.validate()?;
    let mut outcome = SnapshotOutcome {
        files: Vec::new(),
        total_bytes: 0,
        skipped_oversized: 0,
        skipped_sensitive: 0,
        skipped_count_cap: 0,
        skipped_size_cap: 0,
    };
    if !config.workspace_root.exists() {
        // An empty workspace is a valid (empty) snapshot.
        return Ok(outcome);
    }
    fs::create_dir_all(&config.destination).map_err(|_| CheckpointError::io("create"))?;
    walk_and_copy(
        &config.workspace_root,
        config,
        &mut outcome,
        &config.workspace_root,
    )?;
    Ok(outcome)
}

/// Recursive walker. `current` is the directory being walked; `root` is
/// the workspace root used to compute relative paths.
fn walk_and_copy(
    current: &Path,
    config: &SnapshotConfig,
    outcome: &mut SnapshotOutcome,
    root: &Path,
) -> Result<(), CheckpointError> {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return Ok(()), // unreadable dir → skip silently
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            // Ignored-tree guard.
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && is_ignored_dir(name)
            {
                continue;
            }
            // Recurse (bounded by the filesystem depth — workspaces are
            // shallow by convention).
            walk_and_copy(&path, config, outcome, root)?;
            continue;
        }
        if file_type.is_file() {
            // Sensitive-file guard.
            if is_sensitive(&path) {
                outcome.skipped_sensitive += 1;
                continue;
            }
            // Per-checkpoint file-count cap.
            if outcome.files.len() >= config.max_files_per_checkpoint {
                outcome.skipped_count_cap += 1;
                continue;
            }
            // Per-file size cap.
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size_bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
            if size_bytes > config.max_file_size_bytes {
                outcome.skipped_oversized += 1;
                continue;
            }
            // Per-checkpoint total-size cap.
            if outcome.total_bytes + size_bytes > config.max_checkpoint_size_bytes {
                outcome.skipped_size_cap += 1;
                continue;
            }
            // Compute the workspace-relative path (forward slashes).
            let relative = match path.strip_prefix(root) {
                Ok(rel) => rel,
                Err(_) => continue, // path escape — should not happen here
            };
            let relative_path = path_to_forward_slashes(relative);
            // Copy the file: scoped open/read, scoped write, both dropped
            // before the loop continues. This is the RAII contract.
            let destination_file = config.destination.join(&relative_path);
            if let Some(parent) = destination_file.parent()
                && fs::create_dir_all(parent).is_err()
            {
                continue;
            }
            let (bytes, sha256) = match read_and_hash(&path) {
                Ok(value) => value,
                Err(_) => continue,
            };
            // Write atomically so a crash mid-copy never leaves a torn file.
            crate::io::write_atomic(&destination_file, &bytes)?;
            outcome.files.push(FileSnapshot {
                relative_path,
                size_bytes,
                sha256,
            });
            outcome.total_bytes += size_bytes;
        }
        // Symlinks and other file types are deliberately skipped.
    }
    Ok(())
}

/// Reads `path` fully into memory and returns `(bytes, sha256_hex)`.
/// Scoped: the `File` is dropped at the closing brace.
fn read_and_hash(path: &Path) -> Result<(Vec<u8>, String), CheckpointError> {
    let mut file = fs::File::open(path).map_err(|_| CheckpointError::io("read"))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|_| CheckpointError::io("read"))?;
    // `file` drops here → descriptor returned to the OS.
    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    let digest = hasher.finalize();
    Ok((buffer, hex_encode(&digest)))
}

/// Restores every file in `files` from `source_dir` back into the workspace
/// at `workspace_root`. Files that are absent in the snapshot are NOT
/// deleted from the workspace (the snapshot is additive-restore, not a
/// full tree diff). Returns the number of files restored.
pub fn restore(
    workspace_root: &Path,
    source_dir: &Path,
    files: &[FileSnapshot],
) -> Result<usize, CheckpointError> {
    if !workspace_root.is_absolute() {
        return Err(CheckpointError::InvalidWorkspaceRoot);
    }
    if !source_dir.is_absolute() {
        return Err(CheckpointError::InvalidRoot);
    }
    let mut restored = 0;
    for file in files {
        // Path-escape guard: the relative path must not escape the workspace.
        let relative = Path::new(&file.relative_path);
        if !is_safe_relative(relative) {
            continue;
        }
        let source_file = source_dir.join(&file.relative_path);
        let destination_file = workspace_root.join(relative);
        // Confinement: the destination must stay inside the workspace root.
        if !destination_file.starts_with(workspace_root) {
            continue;
        }
        let Some(parent) = destination_file.parent() else {
            continue;
        };
        if fs::create_dir_all(parent).is_err() {
            continue;
        }
        // Copy atomically: read the source fully into memory (scoped),
        // then write to the destination via write_atomic (scoped). This
        // avoids holding two descriptors open across the copy.
        let bytes = match fs::read(&source_file) {
            Ok(bytes) => bytes,
            Err(_) => continue, // source missing → skip
        };
        // Optional integrity check: recompute the SHA-256 and bail if it
        // does not match. We skip on mismatch rather than failing the whole
        // restore — a torn checkpoint should not brick the workspace.
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = hex_encode(&hasher.finalize());
        if actual != file.sha256 {
            continue;
        }
        crate::io::write_atomic(&destination_file, &bytes)?;
        restored += 1;
    }
    Ok(restored)
}

/// Returns true when `relative` is a safe relative path (no `..`, no
/// absolute components, no leading `/`).
fn is_safe_relative(relative: &Path) -> bool {
    use std::path::Component;
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => return false,
        }
    }
    !relative.as_os_str().is_empty()
}

/// Removes the entire checkpoint payload directory at `dir`. Idempotent:
/// returns `Ok(())` when the directory is already gone.
pub fn remove_payload(dir: &Path) -> Result<(), CheckpointError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CheckpointError::io("remove")),
    }
}

/// Converts a `Path` to a forward-slash string (POSIX-normalized) so the
/// JSONL record is cross-platform.
fn path_to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Lowercase hex encoder for SHA-256 digests (avoids pulling in `hex`).
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    //! Snapshot: copy, restore, guards, bounds, RAII discipline.

    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn workspace_with_files() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("README.md"), "# project").unwrap();
        fs::write(root.join(".env"), "SECRET=abc").unwrap();
        fs::write(root.join("id_rsa"), "PRIVATE KEY").unwrap();
        fs::write(root.join(".git/config"), "[core]").unwrap();
        fs::write(root.join("target/debug.bin"), b"\0").unwrap();
        (temp, root)
    }

    #[test]
    fn snapshot_copies_regular_files_and_skips_sensitive_and_ignored() {
        let (_temp, workspace) = workspace_with_files();
        let dest_temp = TempDir::new().unwrap();
        let dest = dest_temp.path().join("ckpt-1");
        let config = SnapshotConfig::new(workspace.clone(), dest.clone());
        let outcome = snapshot(&config).unwrap();
        let names: Vec<&str> = outcome
            .files
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert!(names.contains(&"src/main.rs"));
        assert!(names.contains(&"README.md"));
        // Sensitive files skipped.
        assert!(!names.contains(&".env"));
        assert!(!names.contains(&"id_rsa"));
        assert!(outcome.skipped_sensitive >= 2);
        // Ignored dirs not descended into.
        assert!(!names.iter().any(|n| n.starts_with(".git/")));
        assert!(!names.iter().any(|n| n.starts_with("target/")));
    }

    #[test]
    fn restore_round_trips_file_contents_into_the_workspace() {
        let (_temp, workspace) = workspace_with_files();
        let dest_temp = TempDir::new().unwrap();
        let dest = dest_temp.path().join("ckpt-1");
        let config = SnapshotConfig::new(workspace.clone(), dest.clone());
        let outcome = snapshot(&config).unwrap();
        // Mutate the workspace files.
        fs::write(workspace.join("src/main.rs"), "MODIFIED").unwrap();
        fs::write(workspace.join("README.md"), "MODIFIED").unwrap();
        // Restore.
        let restored = restore(&workspace, &dest, &outcome.files).unwrap();
        assert_eq!(restored, outcome.files.len());
        // Contents must match the snapshot.
        assert_eq!(
            fs::read_to_string(workspace.join("src/main.rs")).unwrap(),
            "fn main() {}"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("README.md")).unwrap(),
            "# project"
        );
    }

    #[test]
    fn restore_skips_files_with_mismatched_sha256() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("file.txt"), "original").unwrap();
        let dest = temp.path().join("ckpt-1");
        let config = SnapshotConfig::new(workspace.clone(), dest.clone());
        let outcome = snapshot(&config).unwrap();
        // Tamper with the on-disk snapshot.
        fs::write(dest.join("file.txt"), "TAMPERED").unwrap();
        // Restore must skip the tampered file.
        let restored = restore(&workspace, &dest, &outcome.files).unwrap();
        assert_eq!(restored, 0);
        // Workspace is unchanged by the failed restore.
        assert_eq!(
            fs::read_to_string(workspace.join("file.txt")).unwrap(),
            "original"
        );
    }

    #[test]
    fn snapshot_rejects_path_escape_in_restore() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let dest = temp.path().join("ckpt-1");
        let config = SnapshotConfig::new(workspace.clone(), dest.clone());
        let outcome = snapshot(&config).unwrap();
        // Inject a malicious record with a `..` path.
        let mut evil = outcome.files.clone();
        evil.push(FileSnapshot {
            relative_path: "../escape.txt".into(),
            size_bytes: 1,
            sha256: "00".into(),
        });
        let restored = restore(&workspace, &dest, &evil).unwrap();
        // The legitimate files restored; the escape attempt was skipped.
        assert_eq!(restored, outcome.files.len());
        assert!(!temp.path().join("escape.txt").exists());
    }

    #[test]
    fn bounds_violations_are_rejected() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let dest = temp.path().join("ckpt-1");
        let mut config = SnapshotConfig::new(workspace, dest);
        config.max_file_size_bytes = HARD_MAX_FILE_SIZE_BYTES + 1;
        let err = snapshot(&config).unwrap_err();
        assert_eq!(err, CheckpointError::BoundsViolated("max_file_size_bytes"));
    }

    #[test]
    fn empty_workspace_produces_empty_snapshot() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let dest = temp.path().join("ckpt-1");
        let config = SnapshotConfig::new(workspace, dest);
        let outcome = snapshot(&config).unwrap();
        assert!(outcome.files.is_empty());
        assert_eq!(outcome.total_bytes, 0);
    }

    #[test]
    fn remove_payload_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("ckpt-gone");
        // Removing a non-existent directory is Ok.
        remove_payload(&dir).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), "x").unwrap();
        remove_payload(&dir).unwrap();
        assert!(!dir.exists());
        // Idempotent on the second call.
        remove_payload(&dir).unwrap();
    }

    #[test]
    fn is_sensitive_catches_known_patterns() {
        assert!(is_sensitive(Path::new(".env")));
        assert!(is_sensitive(Path::new("id_rsa")));
        assert!(is_sensitive(Path::new("cert.pem")));
        assert!(is_sensitive(Path::new("private.key")));
        assert!(is_sensitive(Path::new("ssh/id_rsa")));
        assert!(!is_sensitive(Path::new("README.md")));
        assert!(!is_sensitive(Path::new("src/main.rs")));
    }

    #[test]
    fn is_ignored_dir_catches_known_trees() {
        assert!(is_ignored_dir(".git"));
        assert!(is_ignored_dir("target"));
        assert!(is_ignored_dir("node_modules"));
        assert!(!is_ignored_dir("src"));
        assert!(!is_ignored_dir("docs"));
    }
}
