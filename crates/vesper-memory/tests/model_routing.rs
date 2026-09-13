use std::collections::{BTreeMap, BTreeSet};
use vesper_memory::model_routing::{ModelSelectionOutcome, ModelSkillDecision};
use vesper_memory::routing_quality::{RoutingMode, RoutingOptions};
use vesper_memory::{SkillRoutingQuery, SkillSlug, SkillStore};
fn options() -> RoutingOptions {
    let mut options = RoutingOptions::default();
    options.preferences.mode = RoutingMode::Enhanced;
    options.preferences.model_assistance = true;
    options
}
fn query<'a>(
    prompt: &'a str,
    tools: &'a BTreeSet<String>,
    outcomes: &'a BTreeMap<String, i16>,
) -> SkillRoutingQuery<'a> {
    SkillRoutingQuery {
        prompt,
        explicit_skill: None,
        available_tools: tools,
        platform: "linux",
        outcome_adjustments: outcomes,
    }
}
fn write(store: &SkillStore, extra: &str) {
    store.write(&SkillSlug::new("ledger-audit").unwrap(), &format!("---\nname: ledger-audit\ndescription: Audit ledger balances and transactions\nrisk: read-only\n{extra}---\nBODY_CANARY")).unwrap();
}
#[test]
fn prepare_is_metadata_only_and_strict_decision_loads_after_validation() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(&store, "");
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    let query = query("Audit ledger balances", &tools, &outcomes);
    let report = store.orchestrate_with_options(&query, &options());
    assert!(report.selected.is_empty());
    assert!(report.context().is_none());
    let prepared = report.prepared_selection.unwrap();
    assert!(
        !prepared
            .request(query.prompt)
            .unwrap()
            .contains("BODY_CANARY")
    );
    assert!(prepared.request("Different task").is_err());
    for invalid in [
        r#"{"outcome":"selected","skills":["missing"]}"#,
        r#"{"outcome":"selected","skills":["ledger-audit","ledger-audit"]}"#,
        r#"{"outcome":"selected","skills":[]}"#,
        r#"{"outcome":"ambiguous","skills":["ledger-audit"]}"#,
        r#"{"outcome":"selected","skills":["ledger-audit"],"permission":"bypass"}"#,
        r#"```json {"outcome":"no_skill_needed","skills":[]} ```"#,
    ] {
        assert!(prepared.parse_decision(invalid).is_err(), "{invalid}");
    }
    assert!(prepared.parse_decision(&" ".repeat(4097)).is_err());
    let decision = prepared
        .parse_decision(r#"{"outcome":"selected","skills":["ledger-audit"]}"#)
        .unwrap();
    let report = store.complete_model_selection(&query, &options(), &prepared, &decision);
    assert_eq!(report.selected_names(), vec!["ledger-audit"]);
    assert!(report.context().unwrap().contains("BODY_CANARY"));
    for outcome in [
        ModelSelectionOutcome::NoSkillNeeded,
        ModelSelectionOutcome::Ambiguous,
    ] {
        let report = store.complete_model_selection(
            &query,
            &options(),
            &prepared,
            &ModelSkillDecision {
                outcome,
                skills: vec![],
            },
        );
        assert!(report.selected.is_empty());
    }
}
#[test]
fn changed_settings_task_store_and_source_revoke_selection() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(&store, "");
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    let original = query("Audit ledger balances", &tools, &outcomes);
    let prepared = store
        .orchestrate_with_options(&original, &options())
        .prepared_selection
        .unwrap();
    let decision = prepared
        .parse_decision(r#"{"outcome":"selected","skills":["ledger-audit"]}"#)
        .unwrap();
    let mut changed = options();
    changed.preferences.disabled.insert("ledger-audit".into());
    assert!(
        store
            .complete_model_selection(&original, &changed, &prepared, &decision)
            .selected
            .is_empty()
    );
    changed = options();
    changed.preferences.model_assistance = false;
    assert!(
        store
            .complete_model_selection(&original, &changed, &prepared, &decision)
            .selected
            .is_empty()
    );
    assert!(
        store
            .complete_model_selection(
                &query("Audit another ledger", &tools, &outcomes),
                &options(),
                &prepared,
                &decision
            )
            .selected
            .is_empty()
    );
    let other = tempfile::tempdir().unwrap();
    let other_store = SkillStore::open(other.path()).unwrap();
    write(&other_store, "");
    assert!(
        other_store
            .complete_model_selection(&original, &options(), &prepared, &decision)
            .selected
            .is_empty()
    );
    write(&store, "disable-model-invocation: true\n");
    assert!(
        store
            .complete_model_selection(&original, &options(), &prepared, &decision)
            .selected
            .is_empty()
    );
}
#[test]
fn explicit_request_needs_no_selector_and_model_cannot_promote_itself() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(&store, "disable-model-invocation: true\n");
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    let mut explicit = query("Audit ledger balances", &tools, &outcomes);
    explicit.explicit_skill = Some("ledger-audit");
    let report = store.orchestrate_with_options(&explicit, &options());
    assert!(report.prepared_selection.is_none());
    assert_eq!(report.selected_names(), vec!["ledger-audit"]);
    let auto = query("Audit ledger balances", &tools, &outcomes);
    let prepared = store
        .orchestrate_with_options(&auto, &options())
        .prepared_selection
        .unwrap();
    assert!(prepared.offers().is_empty());
    let forged = ModelSkillDecision {
        outcome: ModelSelectionOutcome::Selected,
        skills: vec!["ledger-audit".into()],
    };
    assert!(
        store
            .complete_model_selection(&auto, &options(), &prepared, &forged)
            .selected
            .is_empty()
    );
}

#[test]
fn selected_isolated_skill_never_returns_body_and_budget_fallback_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(&store, "context: fork\n");
    let tools = BTreeSet::from(["delegate_task".into()]);
    let outcomes = BTreeMap::new();
    let request = query("Audit ledger balances", &tools, &outcomes);
    let prepared = store
        .orchestrate_with_options(&request, &options())
        .prepared_selection
        .unwrap();
    let decision = prepared
        .parse_decision(r#"{"outcome":"selected","skills":["ledger-audit"]}"#)
        .unwrap();
    let report = store.complete_model_selection(&request, &options(), &prepared, &decision);
    assert_eq!(report.selected_names(), vec!["ledger-audit"]);
    assert!(!report.context().unwrap_or_default().contains("BODY_CANARY"));
    assert!(report.selected[0].body.is_empty());
    let oversized = format!("Audit ledger balances {}", "x".repeat(8193));
    let report = store.orchestrate_with_options(&query(&oversized, &tools, &outcomes), &options());
    assert!(report.prepared_selection.is_none());
    assert!(report.routing_trace.reason.contains("lexical routing used"));
}

#[test]
fn offered_effect_preserves_stricter_descriptor_and_rechecks_read_only_controls() {
    let root = tempfile::tempdir().unwrap();
    let store = SkillStore::open(root.path()).unwrap();
    write(&store, "");
    let slug = SkillSlug::new("ledger-audit").unwrap();
    let descriptor = serde_json::json!({"version":1,"revision":store.routing_revision(&slug).unwrap(),"family":"ledger","purpose":"Audit ledger balances","effects":"external"});
    std::fs::write(
        root.path().join("skills/ledger-audit.routing.json"),
        serde_json::to_vec(&descriptor).unwrap(),
    )
    .unwrap();
    let tools = BTreeSet::new();
    let outcomes = BTreeMap::new();
    let query = query("Audit ledger balances", &tools, &outcomes);
    let prepared = store
        .orchestrate_with_options(&query, &options())
        .prepared_selection
        .unwrap();
    assert_eq!(prepared.offers()[0].effect, "external");
    let decision = prepared
        .parse_decision(r#"{"outcome":"selected","skills":["ledger-audit"]}"#)
        .unwrap();
    let mut limited = options();
    limited.task.maximum_effect = Some(vesper_memory::routing_quality::RoutingEffect::ReadOnly);
    assert!(
        store
            .complete_model_selection(&query, &limited, &prepared, &decision)
            .selected
            .is_empty()
    );
}
