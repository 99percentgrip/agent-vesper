//! Independently frozen labels are measured, never used to tune this runner.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use vesper_memory::routing_quality::{RoutingMode, RoutingOptions};
use vesper_memory::{SkillRoutingQuery, SkillStore};

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    kind: String,
    prompt: String,
    expected_skill_ids: Vec<String>,
    acceptable_abstention: bool,
    forbidden_skill_ids: Vec<String>,
    context: Context,
}
#[derive(Deserialize)]
struct Context {
    unavailable_tools: Vec<String>,
    denied_permissions: Vec<String>,
}

fn source_manifest(
    root: &std::path::Path,
    directory: &std::path::Path,
    files: &mut BTreeMap<String, String>,
) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            source_manifest(root, &path, files);
        } else if entry.file_type().unwrap().is_file() {
            let hash: String = Sha256::digest(std::fs::read(&path).unwrap())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            files.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                hash,
            );
        }
    }
}

#[test]
fn independently_frozen_cases_report_native_outcomes_without_weakening_labels() {
    let bytes = include_bytes!("../../../docs/foundation/skill-routing-independent-corpus.json");
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        digest,
        "79ef89a4934a0e2c61aa08d4e2de6a3efd0272c18f1ee3e9507271d5c6111275"
    );
    let corpus: Corpus = serde_json::from_slice(bytes).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills")
        .canonicalize()
        .unwrap();
    let mut library_before = BTreeMap::new();
    source_manifest(&root, &root, &mut library_before);
    let library_hash: String = Sha256::digest(serde_json::to_vec(&library_before).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let store = SkillStore::open(&root).unwrap();
    let registered_tools: BTreeSet<String> = [
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
    let mut records = Vec::new();
    let mut summary = BTreeMap::<String, [usize; 7]>::new();
    for mode in [RoutingMode::Standard, RoutingMode::Enhanced] {
        for case in &corpus.cases {
            let mut tools = registered_tools.clone();
            let unsupported_tools: Vec<_> = case
                .context
                .unavailable_tools
                .iter()
                .filter(|t| !registered_tools.contains(*t))
                .cloned()
                .collect();
            for tool in &case.context.unavailable_tools {
                tools.remove(tool);
            }
            // An empty set means unknown to the production port, not no capabilities.
            if tools.is_empty() {
                tools.insert("fixture_no_executable_tools".into());
            }
            let mut options = RoutingOptions::default();
            options.preferences.mode = mode;
            let report = store.orchestrate_with_options(
                &SkillRoutingQuery {
                    prompt: &case.prompt,
                    explicit_skill: None,
                    available_tools: &tools,
                    platform: "linux",
                    outcome_adjustments: &BTreeMap::new(),
                },
                &options,
            );
            let selected = report.selected_names();
            assert!(selected.len() <= 3);
            let hit = selected.iter().any(|s| case.expected_skill_ids.contains(s));
            let extra = selected
                .iter()
                .any(|s| !case.expected_skill_ids.contains(s));
            let forbidden = selected
                .iter()
                .any(|s| case.forbidden_skill_ids.contains(s));
            let satisfied = !forbidden
                && !extra
                && if case.expected_skill_ids.is_empty() {
                    selected.is_empty()
                } else {
                    hit || (case.acceptable_abstention && selected.is_empty())
                };
            let unsupported =
                !unsupported_tools.is_empty() || !case.context.denied_permissions.is_empty();
            let metrics = summary
                .entry(format!("{mode:?}:{}", case.kind))
                .or_default();
            metrics[0] += 1;
            metrics[1] += usize::from(hit);
            metrics[2] += usize::from(selected.is_empty());
            metrics[3] += usize::from(extra);
            metrics[4] += usize::from(forbidden);
            metrics[5] += usize::from(satisfied && !unsupported);
            metrics[6] += usize::from(unsupported);
            let expected_rejections: Vec<_> = report
                .rejected
                .iter()
                .filter(|(slug, _)| case.expected_skill_ids.contains(slug))
                .collect();
            records.push(serde_json::json!({"id":case.id,"mode":format!("{mode:?}"),"kind":case.kind,"selected":selected,"expected_rejections":expected_rejections,"hit":hit,"extra_activation":extra,"forbidden_activation":forbidden,"label_satisfied":satisfied,"unsupported_unavailable_tools":unsupported_tools,"unsupported_denied_permissions":case.context.denied_permissions,"context_supported":!unsupported}));
        }
    }
    let mut library_after = BTreeMap::new();
    source_manifest(&root, &root, &mut library_after);
    assert_eq!(
        library_before, library_after,
        "evaluation mutated skill sources"
    );
    let routing_source = [
        include_str!("../src/routing_quality.rs"),
        include_str!("../src/skill_orchestrator.rs"),
        include_str!("../src/skills.rs"),
    ]
    .join("\n");
    let routing_hash: String = Sha256::digest(routing_source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let output = serde_json::json!({"corpus_sha256":digest,"routing_source_sha256":routing_hash,"library_manifest_sha256":library_hash,"library_files":library_before.len(),"status":"measurement; failed quality remains visible","platform":"linux","registered_tools":registered_tools,"columns":["n","recall3","abstentions","extra_activations","forbidden_activations","strict_supported_success","unsupported_context"],"summary":summary,"predictions":records});
    println!(
        "{}",
        serde_json::to_string_pretty(&output["summary"]).unwrap()
    );
    if let Ok(path) = std::env::var("VESPER_ROUTING_INDEPENDENT_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }
}
