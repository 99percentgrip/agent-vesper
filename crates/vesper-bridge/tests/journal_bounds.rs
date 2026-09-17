//! VB-PRD-001 NF-05/AT-37: the production-path journal (`MemoryJournal`
//! is the harness composition's journal) must bound its own retention.
//! A long session dispatching forever must not grow journal memory
//! without limit, and eviction must never corrupt duplicate-suppression
//! (BR-16): an evicted id is simply `Fresh` again, which is the honest
//! answer once no intent record survives for it.
//!
//! Red-first: every case below fails on the unbounded pre-repair journal.

use vesper_bridge::journal::{JournalPort, MemoryJournal};

fn record_intent(journal: &MemoryJournal, sequence: u64) {
    let request =
        vesper_bridge::operation::OperationRequestId(format!("journal-bounds/{sequence}"));
    journal
        .record_intent(vesper_bridge::journal::IntentRecord {
            request: vesper_bridge::operation::AuthorityRequest {
                request_id: request,
                bridge_session: vesper_bridge::identity::BridgeSessionId::new("bounds").unwrap(),
                application: vesper_bridge::identity::ApplicationInstanceId::new(
                    "fixture/seat-0",
                    vesper_bridge::identity::Generation(1),
                )
                .unwrap(),
                capability_generation: 1,
                authority_generation: 1,
                lease_token: "lease".into(),
                spec: vesper_bridge::operation::OperationSpec {
                    capability: vesper_bridge::capability::CapabilityId::new(
                        "fixture.timeline.create",
                    )
                    .unwrap(),
                    arguments: serde_json::json!({}),
                },
                preconditions: vesper_bridge::operation::Preconditions {
                    observation: vesper_bridge::observation::ObservationId("obs-1".into()),
                    resource_revision: vesper_bridge::identity::ResourceRevision(1),
                    application_generation: vesper_bridge::identity::Generation(1),
                },
                timeout: vesper_bridge::operation::TimeoutPolicy::default(),
                resolved_mutability: vesper_bridge::capability::Mutability::Mutating,
                idempotency: vesper_bridge::operation::Idempotency::NonIdempotent,
            },
            outcome: None,
            reconciled: false,
            partial_evidence: None,
        })
        .expect("record_intent under bounds");
}

#[test]
fn memory_journal_bounds_its_retention() {
    let journal = MemoryJournal::new();
    for sequence in 0..(MemoryJournal::MAX_RECORDS * 2) as u64 {
        record_intent(&journal, sequence);
    }
    assert!(
        journal.known_request_ids().len() <= MemoryJournal::MAX_RECORDS,
        "journal must stay <= cap; got {}",
        journal.known_request_ids().len()
    );
}

#[test]
fn evicted_ids_read_as_fresh_not_as_errors() {
    // Eviction must keep the journal usable: the newest window resolves,
    // an evicted id is absent (Fresh), and recording still succeeds.
    let journal = MemoryJournal::new();
    for sequence in 0..(MemoryJournal::MAX_RECORDS + 8) as u64 {
        record_intent(&journal, sequence);
    }
    let ids = journal.known_request_ids();
    assert_eq!(ids.len(), MemoryJournal::MAX_RECORDS);
    // Newest window retained: the final sequence is present.
    let last = format!("journal-bounds/{}", MemoryJournal::MAX_RECORDS + 7);
    assert!(
        ids.iter().any(|id| id.0 == last),
        "newest intent must survive eviction"
    );
    // Evicted oldest id reads as absent (None), never a corrupt entry.
    let evicted = vesper_bridge::operation::OperationRequestId("journal-bounds/0".into());
    assert!(journal.lookup(&evicted).is_none());
    // And the journal still accepts writes at the cap.
    record_intent(&journal, (MemoryJournal::MAX_RECORDS + 8) as u64);
    assert!(
        journal.known_request_ids().iter().any(|id| id.0
            == "journal-bounds/".to_owned() + &(MemoryJournal::MAX_RECORDS + 8).to_string())
    );
}
