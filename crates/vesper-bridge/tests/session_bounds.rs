//! VB-PRD-001 NF-05/07/08, BR-04/13/18, AT-37: the core session must bound
//! its own retained state. "Bounded by caller" is a hidden coupling — a
//! runaway model loop or a slow consumer must not be able to grow Bridge
//! session memory without limit, regardless of who drives it.
//!
//! Red-first: every case below fails on the unbounded pre-repair session.

use vesper_bridge::ResourceKey;
use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::MemoryJournal;
use vesper_bridge::lease::{LeaseId, LeaseState};
use vesper_bridge::observation::{Observation, ObservationId, ObservationKind};
use vesper_bridge::operation::OperationSpec;
use vesper_bridge::session::{Admission, ApplicationSessionState, BridgeSession};

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "bounds",
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

fn ready_session() -> BridgeSession {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bounds-session").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(1)).unwrap(),
        manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();
    session
}

fn observation(revision: u64) -> Observation {
    Observation {
        id: ObservationId(format!("obs-{revision}")),
        kind: ObservationKind::Semantic,
        revision,
        captured_at_ms: 0,
        semantic: serde_json::json!({"state": "fixture"}),
        images: vec![],
        transforms: vec![],
        bounds: vec![],
        degraded_capture: false,
    }
}

fn dispatch(session: &mut BridgeSession, journal: &MemoryJournal, n: usize) {
    // Model a sequential flow: each operation takes the lease, dispatches,
    // and the host releases it before the next one (same as the existing
    // contract tests). Distinct lease ids keep exclusivity honest while
    // fences advance monotonically per resource.
    let lease = LeaseState {
        id: LeaseId(format!("lease-{n}")),
        resource: ResourceKey::Document(format!("fixture-{n}")),
        fence: n as u64,
        remaining_ms: 60_000,
        holds_input: false,
        owner: None,
    };
    // Distinct arguments per dispatch: semantic duplicate suppression
    // (VB §6.6) refuses replays of the same capability+arguments, which
    // would otherwise masquerade as a bounds failure here. The manifest
    // stays a single declared capability.
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({"n": n}),
    };
    session
        .authorize(
            journal,
            &spec,
            false,
            false,
            Generation(1),
            &observation(session.latest_observation_revision() + 1),
            lease,
            session.authority_epoch(),
            0,
        )
        .expect("authorize under bounds");
}

#[test]
fn operation_records_are_bounded_by_the_core() {
    // A runaway caller dispatching well past the cap must not grow the
    // retained record set without limit (NF-05, AT-37).
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    for n in 0..(BridgeSession::MAX_OPERATION_RECORDS * 2) {
        dispatch(&mut session, &journal, n);
    }
    assert!(
        session.records().len() <= BridgeSession::MAX_OPERATION_RECORDS,
        "records must stay <= cap; got {}",
        session.records().len()
    );
}

#[test]
fn outstanding_jobs_are_bounded_by_the_core() {
    // BR-13/18: an application reporting unbounded job strings must not
    // grow the session's outstanding-job list without limit.
    let mut session = ready_session();
    for i in 0..(BridgeSession::MAX_OUTSTANDING_JOBS * 2) {
        session.add_outstanding_job(format!("job-{i}"));
    }
    assert!(
        session.outstanding_jobs().len() <= BridgeSession::MAX_OUTSTANDING_JOBS,
        "outstanding jobs must stay <= cap; got {}",
        session.outstanding_jobs().len()
    );
}

#[test]
fn bounds_are_eviction_not_refusal() {
    // The cap must not break availability: dispatch still succeeds past
    // the cap (oldest records are evicted, newest retained), so a long
    // session keeps working (BR-04 lifecycle, NF-05).
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    for n in 0..(BridgeSession::MAX_OPERATION_RECORDS + 5) {
        dispatch(&mut session, &journal, n);
    }
    assert_eq!(session.state(), ApplicationSessionState::Ready);
    assert_eq!(session.admission(), Admission::Open);
    // The newest records are retained: the final dispatched sequence
    // number must still be present after eviction trimmed the oldest.
    let final_sequence = BridgeSession::MAX_OPERATION_RECORDS + 5; // loop end = exclusive
    let expected_last = format!("bounds-session/{final_sequence}");
    assert!(
        session
            .records()
            .iter()
            .any(|record| record.request_id.0 == expected_last),
        "newest record {expected_last} must survive eviction; retained: {:?}",
        session
            .records()
            .iter()
            .map(|record| record.request_id.0.as_str())
            .collect::<Vec<_>>()
            .last()
    );
    assert!(
        session.records().len() <= BridgeSession::MAX_OPERATION_RECORDS,
        "eviction must hold the cap: {}",
        session.records().len()
    );
}
