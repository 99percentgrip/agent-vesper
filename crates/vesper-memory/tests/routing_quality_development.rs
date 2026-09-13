//! Development-only diagnostics; the frozen holdout is never used for tuning.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
};
use vesper_memory::routing_quality::{
    RoutingCatalogEntry, RoutingIndex, RoutingMode, RoutingOptions,
};
use vesper_memory::{SkillRoutingQuery, SkillStore, parse_metadata};

#[test]
fn development_predictions_explain_selection_losses() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills")
        .canonicalize()
        .unwrap();
    let store = SkillStore::open(&root).unwrap();
    let entries: Vec<_> = store
        .list()
        .iter()
        .map(|s| {
            let mut body = String::new();
            std::fs::File::open(root.join("skills").join(format!("{}.md", s.slug)))
                .unwrap()
                .take(32000)
                .read_to_string(&mut body)
                .unwrap();
            RoutingCatalogEntry {
                metadata: parse_metadata(s, &body),
                revision: "development".into(),
                descriptor: None,
            }
        })
        .collect();
    if let Ok(path) = std::env::var("VESPER_ROUTING_METADATA_OUTPUT") {
        let records:Vec<_>=entries.iter().map(|e|serde_json::json!({"slug":e.metadata.slug,"name":e.metadata.name,"description":e.metadata.description,"tags":e.metadata.tags,"triggers":e.metadata.triggers,"extensions":e.metadata.file_extensions})).collect();
        std::fs::write(path, serde_json::to_vec_pretty(&records).unwrap()).unwrap();
    }
    let index = RoutingIndex::build(&entries).unwrap();
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("routing_quality_cases.json")).unwrap();
    let tools: BTreeSet<_> = [
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
    let outcomes = BTreeMap::new();
    let mut options = RoutingOptions::default();
    options.preferences.mode = RoutingMode::Enhanced;
    let (mut positives, mut hits, mut abstentions, mut negative_activations) = (0, 0, 0, 0);
    for case in corpus["cases"].as_array().unwrap().iter().filter(|c| {
        c["split"] == "development" && (c["category"] == "positive" || c["category"] == "no_skill")
    }) {
        let prompt = case["prompt"].as_str().unwrap();
        let report = store.orchestrate_with_options(
            &SkillRoutingQuery {
                prompt,
                explicit_skill: None,
                available_tools: &tools,
                platform: "linux",
                outcome_adjustments: &outcomes,
            },
            &options,
        );
        let names = report.selected_names();
        let hit = case["acceptable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| names.iter().any(|n| v == n));
        if case["category"] == "positive" {
            positives += 1;
            hits += usize::from(hit);
            abstentions += usize::from(names.is_empty());
        } else {
            negative_activations += usize::from(!names.is_empty());
        }
        if !hit {
            let search = index.search(prompt, &options.task.clone().narrow_from_prompt(prompt));
            println!(
                "{} prompt={} selected={:?} expected={} candidates={:?} rejected={:?}",
                case["id"],
                prompt,
                names,
                case["acceptable"],
                search.candidates.iter().take(3).collect::<Vec<_>>(),
                report.rejected
            );
        }
    }
    println!(
        "DEVELOPMENT positives={positives} hits={hits} abstentions={abstentions} negative_activations={negative_activations}/20"
    );
}
