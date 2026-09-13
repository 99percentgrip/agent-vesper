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
            actions: vec![],
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

#[test]
fn missing_resource_does_not_activate_a_different_operation() {
    let mut prepare = entry(
        "prepare",
        "Prepare report document",
        RoutingEffect::Workspace,
    );
    let mut inspect = entry(
        "inspect",
        "Inspect report document",
        RoutingEffect::ReadOnly,
    );
    prepare.descriptor.as_mut().unwrap().actions = vec!["prepare".into()];
    prepare.descriptor.as_mut().unwrap().preconditions = vec!["template".into()];
    inspect.descriptor.as_mut().unwrap().actions = vec!["inspect".into()];
    let result = RoutingIndex::build(&[prepare, inspect]).unwrap().search(
        "Prepare report document",
        &RoutingTask {
            action: Some("prepare".into()),
            available_resources: Some(Default::default()),
            ..Default::default()
        },
    );
    assert!(result.candidates.is_empty());
    assert!(
        result
            .rejected
            .contains(&("prepare".into(), RoutingRejection::MissingResource))
    );
    assert!(
        result
            .rejected
            .contains(&("inspect".into(), RoutingRejection::ActionMismatch))
    );
}

#[test]
fn language_normalization_uses_metadata_not_catalog_names() {
    let entries = vec![
        entry(
            "alpha",
            "Edit spreadsheets and workbooks",
            RoutingEffect::Workspace,
        ),
        entry("beta", "Read PDF forms", RoutingEffect::ReadOnly),
        entry("gamma", "Debug Python programs", RoutingEffect::ReadOnly),
    ];
    let index = RoutingIndex::build(&entries).unwrap();
    for (prompt, expected) in [
        ("Build a workbook containing annual expenditure", "alpha"),
        ("Read these Portable Document Format forms", "beta"),
        ("Inspect a Python debugger session", "gamma"),
    ] {
        let found = index.search(prompt, &RoutingTask::default());
        assert_eq!(found.candidates[0].slug, expected);
        assert!(found.candidates[0].anchor_terms > 0);
        assert!(found.candidates[0].task_request);
    }
}

#[test]
fn a_single_topic_in_a_general_question_is_not_a_task_request() {
    let index = RoutingIndex::build(&[entry(
        "calendar-review",
        "Review weekly plans",
        RoutingEffect::ReadOnly,
    )])
    .unwrap();
    let question = index.search("How many days are in a week?", &RoutingTask::default());
    assert!(!question.candidates[0].task_request);
    assert_eq!(question.candidates[0].matched_terms, 1);
    let request = index.search("Review my week", &RoutingTask::default());
    assert!(request.candidates[0].task_request);
    assert!(
        index
            .search("Do not do anything yet", &RoutingTask::default())
            .candidates
            .is_empty()
    );
}

#[test]
fn sourced_verbs_are_sorted_and_recognize_new_task_wording() {
    let source = include_str!("../assets/routing-verbs.txt");
    assert!(source.contains("Copyright 2006 by Princeton University"));
    let words: Vec<_> = source
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(words.len(), 8429);
    assert!(words.windows(2).all(|w| w[0] < w[1]));
    assert!(
        words
            .iter()
            .all(|w| w.is_ascii() && w.chars().all(|c| c.is_ascii_lowercase()))
    );
    let index = RoutingIndex::build(&[entry(
        "instrument",
        "Calibration of optical instruments",
        RoutingEffect::ReadOnly,
    )])
    .unwrap();
    for prompt in [
        "Calibrate the optical instrument",
        "Diagnose the instrument",
        "Locate the instrument",
    ] {
        let found = index.search(prompt, &RoutingTask::default());
        assert!(found.candidates[0].task_request, "{prompt}");
    }
    assert!(
        index
            .search(
                "Explain what an optical instrument is",
                &RoutingTask::default()
            )
            .candidates
            .is_empty()
    );
}

#[test]
fn conversation_controls_do_not_request_a_single_topic_skill() {
    let index = RoutingIndex::build(&[entry(
        "control-reference",
        "Stop pause wait hold instrument",
        RoutingEffect::ReadOnly,
    )])
    .unwrap();
    for prompt in ["Stop now", "Pause here", "Wait please", "Hold on"] {
        assert!(
            !index.search(prompt, &RoutingTask::default()).candidates[0].task_request,
            "{prompt}"
        );
    }
}

#[test]
fn generic_request_verbs_cannot_supply_a_missing_topic() {
    let index = RoutingIndex::build(&[
        entry("weather", "See weather forecasts", RoutingEffect::ReadOnly),
        entry("dataset", "Count dataset rows", RoutingEffect::ReadOnly),
    ])
    .unwrap();
    for prompt in ["See you later", "Count to ten"] {
        assert!(
            index
                .search(prompt, &RoutingTask::default())
                .candidates
                .is_empty(),
            "{prompt}"
        );
    }
    assert_eq!(
        index
            .search("Count the dataset rows", &RoutingTask::default())
            .candidates[0]
            .slug,
        "dataset"
    );
}

#[test]
fn an_unrelated_identity_cannot_make_a_generic_verb_a_topic() {
    let index = RoutingIndex::build(&[
        entry("weather", "See weather forecasts", RoutingEffect::ReadOnly),
        entry("see", "Visual inspection", RoutingEffect::ReadOnly),
    ])
    .unwrap();
    let found = index.search("See you later", &RoutingTask::default());
    assert!(
        found
            .candidates
            .iter()
            .all(|candidate| candidate.slug != "weather")
    );
}

#[test]
fn excluded_alternatives_are_not_positive_retrieval_evidence() {
    let index = RoutingIndex::build(&[
        entry("alpha", "Optical calibration", RoutingEffect::ReadOnly),
        entry("beta", "Acoustic calibration", RoutingEffect::ReadOnly),
    ])
    .unwrap();
    for prompt in [
        "Calibrate optical instruments, not acoustic instruments",
        "Calibrate optical instruments rather than acoustic instruments",
        "Do not calibrate acoustic instruments; calibrate optical instruments",
        "Do not calibrate acoustic instruments but calibrate optical instruments",
    ] {
        let result = index.search(prompt, &RoutingTask::default());
        assert_eq!(result.candidates[0].slug, "alpha", "{prompt}");
        assert_eq!(
            result,
            index.search("Calibrate optical instruments", &RoutingTask::default()),
            "{prompt}"
        );
    }
    assert!(
        index
            .search(
                "Do not calibrate optical instruments",
                &RoutingTask::default()
            )
            .candidates
            .is_empty()
    );
}

#[test]
fn negative_lists_and_dotted_names_do_not_restart_positive_scope() {
    let index = RoutingIndex::build(&[
        entry(
            "alpha",
            "Thermal instrument quartz",
            RoutingEffect::ReadOnly,
        ),
        entry(
            "beta",
            "Magnetic instrument cobalt",
            RoutingEffect::ReadOnly,
        ),
        entry("gamma", "Document txt", RoutingEffect::ReadOnly),
    ])
    .unwrap();
    for prompt in [
        "Do not calibrate acoustic, thermal, or magnetic instruments",
        "Do not grind quartz, polish cobalt, or weave velvet",
        "Do not open acoustic.wav, notes.txt",
        "Use anything but thermal instruments",
    ] {
        assert!(
            index
                .search(prompt, &RoutingTask::default())
                .candidates
                .is_empty(),
            "{prompt}"
        );
    }
}

#[test]
fn task_synonyms_use_the_stemmers_actual_word_form() {
    let index =
        RoutingIndex::build(&[entry("alpha", "Planning", RoutingEffect::ReadOnly)]).unwrap();
    for prompt in [
        "Outline the work",
        "I need an outline",
        "Breakdown of the work",
    ] {
        assert_eq!(
            index.search(prompt, &RoutingTask::default()).candidates[0].slug,
            "alpha",
            "{prompt}"
        );
    }
}

#[test]
fn definitions_acknowledgments_and_quoted_repetition_do_not_request_procedures() {
    let index = RoutingIndex::build(&[entry(
        "alpha",
        "Optical instrument calibration",
        RoutingEffect::ReadOnly,
    )])
    .unwrap();
    for prompt in [
        "Define optical instrument calibration",
        "What is an optical instrument?",
        "Thanks for the optical instrument calibration",
        "Repeat \"optical instrument calibration\"",
    ] {
        assert!(
            index
                .search(prompt, &RoutingTask::default())
                .candidates
                .is_empty(),
            "{prompt}"
        );
    }
    assert_eq!(
        index
            .search(
                "Thanks. Calibrate the optical instrument",
                &RoutingTask::default()
            )
            .candidates[0]
            .slug,
        "alpha"
    );
}
