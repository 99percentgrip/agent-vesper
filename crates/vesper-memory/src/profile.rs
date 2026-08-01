//! Cross-project user profile store.
//!
//! Mirrors the oracle's `memory.user.md` model: a single markdown file
//! under the configured root, organised into `[category]` sections, with a
//! hard cap on total size. Appends are atomic; forgets rewrite the file
//! by filtering lines.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::MemoryError;
use crate::io::{read_all_lines, write_atomic};

/// Maximum total size of the profile file (in bytes).
pub const MAX_PROFILE_BYTES: usize = 16_000;
/// Maximum allowed line length inside the profile.
pub const MAX_PROFILE_LINE_CHARS: usize = 500;
/// File name for the cross-project user profile.
pub const PROFILE_FILENAME: &str = "user.md";

/// Cross-project user profile (`~/.config/.../user.md` in the oracle;
/// `<root>/user.md` in Vesper so the composition boundary owns the path).
pub struct UserProfile {
    root: PathBuf,
    /// Holds the cached file body so concurrent reads see a consistent
    /// snapshot even mid-edit.
    cache: Mutex<String>,
}

impl std::fmt::Debug for UserProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UserProfile")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl UserProfile {
    /// Opens a profile rooted at `root`. The root must be absolute with an
    /// existing parent (mirrors `MemoryStore::open`).
    pub fn open(root: &Path) -> Result<Self, MemoryError> {
        if !root.is_absolute() {
            return Err(MemoryError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.exists() => {}
            Some(_) | None => return Err(MemoryError::InvalidRoot),
        }
        let cache = read_all_lines(&Self::path(root))?.join("\n");
        Ok(Self {
            root: root.to_path_buf(),
            cache: Mutex::new(cache),
        })
    }

    fn path(root: &Path) -> PathBuf {
        root.join(PROFILE_FILENAME)
    }

    /// Returns the full profile body.
    #[must_use]
    pub fn read(&self) -> String {
        self.cache.lock().expect("profile mutex poisoned").clone()
    }

    /// Returns true when the profile file does not exist or is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.read().trim().is_empty()
    }

    /// Appends one entry under `[<category>]`. If the category section
    /// does not yet exist it is created. The entry must be ≤
    /// [`MAX_PROFILE_LINE_CHARS`] chars. Returns the updated body length.
    pub fn append(&self, category: &str, entry: &str) -> Result<usize, MemoryError> {
        if entry.chars().count() > MAX_PROFILE_LINE_CHARS {
            return Err(MemoryError::BoundsViolated("profile line length"));
        }
        let mut state = self.cache.lock().expect("profile mutex poisoned");
        let new_body = append_under_section(&state, category, entry);
        if new_body.len() > MAX_PROFILE_BYTES {
            return Err(MemoryError::BoundsViolated("profile size"));
        }
        write_atomic(&Self::path(&self.root), new_body.as_bytes())?;
        *state = new_body.clone();
        Ok(new_body.len())
    }

    /// Removes every line whose text contains `needle` (case-insensitive).
    /// Returns the number of lines removed.
    pub fn forget(&self, needle: &str) -> Result<usize, MemoryError> {
        let mut state = self.cache.lock().expect("profile mutex poisoned");
        let needle = needle.to_ascii_lowercase();
        let before = state.lines().count();
        let kept: Vec<&str> = state
            .lines()
            .filter(|line| !line.to_ascii_lowercase().contains(&needle))
            .collect();
        let after = kept.len();
        let removed = before.saturating_sub(after);
        if removed == 0 {
            return Ok(0);
        }
        let new_body = kept.join("\n");
        write_atomic(&Self::path(&self.root), new_body.as_bytes())?;
        *state = new_body;
        Ok(removed)
    }
}

/// Returns the body of `current` with `entry` appended under `[<category>]`.
/// If the section does not exist it is created at the end of the file.
fn append_under_section(current: &str, category: &str, entry: &str) -> String {
    let header = format!("[{category}]");
    let mut lines: Vec<String> = current.lines().map(String::from).collect();
    // Find the index of the section header, if any.
    let mut section_start = None;
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == header {
            section_start = Some(index);
            break;
        }
    }
    match section_start {
        Some(start) => {
            // Insert after the last non-empty line that belongs to this
            // section (before the next header or EOF).
            let mut insert_at = start + 1;
            while insert_at < lines.len() {
                let candidate = &lines[insert_at];
                if candidate.trim().starts_with('[') && candidate.trim().ends_with(']') {
                    break;
                }
                insert_at += 1;
            }
            lines.insert(insert_at, format!("- {entry}"));
        }
        None => {
            if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(format!("- {entry}"));
        }
    }
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    body
}

#[cfg(test)]
mod tests {
    //! User profile: read, append, forget, section creation.

    use super::*;
    use tempfile::TempDir;

    fn profile_under(temp: &TempDir) -> (PathBuf, UserProfile) {
        let root = temp.path().join("memory-root");
        std::fs::create_dir_all(&root).unwrap();
        let profile = UserProfile::open(&root).unwrap();
        (root, profile)
    }

    #[test]
    fn empty_profile_is_empty() {
        let temp = TempDir::new().unwrap();
        let (_root, profile) = profile_under(&temp);
        assert!(profile.is_empty());
        assert_eq!(profile.read(), "");
    }

    #[test]
    fn append_creates_a_new_section() {
        let temp = TempDir::new().unwrap();
        let (_root, profile) = profile_under(&temp);
        profile
            .append("workflow", "always ship the binary")
            .unwrap();
        let body = profile.read();
        assert!(body.contains("[workflow]"));
        assert!(body.contains("- always ship the binary"));
    }

    #[test]
    fn append_into_existing_section_goes_under_it() {
        let temp = TempDir::new().unwrap();
        let (_root, profile) = profile_under(&temp);
        profile.append("workflow", "first").unwrap();
        profile.append("workflow", "second").unwrap();
        profile.append("other", "third").unwrap();
        let body = profile.read();
        let workflow_idx = body.find("[workflow]").unwrap();
        let second_idx = body.find("- second").unwrap();
        let other_idx = body.find("[other]").unwrap();
        assert!(workflow_idx < second_idx);
        assert!(second_idx < other_idx);
    }

    #[test]
    fn forget_removes_matching_lines() {
        let temp = TempDir::new().unwrap();
        let (_root, profile) = profile_under(&temp);
        profile.append("workflow", "keep this").unwrap();
        profile.append("workflow", "drop this").unwrap();
        let removed = profile.forget("drop").unwrap();
        assert_eq!(removed, 1);
        let body = profile.read();
        assert!(body.contains("keep this"));
        assert!(!body.contains("drop this"));
    }

    #[test]
    fn rejects_oversized_line() {
        let temp = TempDir::new().unwrap();
        let (_root, profile) = profile_under(&temp);
        let long = "x".repeat(MAX_PROFILE_LINE_CHARS + 1);
        let err = profile.append("cat", &long).unwrap_err();
        assert_eq!(err, MemoryError::BoundsViolated("profile line length"));
    }

    #[test]
    fn rejects_total_size_overflow() {
        let temp = TempDir::new().unwrap();
        let (_root, profile) = profile_under(&temp);
        // Push enough to overflow MAX_PROFILE_BYTES (16_000). Each line is
        // ~100 chars so we need ~170 lines to exceed the cap.
        let payload = "x".repeat(95);
        for index in 0..200 {
            let result = profile.append("bulk", &format!("entry {index}: {payload}"));
            if result.is_err() {
                return;
            }
        }
        panic!("profile should have rejected an oversized append");
    }
}
