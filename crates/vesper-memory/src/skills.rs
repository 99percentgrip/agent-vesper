//! Learned-skill store: read-side access to markdown skill files under
//! `<root>/skills/<slug>.md`.
//!
//! The oracle stores skills as one markdown file per skill
//! (`glm_acp/memory.py:LEARNED_SKILLS_RELATIVE_PATH`). Vesper keeps the
//! same shape: each skill is a markdown document with YAML frontmatter
//! (or a leading `# <name>` header), and the store enumerates them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::MemoryError;
use crate::io::write_atomic;
use crate::types::SkillSlug;

/// Maximum allowed markdown body size for a single skill (in bytes).
pub const MAX_SKILL_BYTES: usize = 24_000;
/// Hard cap on the number of skill files the store will enumerate.
pub const MAX_SKILL_FILES: usize = 200;

/// One-line summary of a learned skill (name + first non-empty line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSummary {
    /// Slug used as the file stem.
    pub slug: String,
    /// First non-empty, non-header line of the markdown body (≤ 120 chars).
    pub headline: String,
}

/// Read/write access to learned-skill markdown files.
pub struct SkillStore {
    root: PathBuf,
    /// Lazily populated slug cache (slug → file mtime seen at last scan).
    cache: Mutex<Vec<String>>,
}

impl std::fmt::Debug for SkillStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillStore")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl SkillStore {
    /// Opens a skill store rooted at `<root>/skills`. The root must already
    /// be absolute with an existing parent (mirrors `MemoryStore::open`).
    pub fn open(root: &Path) -> Result<Self, MemoryError> {
        if !root.is_absolute() {
            return Err(MemoryError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.exists() => {}
            Some(_) | None => return Err(MemoryError::InvalidRoot),
        }
        Ok(Self {
            root: root.to_path_buf(),
            cache: Mutex::new(Vec::new()),
        })
    }

    fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    fn skill_path(&self, slug: &SkillSlug) -> PathBuf {
        self.skills_dir().join(format!("{}.md", slug.as_str()))
    }

    /// Lists every `.md` file under `skills/`. Files whose stems fail slug
    /// validation are skipped so the store never surfaces unsafe paths.
    pub fn list(&self) -> Vec<SkillSummary> {
        let dir = self.skills_dir();
        let mut summaries = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Vec::new();
            }
            Err(_) => return Vec::new(),
        };
        for entry in entries.flatten() {
            if summaries.len() >= MAX_SKILL_FILES {
                break;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(stem) => stem,
                None => continue,
            };
            let slug = match SkillSlug::new(stem) {
                Ok(slug) => slug,
                Err(_) => continue,
            };
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            let headline = headline_of(&body);
            summaries.push(SkillSummary {
                slug: slug.as_str().to_string(),
                headline,
            });
        }
        // Update the cache for tests + the curator.
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
            cache.extend(summaries.iter().map(|s| s.slug.clone()));
        }
        summaries
    }

    /// Reads the full markdown body of one skill. Returns
    /// [`MemoryError::NotFound`] when the skill is absent.
    pub fn read(&self, slug: &SkillSlug) -> Result<String, MemoryError> {
        let path = self.skill_path(slug);
        let body = std::fs::read_to_string(&path)
            .map_err(|_| MemoryError::NotFound(format!("skill:{}", slug.as_str())))?;
        Ok(body)
    }

    /// Writes (or replaces) a skill file. The body is bounded by
    /// [`MAX_SKILL_BYTES`].
    pub fn write(&self, slug: &SkillSlug, body: &str) -> Result<(), MemoryError> {
        if body.len() > MAX_SKILL_BYTES {
            return Err(MemoryError::BoundsViolated("skill body size"));
        }
        let dir = self.skills_dir();
        std::fs::create_dir_all(&dir).map_err(|_| MemoryError::io("create"))?;
        write_atomic(&self.skill_path(slug), body.as_bytes())
    }

    /// Removes a skill file. Idempotent: returns `Ok(false)` when the skill
    /// is already absent.
    pub fn forget(&self, slug: &SkillSlug) -> Result<bool, MemoryError> {
        let path = self.skill_path(slug);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(MemoryError::io("remove")),
        }
    }
}

/// Returns the first non-empty, non-frontmatter, non-leading-`#`-header
/// line of `body`, truncated to 120 chars.
fn headline_of(body: &str) -> String {
    let mut in_frontmatter = false;
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            return truncate(rest.trim(), 120);
        }
        if !line.starts_with('#') {
            return truncate(line, 120);
        }
    }
    String::new()
}

fn truncate(value: &str, limit: usize) -> String {
    let truncated: String = value.chars().take(limit).collect();
    truncated
}

#[cfg(test)]
mod tests {
    //! Skill store: list, read, write, forget.

    use super::*;
    use tempfile::TempDir;

    fn store_under(temp: &TempDir) -> (PathBuf, SkillStore) {
        let root = temp.path().join("memory-root");
        std::fs::create_dir_all(&root).unwrap();
        let store = SkillStore::open(&root).unwrap();
        (root, store)
    }

    #[test]
    fn list_returns_empty_when_skills_dir_does_not_exist() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        assert!(store.list().is_empty());
    }

    #[test]
    fn write_then_read_round_trips() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let slug = SkillSlug::new("rust-cargo-bump").unwrap();
        store
            .write(&slug, "# Rust Cargo Bump\n\nBump the workspace version.")
            .unwrap();
        let body = store.read(&slug).unwrap();
        assert!(body.starts_with("# Rust Cargo Bump"));
        let summaries = store.list();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].slug, "rust-cargo-bump");
        assert_eq!(summaries[0].headline, "Rust Cargo Bump");
    }

    #[test]
    fn forget_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let slug = SkillSlug::new("ephemeral").unwrap();
        store.write(&slug, "body").unwrap();
        assert!(store.forget(&slug).unwrap());
        assert!(!store.forget(&slug).unwrap());
    }

    #[test]
    fn rejects_oversized_body() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let slug = SkillSlug::new("big").unwrap();
        let body = "x".repeat(MAX_SKILL_BYTES + 1);
        let err = store.write(&slug, &body).unwrap_err();
        assert_eq!(err, MemoryError::BoundsViolated("skill body size"));
    }

    #[test]
    fn skips_files_with_unsafe_stems() {
        let temp = TempDir::new().unwrap();
        let (root, store) = store_under(&temp);
        let skills_dir = root.join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        // Safe slug.
        std::fs::write(skills_dir.join("safe.md"), "# Safe\nbody").unwrap();
        // Unsafe: dotfile with leading dot.
        std::fs::write(skills_dir.join(".hidden.md"), "# hidden").unwrap();
        // Unsafe: path-traversal stem.
        std::fs::write(skills_dir.join("..traversal.md"), "# traversal").unwrap();
        let summaries = store.list();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].slug, "safe");
    }
}
