//! VB-PRD-001 BR-18 / §7 quarantine lifecycle: quarantine must be
//! honestly terminal until reconciliation, and reconciliation with
//! evidence must be able to resolve it. Before this increment:
//! 1. `resume()` on a QUARANTINED session claimed "admission open;
//!    fresh observation required" while every dispatch stayed denied —
//!    a truthful-reporting lie.
//! 2. `reconcile()` quarantined on missing evidence but nothing could
//!    ever clear quarantine — the doc's "until a human/reconciler
//!    resolves it" path did not exist.
//!
//! Red-first: both cases fail on the pre-repair session.

use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::MemoryJournal;
use vesper_bridge::operation::{OperationRequestId, OperationSpec};
use vesper_bridge::session::BridgeSession;

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "quarantine",
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

fn quarantined_session_with_uncertain_record() -> BridgeSession {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("quarantine-session").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(1)).unwrap(),
        manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();
    // Dispatch one operation, settle it as unknown, then reconcile with
    // NO evidence: the core contract is that this quarantines the session.
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let lease = vesper_bridge::LeaseState {
        id: vesper_bridge::lease::LeaseId("lease-q".into()),
        resource: vesper_bridge::ResourceKey::Document("fixture".into()),
        fence: 0,
        remaining_ms: 60_000,
        holds_input: false,
        owner: None,
    };
    let request = session
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
    session.settle(
        &request.request_id,
        vesper_bridge::operation::OperationOutcome::UnknownOutcome,
    );
    let settled = session
        .reconcile(&request.request_id, None)
        .expect("reconcile returns the outcome");
    assert_eq!(
        settled,
        vesper_bridge::operation::OperationOutcome::UnknownOutcome
    );
    assert_eq!(
        session.state(),
        vesper_bridge::session::ApplicationSessionState::Quarantined
    );
    session
}

fn observation(revision: u64) -> vesper_bridge::observation::Observation {
    vesper_bridge::observation::Observation {
        id: vesper_bridge::observation::ObservationId(format!("obs-{revision}")),
        kind: vesper_bridge::observation::ObservationKind::Semantic,
        revision,
        captured_at_ms: 0,
        semantic: serde_json::json!({"state": "fixture"}),
        images: vec![],
        transforms: vec![],
        bounds: vec![],
        degraded_capture: false,
    }
}

#[test]
fn resume_on_a_quarantined_session_refuses_truthfully() {
    let mut session = quarantined_session_with_uncertain_record();
    // Red on pre-repair code: resume() returned Ok(()) and the harness
    // then claimed "admission open; fresh observation required" while
    // every dispatch stayed quarantined-denied.
    let resumed = session.resume();
    assert!(
        resumed.is_err(),
        "resume on a quarantined session must refuse: quarantine requires reconciliation, not resume"
    );
    // And the session stays quarantined — resume must not silently
    // weaken the quarantine state.
    assert_eq!(
        session.state(),
        vesper_bridge::session::ApplicationSessionState::Quarantined
    );
}

#[test]
fn reconciliation_with_evidence_resolves_the_quarantine() {
    let mut session = quarantined_session_with_uncertain_record();
    // A reconciler now supplies actual evidence: the effect did not
    // hold. That resolution must clear the quarantine (BR-18: the
    // session becomes recoverable) — red on pre-repair code, which had
    // no path out of Quarantined at all.
    let request_id = OperationRequestId("quarantine-session/1".into());
    session
        .reconcile(&request_id, Some(false))
        .expect("evidence-backed reconcile settles");
    assert_eq!(
        session.state(),
        vesper_bridge::session::ApplicationSessionState::Recovering,
        "resolved quarantine must move to Recovering so resume is possible"
    );
    // After resolution, resume works and the session is usable again.
    session.resume().expect("resume after reconciliation");
    assert_eq!(
        session.state(),
        vesper_bridge::session::ApplicationSessionState::Recovering
    );
}
