//! Path confinement enforcement (ADR 0010, Tier C Phase 3/4 security core).
//!
//! `vesper-security` deliberately ships only authority *descriptors*
//! (`RootIdentity`, `RelativePath`, `PathCapability`); complete filesystem
//! enforcement belongs to the executor layer. This module owns that
//! enforcement for the real tool executors.
//!
//! [`confine`] resolves a model-supplied path against a workspace root and
//! verifies the resolved path stays inside the root. It canonicalizes (following
//! symlinks) so a symlink that points outside the root is rejected, and for
//! not-yet-existing write targets it canonicalizes the parent and re-checks.

use std::io;
use std::path::{Path, PathBuf};

/// Why a requested path was rejected by confinement.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfinementError {
    /// The path was blank or malformed.
    #[error("invalid path argument: {0}")]
    InvalidPath(String),
    /// The resolved path escapes every authorized workspace root.
    #[error("path escapes the workspace root: {0}")]
    Escape(PathBuf),
    /// The workspace root itself could not be canonicalized.
    #[error("workspace root is not accessible: {0}")]
    RootNotAccessible(String),
}

/// Confinement failures surface to executors as a bounded `ToolError::Failed`
/// so the agent loop can feed them back to the model.
impl From<ConfinementError> for crate::executor::ToolError {
    fn from(error: ConfinementError) -> Self {
        crate::executor::ToolError::Failed(error.to_string())
    }
}

/// Resolves `requested` against `root`, following symlinks, and verifies the
/// result stays inside `root`.
///
/// - Relative `requested` paths are joined to `root`; absolute paths are used
///   verbatim but still must resolve inside `root`.
/// - Existing paths are canonicalized (symlinks followed); a symlink that
///   resolves outside `root` is rejected.
/// - Not-yet-existing targets (typical for `write_file`) canonicalize the
///   parent directory (which must exist) and re-append the file name; the
///   parent must be inside `root`.
pub fn confine(root: &Path, requested: &str) -> Result<PathBuf, ConfinementError> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err(ConfinementError::InvalidPath(
            "path must not be empty".into(),
        ));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| ConfinementError::RootNotAccessible(error.to_string()))?;
    let requested_path = Path::new(trimmed);
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        canonical_root.join(requested_path)
    };
    let resolved = resolve_within(&candidate)?;
    if !resolved.starts_with(&canonical_root) {
        return Err(ConfinementError::Escape(resolved));
    }
    Ok(resolved)
}

/// Canonicalizes `candidate`, falling back to ancestor-canonicalization for
/// not-yet-existing write targets. Symlinks are followed either way. Walks up
/// to the longest existing ancestor so deep non-existent paths (e.g.
/// `new/nested/file.txt`) confine correctly; the confine-level prefix check
/// still rejects any `..`/symlink escape.
fn resolve_within(candidate: &Path) -> Result<PathBuf, ConfinementError> {
    if let Ok(canonical) = candidate.canonicalize() {
        return Ok(canonical);
    }
    // Walk up to the longest existing ancestor, collecting the non-existent
    // tail in child-first order.
    let mut existing = candidate.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        let name = existing
            .file_name()
            .ok_or_else(|| ConfinementError::InvalidPath("path has no resolvable ancestor".into()))?
            .to_os_string();
        tail.push(name);
        let parent = existing
            .parent()
            .ok_or_else(|| ConfinementError::InvalidPath("path has no parent component".into()))?
            .to_path_buf();
        if parent.as_os_str().is_empty() {
            return Err(ConfinementError::InvalidPath(
                "path has no existing ancestor".into(),
            ));
        }
        existing = parent;
    }
    let mut resolved = existing.canonicalize().map_err(|error| {
        ConfinementError::InvalidPath(format!("ancestor not accessible: {error}"))
    })?;
    for name in tail.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// Extracts a required string argument `key` from a JSON object.
pub(crate) fn string_arg(
    arguments: &serde_json::Value,
    key: &str,
) -> Result<String, crate::executor::ToolError> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| crate::executor::ToolError::InvalidArguments {
            tool: String::new(),
            reason: format!("missing string argument `{key}`"),
        })
}

/// Extracts an optional string argument `key` from a JSON object.
pub(crate) fn optional_string_arg(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// Extracts an optional integer argument `key` from a JSON object.
pub(crate) fn optional_u64_arg(arguments: &serde_json::Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(|value| value.as_u64())
}

/// Picks the primary workspace root from a tool context, or errors.
pub(crate) fn primary_root(
    context: &crate::executor::ToolContext,
) -> Result<&Path, crate::executor::ToolError> {
    context
        .workspace_roots
        .first()
        .map(|root| std::path::Path::new(root.path.as_str()))
        .ok_or_else(|| {
            crate::executor::ToolError::Failed(
                "no workspace root is configured for this tool".into(),
            )
        })
}

/// Maps an io::Error to a bounded ToolError::Failed string.
pub(crate) fn io_failure(operation: &str, error: io::Error) -> crate::executor::ToolError {
    crate::executor::ToolError::Failed(format!("{operation} failed: {error}"))
}

#[cfg(test)]
mod tests {
    //! Confinement rejects escapes and accepts in-root paths. Uses a temp dir
    //! so no real filesystem outside the test root is touched.

    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn accepts_relative_paths_inside_the_root() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("inside.txt"), "hi").unwrap();
        let resolved = confine(root.path(), "inside.txt").unwrap();
        assert!(resolved.starts_with(root.path()));
    }

    #[test]
    fn accepts_not_yet_existing_write_targets() {
        let root = tempdir().unwrap();
        let resolved = confine(root.path(), "new/nested/file.txt").unwrap();
        assert!(resolved.starts_with(root.path()));
        assert!(resolved.ends_with("file.txt"));
    }

    #[test]
    fn rejects_parent_directory_escape() {
        let root = tempdir().unwrap();
        let result = confine(root.path(), "../../../etc/passwd");
        assert!(
            matches!(result, Err(ConfinementError::Escape(_))),
            "got {result:?}"
        );
    }

    #[test]
    fn rejects_absolute_paths_outside_the_root() {
        let root = tempdir().unwrap();
        let result = confine(root.path(), "/etc/passwd");
        assert!(matches!(result, Err(ConfinementError::Escape(_))));
    }

    #[test]
    fn rejects_blank_path_arguments() {
        let root = tempdir().unwrap();
        assert!(matches!(
            confine(root.path(), "   "),
            Err(ConfinementError::InvalidPath(_))
        ));
    }

    #[test]
    fn rejects_symlink_escape() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        // Create a symlink inside the root pointing outside it.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
            let result = confine(root.path(), "escape/secret.txt");
            assert!(
                matches!(result, Err(ConfinementError::Escape(_))),
                "symlink escape must be rejected, got {result:?}"
            );
        }
        // Non-unix hosts skip the symlink case (no std symlink API stable).
    }
}
