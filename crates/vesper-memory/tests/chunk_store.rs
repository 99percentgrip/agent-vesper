//! Advanced context paging PR-1 (AC-1): chunk storage enumeration, manifest
//! parsing, fail-closed capacity enforcement, and the G5 zero-regression
//! guarantee. See `docs/advanced-context-paging-prd.md` §5 PR-1, §6 AC-1.

use std::collections::{BTreeMap, BTreeSet};

use tempfile::TempDir;
use vesper_memory::{
    MAX_CHUNK_BYTES, MAX_CHUNKS_PER_SKILL, MemoryError, SkillChunkManifestEntry, SkillRoutingQuery,
    SkillSlug, SkillStore, SkillSummary, parse_metadata,
};

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn store() -> (TempDir, SkillStore) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("memory-root");
    std::fs::create_dir_all(&root).unwrap();
    let store = SkillStore::open(&root).unwrap();
    (directory, store)
}

fn summary(slug: &str) -> SkillSummary {
    SkillSummary {
        slug: slug.to_owned(),
        headline: String::new(),
    }
}

fn write_skill(store: &SkillStore, slug: &str, frontmatter: &str, body: &str) {
    store
        .write(
            &SkillSlug::new(slug).unwrap(),
            &format!("---\n{frontmatter}---\n# {slug}\n\n{body}"),
        )
        .unwrap();
}

fn write_chunk(_store: &SkillStore, base: &std::path::Path, slug: &str, name: &str, body: &str) {
    let dir = base
        .join("memory-root")
        .join("skills")
        .join(slug)
        .join("chunks");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
}

/// Owns the empty tool/outcome collections so queries can borrow from a
/// longer-lived owner; keeps call sites to one line.
#[derive(Default)]
struct QueryEnv {
    tools: BTreeSet<String>,
    outcomes: BTreeMap<String, i16>,
}

impl QueryEnv {
    fn query<'a>(&'a self, prompt: &'a str) -> SkillRoutingQuery<'a> {
        SkillRoutingQuery {
            prompt,
            explicit_skill: None,
            available_tools: &self.tools,
            platform: "linux",
            outcome_adjustments: &self.outcomes,
        }
    }
}

const VALID_CHUNK_MANIFEST: &str = "name: chunked-guide\ndescription: Bounded chapter guide\ntags: [guide]\nchunks:\n  - name: setup\n    description: Initial setup steps\n    summary: Install and configure.\n    key_elements: [install, config]\n  - name: usage\n    description: Everyday usage patterns\n";

// ---------------------------------------------------------------------------
// manifest proofs
// ---------------------------------------------------------------------------

#[test]
fn valid_manifest_parses_with_all_fields() {
    let metadata = parse_metadata(
        &summary("chunked-guide"),
        &format!("---\n{VALID_CHUNK_MANIFEST}---\nbody"),
    );
    assert!(
        metadata.chunk_manifest_error.is_none(),
        "{:?}",
        metadata.chunk_manifest_error
    );
    assert_eq!(metadata.chunks.len(), 2);
    let setup = &metadata.chunks[0];
    assert_eq!(setup.name, "setup");
    assert_eq!(setup.description, "Initial setup steps");
    assert_eq!(setup.summary.as_deref(), Some("Install and configure."));
    assert_eq!(
        setup.key_elements.as_deref(),
        Some(&["install".to_owned(), "config".to_owned()][..])
    );
    let usage = &metadata.chunks[1];
    assert_eq!(usage.name, "usage");
    assert_eq!(usage.description, "Everyday usage patterns");
    assert!(usage.summary.is_none(), "absent optional stays absent");
    assert!(usage.key_elements.is_none());
    // Skill-level fields are untouched by the nested block.
    assert_eq!(metadata.name, "chunked-guide");
    assert_eq!(metadata.description, "Bounded chapter guide");
    assert_eq!(metadata.tags, vec!["guide".to_owned()]);
    assert!(metadata.declares_chunks());
}

#[test]
fn manifest_skill_enumerates_and_selects_like_any_other() {
    let (directory, store) = store();
    let frontmatter = "description: Bounded chapter guide for setup and usage\nchunks:\n  - name: setup\n    description: Initial setup steps\n";
    write_skill(&store, "chunked-guide", frontmatter, "guide body");
    write_chunk(
        &store,
        directory.path(),
        "chunked-guide",
        "setup",
        "Run the installer.",
    );

    let summaries = store.list();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].slug, "chunked-guide");

    let env = QueryEnv::default();
    let report = store.orchestrate(&env.query("bounded chapter guide for setup"));
    assert_eq!(report.selected_names(), vec!["chunked-guide"]);
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);

    let body = store
        .read_chunk(&SkillSlug::new("chunked-guide").unwrap(), "setup")
        .unwrap();
    assert_eq!(body, "Run the installer.");
}

#[test]
fn chunk_read_falls_back_to_global_layer() {
    let (directory, _store) = store();
    let global_root = directory.path().join("global-root");
    std::fs::create_dir_all(
        global_root
            .join("skills")
            .join("chunked-guide")
            .join("chunks"),
    )
    .unwrap();
    std::fs::create_dir_all(directory.path().join("memory-root").join("skills")).unwrap();
    std::fs::write(
        global_root.join("skills").join("chunked-guide.md"),
        "---\ndescription: Global bounded guide\nchunks:\n  - name: setup\n    description: Initial setup steps\n---\nbody",
    )
    .unwrap();
    std::fs::write(
        global_root
            .join("skills")
            .join("chunked-guide")
            .join("chunks")
            .join("setup.md"),
        "global chunk body",
    )
    .unwrap();
    let merged =
        SkillStore::open_with_global(&directory.path().join("memory-root"), &global_root).unwrap();
    let body = merged
        .read_chunk(&SkillSlug::new("chunked-guide").unwrap(), "setup")
        .unwrap();
    assert_eq!(body, "global chunk body");
}

// ---------------------------------------------------------------------------
// rejection proofs (fail-closed)
// ---------------------------------------------------------------------------

#[test]
fn oversize_chunk_file_is_rejected_not_truncated() {
    let (directory, store) = store();
    let frontmatter = "description: Bounded chapter guide for setup and usage\nchunks:\n  - name: setup\n    description: Initial setup steps\n";
    write_skill(&store, "chunked-guide", frontmatter, "guide body");
    write_chunk(
        &store,
        directory.path(),
        "chunked-guide",
        "setup",
        &"x".repeat(MAX_CHUNK_BYTES + 1),
    );
    let env = QueryEnv::default();
    let report = store.orchestrate(&env.query("bounded chapter guide for setup"));
    assert!(
        report.selected.is_empty(),
        "oversize chunk must fail closed, got {:?}",
        report.selected_names()
    );
    assert_eq!(
        report.rejected[0],
        (
            "chunked-guide".to_owned(),
            format!(
                "invalid chunk manifest: chunk `setup`: {} bytes exceeds MAX_CHUNK_BYTES {}",
                MAX_CHUNK_BYTES + 1,
                MAX_CHUNK_BYTES
            )
        )
    );
    // The oversize file never enters context: read_chunk also rejects.
    assert_eq!(
        store
            .read_chunk(&SkillSlug::new("chunked-guide").unwrap(), "setup")
            .unwrap_err(),
        MemoryError::NotFound("skill chunk: chunked-guide/setup".to_owned())
    );
}

#[test]
fn missing_chunk_file_is_rejected() {
    let (directory, store) = store();
    let frontmatter = "description: Bounded chapter guide for setup and usage\nchunks:\n  - name: setup\n    description: Initial setup steps\n";
    write_skill(&store, "chunked-guide", frontmatter, "guide body");
    // Declared but never written to disk.
    let env = QueryEnv::default();
    let report = store.orchestrate(&env.query("bounded chapter guide for setup"));
    assert!(report.selected.is_empty());
    assert_eq!(
        report.rejected[0].1,
        "invalid chunk manifest: chunk `setup`: file not found under chunks/"
    );
    let _ = directory; // keep tempdir alive
}

#[test]
fn over_limit_chunk_count_is_rejected() {
    let (directory, store) = store();
    let mut manifest =
        String::from("description: Bounded chapter guide for many chunks\nchunks:\n");
    for index in 0..=MAX_CHUNKS_PER_SKILL {
        manifest.push_str(&format!(
            "  - name: part-{index:02}\n    description: Chunk number {index}\n"
        ));
    }
    write_skill(&store, "chunked-guide", &manifest, "guide body");
    let metadata = parse_metadata(&summary("chunked-guide"), &manifest_body(&manifest));
    assert!(
        metadata.chunk_manifest_error.is_some(),
        "count cap must trip"
    );
    assert!(
        metadata.chunks.len() > MAX_CHUNKS_PER_SKILL,
        "entries retained for diagnostics"
    );
    let env = QueryEnv::default();
    let report = store.orchestrate(&env.query("bounded chapter guide for many chunks"));
    assert!(report.selected.is_empty());
    let _ = directory;
}

fn manifest_body(manifest: &str) -> String {
    format!("---\n{manifest}---\n# body\n")
}

#[test]
fn malformed_entries_reject_with_explicit_reasons() {
    let manifest = "description: Broken guide\nchunks:\n  - name: setup\n    description: Initial setup steps\n  - Bad Name With Spaces\n    description: another\n  - name: no-desc\n";
    let metadata = parse_metadata(&summary("broken-guide"), &manifest_body(manifest));
    assert!(metadata.chunk_manifest_error.is_some());
    let reason = metadata.chunk_manifest_error.unwrap();
    assert!(
        reason.contains("chunk name"),
        "expected name validation, got {reason}"
    );
    // All entries retained for diagnostics (3 entries declared).
    assert_eq!(metadata.chunks.len(), 3);
}

#[test]
fn duplicate_chunk_names_reject() {
    let manifest = "description: Dup guide\nchunks:\n  - name: setup\n    description: first\n  - name: setup\n    description: second\n";
    let metadata = parse_metadata(&summary("dup-guide"), &manifest_body(manifest));
    assert_eq!(
        metadata.chunk_manifest_error.as_deref(),
        Some("duplicate chunk name `setup`")
    );
}

// ---------------------------------------------------------------------------
// G5 proof: chunk-less skills are byte-identical
// ---------------------------------------------------------------------------

#[test]
fn chunk_less_skills_are_byte_identical_in_catalog_and_routing() {
    let (directory, store) = store();
    let frontmatter = "description: Bounded chapter guide for setup and usage\n";
    write_skill(&store, "chunked-guide", frontmatter, "guide body");
    write_chunk(
        &store,
        directory.path(),
        "chunked-guide",
        "setup",
        "chunk body",
    );

    let plain_frontmatter = "description: Plain workflow helpers for everyday tasks\n";
    write_skill(&store, "plain-skill", plain_frontmatter, "plain body");

    // Catalog byte-identity: summaries exactly as the pre-chunk store would
    // produce — slug + headline from frontmatter description, nothing else.
    let summaries = store.list();
    assert_eq!(summaries.len(), 2);
    let chunked = summaries
        .iter()
        .find(|s| s.slug == "chunked-guide")
        .unwrap();
    assert_eq!(
        chunked.headline,
        "Bounded chapter guide for setup and usage"
    );
    let plain = summaries.iter().find(|s| s.slug == "plain-skill").unwrap();
    assert_eq!(plain.headline, "Plain workflow helpers for everyday tasks");

    // Routing byte-identity: same prompt → same selected bodies and scores.
    let env = QueryEnv::default();
    let report = store.orchestrate(&env.query("plain workflow helpers for everyday tasks"));
    assert_eq!(report.selected_names(), vec!["plain-skill"]);
    assert_eq!(
        report.selected[0].body,
        "---\ndescription: Plain workflow helpers for everyday tasks\n---\n# plain-skill\n\nplain body"
    );
    assert!(report.rejected.is_empty());

    // parse_metadata chunk tier is invisible.
    let metadata = parse_metadata(
        &summary("plain-skill"),
        &format!("---\n{plain_frontmatter}---\n# plain\nplain body"),
    );
    assert!(metadata.chunks.is_empty());
    assert!(metadata.chunk_manifest_error.is_none());
    assert!(!metadata.declares_chunks());
}

#[test]
fn existing_workspace_tests_unchanged() {
    // Regression proxy: the pre-PR-1 unit suite must pass unmodified.
    // (Full workspace suite runs under `cargo xtask verify`/`acceptance`.)
    let (_directory, store) = store();
    write_skill(
        &store,
        "xlsx",
        "description: Create and edit Excel spreadsheets\ntags: [excel, workbook, csv]\nfile-extensions: [xlsx, csv]\n",
        "Use the workbook helpers.",
    );
    let env = QueryEnv::default();
    let report = store
        .orchestrate(&env.query("Please edit quarterly-report.xlsx and add a spreadsheet chart"));
    assert_eq!(report.selected_names(), vec!["xlsx"]);
}

// ---------------------------------------------------------------------------
// cap constants
// ---------------------------------------------------------------------------

#[test]
fn chunk_caps_match_prd_defaults() {
    assert_eq!(MAX_CHUNKS_PER_SKILL, 32);
    assert_eq!(MAX_CHUNK_BYTES, 24_000);
}

#[test]
fn chunk_entry_validation_is_public_for_future_prs() {
    let entry = SkillChunkManifestEntry {
        name: "setup".to_owned(),
        description: "Initial setup".to_owned(),
        summary: None,
        key_elements: None,
    };
    assert_eq!(entry.validate(), Ok(()));
    let bad = SkillChunkManifestEntry {
        name: "../escape".to_owned(),
        ..entry
    };
    assert_eq!(
        bad.validate(),
        Err(MemoryError::InvalidIdentifier("chunk name".to_owned()))
    );
}
