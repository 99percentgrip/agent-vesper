//! Local CPU/index cost receipt; no timing assertion in shared CI.
use std::time::Instant;
use vesper_memory::routing_quality::{RoutingCatalogEntry, RoutingIndex, RoutingTask};
use vesper_memory::{SkillSummary, parse_metadata};

#[test]
fn bounded_five_hundred_entry_index_cost_receipt() {
    let entries: Vec<_> = (0..500)
        .map(|i| {
            let slug = format!("fixture-{i}");
            let description =
                format!("Review document artifact transaction schema migration report variant {i}");
            RoutingCatalogEntry {
                metadata: parse_metadata(
                    &SkillSummary {
                        slug,
                        headline: description.clone(),
                    },
                    &format!("---\ndescription: {description}\n---\nNot indexed"),
                ),
                revision: "fixture-v1".into(),
                descriptor: None,
            }
        })
        .collect();
    let start = Instant::now();
    let index = RoutingIndex::build(&entries).unwrap();
    let cold = start.elapsed();
    let task = RoutingTask::default();
    for _ in 0..10 {
        std::hint::black_box(index.search("Review the migration transaction schema", &task));
    }
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let result = index.search("Review the migration transaction schema", &task);
        times.push(start.elapsed().as_micros());
        assert_eq!(result.candidates.len(), 12);
    }
    times.sort_unstable();
    println!(
        "ROUTING_COST entries=500 warmups=10 timed=100 cold_us={} warm_p95_us={} warm_max_us={}",
        cold.as_micros(),
        times[94],
        times[99]
    );
}

#[test]
fn store_routing_including_live_identity_checks_cost_receipt() {
    use std::collections::{BTreeMap, BTreeSet};
    use vesper_memory::routing_quality::{RoutingMode, RoutingOptions};
    use vesper_memory::{SkillRoutingQuery, SkillSlug, SkillStore};
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    for i in 0..500 {
        store.write(&SkillSlug::new(&format!("fixture-{i}")).unwrap(), &format!("---\ndescription: Review document artifact transaction schema migration report variant {i}\nrisk: read-only\n---\nFixture body")).unwrap();
    }
    let mut options = RoutingOptions::default();
    options.preferences.mode = RoutingMode::Enhanced;
    let query = SkillRoutingQuery {
        prompt: "Review the migration transaction schema",
        explicit_skill: None,
        available_tools: &BTreeSet::new(),
        platform: "linux",
        outcome_adjustments: &BTreeMap::new(),
    };
    let start = Instant::now();
    store.orchestrate_with_options(&query, &options);
    let cold = start.elapsed();
    for _ in 0..10 {
        std::hint::black_box(store.orchestrate_with_options(&query, &options));
    }
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let result = store.orchestrate_with_options(&query, &options);
        times.push(start.elapsed().as_micros());
        assert_eq!(result.selected.len(), 3);
    }
    times.sort_unstable();
    println!(
        "STORE_ROUTING_COST entries=500 warmups=10 timed=100 cold_us={} warm_p95_us={} warm_max_us={}",
        cold.as_micros(),
        times[94],
        times[99]
    );
}
