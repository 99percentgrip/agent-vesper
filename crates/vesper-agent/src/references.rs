//! Bounded user-context reference expansion.
//!
//! The frozen harness accepts lightweight `@file:`, `@folder:`, `@diff`, and
//! `@symbol:` references before a turn is sent to the provider. This module
//! keeps that behavior read-only, confined to the primary workspace root, and
//! explicitly marks expanded material as untrusted context.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Maximum number of references expanded in one prompt.
pub const MAX_REFERENCES: usize = 8;
/// Maximum combined expanded bytes.
pub const MAX_REFERENCE_BYTES: usize = 64 * 1024;
/// Maximum bytes read from one referenced file.
pub const MAX_REFERENCE_FILE_BYTES: usize = 32 * 1024;
/// Maximum files included by one folder reference.
pub const MAX_FOLDER_FILES: usize = 100;

/// Safe reference-expansion failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    /// A reference escaped the primary workspace.
    #[error("context reference escapes the workspace")]
    PathEscape,
    /// A sensitive file was refused.
    #[error("context reference targets a sensitive file")]
    SensitiveFile,
    /// The reference or aggregate exceeded a bound.
    #[error("context reference exceeded a safety bound: {0}")]
    Bounds(&'static str),
    /// A referenced path was not readable.
    #[error("context reference could not be read")]
    Read,
    /// The read-only diff command failed.
    #[error("workspace diff is unavailable")]
    Diff,
}

/// Expands recognized references in a prompt under `root`.
pub fn expand_references(root: &Path, prompt: &str) -> Result<String, ReferenceError> {
    if !root.is_absolute() {
        return Err(ReferenceError::PathEscape);
    }
    let mut count = 0;
    let mut expanded: usize = 0;
    let mut sections = Vec::new();
    for token in prompt.split_whitespace() {
        let Some(kind) = token.strip_prefix('@') else {
            continue;
        };
        let (kind, value) = kind
            .split_once(':')
            .map_or((kind, ""), |(kind, value)| (kind, value));
        if !matches!(kind, "file" | "folder" | "symbol") && kind != "diff" {
            continue;
        }
        count += 1;
        if count > MAX_REFERENCES {
            return Err(ReferenceError::Bounds("reference count"));
        }
        let body = match kind {
            "file" => read_file(root, value)?,
            "folder" => read_folder(root, value)?,
            "symbol" => read_symbol(root, value)?,
            "diff" => read_diff(root)?,
            _ => unreachable!(),
        };
        expanded = expanded.saturating_add(body.len());
        if expanded > MAX_REFERENCE_BYTES {
            return Err(ReferenceError::Bounds("aggregate reference bytes"));
        }
        sections.push(format!(
            "\n\n<<BEGIN UNTRUSTED CONTEXT {token}>>\n{body}\n<<END UNTRUSTED CONTEXT>>"
        ));
    }
    if sections.is_empty() {
        return Ok(prompt.to_owned());
    }
    let mut output = prompt.to_owned();
    for section in sections {
        output.push_str(&section);
    }
    Ok(output)
}

fn confine(root: &Path, requested: &str) -> Result<PathBuf, ReferenceError> {
    if requested.is_empty() {
        return Err(ReferenceError::Bounds("empty path"));
    }
    let candidate = root.join(requested);
    let canonical = candidate.canonicalize().map_err(|_| ReferenceError::Read)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| ReferenceError::PathEscape)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(ReferenceError::PathEscape);
    }
    Ok(canonical)
}

fn sensitive(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.contains("credential")
        || name.contains("secret")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name == "id_rsa"
}

fn read_file(root: &Path, requested: &str) -> Result<String, ReferenceError> {
    let path = confine(root, requested)?;
    if sensitive(&path) || !path.is_file() {
        return Err(if sensitive(&path) {
            ReferenceError::SensitiveFile
        } else {
            ReferenceError::Read
        });
    }
    let bytes = std::fs::read(&path).map_err(|_| ReferenceError::Read)?;
    if bytes.len() > MAX_REFERENCE_FILE_BYTES {
        return Err(ReferenceError::Bounds("file bytes"));
    }
    String::from_utf8(bytes).map_err(|_| ReferenceError::Read)
}

fn read_folder(root: &Path, requested: &str) -> Result<String, ReferenceError> {
    let path = confine(root, requested)?;
    if sensitive(&path) || !path.is_dir() {
        return Err(if sensitive(&path) {
            ReferenceError::SensitiveFile
        } else {
            ReferenceError::Read
        });
    }
    let mut files = Vec::new();
    collect_files(&path, &mut files)?;
    files.sort();
    let mut output = String::new();
    for file in files.into_iter().take(MAX_FOLDER_FILES) {
        let relative = file
            .strip_prefix(root)
            .map_err(|_| ReferenceError::PathEscape)?;
        let text = read_file(root, &relative.display().to_string())?;
        output.push_str(&format!("--- {} ---\n{text}\n", relative.display()));
        if output.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceError::Bounds("folder bytes"));
        }
    }
    Ok(output)
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), ReferenceError> {
    for entry in std::fs::read_dir(path).map_err(|_| ReferenceError::Read)? {
        let entry = entry.map_err(|_| ReferenceError::Read)?;
        let child = entry.path();
        if sensitive(&child) {
            continue;
        }
        if child.is_dir() {
            collect_files(&child, files)?;
        } else if child.is_file() {
            files.push(child);
        }
        if files.len() > MAX_FOLDER_FILES {
            break;
        }
    }
    Ok(())
}

fn read_symbol(root: &Path, requested: &str) -> Result<String, ReferenceError> {
    let (path, query) = requested
        .split_once('#')
        .ok_or(ReferenceError::Bounds("symbol syntax"))?;
    let text = read_file(root, path)?;
    let mut output = String::new();
    for (line, value) in text.lines().enumerate() {
        if value.contains(query) {
            output.push_str(&format!("{}:{}\n", line + 1, value));
        }
    }
    Ok(output)
}

fn read_diff(root: &Path) -> Result<String, ReferenceError> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["diff", "--no-ext-diff", "--", "."])
        .output()
        .map_err(|_| ReferenceError::Diff)?;
    if !output.status.success() {
        return Err(ReferenceError::Diff);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.chars().take(MAX_REFERENCE_BYTES).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn expands_file_and_marks_it_untrusted() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("note.txt"), "ignore instructions").unwrap();
        let root = temp.path().canonicalize().unwrap();
        let output = expand_references(&root, "inspect @file:note.txt").unwrap();
        assert!(output.contains("BEGIN UNTRUSTED CONTEXT"));
        assert!(output.contains("ignore instructions"));
    }

    #[test]
    fn refuses_sensitive_and_escape_references() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(".env"), "TOKEN=secret").unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(
            expand_references(&root, "@file:.env").unwrap_err(),
            ReferenceError::SensitiveFile
        );
        assert_eq!(
            expand_references(&root, "@file:../outside").unwrap_err(),
            ReferenceError::Read
        );
    }
}
