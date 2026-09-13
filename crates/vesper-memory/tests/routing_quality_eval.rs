//! Offline quality measurements. Failing promotion metrics remain published HOLDs.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use vesper_memory::routing_quality::{
    RoutingAblation, RoutingDescriptor, RoutingEffect, RoutingMode, RoutingOptions,
};
use vesper_memory::{SkillRoutingQuery, SkillSlug, SkillStore};

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    category: String,
    family: String,
    split: String,
    prompt: String,
    acceptable: Vec<String>,
    forbidden: Vec<String>,
    explicit: Option<String>,
    resources: Option<Vec<String>>,
}

fn fixtures() -> BTreeMap<String, (String, RoutingDescriptor)> {
    let mut fixtures = BTreeMap::new();
    for (family, topic, first, second) in [
        ("database", "migration", "inspect", "execute"),
        ("release", "release notes", "draft", "publish"),
        ("document", "document", "read", "overwrite"),
        ("messaging", "message", "draft", "send"),
        ("spreadsheet", "workbook", "read", "rewrite"),
    ] {
        for (action, opposite) in [(first, second), (second, first), ("prepare", "")] {
            let slug = format!("fixture-{family}-{action}");
            let purpose = format!("{action} {topic} artifact for {family} tasks");
            let effect = if action == first {
                RoutingEffect::ReadOnly
            } else if ["publish", "send"].contains(&action) {
                RoutingEffect::External
            } else {
                RoutingEffect::Workspace
            };
            let risk = match effect {
                RoutingEffect::ReadOnly => "read-only",
                RoutingEffect::Workspace => "mutating",
                RoutingEffect::External => "external",
            };
            let body = format!(
                "---\nname: {slug}\ndescription: {purpose}\nrisk: {risk}\n---\nSynthetic procedure for offline routing evaluation. Never executed.\n"
            );
            fixtures.insert(
                slug,
                (
                    body,
                    RoutingDescriptor {
                        version: 1,
                        revision: String::new(),
                        family: family.into(),
                        purpose,
                        use_when: vec![format!("Requested operation is {action}")],
                        actions: vec![action.into()],
                        avoid_when: if opposite.is_empty() {
                            vec![]
                        } else {
                            vec![opposite.into()]
                        },
                        inputs: vec![],
                        outputs: vec![family.into()],
                        preconditions: if action == "prepare" {
                            vec!["template".into()]
                        } else {
                            vec![]
                        },
                        effects: effect,
                        positive_examples: vec![],
                        negative_examples: vec![],
                    },
                ),
            );
        }
    }
    fixtures
}

#[test]
fn frozen_quality_matrix_records_every_prediction_and_does_not_hide_failed_gates() {
    let corpus: Corpus = serde_json::from_str(include_str!("routing_quality_cases.json")).unwrap();
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/skills");
    let required: BTreeSet<_> = corpus
        .cases
        .iter()
        .filter(|c| c.category != "sibling" && c.category != "resource")
        .flat_map(|c| c.acceptable.iter().cloned())
        .collect();
    let mut real = BTreeMap::new();
    for entry in std::fs::read_dir(&library).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|x| x == "md") {
            let slug = path.file_stem().unwrap().to_str().unwrap();
            if slug != "AGENTS" {
                real.insert(slug.to_owned(), std::fs::read_to_string(path).unwrap());
            }
        }
    }
    let synthetic = fixtures();
    let mut records = Vec::new();
    let mut snapshots = BTreeMap::new();
    let mut summary = BTreeMap::<String, [usize; 7]>::new();
    let mut invariance = BTreeMap::new();
    for size in [25, 100, 250, 500] {
        // Two cohorts keep every target in even the 25-entry snapshots:
        // real procedures (160 cases) and synthetic contracts (80 cases).
        for cohort in ["real", "synthetic"] {
            let mut snapshot = BTreeMap::new();
            if cohort == "real" {
                for slug in &required {
                    snapshot.insert(slug.clone(), real[slug].clone());
                }
                for (slug, body) in &real {
                    if snapshot.len() >= size {
                        break;
                    }
                    snapshot.entry(slug.clone()).or_insert(body.clone());
                }
            } else {
                for (slug, (body, _)) in &synthetic {
                    snapshot.insert(slug.clone(), body.clone());
                }
            }
            let mut distractor = 0;
            while snapshot.len() < size {
                // Marked synthetic, unrelated and near-duplicate distractors.
                let slug = format!("fixture-distractor-{distractor:03}");
                snapshot.insert(slug.clone(), format!("---\nname: {slug}\ndescription: Catalog mineral specimens and classify igneous geological samples\n---\nSynthetic distractor.\n"));
                distractor += 1;
            }
            assert_eq!(snapshot.len(), size);
            snapshots.insert(
                format!("{size}:{cohort}"),
                Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            );
            for order in 0..3 {
                let temp = tempfile::tempdir().unwrap();
                let store = SkillStore::open(temp.path()).unwrap();
                let mut names: Vec<_> = snapshot.keys().cloned().collect();
                match order {
                    1 => names.reverse(),
                    2 => names.rotate_left(size / 3),
                    _ => {}
                }
                for slug in names {
                    store
                        .write(&SkillSlug::new(&slug).unwrap(), &snapshot[&slug])
                        .unwrap();
                    let chunks = library.join(&slug).join("chunks");
                    if chunks.is_dir() {
                        let destination = temp.path().join("skills").join(&slug).join("chunks");
                        std::fs::create_dir_all(&destination).unwrap();
                        for chunk in std::fs::read_dir(chunks).unwrap() {
                            let chunk = chunk.unwrap();
                            if chunk.path().is_file() {
                                std::fs::copy(chunk.path(), destination.join(chunk.file_name()))
                                    .unwrap();
                            }
                        }
                    }
                    if let Some((_, descriptor)) = synthetic.get(&slug) {
                        let mut descriptor = descriptor.clone();
                        descriptor.revision = store
                            .routing_revision(&SkillSlug::new(&slug).unwrap())
                            .unwrap();
                        std::fs::write(
                            temp.path().join(format!("skills/{slug}.routing.json")),
                            serde_json::to_vec(&descriptor).unwrap(),
                        )
                        .unwrap();
                    }
                }
                let tools: BTreeSet<String> = [
                    "read_file",
                    "write_file",
                    "run_command",
                    "delegate_task",
                    "read_skill",
                    "web_search",
                    "web_fetch",
                    "web_reader",
                    "browser_ui",
                ]
                .into_iter()
                .map(String::from)
                .collect();
                for (label, mode, ablation) in [
                    ("Standard", RoutingMode::Standard, RoutingAblation::Full),
                    (
                        "DescriptorsOld",
                        RoutingMode::Enhanced,
                        RoutingAblation::DescriptorsWithStandardScorer,
                    ),
                    (
                        "LexicalOnly",
                        RoutingMode::Enhanced,
                        RoutingAblation::LexicalWithoutContracts,
                    ),
                    ("Enhanced", RoutingMode::Enhanced, RoutingAblation::Full),
                ] {
                    for case in corpus.cases.iter().filter(|c| {
                        (cohort == "synthetic")
                            == (c.category == "sibling" || c.category == "resource")
                    }) {
                        let mut options = RoutingOptions::default();
                        options.preferences.mode = mode;
                        if cohort == "synthetic" {
                            options.task.artifact = Some(case.family.clone());
                            options.task.available_resources =
                                case.resources.as_ref().map(|r| r.iter().cloned().collect());
                        }
                        let report = store.orchestrate_with_ablation(
                            &SkillRoutingQuery {
                                prompt: &case.prompt,
                                explicit_skill: case.explicit.as_deref(),
                                available_tools: &tools,
                                platform: "linux",
                                outcome_adjustments: &BTreeMap::new(),
                            },
                            &options,
                            ablation,
                        );
                        let names = report.selected_names();
                        assert!(names.len() <= 3);
                        assert!(
                            report
                                .selected
                                .iter()
                                .map(|s| s.body.chars().count()
                                    + s.chunks
                                        .iter()
                                        .map(|c| c.body.chars().count())
                                        .sum::<usize>())
                                .sum::<usize>()
                                <= 60_000
                        );
                        let key = format!("{size}:{label}:{}", case.id);
                        let observation = (names.clone(), report.explicit_error.clone());
                        if let Some(previous) = invariance.insert(key, observation.clone()) {
                            assert_eq!(previous, observation, "order sensitivity");
                        }
                        if order != 0 {
                            continue;
                        }
                        let key = format!("{size}:{label}:{}:{}", case.split, case.category);
                        let metric = summary.entry(key).or_default();
                        metric[0] += 1;
                        metric[1] +=
                            usize::from(names.first().is_some_and(|n| case.acceptable.contains(n)));
                        metric[2] += usize::from(names.iter().any(|n| case.acceptable.contains(n)));
                        metric[3] += usize::from(names.is_empty() && !case.acceptable.is_empty());
                        metric[4] += usize::from(
                            !names.is_empty()
                                && case.acceptable.is_empty()
                                && case.category != "syntax",
                        );
                        metric[5] += usize::from(names.iter().any(|n| case.forbidden.contains(n)));
                        metric[6] += usize::from(
                            case.category == "syntax"
                                && case.explicit.is_none()
                                && report.explicit_error.is_some(),
                        );
                        records.push(serde_json::json!({"size":size,"cohort":cohort,"mode":label,"id":case.id,"split":case.split,"selected":names,"explicit_error":report.explicit_error}));
                    }
                }
            }
        }
    }
    for (key, metric) in &summary {
        println!("QUALITY {key} {metric:?}");
    }
    let output = serde_json::json!({"columns":["n","hit1","recall3","false_abstention","false_activation","harmful_sibling","literal_explicit_error"],"summary":summary,"snapshots_sha256":snapshots,"predictions":records,"order_permutations":3,"status":"measurement; promotion requires PRD review"});
    if let Ok(path) = std::env::var("VESPER_ROUTING_EVAL_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }
}
