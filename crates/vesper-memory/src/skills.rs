//! Learned-skill store: read-side access to markdown skill files under
//! `<root>/skills/<slug>.md`.
//!
//! The oracle stores skills as one markdown file per skill
//! (`glm_acp/memory.py:LEARNED_SKILLS_RELATIVE_PATH`). Vesper keeps the
//! same shape: each skill is a markdown document with YAML frontmatter
//! (or a leading `# <name>` header), and the store enumerates them.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::MemoryError;
use crate::io::write_atomic;
use crate::types::SkillSlug;

/// Maximum allowed markdown body size for a single skill (in bytes).
/// Sized to admit curated reference skills (the largest migrated library
/// document is ~72 KB) with growth headroom for enriched skills; raised
/// 24 KB -> 128 KB -> 200 KB (2026-08, quality-first migration).
pub const MAX_SKILL_BYTES: usize = 200_000;
/// Hard cap on the number of skill files the store will enumerate.
pub const MAX_SKILL_FILES: usize = 500;
/// Prefix sufficient for bounded frontmatter and catalog headlines. Full
/// skill bodies are read only after orchestration selects them.
pub(crate) const MAX_SKILL_CATALOG_PREFIX_BYTES: usize = 32_000;
/// Maximum number of skills referenced by one bundle.
pub const MAX_BUNDLE_SKILLS: usize = 32;
/// Maximum serialized size of one bundle.
pub const MAX_BUNDLE_BYTES: usize = 32_000;

/// One-line summary of a learned skill (name + first non-empty line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSummary {
    /// Slug used as the file stem.
    pub slug: String,
    /// First non-empty, non-header line of the markdown body (≤ 120 chars).
    pub headline: String,
}

/// A project-local group of learned skills and its activation instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBundle {
    /// Stable bundle slug.
    pub name: String,
    /// Human-readable bundle description.
    pub description: String,
    /// Skill slugs included in the bundle.
    pub skills: Vec<String>,
    /// Optional instruction shown when the bundle is loaded.
    #[serde(default)]
    pub instruction: String,
}

/// Read/write access to learned-skill markdown files.
pub struct SkillStore {
    root: PathBuf,
    /// Optional cross-project read layer (a second memory-style root that
    /// contains `skills/` and `bundles/`). Local skills always shadow it;
    /// writes never touch it. Invalid roots degrade to `None`.
    global_root: Option<PathBuf>,
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
            global_root: None,
            cache: Mutex::new(Vec::new()),
        })
    }

    /// Opens a skill store with an additional cross-project read layer.
    ///
    /// The global root follows the same validation rule as the local root;
    /// an invalid or missing global root degrades to "no global layer"
    /// instead of failing the whole store. Reads fall back to the global
    /// layer when a skill is absent locally, and listings append
    /// global-only skills after local ones (local slugs shadow).
    pub fn open_with_global(root: &Path, global_root: &Path) -> Result<Self, MemoryError> {
        let mut store = Self::open(root)?;
        if global_root.is_absolute()
            && global_root
                .parent()
                .is_some_and(|parent| parent.exists() && global_root.exists())
        {
            store.global_root = Some(global_root.to_path_buf());
        }
        Ok(store)
    }

    fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    fn bundles_dir(&self) -> PathBuf {
        self.root.join("bundles")
    }

    fn global_skills_dir(&self) -> Option<PathBuf> {
        self.global_root.as_ref().map(|root| root.join("skills"))
    }

    fn global_bundles_dir(&self) -> Option<PathBuf> {
        self.global_root.as_ref().map(|root| root.join("bundles"))
    }

    fn global_skill_path(&self, slug: &SkillSlug) -> Option<PathBuf> {
        self.global_skills_dir()
            .map(|dir| dir.join(format!("{}.md", slug.as_str())))
    }

    fn skill_path(&self, slug: &SkillSlug) -> PathBuf {
        self.skills_dir().join(format!("{}.md", slug.as_str()))
    }

    /// Lists every readable `.md` skill: project-local files first, then
    /// cross-project global-layer files whose slugs are not shadowed by a
    /// local skill. Files whose stems fail slug validation are skipped so
    /// the store never surfaces unsafe paths.
    pub fn list(&self) -> Vec<SkillSummary> {
        let mut summaries = Vec::new();
        scan_skills_dir(&self.skills_dir(), &mut summaries);
        if let Some(global_dir) = self.global_skills_dir() {
            let local: Vec<String> = summaries.iter().map(|s| s.slug.clone()).collect();
            let mut global = Vec::new();
            scan_skills_dir(&global_dir, &mut global);
            for summary in global {
                if !local.contains(&summary.slug) && summaries.len() < MAX_SKILL_FILES {
                    summaries.push(summary);
                }
            }
        }
        // Update the cache for tests + the curator.
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
            cache.extend(summaries.iter().map(|s| s.slug.clone()));
        }
        summaries
    }

    /// Reads the full markdown body of one skill, falling back to the
    /// cross-project global layer when the skill is absent locally.
    /// Returns [`MemoryError::NotFound`] when both layers miss.
    pub fn read(&self, slug: &SkillSlug) -> Result<String, MemoryError> {
        let path = self.skill_path(slug);
        if let Ok(body) = std::fs::read_to_string(&path) {
            return Ok(body);
        }
        if let Some(body) = self
            .global_skill_path(slug)
            .and_then(|path| std::fs::read_to_string(path).ok())
        {
            return Ok(body);
        }
        Err(MemoryError::NotFound(format!("skill:{}", slug.as_str())))
    }

    /// Reads only the bounded catalog prefix used for discovery/ranking.
    pub(crate) fn read_catalog_prefix(&self, slug: &SkillSlug) -> Result<String, MemoryError> {
        if let Some(body) = read_prefix(&self.skill_path(slug)) {
            return Ok(body);
        }
        if let Some(body) = self
            .global_skill_path(slug)
            .and_then(|path| read_prefix(&path))
        {
            return Ok(body);
        }
        Err(MemoryError::NotFound(format!("skill:{}", slug.as_str())))
    }

    /// Reads one `##`-style section of a skill (the heading line through the
    /// line before the next heading of the same or higher level). The
    /// heading match is case-insensitive on trimmed text and accepts either
    /// `Setup` or `## Setup`. When no section matches, the error lists the
    /// first available headings so the caller can retry precisely.
    pub fn read_section(&self, slug: &SkillSlug, heading: &str) -> Result<String, MemoryError> {
        let body = self.read(slug)?;
        match extract_section(&body, heading) {
            Some(section) => Ok(section),
            None => {
                let available: Vec<String> =
                    body.lines().filter_map(heading_text_of).take(10).collect();
                Err(MemoryError::NotFound(format!(
                    "skill:{} section not found; available: {}",
                    slug.as_str(),
                    if available.is_empty() {
                        "none".to_string()
                    } else {
                        available.join(", ")
                    }
                )))
            }
        }
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

    /// Lists skill bundles: project-local first, then cross-project global
    /// bundles whose names are not shadowed locally.
    pub fn list_bundles(&self) -> Vec<SkillBundle> {
        let mut bundles = scan_bundles_dir(&self.bundles_dir());
        if let Some(global_dir) = self.global_bundles_dir() {
            let local: Vec<String> = bundles.iter().map(|b| b.name.clone()).collect();
            for bundle in scan_bundles_dir(&global_dir) {
                if !local.contains(&bundle.name) && bundles.len() < MAX_SKILL_FILES {
                    bundles.push(bundle);
                }
            }
        }
        bundles
    }

    /// Reads one bundle by validated slug, falling back to the global layer.
    pub fn read_bundle(&self, name: &SkillSlug) -> Result<SkillBundle, MemoryError> {
        let path = self.bundles_dir().join(format!("{}.json", name.as_str()));
        if let Ok(body) = std::fs::read_to_string(&path) {
            return serde_json::from_str(&body).map_err(MemoryError::from);
        }
        if let Some(global_dir) = self.global_bundles_dir() {
            let global_path = global_dir.join(format!("{}.json", name.as_str()));
            if let Ok(body) = std::fs::read_to_string(global_path) {
                return serde_json::from_str(&body).map_err(MemoryError::from);
            }
        }
        Err(MemoryError::NotFound(format!("bundle:{}", name.as_str())))
    }

    /// Creates or replaces one bundle atomically.
    pub fn write_bundle(&self, bundle: SkillBundle) -> Result<(), MemoryError> {
        let slug = SkillSlug::new(&bundle.name)?;
        if bundle.description.chars().count() > 1024
            || bundle.instruction.chars().count() > 8_000
            || bundle.skills.len() > MAX_BUNDLE_SKILLS
            || bundle
                .skills
                .iter()
                .any(|skill| SkillSlug::new(skill).is_err())
        {
            return Err(MemoryError::BoundsViolated("skill bundle"));
        }
        let body = serde_json::to_vec(&bundle)?;
        if body.len() > MAX_BUNDLE_BYTES {
            return Err(MemoryError::BoundsViolated("skill bundle bytes"));
        }
        let dir = self.bundles_dir();
        std::fs::create_dir_all(&dir).map_err(|_| MemoryError::io("create"))?;
        write_atomic(&dir.join(format!("{}.json", slug.as_str())), &body)
    }

    /// Removes one bundle. Idempotent.
    pub fn forget_bundle(&self, name: &SkillSlug) -> Result<bool, MemoryError> {
        let path = self.bundles_dir().join(format!("{}.json", name.as_str()));
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(MemoryError::io("remove")),
        }
    }
}

/// Returns the headline shown by `list_skills`: the frontmatter
/// `description:` when present (mirroring the oracle's context format
/// `- {name}: {description}`), otherwise the first non-empty,
/// non-frontmatter line or leading `# ` header. Truncated to 120 chars.
fn headline_of(body: &str) -> String {
    if let Some(description) = frontmatter_description_of(body) {
        return truncate(&description, 120);
    }
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

/// Extracts a single-line frontmatter `description:` value from the leading
/// YAML block, stripping balanced quotes. Returns `None` for missing, empty,
/// or multi-line (folded) values.
fn frontmatter_description_of(body: &str) -> Option<String> {
    let mut lines = body.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for raw in lines {
        let line = raw.trim();
        if line == "---" {
            return None;
        }
        if let Some(rest) = line.strip_prefix("description:") {
            let mut value = rest.trim();
            if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                value = &value[1..value.len() - 1];
            }
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Returns the trimmed text of one markdown heading line (`#`..`######`),
/// or `None` for non-heading lines.
fn heading_text_of(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.trim_start_matches('#');
    if !rest.starts_with(' ') {
        return None;
    }
    let text = rest.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Returns the heading level (1-6) of a heading line, or `None`.
fn heading_level_of(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        trimmed[hashes..].starts_with(' ').then_some(hashes)
    } else {
        None
    }
}

/// Extracts the section starting at the heading whose trimmed text equals
/// `heading` (case-insensitive) through the line before the next heading of
/// the same or higher level. Returns `None` when the heading is absent.
fn extract_section(body: &str, heading: &str) -> Option<String> {
    let target = heading.trim().to_ascii_lowercase();
    let mut lines = body.lines();
    // Find the opening heading.
    let mut collected: Vec<&str> = Vec::new();
    let mut level = 0usize;
    for raw in lines.by_ref() {
        if let (Some(text), Some(lvl)) = (heading_text_of(raw), heading_level_of(raw))
            && text.to_ascii_lowercase() == target
        {
            collected.push(raw);
            level = lvl;
            break;
        }
    }
    if collected.is_empty() {
        return None;
    }
    for raw in lines {
        if heading_level_of(raw).is_some_and(|lvl| lvl <= level) {
            break;
        }
        collected.push(raw);
    }
    Some(collected.join("\n"))
}

/// Scans one `skills/` directory into `out`, respecting [`MAX_SKILL_FILES`].
fn scan_skills_dir(dir: &Path, out: &mut Vec<SkillSummary>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_SKILL_FILES {
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
        let body = read_prefix(&path).unwrap_or_default();
        let headline = headline_of(&body);
        out.push(SkillSummary {
            slug: slug.as_str().to_string(),
            headline,
        });
    }
}

fn read_prefix(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(MAX_SKILL_CATALOG_PREFIX_BYTES);
    file.take(MAX_SKILL_CATALOG_PREFIX_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Scans one `bundles/` directory into a vector of parsed bundles.
fn scan_bundles_dir(dir: &Path) -> Vec<SkillBundle> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            (entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
                .then(|| std::fs::read_to_string(entry.path()).ok())
                .flatten()
                .and_then(|body| serde_json::from_str::<SkillBundle>(&body).ok())
        })
        .take(MAX_SKILL_FILES)
        .collect()
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
    fn caps_admit_curated_skill_library() {
        // Caps were raised (24 KB -> 128 KB -> 200 KB body, 200 -> 500
        // files) so migrated curated reference skills — the largest
        // observed is ~72 KB — round-trip through learn_skill/manage_skill
        // without tripping BoundsViolated, with headroom for enrichment.
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let slug = SkillSlug::new("curated-large").unwrap();
        let body = format!("# Curated library skill\n\n{}", "x".repeat(150_000));
        store.write(&slug, &body).unwrap();
        assert!(store.read(&slug).unwrap().len() > 150_000);
        let over = SkillSlug::new("over").unwrap();
        let err = store
            .write(&over, &format!("x{}", "y".repeat(MAX_SKILL_BYTES)))
            .unwrap_err();
        assert_eq!(err, MemoryError::BoundsViolated("skill body size"));
    }

    #[test]
    fn headline_prefers_frontmatter_description() {
        // Mirrors the oracle context format `- {name}: {description}`:
        // the listing should surface the description authors wrote.
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let slug = SkillSlug::new("described").unwrap();
        let body = "---\nname: described\ndescription: \"Do the thing well.\"\n---\n\n# Described\n\nBody line.";
        store.write(&slug, body).unwrap();
        let summaries = store.list();
        assert_eq!(summaries[0].headline, "Do the thing well.");

        let plain = SkillSlug::new("plain").unwrap();
        store
            .write(&plain, "---\nname: plain\n---\n\n# Plain Header\n\nBody.")
            .unwrap();
        let summaries = store.list();
        let plain = summaries.iter().find(|s| s.slug == "plain").unwrap();
        assert_eq!(plain.headline, "Plain Header");
    }

    #[test]
    fn global_layer_appends_and_local_shadows() {
        let temp = TempDir::new().unwrap();
        let (root, _store) = store_under(&temp);
        let global_root = temp.path().join("global-root");
        std::fs::create_dir_all(global_root.join("skills")).unwrap();
        std::fs::create_dir_all(root.join("skills")).unwrap();
        // Same slug in both layers; different headline.
        std::fs::write(
            global_root.join("skills").join("shared.md"),
            "---\ndescription: global version\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            root.join("skills").join("shared.md"),
            "---\ndescription: local version\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            global_root.join("skills").join("global-only.md"),
            "---\ndescription: only global\n---\nbody",
        )
        .unwrap();
        let store = SkillStore::open_with_global(&root, &global_root).unwrap();
        let summaries = store.list();
        assert_eq!(summaries.len(), 2);
        let shared = summaries.iter().find(|s| s.slug == "shared").unwrap();
        assert_eq!(shared.headline, "local version");
        assert!(summaries.iter().any(|s| s.slug == "global-only"));
        // Reads shadow locally and fall back globally.
        let shared_slug = SkillSlug::new("shared").unwrap();
        assert!(store.read(&shared_slug).unwrap().contains("local version"));
        let global_only = SkillSlug::new("global-only").unwrap();
        assert!(store.read(&global_only).unwrap().contains("only global"));
        // Writes stay local: the global file is untouched.
        store.write(&global_only, "# rewritten locally").unwrap();
        assert!(
            store
                .read(&global_only)
                .unwrap()
                .contains("rewritten locally")
        );
        assert!(
            global_root
                .join("skills")
                .join("global-only.md")
                .metadata()
                .is_ok()
        );
    }

    #[test]
    fn global_layer_invalid_root_is_ignored() {
        let temp = TempDir::new().unwrap();
        let (root, _store) = store_under(&temp);
        let missing = temp.path().join("does-not-exist");
        let store = SkillStore::open_with_global(&root, &missing).unwrap();
        assert!(store.list().is_empty());
        let slug = SkillSlug::new("anything").unwrap();
        assert!(store.read(&slug).is_err());
    }

    #[test]
    fn global_bundles_merge_after_local() {
        let temp = TempDir::new().unwrap();
        let (root, _store) = store_under(&temp);
        let global_root = temp.path().join("global-root");
        std::fs::create_dir_all(global_root.join("bundles")).unwrap();
        let global_bundle = SkillBundle {
            name: "categories".into(),
            description: "global categories".into(),
            skills: vec!["plan".into()],
            instruction: String::new(),
        };
        std::fs::write(
            global_root.join("bundles").join("categories.json"),
            serde_json::to_string(&global_bundle).unwrap(),
        )
        .unwrap();
        let local_bundle = SkillBundle {
            name: "local".into(),
            description: "project-local bundle".into(),
            skills: vec![],
            instruction: String::new(),
        };
        let store = SkillStore::open_with_global(&root, &global_root).unwrap();
        store.write_bundle(local_bundle).unwrap();
        let bundles = store.list_bundles();
        assert_eq!(bundles.len(), 2);
        assert_eq!(bundles[0].name, "local");
        assert_eq!(
            store
                .read_bundle(&SkillSlug::new("categories").unwrap())
                .unwrap()
                .description,
            "global categories"
        );
    }

    #[test]
    fn read_section_extracts_between_matching_headings() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let slug = SkillSlug::new("sectioned").unwrap();
        let body = "# Title\n\n## Setup\n\ndo this\n\n### Detail\n\ninner\n\n## Usage\n\nthat\n";
        store.write(&slug, body).unwrap();
        let setup = store.read_section(&slug, "Setup").unwrap();
        assert!(setup.starts_with("## Setup"));
        assert!(setup.contains("do this"));
        assert!(setup.contains("### Detail"));
        assert!(setup.contains("inner"));
        assert!(!setup.contains("## Usage"));
        // Case-insensitive match.
        let usage = store.read_section(&slug, "usage").unwrap();
        assert!(usage.contains("that"));
        // Missing section reports the available headings.
        let err = store.read_section(&slug, "Missing").unwrap_err();
        match err {
            MemoryError::NotFound(message) => {
                assert!(message.contains("Setup"));
                assert!(message.contains("Usage"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
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

    #[test]
    fn bundle_round_trips_and_is_idempotently_removable() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let bundle = SkillBundle {
            name: "release-workflow".into(),
            description: "Release checks".into(),
            skills: vec!["rust-cargo-bump".into(), "release-notes".into()],
            instruction: "Load both skills before publishing.".into(),
        };
        store.write_bundle(bundle.clone()).unwrap();
        assert_eq!(
            store
                .read_bundle(&SkillSlug::new("release-workflow").unwrap())
                .unwrap(),
            bundle
        );
        assert_eq!(store.list_bundles(), vec![bundle]);
        assert!(
            store
                .forget_bundle(&SkillSlug::new("release-workflow").unwrap())
                .unwrap()
        );
        assert!(
            !store
                .forget_bundle(&SkillSlug::new("release-workflow").unwrap())
                .unwrap()
        );
    }

    #[test]
    fn bundle_rejects_invalid_skill_slug_and_oversized_instruction() {
        let temp = TempDir::new().unwrap();
        let (_root, store) = store_under(&temp);
        let invalid = store.write_bundle(SkillBundle {
            name: "safe".into(),
            description: String::new(),
            skills: vec!["../escape".into()],
            instruction: String::new(),
        });
        assert_eq!(
            invalid.unwrap_err(),
            MemoryError::BoundsViolated("skill bundle")
        );
        let oversized = store.write_bundle(SkillBundle {
            name: "large".into(),
            description: String::new(),
            skills: Vec::new(),
            instruction: "x".repeat(8_001),
        });
        assert_eq!(
            oversized.unwrap_err(),
            MemoryError::BoundsViolated("skill bundle")
        );
    }
}
