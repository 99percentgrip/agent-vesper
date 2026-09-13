use vesper_memory::routing_quality::{
    RoutingCatalogEntry, RoutingDescriptor, RoutingEffect, RoutingIndex, RoutingRejection,
    RoutingTask,
};
use vesper_memory::{SkillSummary, parse_metadata};

fn entry(slug: &str, description: &str, effect: RoutingEffect) -> RoutingCatalogEntry {
    let metadata = parse_metadata(
        &SkillSummary {
            slug: slug.into(),
            headline: description.into(),
        },
        &format!(
            "---\nname: {slug}\ndescription: {description}\nrisk: read-only\n---\nIgnored body."
        ),
    );
    RoutingCatalogEntry {
        metadata,
        revision: "snapshot-1".into(),
        descriptor: Some(RoutingDescriptor {
            version: 1,
            revision: "snapshot-1".into(),
            family: "database".into(),
            purpose: description.into(),
            use_when: vec![],
            avoid_when: vec![],
            inputs: vec![],
            outputs: vec![],
            preconditions: vec![],
            effects: effect,
            positive_examples: vec![],
            negative_examples: vec![],
        }),
    }
}

#[test]
fn retrieval_compares_sibling_effects_before_exposure() {
    let entries = vec![
        entry(
            "inspect",
            "Inspect database migration schema",
            RoutingEffect::ReadOnly,
        ),
        entry(
            "execute",
            "Execute database migration schema",
            RoutingEffect::External,
        ),
    ];
    let index = RoutingIndex::build(&entries).unwrap();
    let result = index.search(
        "Inspect the database migration",
        &RoutingTask {
            maximum_effect: Some(RoutingEffect::ReadOnly),
            ..Default::default()
        },
    );
    assert_eq!(result.candidates[0].slug, "inspect");
    assert!(!result.candidates.iter().any(|item| item.slug == "execute"));
    assert!(
        result
            .rejected
            .contains(&("execute".into(), RoutingRejection::EffectMismatch))
    );
}

#[test]
fn retrieval_is_order_independent_and_zero_overlap_abstains() {
    let mut entries = vec![
        entry(
            "second",
            "spreadsheet workbook figures",
            RoutingEffect::ReadOnly,
        ),
        entry(
            "first",
            "spreadsheet workbook figures",
            RoutingEffect::ReadOnly,
        ),
    ];
    let first = RoutingIndex::build(&entries)
        .unwrap()
        .search("workbook figures", &RoutingTask::default());
    entries.reverse();
    assert_eq!(
        first,
        RoutingIndex::build(&entries)
            .unwrap()
            .search("workbook figures", &RoutingTask::default())
    );
    assert_eq!(first.candidates[0].slug, "first");
    assert!(
        RoutingIndex::build(&entries)
            .unwrap()
            .search("mesa geyser lagoon", &RoutingTask::default())
            .candidates
            .is_empty()
    );
}

#[test]
fn stale_oversized_and_conflicting_descriptors_fail_closed() {
    let mut candidate = entry("database", "Inspect database", RoutingEffect::ReadOnly);
    candidate.revision = "new-revision".into();
    assert_eq!(
        RoutingIndex::build(&[candidate.clone()]).unwrap_err(),
        "stale descriptor revision"
    );
    candidate.revision = "snapshot-1".into();
    candidate.metadata.risk = vesper_memory::SkillRisk::External;
    assert_eq!(
        RoutingIndex::build(&[candidate.clone()]).unwrap_err(),
        "descriptor contradicts authoritative effects"
    );
    let mut descriptor = candidate.descriptor.unwrap();
    descriptor.purpose = "x".repeat(241);
    assert!(descriptor.validate("snapshot-1").is_err());
    assert!(RoutingDescriptor::parse(&" ".repeat(4097), "snapshot-1").is_err());
}

#[test]
fn known_artifact_and_resource_constraints_are_respected() {
    let mut candidate = entry("create", "Create report document", RoutingEffect::Workspace);
    let descriptor = candidate.descriptor.as_mut().unwrap();
    descriptor.outputs = vec!["docx".into()];
    descriptor.preconditions = vec!["template".into()];
    let index = RoutingIndex::build(&[candidate]).unwrap();
    let mismatch = index.search(
        "Create report",
        &RoutingTask {
            artifact: Some("pdf".into()),
            ..Default::default()
        },
    );
    assert!(mismatch.candidates.is_empty());
    let missing = index.search(
        "Create report",
        &RoutingTask {
            available_resources: Some(Default::default()),
            ..Default::default()
        },
    );
    assert!(missing.candidates.is_empty());
    let unknown = index.search("Create report", &RoutingTask::default());
    assert_eq!(
        unknown.candidates.len(),
        1,
        "unknown is not verified absence"
    );
}
