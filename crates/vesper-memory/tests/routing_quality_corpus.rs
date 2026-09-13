use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use vesper_memory::{SkillRoutingQuery, SkillStore};

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    category: String,
    family: String,
    group: String,
    split: String,
    prompt: String,
    acceptable: Vec<String>,
    forbidden: Vec<String>,
    explicit: Option<String>,
}
fn corpus() -> Corpus {
    serde_json::from_str(include_str!("routing_quality_cases.json")).unwrap()
}

#[test]
fn corpus_has_frozen_disjoint_labels_and_required_coverage() {
    let corpus = corpus();
    let mut counts = BTreeMap::new();
    let mut splits = BTreeMap::new();
    let mut groups = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut families = BTreeSet::new();
    let mut unnamed = 0;
    for case in &corpus.cases {
        assert!(ids.insert(&case.id));
        assert!(!case.prompt.trim().is_empty());
        assert!(
            case.acceptable
                .iter()
                .all(|slug| !case.forbidden.contains(slug))
        );
        *counts.entry(case.category.as_str()).or_insert(0) += 1;
        *splits.entry(case.split.as_str()).or_insert(0) += 1;
        if let Some(previous) = groups.insert(&case.group, &case.split) {
            assert_eq!(previous, &case.split, "group leakage");
        }
        if case.category == "positive" {
            families.insert(&case.family);
            if case.acceptable.iter().all(|slug| {
                !case
                    .prompt
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric() && c != '-')
                    .any(|word| word == slug)
            }) {
                unnamed += 1;
            }
        }
    }
    assert_eq!(corpus.cases.len(), 240);
    assert_eq!(
        counts,
        BTreeMap::from([
            ("positive", 80),
            ("no_skill", 40),
            ("sibling", 40),
            ("syntax", 40),
            ("resource", 40)
        ])
    );
    assert_eq!(
        splits,
        BTreeMap::from([("development", 120), ("held_out", 120)])
    );
    assert!(families.len() >= 12);
    assert!(
        unnamed >= 60,
        "only {unnamed} natural requests omit the skill slug"
    );
}

/// Records actual baseline errors; does not declare the accuracy gate passed.
#[test]
fn current_catalog_baseline_records_predictions_without_relabeling() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills")
        .canonicalize()
        .unwrap();
    let store = SkillStore::open(&root).unwrap();
    let catalog: BTreeSet<_> = store.list().into_iter().map(|s| s.slug).collect();
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    let mut measured = 0;
    let mut hit = 0;
    let mut false_activation = 0;
    let mut explicit_errors = 0;
    for case in corpus().cases.into_iter().filter(|case| {
        case.category == "positive" || case.category == "no_skill" || case.category == "syntax"
    }) {
        for slug in &case.acceptable {
            assert!(
                catalog.contains(slug),
                "label references absent skill {slug}"
            );
        }
        let result = store.orchestrate(&SkillRoutingQuery {
            prompt: &case.prompt,
            explicit_skill: case.explicit.as_deref(),
            available_tools: &tools,
            platform: "linux",
            outcome_adjustments: &outcomes,
        });
        let names = result.selected_names();
        if case.category == "positive" {
            measured += 1;
            if names.iter().any(|n| case.acceptable.contains(n)) {
                hit += 1;
            }
        }
        if case.category == "no_skill" && !names.is_empty() {
            false_activation += 1;
        }
        if case.category == "syntax" && case.explicit.is_none() && result.explicit_error.is_some() {
            explicit_errors += 1;
        }
        println!(
            "{}",
            serde_json::json!({"id":case.id,"split":case.split,"selected":names,"explicit_error":result.explicit_error})
        );
    }
    println!(
        "BASELINE positives={measured} recall_hits={hit} no_skill_false_activations={false_activation}/40 literal_explicit_errors={explicit_errors}/20"
    );
    assert_eq!(measured, 80);
}
