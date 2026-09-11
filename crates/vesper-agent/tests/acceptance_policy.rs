//! Adversarial contract/receipt policy tests. These test deterministic rejection,
//! not the semantic competence of a live language model.
use vesper_agent::acceptance::*;
use vesper_domain::acceptance::*;

fn fixture() -> (
    AcceptanceContract,
    Vec<AcceptanceSource>,
    Vec<AcceptanceCheck>,
    Vec<AcceptanceReceipt>,
) {
    let sources = vec![AcceptanceSource {
        id: "source-001".into(),
        text: "Both hosts must execute real concurrent workers.".into(),
    }];
    let contract = AcceptanceContract {
        version: 1,
        objective: "Implement both hosts".into(),
        requirements: vec![AcceptanceRequirement {
            id: "R1".into(),
            description: "Native concurrent workers in both hosts".into(),
            source_ids: vec!["source-001".into()],
            scenarios: ["ACP", "TUI"]
                .into_iter()
                .enumerate()
                .map(|(i, scope)| AcceptanceScenario {
                    id: format!("S{i}"),
                    assertion: "Three real distinct workers overlap at a barrier".into(),
                    scope: scope.into(),
                    evidence: AcceptanceEvidenceKind::Native,
                    platform: AcceptancePlatform::Any,
                })
                .collect(),
        }],
        context: vec![],
    };
    let checks: Vec<_> = ["ACP", "TUI"]
        .into_iter()
        .enumerate()
        .map(|(i, scope)| AcceptanceCheck {
            id: format!("C{i}"),
            scenario_ids: vec![format!("S{i}")],
            evidence: AcceptanceEvidenceKind::Native,
            platform: AcceptancePlatform::Any,
            scope: scope.into(),
            package: "example".into(),
            target: Some("native".into()),
            test: format!("native_{i}"),
            features: vec![],
            all_features: true,
            ignored: true,
            timeout_seconds: 60,
        })
        .collect();
    let receipts = checks
        .iter()
        .enumerate()
        .map(|(i, c)| AcceptanceReceipt {
            version: 1,
            collector: ACCEPTANCE_COLLECTOR.into(),
            observed_at_unix_ms: 1,
            run_id: i as u64,
            check_id: c.id.clone(),
            contract_digest: "contract".into(),
            checks_digest: "checks".into(),
            source_digest: "source".into(),
            environment: "platform".into(),
            argv: cargo_argv(c),
            test: c.test.clone(),
            state: AcceptanceState::Verified,
            elapsed_ms: 5,
            output_digest: "output".into(),
            diagnostic: "executed".into(),
        })
        .collect();
    (contract, sources, checks, receipts)
}
fn report(
    c: &AcceptanceContract,
    checks: &[AcceptanceCheck],
    r: &[AcceptanceReceipt],
) -> AcceptanceReport {
    evaluate_receipts(
        c,
        checks,
        r,
        "contract",
        "checks",
        "source",
        "platform",
        &[],
    )
}

#[test]
fn every_required_host_needs_its_own_evidence() {
    let (c, s, checks, mut receipts) = fixture();
    validate_contract(&c, &s).unwrap();
    validate_checks(&c, &checks).unwrap();
    assert!(report(&c, &checks, &receipts).is_verified());
    receipts.pop();
    let result = report(&c, &checks, &receipts);
    assert!(!result.is_verified());
    assert_eq!(result.verified_scenarios, 1);
    assert_eq!(result.gaps[0].subject, "S1");
}
#[test]
fn omitted_source_and_removed_scenario_checks_refuse() {
    let (c, mut s, mut checks, _) = fixture();
    s.push(AcceptanceSource {
        id: "source-002".into(),
        text: "Cancellation must reap actual children.".into(),
    });
    assert!(validate_contract(&c, &s).is_err());
    checks.pop();
    assert!(validate_checks(&c, &checks).is_err());
}
#[test]
fn unit_tests_cannot_substitute_for_native_tests() {
    let (c, _, mut checks, _) = fixture();
    checks[0].evidence = AcceptanceEvidenceKind::Unit;
    assert!(validate_checks(&c, &checks).is_err());
}
#[test]
fn all_nonpositive_states_refuse_completion() {
    let (c, _, checks, original) = fixture();
    for state in [
        AcceptanceState::Missing,
        AcceptanceState::Failed,
        AcceptanceState::Stale,
        AcceptanceState::Inconclusive,
    ] {
        let mut r = original.clone();
        r[0].state = state;
        assert!(!report(&c, &checks, &r).is_verified(), "{state:?}");
    }
}
#[test]
fn changed_source_contract_checks_platform_or_test_invalidates() {
    let (c, _, checks, original) = fixture();
    for field in [
        "source_digest",
        "contract_digest",
        "checks_digest",
        "environment",
        "test",
    ] {
        let mut encoded = serde_json::to_value(&original).unwrap();
        encoded[0][field] = "changed".into();
        let r: Vec<AcceptanceReceipt> = serde_json::from_value(encoded).unwrap();
        assert!(!report(&c, &checks, &r).is_verified(), "{field}");
    }
}
#[test]
fn duplicate_unknown_and_wrong_command_receipts_refuse() {
    let (c, _, checks, mut r) = fixture();
    r[1].run_id = r[0].run_id;
    assert!(!report(&c, &checks, &r).is_verified());
    r[1].run_id = 1;
    r[0].check_id = "unrelated".into();
    assert!(!report(&c, &checks, &r).is_verified());
    r[0].check_id = "C0".into();
    r[0].argv = vec!["echo".into(), "PASS".into()];
    assert!(!report(&c, &checks, &r).is_verified());
    let (c, _, checks, mut r) = fixture();
    r[0].collector = "model-written".into();
    assert!(!report(&c, &checks, &r).is_verified());
    r[0].collector = ACCEPTANCE_COLLECTOR.into();
    r[0].observed_at_unix_ms = 0;
    assert!(!report(&c, &checks, &r).is_verified());
}
#[test]
fn malformed_review_and_unresolved_findings_refuse() {
    let (c, s, checks, r) = fixture();
    let mut review = AcceptanceReview {
        inspected_source_ids: vec!["source-001".into()],
        inspected_scenario_ids: vec!["S0".into()],
        findings: vec![],
    };
    assert!(validate_review(&c, &s, &review).is_err());
    review.inspected_scenario_ids.push("S1".into());
    validate_review(&c, &s, &review).unwrap();
    let findings = vec![AcceptanceFinding {
        subject: "S1".into(),
        evidence: "ACP registration never wires the worker service".into(),
        repair: "Wire the native service and add a transport test".into(),
    }];
    let report = evaluate_receipts(
        &c, &checks, &r, "contract", "checks", "source", "platform", &findings,
    );
    assert!(!report.is_verified());
    assert!(report.render().contains("ACP registration"));
}
#[test]
fn injection_empty_and_unknown_versions_refuse() {
    let (mut c, s, mut checks, _) = fixture();
    for bad in ["", "test;echo PASS", "../test", "--skip", "test\nother"] {
        checks[0].test = bad.into();
        assert!(validate_checks(&c, &checks).is_err(), "{bad}");
    }
    c.version = 999;
    assert!(validate_contract(&c, &s).is_err());
    c.version = 1;
    c.requirements.clear();
    assert!(validate_contract(&c, &s).is_err());
    let raw = r#"{"version":1,"objective":"x","requirements":[],"context":[],"verified":true}"#;
    assert!(serde_json::from_str::<AcceptanceContract>(raw).is_err());
}
