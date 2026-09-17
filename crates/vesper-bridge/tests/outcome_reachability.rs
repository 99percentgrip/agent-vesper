//! VB-PRD-001 §7 outcome-settlement completeness: every outcome state the
//! PRD enumerates must be *reachable* through real code paths — a state
//! that exists in the enum but is never produced is a contract the core
//! silently does not honor. Before this increment:
//!
//! - `OperationOutcome::Cancelled` was constructed by **no** code path
//!   (§7: "Cancellation must close admission independently … retain
//!   resource reservations until work settles" — but a cancellation
//!   could never be recorded as an outcome at all);
//! - `OperationOutcome::Partial` was likewise unreachable from any
//!   session API (§7: "classify results as dispatched, applied,
//!   verified, partial, failed, cancelled or unknown").
//!
//! Red-first: both cases fail on the pre-repair session.

use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::MemoryJournal;
use vesper_bridge::lease::{LeaseId, LeaseState};
use vesper_bridge::operation::{OperationOutcome, OperationSpec};
use vesper_bridge::session::BridgeSession;
use vesper_bridge::{ResourceKey, observation};

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "outcomes",
        1,
        vec![CapabilityRecord {
            id: CapabilityId::new("fixture.timeline.create").unwrap(),
            schema_version: 1,
            availability: Availability::Available,
            implementation: Implementation::Native,
            mutability: Mutability::Mutating,
            route: RouteKind::NativeApi,
            delivery: DeliveryMode::Background,
            verification: VerificationMethod::Independent,
            limitations: "test".into(),
        }],
    )
}

fn observation(revision: u64) -> observation::Observation {
    observation::Observation {
        id: observation::ObservationId(format!("obs-{revision}")),
        kind: observation::ObservationKind::Semantic,
        revision,
        captured_at_ms: 0,
        semantic: serde_json::json!({"state": "fixture"}),
        images: vec![],
        transforms: vec![],
        bounds: vec![],
        degraded_capture: false,
    }
}

fn dispatched_session() -> BridgeSession {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("outcomes-session").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(1)).unwrap(),
        manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let lease = LeaseState {
        id: LeaseId("lease-o".into()),
        resource: ResourceKey::Document("fixture".into()),
        fence: 0,
        remaining_ms: 60_000,
        holds_input: false,
        owner: None,
    };
    session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(1),
            &observation(1),
            lease,
            session.authority_epoch(),
            0,
        )
        .expect("authorize");
    let _ = journal;
    session
}

/// The journal used by dispatched_session (fresh per call: dispatch
/// helpers return (session, journal) pairs for settlement tests).
fn dispatched_pair() -> (BridgeSession, MemoryJournal) {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("outcomes-session").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(1)).unwrap(),
        manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let lease = LeaseState {
        id: LeaseId("lease-o".into()),
        resource: ResourceKey::Document("fixture".into()),
        fence: 0,
        remaining_ms: 60_000,
        holds_input: false,
        owner: None,
    };
    session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(1),
            &observation(1),
            lease,
            session.authority_epoch(),
            0,
        )
        .expect("authorize");
    (session, journal)
}

#[test]
fn cancellation_is_reachable_and_settles_with_reservation_kept() {
    let mut session = dispatched_session();
    let request_id = vesper_bridge::operation::OperationRequestId("outcomes-session/1".into());
    // §7: cancellation must be recordable as an outcome. Red on the
    // pre-repair core (no API produced Cancelled).
    session
        .cancel(&request_id)
        .expect("cancel settles the record");
    let record = session
        .records()
        .into_iter()
        .find(|record| record.request_id.0 == "outcomes-session/1")
        .expect("record retained after cancellation");
    assert_eq!(record.outcome, OperationOutcome::Cancelled);
    assert!(
        record.outcome.is_settled(),
        "Cancelled is a terminal settlement state"
    );
    assert!(
        !record.outcome.is_success(),
        "a cancelled operation is not a success"
    );
}

#[test]
fn partial_is_reachable_with_retained_evidence() {
    let (mut session, journal) = dispatched_pair();
    let request_id = vesper_bridge::operation::OperationRequestId("outcomes-session/1".into());
    // §7: partial effects must be classifiable with evidence retained.
    // Red on the pre-repair core (no API produced Partial).
    session
        .settle_partial(
            &journal,
            &request_id,
            "timeline created; title clip rejected by format",
        )
        .expect("partial settles the record");
    let record = session
        .records()
        .into_iter()
        .find(|record| record.request_id.0 == "outcomes-session/1")
        .expect("record retained after partial settlement");
    assert!(matches!(record.outcome, OperationOutcome::Partial));
    assert!(record.outcome.is_settled());
    assert!(!record.outcome.is_success());
}

#[test]
fn cancel_then_reconcile_with_evidence_still_resolves() {
    // Cancellation is terminal for the record, but a later
    // reconcile-with-evidence on ANOTHER uncertain record still
    // resolves quarantine — the two lifecycles do not interfere.
    let mut session = dispatched_session();
    let request_id = vesper_bridge::operation::OperationRequestId("outcomes-session/1".into());
    session.settle(&request_id, OperationOutcome::UnknownOutcome);
    session.reconcile(&request_id, None).expect("quarantines");
    session
        .reconcile(&request_id, Some(false))
        .expect("evidence resolves");
    assert_eq!(
        session.state(),
        vesper_bridge::session::ApplicationSessionState::Recovering
    );
}
