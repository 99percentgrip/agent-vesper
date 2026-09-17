//! Exemplar migration Stage D (recon blueprint `recon_exemplar_candidate.md`):
//! manifest proof, per-chunk routing proofs (one synthetic prompt per chunk
//! topic), budget proof, and the G5 chunk-less proof — all against the real
//! shipped `research-paper-writing` skill, not fixtures.

use std::collections::{BTreeMap, BTreeSet};

use vesper_memory::{ChunkRoutingCondition, SkillRoutingQuery, SkillSlug, SkillStore};

const SKILL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills");

fn exemplar_store() -> SkillStore {
    SkillStore::open(std::path::Path::new(SKILL_DIR)).unwrap()
}

fn slug() -> SkillSlug {
    SkillSlug::new("research-paper-writing").unwrap()
}

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

fn env() -> QueryEnv {
    QueryEnv {
        tools: BTreeSet::new(),
        outcomes: BTreeMap::new(),
    }
}

#[test]
fn manifest_proof_sixteen_valid_entries_all_files_present_under_cap() {
    let store = exemplar_store();
    let body = store.read(&slug()).unwrap();
    let summary = vesper_memory::SkillSummary {
        slug: "research-paper-writing".to_owned(),
        headline: String::new(),
    };
    let metadata = vesper_memory::parse_metadata(&summary, &body);
    assert!(
        metadata.chunk_manifest_error.is_none(),
        "manifest error: {:?}",
        metadata.chunk_manifest_error
    );
    assert_eq!(metadata.chunks.len(), 16, "expected 16 manifest entries");
    for entry in &metadata.chunks {
        entry
            .validate()
            .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
    }
    // Disk-level validation runs inside orchestrate; the routing proofs below
    // assert its success indirectly (a broken manifest would make the skill
    // routing-ineligible with `invalid chunk manifest` and the skill would
    // not appear in `selected`).
}

#[test]
fn routing_proofs_target_chunk_ranks_first_for_its_phase() {
    let store = exemplar_store();
    let query_env = env();
    let cases: &[(&str, &str)] = &[
        (
            "phase0-setup",
            "Set up the paper project: workspace, contribution framing, compute budget, and co-author conventions.",
        ),
        (
            "phase1-literature-review",
            "Search related work and verify every citation with bibtex retrieval by DOI before adding to the bibliography.",
        ),
        (
            "phase2-experiment-design",
            "Map claims to experiments and design baselines with an evaluation protocol and annotator study.",
        ),
        (
            "phase3-execution-monitoring",
            "Experiments are running: set up cron monitoring with the silent mode check and handle rate-limit recovery.",
        ),
        (
            "phase4-analysis",
            "Aggregate the finished results, compute error bars and significance, and identify the story for the figures.",
        ),
        (
            "phase5-drafting",
            "Draft the paper: latex scaffolding, abstract and introduction formulas.",
        ),
        (
            "phase6-review-revision",
            "Simulate reviewers with the ensemble prompt, aggregate via meta-review, then run the visual review pass.",
        ),
        (
            "phase7-submission",
            "Prepare the camera-ready submission: reproduction package, citation export, and the deadline build checklist.",
        ),
        (
            "phase8-post-acceptance",
            "Paper accepted: prepare the poster, the talk, and the code release.",
        ),
        (
            "paper-types",
            "This is a survey paper, not an empirical study: what structure and evidence standards apply?",
        ),
        (
            "workshop-short-papers",
            "Target the four-page workshop short paper: how should scope be constrained?",
        ),
        (
            "iterative-refinement",
            "The draft is stuck in a generation-evaluation loop: convergence criteria, drift, and judge failure modes.",
        ),
        (
            "agent-integration",
            "Run the pipeline inside an agent loop with delegation and parallel search rounds.",
        ),
        (
            "reviewer-criteria",
            "Calibrate the simulated reviewers on the soundness, clarity, significance, originality rubric.",
        ),
        (
            "common-issues",
            "The pipeline has stalled on repeated failures: symptoms and remedies.",
        ),
        (
            "reference-docs",
            "Which deep-dive companion document covers citation workflow?",
        ),
    ];
    assert_eq!(cases.len(), 16);
    for (target, prompt) in cases {
        let query = SkillRoutingQuery {
            prompt,
            explicit_skill: Some("research-paper-writing"),
            available_tools: &query_env.tools,
            platform: "linux",
            outcome_adjustments: &query_env.outcomes,
        };
        let report =
            store.orchestrate_with_condition(&query, ChunkRoutingCondition::SummaryKeyElements);
        assert!(
            report
                .selected
                .iter()
                .any(|s| s.candidate.metadata.slug.as_str() == "research-paper-writing"),
            "skill not selected for case `{target}`"
        );
        let routed: Vec<&str> = report
            .selected
            .iter()
            .flat_map(|s| s.chunks.iter().map(|c| c.name.as_str()))
            .collect();
        assert!(!routed.is_empty(), "no chunks routed for case `{target}`");
        assert_eq!(routed[0], *target, "case `{target}` routed {routed:?}");
    }
}

#[test]
fn budget_proof_worst_case_activation_inside_per_skill_cap() {
    let store = exemplar_store();
    let query_env = env();
    // Worst case: explicit skill naming (max body weight) plus name-matches
    // on the largest chunks.
    let prompt = "use skill research-paper-writing: phase7-submission phase6-review-revision agent-integration phase4-analysis";
    let report = store.orchestrate(&query_env.query(prompt));
    let selected = report
        .selected
        .iter()
        .find(|s| s.candidate.metadata.slug.as_str() == "research-paper-writing")
        .expect("skill selected");
    let body_chars = selected.body.chars().count();
    let chunk_chars: usize = selected.chunks.iter().map(|c| c.body.chars().count()).sum();
    let total = body_chars + chunk_chars;
    assert!(
        total <= 24_000,
        "worst case {total} exceeds cap (body {body_chars} + chunks {chunk_chars})"
    );
    assert!(selected.chunks.len() <= 3, "chunk count cap violated");
}

#[test]
fn g5_proof_no_chunk_vocabulary_routes_lean_body_only() {
    let store = exemplar_store();
    let query_env = env();
    // No chunk vocabulary: the skill activates (name-match + description
    // overlap at the skill tier), but the prompt carries ZERO tokens from
    // any chunk manifest field. Chunk-tier activation therefore requires
    // phase/topic vocabulary — generic "I want a paper" requests load the
    // lean body alone.
    //
    // FIXTURE CORRECTION (score-floor PRD PR-3): the original prompt
    // ("...from finished trials...") was NOT vocabulary-free — `finished`
    // stems to `finish`, overlapping phase4-analysis' description "Load
    // after experiments finish:" (verified by exact offline replication;
    // 520-pt genuine token). The bounded `<= 3` era never exposed this
    // because phase4's honest admission hid inside the noise mass. The
    // corrected prompt is verified zero-overlap across all 16 chunk pools
    // while still carrying unhardened-ranker noise: phase8-post-acceptance
    // cosine +0.1826 (→ 401 pts), paper-types +0.0945, phase4 +0.0722 —
    // all sub-floor, none admittable without literal signal.
    let prompt = "turn these outcomes into a research publication on gating mixtures";
    let query = SkillRoutingQuery {
        prompt,
        explicit_skill: Some("research-paper-writing"),
        available_tools: &query_env.tools,
        platform: "linux",
        outcome_adjustments: &query_env.outcomes,
    };
    let report = store.orchestrate(&query);
    let selected = report
        .selected
        .iter()
        .find(|s| s.candidate.metadata.slug.as_str() == "research-paper-writing")
        .expect("skill selected");
    // G5 LITERAL (score-floor PRD PR-3, restored 2026-09-13): a prompt
    // with no chunk vocabulary routes the lean body only — ZERO chunks.
    // History: the original exemplar migration weakened this to a bounded
    // `<= 3` assertion because the pre-gate ranker admitted chunks on
    // positive signed-hash cosine alone (measured on this exact prompt:
    // phase3/phase4/phase8 routed with zero overlap, phase8 +0.309 →
    // 678 pts). PR-2's conjunction gate `(overlap >= 1 || name_match) &&
    // score >= MIN_CHUNK_ROUTING_SCORE` makes zero-signal admissions
    // structurally impossible, so the literal form holds. Non-vacuity
    // proven: the literal assertion FAILS on the pre-PR-2 tree (receipt
    // in docs/foundation/chunk-score-floor-pr3-execution.md). The lean
    // body's routing map remains the reachability fallback.
    assert!(
        selected.chunks.is_empty(),
        "G5 LITERAL: vocabulary-free prompt must route ZERO chunks, routed: {:?}",
        selected
            .chunks
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(selected.body.contains("When To Use This Skill"));
    assert!(selected.body.contains("Phase Routing Map"));
    assert!(!selected.body.contains("### Step 0.1"));
}
