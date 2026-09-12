//! Advanced context paging PR-4 (D3 eval gate): deterministic offline
//! evaluation harness for chunk-routing metadata. See
//! `docs/advanced-context-paging-prd.md` §4 D3, §5 PR-4, §6 AC-4.
//!
//! Strict ablation: the ONLY varied axis is the routing condition
//! (`FLAT_DESCRIPTION` vs `SUMMARY_ONLY` vs `SUMMARY_KEY_ELEMENTS`); the
//! fixture corpus, queries, expected targets, budgets, and ranking
//! arithmetic are identical across conditions. No provider, network, or
//! process I/O — the harness evaluates the router directly (the provider
//! seam is exercised by the PR-3 composition suite; the synthetic-provider
//! injection leg lives in `vesper-harness` per the architecture boundary).
//!
//! The full-metrics test prints the canonical results table that the eval
//! report (`docs/foundation/context-paging-pr4-eval.md`) records verbatim;
//! run with `-- --nocapture` to emit it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use vesper_memory::{
    CHUNK_METADATA_ROUTING_ENABLED, ChunkRoutingCondition, SkillRoutingQuery, SkillSlug, SkillStore,
};

// ---------------------------------------------------------------------------
// fixture corpus: two task families, deterministic expected targets
// ---------------------------------------------------------------------------

struct CorpusCase {
    family: &'static str,
    slug: &'static str,
    /// Manifest text (frontmatter block).
    manifest: &'static str,
    /// Chunk files: (name, body).
    chunks: &'static [(&'static str, &'static str)],
    /// Query + the expected target chunk name.
    query: &'static str,
    expected: &'static str,
}

/// Family 1: factual retrieval — a reference skill whose chunks are
/// topic-indexed entries. Family 2: procedure application — a runbook
/// whose chunks are operational procedures.
const CORPUS: &[CorpusCase] = &[
    // Family: factual retrieval //////////////////////////////////////
    CorpusCase {
        family: "factual-retrieval",
        slug: "observability-reference",
        manifest: "---\nname: observability-reference\ndescription: Observability reference for telemetry and reliability\nchunks:\n  - name: metrics\n    description: Metric collection and dashboards\n    summary: Telemetry ingestion pipelines and metric cardinality control\n    key_elements: [rollup, recording-rules]\n  - name: traces\n    description: Distributed tracing and span correlation\n    summary: Trace propagation headers and span sampling strategies\n    key_elements: [trace, span, sampling]\n  - name: logs\n    description: Structured logging and log retention\n    summary: Log pipelines with retention windows and redaction\n    key_elements: [retention, redaction]\n",
        chunks: &[
            ("metrics", "Metrics chunk body."),
            ("traces", "Traces chunk body."),
            ("logs", "Logs chunk body."),
        ],
        // "span sampling" appears in traces' summary/key_elements only —
        // the description says "span correlation"... wait, it does appear.
        // This case is description-sufficient.
        query: "use skill observability-reference: how do distributed tracing spans correlate",
        expected: "traces",
    },
    CorpusCase {
        family: "factual-retrieval",
        slug: "observability-reference",
        manifest: "---\nname: observability-reference\ndescription: Observability reference for telemetry and reliability\nchunks:\n  - name: metrics\n    description: Metric collection and dashboards\n    summary: Telemetry ingestion pipelines and metric cardinality control\n    key_elements: [rollup, recording-rules]\n  - name: traces\n    description: Distributed tracing and span correlation\n    summary: Trace propagation headers and span sampling strategies\n    key_elements: [trace, span, sampling]\n  - name: logs\n    description: Structured logging and log retention\n    summary: Log pipelines with retention windows and redaction\n    key_elements: [retention, redaction]\n",
        chunks: &[
            ("metrics", "Metrics chunk body."),
            ("traces", "Traces chunk body."),
            ("logs", "Logs chunk body."),
        ],
        // Probe A: "telemetry ingestion pipelines" exists ONLY in metrics'
        // summary (key_elements deliberately disjoint: rollup, recording).
        query: "use skill observability-reference: telemetry ingestion pipelines",
        expected: "metrics",
    },
    // Family: procedure application //////////////////////////////////
    CorpusCase {
        family: "procedure-application",
        slug: "cutover-runbook",
        manifest: "---\nname: cutover-runbook\ndescription: Operational runbook for staging and production cutover procedures\nchunks:\n  - name: rollback\n    description: Undo an unsuccessful release\n    summary: Database rollback choreography and restore points\n    key_elements: [restore-point, choreography]\n  - name: migrate\n    description: Apply forward database changes\n    summary: Forward-only migration discipline and ordering\n    key_elements: [canary, rehearsal]\n  - name: certificates\n    description: Rotate TLS certificates\n    summary: Certificate rotation ceremony and expiry monitoring\n    key_elements: [expiry, ceremony]\n",
        chunks: &[
            ("rollback", "Rollback chunk body."),
            ("migrate", "Migration chunk body."),
            ("certificates", "Certificate chunk body."),
        ],
        query: "use skill cutover-runbook: rotate the tls certificates",
        expected: "certificates",
    },
    CorpusCase {
        family: "procedure-application",
        slug: "cutover-runbook",
        manifest: "---\nname: cutover-runbook\ndescription: Operational runbook for staging and production cutover procedures\nchunks:\n  - name: rollback\n    description: Undo an unsuccessful release\n    summary: Database rollback choreography and restore points\n    key_elements: [restore-point, choreography]\n  - name: migrate\n    description: Apply forward database changes\n    summary: Forward-only migration discipline and ordering\n    key_elements: [canary, rehearsal]\n  - name: certificates\n    description: Rotate TLS certificates\n    summary: Certificate rotation ceremony and expiry monitoring\n    key_elements: [expiry, ceremony]\n",
        chunks: &[
            ("rollback", "Rollback chunk body."),
            ("migrate", "Migration chunk body."),
            ("certificates", "Certificate chunk body."),
        ],
        // Probe B: "canary rehearsal" exists ONLY in migrate's
        // key_elements — deliberately absent from every description AND
        // summary so only SUMMARY_KEY_ELEMENTS can resolve it (the
        // hyphen in a "dry-run" probe would tokenize into "run" and leak
        // into the migrate description).
        query: "use skill cutover-runbook: canary rehearsal",
        expected: "migrate",
    },
];

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

fn build_store(case: &CorpusCase) -> (tempfile::TempDir, SkillStore) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("memory-root");
    std::fs::create_dir_all(&root).unwrap();
    let store = SkillStore::open(&root).unwrap();
    store
        .write(
            &SkillSlug::new(case.slug).unwrap(),
            &format!("{}---\n# {}\nPrimary body.", case.manifest, case.slug),
        )
        .unwrap();
    for (name, body) in case.chunks {
        let dir = root.join("skills").join(case.slug).join("chunks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
    }
    (directory, store)
}

fn run_case(case: &CorpusCase, condition: ChunkRoutingCondition) -> (bool, usize, usize, usize) {
    let (_dir, store) = build_store(case);
    let env = QueryEnv::default();
    let report = store.orchestrate_with_condition(&env.query(case.query), condition);
    let selected = report.selected_names();
    if selected.is_empty() {
        return (false, 0, 0, 0);
    }
    let skill = &report.selected[0];
    let positions: Vec<&str> = skill.chunks.iter().map(|c| c.name.as_str()).collect();
    let success = positions.first() == Some(&case.expected);
    let irrelevant_before = positions
        .iter()
        .position(|n| *n == case.expected)
        .unwrap_or(positions.len());
    let overhead = store.chunk_routing_metrics(&report, condition)[0].2;
    (success, irrelevant_before, skill.chunks.len(), overhead)
}

// ---------------------------------------------------------------------------
// harness self-tests (PRD PR-4: determinism, condition isolation, invariants)
// ---------------------------------------------------------------------------

#[test]
fn harness_fixture_determinism() {
    // Same case + same condition → identical results across repeated runs
    // (fixture determinism; no hidden state).
    let case = &CORPUS[1]; // metadata-only case
    for condition in [
        ChunkRoutingCondition::FlatDescription,
        ChunkRoutingCondition::SummaryOnly,
        ChunkRoutingCondition::SummaryKeyElements,
    ] {
        let first = run_case(case, condition);
        for _ in 0..3 {
            assert_eq!(run_case(case, condition), first);
        }
    }
}

#[test]
fn harness_condition_isolation() {
    // Isolation proof: a query whose discriminating tokens live ONLY in
    // metadata routes differently across conditions, while a
    // description-sufficient query routes identically across conditions.
    let metadata_case = &CORPUS[1];
    let flat = run_case(metadata_case, ChunkRoutingCondition::FlatDescription);
    let full = run_case(metadata_case, ChunkRoutingCondition::SummaryKeyElements);
    // Flat cannot see "cardinality" (summary/key_elements only): the chunk
    // must NOT be routed as the top hit (or not loaded at all).
    assert!(
        !flat.0,
        "flat routing must fail the metadata-only case; got {:?}",
        flat
    );
    // The metadata conditions CAN: observable difference proves the
    // ablation axis is actually varied.
    let summary_only = run_case(metadata_case, ChunkRoutingCondition::SummaryOnly);
    assert!(
        summary_only.0,
        "summary must resolve the metadata-only case; got {:?}",
        summary_only
    );
    assert!(full.0, "full metadata must resolve the metadata-only case");

    let description_case = &CORPUS[0];
    for condition in [
        ChunkRoutingCondition::FlatDescription,
        ChunkRoutingCondition::SummaryOnly,
        ChunkRoutingCondition::SummaryKeyElements,
    ] {
        let result = run_case(description_case, condition);
        assert!(
            result.0,
            "description-sufficient case must route correctly under every condition: {condition:?} → {result:?}"
        );
    }
}

#[test]
fn harness_metric_invariants() {
    // Invariants: overhead(FLAT) == 0; overhead(SUMMARY_ONLY) ≤
    // overhead(SUMMARY_KEY_ELEMENTS); routed-chunk counts ≤ cap; success
    // measured against the expected FIRST position.
    for case in CORPUS {
        let flat = run_case(case, ChunkRoutingCondition::FlatDescription);
        let summary = run_case(case, ChunkRoutingCondition::SummaryOnly);
        let full = run_case(case, ChunkRoutingCondition::SummaryKeyElements);
        assert_eq!(flat.3, 0, "flat adds no metadata overhead");
        assert!(summary.3 <= full.3, "summary-only overhead ≤ full overhead");
        assert!(
            flat.2 <= 3 && summary.2 <= 3 && full.2 <= 3,
            "routed-chunk counts respect the selection cap"
        );
    }
}

// ---------------------------------------------------------------------------
// full-metrics run (emits the canonical results table)
// ---------------------------------------------------------------------------

#[test]
fn full_metrics_run_emits_canonical_table() {
    let mut rows: Vec<String> = Vec::new();
    for case in CORPUS {
        for condition in [
            ChunkRoutingCondition::FlatDescription,
            ChunkRoutingCondition::SummaryOnly,
            ChunkRoutingCondition::SummaryKeyElements,
        ] {
            let (success, irrelevant_before, routed, overhead) = run_case(case, condition);
            rows.push(format!(
                "| {} | {} | {:?} | {} | {} | {} | {} |",
                case.family, case.slug, condition, success, irrelevant_before, routed, overhead,
            ));
        }
    }
    let table = format!(
        "| family | skill | condition | success | irrelevant_before | routed | overhead_tokens |\n|---|---|---|---|---|---|---|\n{}",
        rows.join("\n")
    );
    // Print for --nocapture capture into the report.
    println!("\n=== D3 EVAL RESULTS ===\n{table}\n");
    // Determinism of the table itself.
    assert!(rows.len() == CORPUS.len() * 3);
    // Sanity: at least one success per condition (the corpus contains
    // description-sufficient cases).
    for condition in ["FlatDescription", "SummaryOnly", "SummaryKeyElements"] {
        let successes = rows
            .iter()
            .filter(|row| row.contains(condition) && row.contains("| true |"))
            .count();
        assert!(successes > 0, "{condition} must succeed somewhere");
    }
}

// ---------------------------------------------------------------------------
// D3 verdict computation (decision rule: repeat across >1 family)
// ---------------------------------------------------------------------------

#[test]
fn d3_verdict_follows_the_decision_rule() {
    // Compute per-family success deltas (metadata − flat) for the
    // metadata-sensitive cases; the flag may flip ONLY if improvement
    // repeats across more than one family.
    let mut family_improved: BTreeMap<&str, bool> = BTreeMap::new();
    for case in CORPUS {
        let flat = run_case(case, ChunkRoutingCondition::FlatDescription);
        let full = run_case(case, ChunkRoutingCondition::SummaryKeyElements);
        let improved = full.0 && !flat.0;
        *family_improved.entry(case.family).or_insert(false) |= improved;
    }
    let improved_families = family_improved
        .iter()
        .filter(|(_, improved)| **improved)
        .count();
    let verdict_adopt = improved_families > 1;
    println!(
        "\n=== D3 VERDICT INPUT ===\nfamilies improved: {:?}, count: {improved_families}, adopt={verdict_adopt}\nflag currently false={}\n",
        family_improved, !CHUNK_METADATA_ROUTING_ENABLED
    );
    // The flag must match the computed verdict's decision rule.
    // (This assertion documents the rule; the shipped flag state stays
    // tied to the recorded verdict in the eval report.)
    // Shipped-flag/verdict consistency in both directions, expressed as a
    // single computed equality so neither branch is constant-folded: the
    // flag state must equal the computed adopt decision (a future REJECT
    // re-eval flips it back the same way).
    assert_eq!(
        CHUNK_METADATA_ROUTING_ENABLED, verdict_adopt,
        "shipped flag must equal the computed D3 adopt decision (adopt={verdict_adopt})"
    );
}

// ---------------------------------------------------------------------------
// PR-1 anchor (ranker-hardening PRD, Option D): the cross-talk pin
//
// A fixture whose ONLY overlap between prompt and chunk routing text is
// alias-manufactured, reproducing the PR-4 incident shape:
//   prompt slug `deploy-runbook` → token `deploy` is an alias SOURCE
//   (`:1384`) → prompt pool gains {publish, release, production};
//   rollback's literal `release` (`:1385` related) → chunk pool gains
//   {publish, deploy, version}; the join at `:869` manufactures
//   {deploy, publish, release} = 3 tokens = 1,560 pts (:873).
//
// TODAY this fixture FAILS: rollback rides manufactured overlap (1,560 +
// cosine ≈ ≥1,700) past migrate's honest two-token score (1,040 + cosine)
// and displaces it. It must keep failing until Option A (chunk pools from
// raw stemmed tokens) lands; afterwards the residual is bounded to one
// 520-pt literal match (prompt `deploy→release` expansion × rollback's
// literal `release`), which must LOSE to migrate's honest 1,040.
//
// Deliberately NOT part of CORPUS: the pin must not perturb the D3 verdict
// inputs or the canonical results table (PRD M4).
// ---------------------------------------------------------------------------

/// Builds the pin skill directly (mirrors `build_store`'s body, but with a
/// runtime slug so the manifest `name:` matches the directory). The manifest
/// text is PR-4's `cutover-runbook` corpus entry with the slug substituted:
/// rollback "Undo an unsuccessful release" (literal `release`, related to the
/// alias row `:1385`), migrate carrying `canary, rehearsal` in key_elements
/// (the probe's honest target), certificates distinct.
fn build_pin_store(slug: &str) -> (tempfile::TempDir, SkillStore) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("memory-root");
    std::fs::create_dir_all(&root).unwrap();
    let store = SkillStore::open(&root).unwrap();
    let manifest = format!(
        "---\nname: {slug}\ndescription: Operational runbook for staging and production cutover procedures\nchunks:\n  - name: rollback\n    description: Undo an unsuccessful release\n    summary: Database rollback choreography and restore points\n    key_elements: [restore-point, choreography]\n  - name: migrate\n    description: Apply forward database changes\n    summary: Forward-only migration discipline and ordering\n    key_elements: [canary, rehearsal]\n  - name: certificates\n    description: Rotate TLS certificates\n    summary: Certificate rotation ceremony and expiry monitoring\n    key_elements: [expiry, ceremony]\n"
    );
    store
        .write(
            &SkillSlug::new(slug).unwrap(),
            &format!("{manifest}---\n# {slug}\nPrimary body."),
        )
        .unwrap();
    for (name, body) in [
        ("rollback", "Rollback chunk body."),
        ("migrate", "Migration chunk body."),
        ("certificates", "Certificate chunk body."),
    ] {
        let dir = root.join("skills").join(slug).join("chunks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
    }
    (directory, store)
}

/// Runs the probe under full metadata routing and returns the routed
/// chunk-name order of the first selected skill.
fn routed_positions(store: &SkillStore, prompt: &str) -> Vec<String> {
    let env = QueryEnv::default();
    let report = store.orchestrate_with_condition(
        &env.query(prompt),
        ChunkRoutingCondition::SummaryKeyElements,
    );
    assert!(
        !report.selected.is_empty(),
        "pin skill must be selected (explicit marker): {prompt}"
    );
    report.selected[0]
        .chunks
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

/// The pin: slug token `deploy` is an alias source; the ONLY prompt<->rollback
/// overlap is manufactured by the two-sided alias fan-out. Desired behavior
/// is `migrate` first (honest two-token key_elements overlap: canary,
/// rehearsal). Pre-fix this FAILS: rollback's manufactured 3-token overlap
/// (1,560 pts) plus cosine outranks migrate. Post-Option-A it must PASS:
/// rollback falls to its bounded 520-pt literal residual, which loses to
/// migrate's honest 1,040.
///
/// Post-Option-A invariant: this test must now PASS. Any change that
/// reintroduces two-sided alias expansion into the chunk tier (or degrades
/// migrate's honest overlap) fails it. The pre-fix failure receipt is
/// recorded in `docs/foundation/ranker-hardening-pr1-execution.md` §4.1.
#[test]
fn cross_talk_pin_deploy_slug_manufactures_rollback_overlap() {
    let (_dir, store) = build_pin_store("deploy-runbook");
    let positions = routed_positions(&store, "use skill deploy-runbook: canary rehearsal");
    assert_eq!(
        positions.first().map(String::as_str),
        Some("migrate"),
        "\nCROSS-TALK PIN regression: chunk-tier alias expansion leaked back.\n\
         Expected migrate first (honest two-token overlap); got rollback,\n\
         which can only win via the manufactured two-sided fan-out\n\
         ({{deploy, publish, release}} = 1,560 pts).\n\
         positions: {positions:?}"
    );
}

/// No-alias control: identical fixture shape and probe, but an alias-free
/// slug. Proves the displacement mechanism is the alias fan-out, not general
/// overlap. Must pass BOTH pre- and post-fix - Option A is chunk-pool-local
/// and must never degrade honest overlap.
#[test]
fn cross_talk_control_no_alias_slug_routes_honestly() {
    let (_dir, store) = build_pin_store("cutover-runbook");
    let positions = routed_positions(&store, "use skill cutover-runbook: canary rehearsal");
    assert_eq!(
        positions.first().map(String::as_str),
        Some("migrate"),
        "no-alias control must route migrate first, got {positions:?}"
    );
}

// keep Path import used for future corpus-directory variants
#[allow(dead_code)]
fn _path_helper(_: &Path) {}
