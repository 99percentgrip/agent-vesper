//! Decision cap, rationale and versioning proofs (VRO-16 PR-2 directive 4).
use super::*;

fn engine() -> DecisionEngine {
    DecisionEngine::new(DecisionConfig::default()).unwrap()
}

/// Cap proof: refine iterations are strictly capped; the (cap+1)-th refine
/// verdict is rewritten to the audited ProceedWithFailure verdict.
#[test]
fn refine_cap_is_enforced_and_audited() {
    let mut engine = engine();
    let resolve = |_id: u64| true;
    let refine = |iteration: u32| {
        serde_json::json!({
            "kind": "refine",
            "amended": [serde_json::json!({
                "index": 0,
                "prompt": "amended",
                "required_capabilities": [],
                "depends_on": []
            })],
            "iteration": iteration
        })
        .to_string()
    };
    for expected_iteration in 1..=DEFAULT_REFINE_CAP {
        let (verdict, event) = engine
            .issue("g", &refine(expected_iteration), &[], &resolve, 0)
            .unwrap();
        assert!(
            matches!(verdict, DecisionVerdict::Refine { iteration, .. } if iteration == expected_iteration)
        );
        assert!(matches!(
            event,
            AuditEvent::DecisionIssued { refine_iterations, .. } if refine_iterations == expected_iteration
        ));
    }
    // The fourth refine is refused as a loop continuation: cap exhaustion.
    let (verdict, event) = engine.issue("g", &refine(4), &[], &resolve, 0).unwrap();
    assert!(matches!(
        verdict,
        DecisionVerdict::ProceedWithFailure { .. } if verdict_loop_reason(&verdict).contains("refine cap exhausted")
    ));
    assert!(matches!(event, AuditEvent::DecisionIssued { .. }));
}

fn verdict_loop_reason(verdict: &DecisionVerdict) -> String {
    match verdict {
        DecisionVerdict::ProceedWithFailure { reason } => reason.clone(),
        _ => String::new(),
    }
}

/// Cap proof: pivots capped at 2, same audited exhaustion.
#[test]
fn pivot_cap_is_enforced_and_audited() {
    let mut engine = engine();
    let resolve = |_id: u64| true;
    let pivot = || {
        serde_json::json!({
            "kind": "pivot",
            "tasks": [serde_json::json!({
                "index": null,
                "prompt": "new direction",
                "required_capabilities": [],
                "depends_on": []
            })]
        })
        .to_string()
    };
    for _ in 0..DEFAULT_PIVOT_CAP {
        let (verdict, _) = engine.issue("g", &pivot(), &[], &resolve, 0).unwrap();
        assert!(matches!(verdict, DecisionVerdict::Pivot { .. }));
    }
    let (verdict, event) = engine.issue("g", &pivot(), &[], &resolve, 0).unwrap();
    assert!(verdict_loop_reason(&verdict).contains("pivot cap exhausted"));
    assert!(matches!(event, AuditEvent::DecisionIssued { .. }));
}

/// Fail-closed rationale: dangling references refuse issuance entirely.
#[test]
fn dangling_rationale_refs_fail_closed() {
    let mut engine = engine();
    let resolver = |id: u64| id == 1; // only id 1 exists
    let error = engine
        .issue(
            "g",
            &serde_json::json!({"kind":"proceed"}).to_string(),
            &[1, 999],
            &resolver,
            0,
        )
        .unwrap_err();
    assert!(error.contains("dangling rationale reference 999"));
    // No iteration was consumed by the refused decision.
    assert_eq!(engine.iterations("g"), (0, 0));
}

/// Malformed decision output fails closed.
#[test]
fn malformed_decisions_fail_closed() {
    let mut engine = engine();
    let resolve = |_id: u64| true;
    for bad in [
        "",
        "proceed",
        "{\"kind\":\"explode\"}",
        "{\"kind\":\"refine\",\"amended\":[]}",
    ] {
        assert!(engine.issue("g", bad, &[], &resolve, 0).is_err(), "{bad}");
    }
    // Amended-task bounds are enforced.
    let oversize = serde_json::json!({
        "kind": "refine",
        "amended": (0..65).map(|index| serde_json::json!({
            "index": index, "prompt": "x", "required_capabilities": [], "depends_on": []
        })).collect::<Vec<_>>(),
        "iteration": 1
    })
    .to_string();
    assert!(engine.issue("g", &oversize, &[], &resolve, 0).is_err());
}

/// Degenerate configs: caps above the hard defaults are refused so no
/// configuration can manufacture an unbounded loop.
#[test]
fn caps_cannot_be_inflated() {
    assert!(
        DecisionEngine::new(DecisionConfig {
            refine_cap: DEFAULT_REFINE_CAP + 1,
            pivot_cap: 1,
        })
        .is_err()
    );
    assert!(
        DecisionEngine::new(DecisionConfig {
            refine_cap: 1,
            pivot_cap: DEFAULT_PIVOT_CAP + 1,
        })
        .is_err()
    );
    // Zero caps are legal (proceed-only decisions).
    assert!(
        DecisionEngine::new(DecisionConfig {
            refine_cap: 0,
            pivot_cap: 0,
        })
        .is_ok()
    );
}

/// Version registry: every recorded version stays retrievable and any
/// iteration's evidence resolves across versions.
#[tokio::test]
async fn version_registry_preserves_evidence_chain() {
    use crate::ledger::store::{BoundedText, EmbeddingPort, EntryDraft, Ledger, MemoryScope};
    use futures_util::future::BoxFuture;

    struct Embedding;
    impl EmbeddingPort for Embedding {
        fn embed<'a>(
            &'a self,
            texts: Vec<BoundedText>,
        ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, crate::ledger::store::LedgerError>> {
            Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
        }
    }
    let mut registry = VersionRegistry::default();
    let live = Ledger::with_retention(
        crate::ledger::hnsw::HnswConfig::new(8),
        std::sync::Arc::new(Embedding),
        crate::ledger::store::LedgerRetention::Limited {
            swarm: 1024,
            worker: 1024,
            task: 1024,
        },
    )
    .unwrap();
    let mut ids = Vec::new();
    for version in 0..2 {
        let draft = EntryDraft {
            scope: MemoryScope::Swarm,
            kind: crate::ledger::store::EntryKind::Observation,
            text: BoundedText::new(format!("evidence-v{version}")).unwrap(),
            provenance: crate::ledger::store::Provenance {
                worker_id: format!("w{version}"),
                role: String::from("driver"),
                task_id: format!("t{version}"),
                sequence: version as u64,
            },
            confidence: 0.9,
            key: Some(format!("k{version}")),
        };
        let entry_id = live.record(draft).await.unwrap();
        ids.push(entry_id);
        registry.push_version("g", live.snapshot());
    }
    assert_eq!(registry.version_count("g"), 2);
    for (version, id) in ids.iter().enumerate() {
        let snapshot = registry.version("g", version).unwrap();
        assert!(snapshot.has_entry(*id));
        let hits = snapshot.exact(&MemoryScope::Swarm, &format!("k{version}"));
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0]
                .entry
                .text
                .as_str()
                .contains(&format!("evidence-v{version}"))
        );
    }
    assert!(registry.resolves_any("g", ids[0]));
    assert!(registry.resolves_any("g", ids[1]));
    assert!(!registry.resolves_any("g", 4242));
    assert!(registry.version("nope", 0).is_err());
    assert!(registry.version("g", 9).is_err());
}
