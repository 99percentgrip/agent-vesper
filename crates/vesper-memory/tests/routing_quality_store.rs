use std::collections::{BTreeMap, BTreeSet};
use vesper_memory::routing_quality::{RoutingEffect, RoutingMode, RoutingOptions};
use vesper_memory::{SkillRoutingQuery, SkillSlug, SkillStore};

fn route(
    store: &SkillStore,
    prompt: &str,
    options: &RoutingOptions,
) -> vesper_memory::SkillRoutingReport {
    store.orchestrate_with_options(
        &SkillRoutingQuery {
            prompt,
            explicit_skill: None,
            available_tools: &BTreeSet::from(["read_file".into(), "delegate_task".into()]),
            platform: "linux",
            outcome_adjustments: &BTreeMap::new(),
        },
        options,
    )
}
fn enhanced() -> RoutingOptions {
    let mut options = RoutingOptions::default();
    options.preferences.mode = RoutingMode::Enhanced;
    options
}
fn write(store: &SkillStore, slug: &str, metadata: &str) {
    store
        .write(&SkillSlug::new(slug).unwrap(), metadata)
        .unwrap();
}

#[test]
fn enhanced_rechecks_disabled_archived_deleted_and_replaced_sources() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    let source = "---\nname: ledger-audit\ndescription: Audit ledger transactions and reconcile balances\nrisk: read-only\n---\nORIGINAL";
    write(&store, "ledger-audit", source);
    let mut options = enhanced();
    assert_eq!(
        route(
            &store,
            "Audit ledger transactions and reconcile balances",
            &options
        )
        .selected_names(),
        vec!["ledger-audit"]
    );
    options.preferences.disabled.insert("ledger-audit".into());
    for mode in [RoutingMode::Standard, RoutingMode::Enhanced] {
        options.preferences.mode = mode;
        assert!(
            route(&store, "Use skill ledger-audit", &options)
                .explicit_error
                .is_some()
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("skills/ledger-audit.md")).unwrap(),
        source
    );
    options.preferences.disabled.clear();
    options.preferences.mode = RoutingMode::Enhanced;
    write(
        &store,
        "ledger-audit",
        "---\ndescription: Audit ledger transactions\n---\n<!-- vesper:archive -->\nARCHIVED",
    );
    assert!(
        route(&store, "Audit ledger transactions", &options)
            .selected
            .is_empty()
    );
    std::fs::remove_file(root.path().join("skills/ledger-audit.md")).unwrap();
    assert!(
        route(&store, "Audit ledger transactions", &options)
            .selected
            .is_empty()
    );
}

#[test]
fn typed_read_only_restriction_applies_to_legacy_metadata() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(
        &store,
        "ledger-write",
        "---\nname: ledger-write\ndescription: Write ledger transactions and reconcile balances\nrisk: mutating\n---\nWRITE",
    );
    let mut options = enhanced();
    options.task.maximum_effect = Some(RoutingEffect::ReadOnly);
    let report = route(
        &store,
        "Write ledger transactions and reconcile balances",
        &options,
    );
    assert!(report.selected.is_empty());
    assert!(
        report
            .rejected
            .iter()
            .any(|(_, reason)| reason == "EffectMismatch")
    );
}

#[test]
fn invalid_descriptor_is_not_silently_treated_as_legacy() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(
        &store,
        "ledger-audit",
        "---\ndescription: Audit ledger transactions\n---\nBODY",
    );
    std::fs::write(root.path().join("skills/ledger-audit.routing.json"), "{}").unwrap();
    let report = route(&store, "Audit ledger transactions", &enhanced());
    assert!(report.selected.is_empty());
    assert!(
        report
            .rejected
            .iter()
            .any(|(_, reason)| reason == "invalid descriptor")
    );
    assert_eq!(
        route(&store, "Use skill ledger-audit", &RoutingOptions::default()).selected_names(),
        vec!["ledger-audit"]
    );
}

#[test]
fn local_skill_shadows_global_descriptor_and_never_mutates_either_library() {
    let local = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    let global_store = SkillStore::open(global.path()).unwrap();
    write(
        &global_store,
        "ledger-audit",
        "---\ndescription: Audit ledger transactions\n---\nGLOBAL",
    );
    std::fs::write(global.path().join("skills/ledger-audit.routing.json"), "{}").unwrap();
    let store = SkillStore::open_with_global(local.path(), global.path()).unwrap();
    write(
        &store,
        "ledger-audit",
        "---\ndescription: Audit ledger transactions\n---\nLOCAL",
    );
    let report = route(&store, "Audit ledger transactions", &enhanced());
    assert_eq!(report.selected_names(), vec!["ledger-audit"]);
    assert!(report.selected[0].body.ends_with("LOCAL"));
    assert!(
        global_store
            .read(&SkillSlug::new("ledger-audit").unwrap())
            .unwrap()
            .ends_with("GLOBAL")
    );
    assert_eq!(
        std::fs::read_to_string(global.path().join("skills/ledger-audit.routing.json")).unwrap(),
        "{}"
    );
}

#[test]
fn descriptor_revision_is_rechecked_when_skill_changes() {
    use vesper_memory::routing_quality::RoutingDescriptor;
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    let slug = SkillSlug::new("ledger-audit").unwrap();
    write(
        &store,
        "ledger-audit",
        "---\ndescription: Audit ledger transactions\nrisk: read-only\n---\nFIRST",
    );
    let descriptor = RoutingDescriptor {
        version: 1,
        revision: store.routing_revision(&slug).unwrap(),
        family: "finance".into(),
        purpose: "Audit ledger transactions".into(),
        actions: vec!["audit".into()],
        use_when: vec![],
        avoid_when: vec![],
        inputs: vec![],
        outputs: vec![],
        preconditions: vec![],
        effects: RoutingEffect::ReadOnly,
        positive_examples: vec![],
        negative_examples: vec![],
    };
    std::fs::write(
        root.path().join("skills/ledger-audit.routing.json"),
        serde_json::to_vec(&descriptor).unwrap(),
    )
    .unwrap();
    assert_eq!(
        route(&store, "Audit ledger transactions", &enhanced()).selected_names(),
        vec!["ledger-audit"]
    );
    write(
        &store,
        "ledger-audit",
        "---\ndescription: Audit ledger transactions\nrisk: read-only\n---\nCHANGED BODY",
    );
    let report = route(&store, "Audit ledger transactions", &enhanced());
    assert!(report.selected.is_empty());
    assert!(
        report
            .rejected
            .iter()
            .any(|(_, reason)| reason == "stale descriptor revision")
    );
}

#[test]
fn explicit_override_never_overrides_read_only_task_restrictions() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(
        &store,
        "ledger-write",
        "---\ndescription: Write ledger transactions\nrisk: mutating\n---\nWRITE",
    );
    let report = route(
        &store,
        "Use skill ledger-write without changing anything",
        &enhanced(),
    );
    assert!(report.selected.is_empty());
    assert!(
        report
            .explicit_error
            .unwrap()
            .contains("task effect restriction")
    );
}
