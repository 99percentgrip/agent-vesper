//! Bounded project inputs copied into fresh worker scopes, never shared writable
//! project mounts. Links and special files cannot broaden the input grant.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 4096;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

struct InputFile {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: std::fs::Permissions,
}

pub struct ProjectInputs {
    files: Vec<InputFile>,
}

impl ProjectInputs {
    /// Capture an explicitly granted project on a blocking composition task.
    /// Generated dependencies, VCS internals and private hidden state are
    /// excluded; known repository build/CI dotfiles remain project inputs.
    /// An over-budget project refuses instead of silently
    /// handing workers an incomplete view. Symlinks refuse before any copy.
    pub fn capture(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let mut stack = vec![(root.clone(), 0)];
        let mut files = Vec::new();
        let mut total = 0usize;
        let mut visited = 0usize;
        while let Some((directory, depth)) = stack.pop() {
            if depth > 64 {
                return Err("Project input directory depth exceeds 64.".into());
            }
            for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                visited += 1;
                if visited > 16384 {
                    return Err("Project input inventory exceeds 16384 entries.".into());
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if (name.starts_with('.')
                    && !matches!(
                        name.as_ref(),
                        ".github"
                            | ".cargo"
                            | ".gitignore"
                            | ".gitattributes"
                            | ".editorconfig"
                            | ".dockerignore"
                    ))
                    || matches!(
                        name.as_ref(),
                        "target" | "node_modules" | "__pycache__" | "dist" | "build"
                    )
                {
                    continue;
                }
                let path = entry.path();
                let metadata = path.symlink_metadata().map_err(|error| error.to_string())?;
                if metadata.file_type().is_symlink() {
                    return Err(
                        "Project input symlinks require an explicit materialized copy.".into(),
                    );
                }
                if metadata.is_dir() {
                    stack.push((path, depth + 1));
                    continue;
                }
                if !metadata.is_file() {
                    return Err("Project input special files are not supported.".into());
                }
                if files.len() == MAX_FILES || metadata.len() > MAX_FILE_BYTES as u64 {
                    return Err("Project inputs exceed 4096 files or 4 MiB per file.".into());
                }
                if path.canonicalize().map_err(|error| error.to_string())? != path {
                    return Err("Project input alias changed during capture.".into());
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .map_err(|error| error.to_string())?
                    .take((MAX_FILE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                total = total.saturating_add(bytes.len());
                if bytes.len() > MAX_FILE_BYTES || total > MAX_BYTES {
                    return Err("Project inputs exceed the 64 MiB aggregate limit.".into());
                }
                files.push(InputFile {
                    path: path
                        .strip_prefix(&root)
                        .map_err(|error| error.to_string())?
                        .to_path_buf(),
                    bytes,
                    permissions: metadata.permissions(),
                });
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self { files })
    }

    /// Only called inside a newly created canonical worker root. create_new
    /// refuses collisions; no input path can overwrite an existing artifact.
    pub(crate) fn materialize(&self, root: &Path) -> Result<(), String> {
        for input in &self.files {
            let destination = root.join(&input.path);
            let parent = destination.parent().ok_or("Input has no parent")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            if parent.canonicalize().map_err(|error| error.to_string())? != parent {
                return Err("Worker input directory alias refused.".into());
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| error.to_string())?;
            file.write_all(&input.bytes)
                .map_err(|error| error.to_string())?;
            file.set_permissions(input.permissions.clone())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_inputs_exclude_private_and_generated_state() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("code.rs"), "original").unwrap();
        std::fs::write(project.path().join(".env"), "private").unwrap();
        std::fs::create_dir_all(project.path().join(".github/workflows")).unwrap();
        std::fs::write(project.path().join(".github/workflows/ci.yml"), "name: CI").unwrap();
        std::fs::write(project.path().join(".gitignore"), "target/").unwrap();
        let inputs = ProjectInputs::capture(project.path()).unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        inputs
            .materialize(&first.path().canonicalize().unwrap())
            .unwrap();
        inputs
            .materialize(&second.path().canonicalize().unwrap())
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(first.path().join(".github/workflows/ci.yml")).unwrap(),
            "name: CI"
        );
        assert_eq!(
            std::fs::read_to_string(second.path().join(".gitignore")).unwrap(),
            "target/"
        );
        std::fs::write(first.path().join("code.rs"), "changed").unwrap();
        assert_eq!(
            std::fs::read_to_string(second.path().join("code.rs")).unwrap(),
            "original"
        );
        assert_eq!(
            std::fs::read_to_string(project.path().join("code.rs")).unwrap(),
            "original"
        );
        assert!(!first.path().join(".env").exists());
        assert!(inputs.materialize(second.path()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_inputs_refuse_without_reading_target() {
        let project = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/unavailable-private-target", project.path().join("linked"))
            .unwrap();
        assert!(ProjectInputs::capture(project.path()).is_err());
    }
    #[test]
    fn oversized_input_refuses() {
        let project = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(project.path().join("large")).unwrap();
        file.set_len((MAX_FILE_BYTES + 1) as u64).unwrap();
        assert!(ProjectInputs::capture(project.path()).is_err());
    }
}
