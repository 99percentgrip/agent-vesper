use std::collections::{BTreeMap, BTreeSet};
use vesper_harness::skill_routing_settings::{self as settings, RoutingMode, RoutingTask};
use vesper_memory::{SkillRoutingQuery, SkillSlug, SkillStore};

#[test]
fn project_controls_are_explicit_read_only_until_save_and_preserve_library() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    let slug = SkillSlug::new("ledger-audit").unwrap();
    let body =
        "---\ndescription: Audit ledger transactions and reconcile balances\n---\nUSER EDITS";
    store.write(&slug, body).unwrap();
    assert_eq!(
        settings::load(root.path()).unwrap().mode,
        RoutingMode::Standard
    );
    settings::command(root.path(), "status").unwrap();
    assert!(!root.path().join(".agent-vesper").exists());
    assert!(settings::command(root.path(), "mode enhanced").is_err());
    assert!(!root.path().join(".agent-vesper").exists());
    settings::command(root.path(), "save mode enhanced").unwrap();
    assert_eq!(
        settings::load(root.path()).unwrap().mode,
        RoutingMode::Enhanced
    );
    let query = SkillRoutingQuery {
        prompt: "Audit ledger transactions and reconcile balances",
        explicit_skill: None,
        available_tools: &BTreeSet::new(),
        platform: "linux",
        outcome_adjustments: &BTreeMap::new(),
    };
    assert_eq!(
        settings::route(root.path(), &store, &query, RoutingTask::default()).selected_names(),
        vec!["ledger-audit"]
    );
    settings::command(root.path(), "save disable ledger-audit").unwrap();
    assert!(
        settings::route(root.path(), &store, &query, RoutingTask::default())
            .selected
            .is_empty()
    );
    settings::command(root.path(), "save mode standard").unwrap();
    assert!(
        settings::route(root.path(), &store, &query, RoutingTask::default())
            .selected
            .is_empty()
    );
    settings::command(root.path(), "save enable ledger-audit").unwrap();
    assert_eq!(
        settings::route(root.path(), &store, &query, RoutingTask::default()).selected_names(),
        vec!["ledger-audit"]
    );
    assert_eq!(store.read(&slug).unwrap(), body);
}

#[test]
fn malformed_preferences_cannot_reenable_disabled_skills() {
    let root = tempfile::tempdir().unwrap();
    settings::command(root.path(), "save disable ledger-audit").unwrap();
    std::fs::write(settings::path(root.path()), "not json").unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    store
        .write(
            &SkillSlug::new("ledger-audit").unwrap(),
            "---\ndescription: Audit ledger transactions\n---\nBODY",
        )
        .unwrap();
    let report = settings::route(
        root.path(),
        &store,
        &SkillRoutingQuery {
            prompt: "Audit ledger transactions",
            explicit_skill: None,
            available_tools: &BTreeSet::new(),
            platform: "linux",
            outcome_adjustments: &BTreeMap::new(),
        },
        RoutingTask::default(),
    );
    assert!(report.selected.is_empty());
    assert!(report.routing_trace.reason.contains("withheld"));
    assert!(report.routing_trace.reason.chars().count() <= 512);
}

#[cfg(unix)]
#[test]
fn settings_symlinks_never_write_outside_workspace() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".agent-vesper")).unwrap();
    assert!(settings::command(root.path(), "save mode enhanced").is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn transition_refinement_is_bounded_and_cannot_execute_tools() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    let query = SkillRoutingQuery {
        prompt: "Discuss options",
        explicit_skill: None,
        available_tools: &BTreeSet::new(),
        platform: "linux",
        outcome_adjustments: &BTreeMap::new(),
    };
    let mut transition = settings::RoutingTransition::default();
    assert!(
        transition
            .search(root.path(), &store, &query, RoutingTask::default(), false)
            .is_ok()
    );
    assert!(
        transition
            .search(root.path(), &store, &query, RoutingTask::default(), false)
            .is_err()
    );
    assert!(
        transition
            .search(root.path(), &store, &query, RoutingTask::default(), true)
            .is_ok()
    );
    assert!(
        transition
            .search(root.path(), &store, &query, RoutingTask::default(), true)
            .is_err()
    );
    assert!(!root.path().join(".agent-vesper").exists());
}
