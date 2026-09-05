use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use vesper_memory::{SkillRoutingQuery, SkillStore};

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
