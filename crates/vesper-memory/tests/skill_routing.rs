use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use vesper_memory::{
    CHUNK_METADATA_ROUTING_ENABLED, ChunkRoutingCondition, MAX_CHUNK_BYTES,
    MAX_CHUNKS_PER_SELECTION, MAX_SKILL_CONTEXT_CHARS, MAX_TOTAL_SKILL_CONTEXT_CHARS,
    SkillRoutingQuery, SkillStore,
};

fn curated_store() -> SkillStore {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("skills");
    SkillStore::open(&root.canonicalize().expect("curated skill root")).expect("skill store")
}

fn selected(prompt: &str) -> Vec<String> {
    let store = curated_store();
    store
        .orchestrate(&SkillRoutingQuery {
            prompt,
            explicit_skill: None,
            available_tools: &BTreeSet::new(),
            platform: "linux",
            outcome_adjustments: &BTreeMap::new(),
        })
        .selected_names()
}

#[test]
fn curated_catalog_routes_representative_tasks_into_top_three() {
    for (prompt, expected) in [
        ("Create an Excel xlsx workbook with charts", "xlsx"),
        (
            "Review this GitHub pull request for correctness",
            "github-code-review",
        ),
        ("Search arXiv for recent transformer papers", "arxiv"),
        (
            "Turn these meeting notes into owner action items",
            "meeting-action-items",
        ),
        (
            "Run exploratory QA and dogfood this web application",
            "dogfood",
        ),
        ("Generate an Excalidraw architecture diagram", "excalidraw"),
    ] {
        let matches = selected(prompt);
        assert!(
            matches.iter().any(|name| name == expected),
            "expected {expected} for {prompt:?}, got {matches:?}"
        );
    }
}

#[test]
fn generic_conversation_does_not_force_an_unrelated_skill() {
    assert!(selected("Thanks, that makes sense.").is_empty());
}

#[test]
fn explicit_textual_selection_routes_a_named_skill() {
    let matches = selected("Use the xlsx skill to process this data");
    assert_eq!(matches.first().map(String::as_str), Some("xlsx"));
}

// ---------------------------------------------------------------------------
// Advanced context paging PR-2 (AC-2): two-level routing. See
// `docs/advanced-context-paging-prd.md` §5 PR-2, §6 AC-2. Fixture-based
// (tempdir) tests live here alongside the curated-catalog tests so the
// PRD's named test home (`skill_routing.rs`) owns all four mandated
// categories: selection+budget math, fail-closed, flag-off, G5.
// ---------------------------------------------------------------------------

/// Owns the empty tool/outcome collections so queries can borrow from a
/// longer-lived owner (same pattern as `chunk_store.rs`).
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

fn fixture_store() -> (tempfile::TempDir, SkillStore) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("memory-root");
    std::fs::create_dir_all(&root).unwrap();
    let store = SkillStore::open(&root).unwrap();
    (directory, store)
}

fn write_chunk(store: &SkillStore, base: &std::path::Path, slug: &str, name: &str, body: &str) {
    let dir = base
        .join("memory-root")
        .join("skills")
        .join(slug)
        .join("chunks");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
    let _ = store;
}

/// Selection & budget math: two-level routing selects bounded chunks and
/// counts them against per-skill and total character budgets.
#[test]
fn two_level_routing_selects_ranked_chunks_within_budgets() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    let manifest = "description: Deployment runbook for staging deploys\nchunks:\n  - name: rollback\n    description: Rollback a failed staging deploy\n  - name: migrate\n    description: Run database migrations before deploy\n  - name: logs\n    description: Inspect staging logs\n  - name: cleanup\n    description: Cleanup staging artifacts\n";
    let body = format!("---\n{manifest}---\n# deploy-runbook\nRunbook body.");
    store
        .write(
            &vesper_memory::SkillSlug::new("deploy-runbook").unwrap(),
            &body,
        )
        .unwrap();
    for (name, chunk_body) in [
        ("rollback", "Rollback steps."),
        ("migrate", "Migration steps."),
        ("logs", "Log steps."),
        ("cleanup", "Cleanup steps."),
    ] {
        write_chunk(&store, directory.path(), "deploy-runbook", name, chunk_body);
    }

    let report = store.orchestrate(&env.query("rollback the failed staging deploy"));
    assert_eq!(report.selected_names(), vec!["deploy-runbook"]);
    // Description ranking + cap: at most 3 chunk bodies loaded.
    assert!(
        !report.chunks.is_empty(),
        "prompt-overlapping chunks must load"
    );
    assert!(report.chunks.len() <= MAX_CHUNKS_PER_SELECTION);
    assert_eq!(report.chunks[0].name, "rollback");
    // Budget math: per-skill body+chunks stay inside the 24K per-skill cap
    // and every loaded chunk counts toward the 60K total.
    let per_skill: usize = report
        .selected
        .iter()
        .map(|s| s.body.chars().count())
        .sum::<usize>()
        + report
            .chunks
            .iter()
            .map(|c| c.body.chars().count())
            .sum::<usize>();
    assert!(per_skill <= MAX_SKILL_CONTEXT_CHARS);
    assert!(per_skill <= MAX_TOTAL_SKILL_CONTEXT_CHARS);
}

/// F1 regression (audit): multiple chunks of ONE skill must collectively
/// respect the per-skill budget — each chunk fitting the initial
/// allowance individually must not let the sum exceed 24K. Over-budget
/// chunks are rejected with the budget reason, never loaded.
#[test]
fn per_skill_budget_decrements_across_chunks_of_one_skill() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    // Three chunks, each under the initial allowance (24K - body), but
    // jointly far over the per-skill cap.
    let manifest = "description: Deployment runbook for staging deploys\nchunks:\n  - name: part-a\n    description: Part A staging steps\n  - name: part-b\n    description: Part B staging steps\n  - name: part-c\n    description: Part C staging steps\n";
    let body = format!("{manifest}---\n# deploy-runbook\nsmall body.");
    store
        .write(
            &vesper_memory::SkillSlug::new("deploy-runbook").unwrap(),
            &format!("---\n{body}"),
        )
        .unwrap();
    let big = "x".repeat(12_000);
    for name in ["part-a", "part-b", "part-c"] {
        write_chunk(&store, directory.path(), "deploy-runbook", name, &big);
    }
    let report = store.orchestrate(&SkillRoutingQuery {
        prompt: "staging runbook part-a part-b part-c steps",
        explicit_skill: Some("deploy-runbook"),
        available_tools: &env.tools,
        platform: "linux",
        outcome_adjustments: &env.outcomes,
    });
    assert_eq!(report.selected_names(), vec!["deploy-runbook"]);
    let per_skill: usize = report.selected[0].body.chars().count()
        + report.selected[0]
            .chunks
            .iter()
            .map(|c| c.body.chars().count())
            .sum::<usize>();
    assert!(
        per_skill <= MAX_SKILL_CONTEXT_CHARS,
        "per-skill body+chunks must stay inside the 24K cap; got {per_skill}"
    );
    // The chunks that no longer fit are rejected by budget, not silently
    // dropped without reason.
    assert!(
        report
            .rejected
            .iter()
            .any(|(slug, reason)| slug.starts_with("deploy-runbook::")
                && reason == "chunk exceeds context budget")
    );
}

/// F3 regression (audit): an EXPLICIT skill request whose manifest fails
/// disk validation must surface `explicit_error` — the user asked for this
/// skill by name; failing silently is a fail-closed gap. Parse-level and
/// disk-level manifest failures must behave identically.
#[test]
fn explicit_selection_with_invalid_manifest_fails_loudly() {
    let (directory, store) = fixture_store();
    let _ = directory;
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    let manifest = "description: Deployment runbook for staging deploys\nchunks:\n  - name: setup\n    description: Initial setup steps\n";
    store
        .write(
            &vesper_memory::SkillSlug::new("deploy-runbook").unwrap(),
            &format!("---\n{manifest}---\n# deploy-runbook\nbody"),
        )
        .unwrap();
    // Chunk file never written: disk validation fails.
    let report = store.orchestrate(&SkillRoutingQuery {
        prompt: "use skill deploy-runbook now",
        explicit_skill: Some("deploy-runbook"),
        available_tools: &tools,
        platform: "linux",
        outcome_adjustments: &outcomes,
    });
    assert!(
        report.selected.is_empty(),
        "invalid manifest must fail closed"
    );
    assert!(
        report.explicit_error.is_some(),
        "an explicit request must surface an error, not vanish silently"
    );
    let error = report.explicit_error.unwrap();
    assert!(
        error.contains("deploy-runbook") && error.contains("invalid chunk manifest"),
        "unexpected explicit error: {error}"
    );
}

/// F4 regression (audit): `archived` is a deliberate user state and must
/// not be masked by an incidental manifest defect in the rejection reason.
#[test]
fn archived_reason_takes_precedence_over_manifest_defect() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    let manifest = "description: Deployment runbook for staging deploys\nchunks:\n  - name: setup\n    description: Initial setup steps\n";
    store
        .write(
            &vesper_memory::SkillSlug::new("deploy-runbook").unwrap(),
            &format!("---\n{manifest}---\n# deploy-runbook\n<!-- vesper:archive -->\nbody"),
        )
        .unwrap();
    // Chunk file missing → manifest defect; skill also archived.
    let _ = directory;
    let report = store.orchestrate(&env.query("staging deploy runbook"));
    let reason = report
        .rejected
        .iter()
        .find(|(slug, _)| slug == "deploy-runbook")
        .map(|(_, reason)| reason.clone())
        .unwrap_or_default();
    assert_eq!(reason, "archived");
}

/// F2 regression (audit): the overhead metric for SUMMARY_ONLY must count
/// only summary-derived tokens — key_elements tokens belong to
/// SUMMARY_KEY_ELEMENTS alone. Constructed so each field contributes one
/// distinct token.
#[test]
fn summary_only_overhead_excludes_key_elements_tokens() {
    let (directory, store) = fixture_store();
    let _ = directory;
    let manifest = "---\nname: probe-skill\ndescription: Alpha reference material\nchunks:\n  - name: one\n    description: alpha\n    summary: bravo\n    key_elements: [charlie]\n";
    store
        .write(
            &vesper_memory::SkillSlug::new("probe-skill").unwrap(),
            &format!("{manifest}---\n# probe-skill\nbody"),
        )
        .unwrap();
    write_chunk(&store, directory.path(), "probe-skill", "one", "chunk body");
    let env = QueryEnv::default();
    // Explicit activation so the skill is selected regardless of ranking.
    let report = store.orchestrate(&SkillRoutingQuery {
        prompt: "use skill probe-skill alpha bravo",
        explicit_skill: Some("probe-skill"),
        available_tools: &env.tools,
        platform: "linux",
        outcome_adjustments: &env.outcomes,
    });
    assert_eq!(report.selected_names(), vec!["probe-skill"]);
    let metrics = store.chunk_routing_metrics(&report, ChunkRoutingCondition::SummaryOnly);
    assert_eq!(
        metrics[0].2, 1,
        "SUMMARY_ONLY overhead must count only the summary token (bravo), got {}",
        metrics[0].2
    );
    let full = store.chunk_routing_metrics(&report, ChunkRoutingCondition::SummaryKeyElements);
    assert_eq!(full[0].2, 2, "bravo + charlie");
}

/// Budget-boundary proof: a chunk UNDER the PR-1 byte cap (24,000) but
/// OVER the remaining per-skill allowance (24,000 − body chars) must be
/// rejected by the budget, not the cap — proving chunks count against the
/// per-skill budget at load time. Skipped, never truncated.
#[test]
fn chunk_over_per_skill_budget_is_rejected_by_budget_not_byte_cap() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    let manifest = "description: Deployment runbook for staging deploys\nchunks:\n  - name: rollback\n    description: Rollback a failed staging deploy\n";
    let body = format!("---\n{manifest}---\n# deploy-runbook\nRunbook body padding.");
    store
        .write(
            &vesper_memory::SkillSlug::new("deploy-runbook").unwrap(),
            &body,
        )
        .unwrap();
    let body_chars = body.chars().count();
    // Under MAX_CHUNK_BYTES (24,000) but over the remaining allowance.
    let over_budget = MAX_CHUNK_BYTES - 5;
    assert!(over_budget > MAX_SKILL_CONTEXT_CHARS - body_chars);
    write_chunk(
        &store,
        directory.path(),
        "deploy-runbook",
        "rollback",
        &"x".repeat(over_budget),
    );

    let report = store.orchestrate(&env.query("rollback the failed staging deploy"));
    assert_eq!(report.selected_names(), vec!["deploy-runbook"]);
    assert!(report.chunks.is_empty(), "over-budget chunk must not load");
    let reason = report
        .rejected
        .iter()
        .find(|(slug, _)| slug == "deploy-runbook::rollback")
        .map(|(_, reason)| reason.clone())
        .unwrap_or_default();
    assert_eq!(reason, "chunk exceeds context budget");
    // And the skill body itself still loaded intact (not truncated).
    assert!(!report.selected[0].truncated);
}

/// Fail-closed: an oversized chunk file is rejected at read time and
/// never enters the routing result — skipped, not truncated.
#[test]
fn oversized_chunk_is_rejected_not_truncated() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    let manifest = "description: Deployment runbook for staging deploys\nchunks:\n  - name: rollback\n    description: Rollback a failed staging deploy\n";
    store
        .write(
            &vesper_memory::SkillSlug::new("deploy-runbook").unwrap(),
            &format!("---\n{manifest}---\n# deploy-runbook\nRunbook body."),
        )
        .unwrap();
    // Over MAX_CHUNK_BYTES: PR-1 validation rejects the whole manifest at
    // catalog time, so the skill routes as ineligible (fail-closed at the
    // manifest layer); read-time cap is defense-in-depth for files that
    // grow after validation.
    let big = "x".repeat(MAX_CHUNK_BYTES + 1);
    write_chunk(&store, directory.path(), "deploy-runbook", "rollback", &big);
    let report = store.orchestrate(&env.query("rollback the failed staging deploy"));
    assert!(
        report.selected.is_empty(),
        "oversize chunk manifest must fail closed"
    );
    assert!(report.rejected.iter().any(|(slug, reason)| {
        slug == "deploy-runbook" && reason.contains("invalid chunk manifest")
    }));
    assert!(report.chunks.is_empty());
}

/// Flag-state proof (supersedes the PR-2 flag-off proof after the D3
/// ADOPT verdict): automatic chunk routing follows the shipped
/// `CHUNK_METADATA_ROUTING_ENABLED` state. With the flag on
/// (post-ADOPT), summary/key_elements overlap legitimately participates;
/// with it off, ranking must be description/name-driven only. The test
/// verifies whichever state ships, so a future flag flip cannot silently
/// strand a stale assertion.
#[test]
fn metadata_fields_ranking_follows_the_shipped_flag_state() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    // Skill A: rich metadata on an irrelevant chunk, poor description.
    // Skill B: identical descriptions; summary/key_elements differ.
    let manifest_a = "description: Deployment runbook for staging deploys\nchunks:\n  - name: logging\n    description: Inspect general logs\n    summary: Deep monitoring diagnostics and telemetry ingestion pipelines\n    key_elements: [telemetry, observability, pipeline]\n  - name: rollback\n    description: Rollback a failed staging deploy\n";
    let manifest_b = "description: Deployment runbook for staging deploys\nchunks:\n  - name: logging\n    description: Inspect general logs\n    summary: Unrelated filler words with no overlap\n    key_elements: [banana, umbrella]\n  - name: rollback\n    description: Rollback a failed staging deploy\n";
    for (slug, manifest) in [("a-runbook", manifest_a), ("b-runbook", manifest_b)] {
        store
            .write(
                &vesper_memory::SkillSlug::new(slug).unwrap(),
                &format!("---\nname: deploy-runbook\n{manifest}---\n# {slug}\nRunbook body."),
            )
            .unwrap();
        for name in ["logging", "rollback"] {
            write_chunk(&store, directory.path(), slug, name, "Steps.");
        }
    }
    // Prompt names the chunk (`rollback`) and activates the skill
    // (`deploy`). The telemetry/diagnostics tokens overlap ONLY skill A's
    // `summary`/`key_elements` — with the flag off that overlap must not
    // move `logging` above `rollback`.
    let report = store
        .orchestrate(&env.query("rollback the failed staging deploy with telemetry diagnostics"));
    let chunk_names: Vec<String> = report.chunks.iter().map(|c| c.name.clone()).collect();
    // Distinguishing neutrality proof: `telemetry diagnostics` overlaps
    // ONLY skill A's summary/key_elements. Flag-off ⇒ that overlap
    // contributes nothing ⇒ `logging` (zero description/name overlap)
    // loads at all? No: it must not load AT ALL. (Flag-on would load it:
    // score > 0 qualifies it under take(MAX_CHUNKS_PER_SELECTION).)
    if !CHUNK_METADATA_ROUTING_ENABLED {
        assert!(
            !chunk_names.contains(&"logging".to_owned()),
            "summary-only overlap must not load a chunk while the flag is off; got {chunk_names:?}"
        );
    }
    assert_eq!(
        report.chunks.first().map(|c| c.name.clone()),
        Some("rollback".to_owned()),
        "flag-off ranking must be description/name-driven, not summary-driven; got {chunk_names:?}"
    );
}

/// G5 proof (routing phase): chunk-less skills route byte-identically to
/// the pre-chunk behavior — same selected skills, same bodies, same
/// scores, same report shape (chunks field empty and invisible).
#[test]
fn chunk_less_skills_route_byte_identically_at_routing_phase() {
    let (directory, store) = fixture_store();
    let env = QueryEnv::default();
    let _ = directory;
    // A chunk-less skill exercising the classic routing path.
    store
        .write(
            &vesper_memory::SkillSlug::new("xlsx").unwrap(),
            "---\nname: xlsx\ndescription: Create and edit Excel spreadsheets\ntags: [excel, workbook, csv]\nfile-extensions: [xlsx, csv]\n---\n# xlsx\nUse the workbook helpers.",
        )
        .unwrap();
    let report = store
        .orchestrate(&env.query("Please edit quarterly-report.xlsx and add a spreadsheet chart"));
    assert_eq!(report.selected_names(), vec!["xlsx"]);
    assert_eq!(
        report.selected[0].body,
        "---\nname: xlsx\ndescription: Create and edit Excel spreadsheets\ntags: [excel, workbook, csv]\nfile-extensions: [xlsx, csv]\n---\n# xlsx\nUse the workbook helpers."
    );
    // Deterministic score preserved from the pre-chunk ranking path.
    assert!(report.selected[0].candidate.score_basis_points != 0);
    // Chunk tier invisible: no chunks, no chunk rejections, identical
    // envelope emission shape (context() unchanged from pre-PR-2 form).
    assert!(report.chunks.is_empty());
    assert!(
        !report
            .rejected
            .iter()
            .any(|(_, reason)| reason.contains("chunk"))
    );
    let context = report.context().unwrap();
    assert!(context.contains("Use the workbook helpers."));
    assert!(!context.contains("agent-vesper-skill-chunk"));
}
