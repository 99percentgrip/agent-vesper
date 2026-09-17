//! VB-PRD-001 BR-17 settlement reporting: `StopOutcome::input_release`
//! must be able to reach `Released` after the host confirms the driver
//! released its inputs. Before this increment `InputLeaseState::Released`
//! existed as an enum variant that NO code path constructed — every stop
//! stayed `Unconfirmed` forever, and the doc's "host fills after driver
//! ack" mechanism did not exist.
//!
//! Red-first: these cases fail on the pre-repair session.

use vesper_bridge::ResourceKey;
use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::MemoryJournal;
use vesper_bridge::lease::{InputLeaseState, LeaseId, LeaseState};
use vesper_bridge::observation::{Observation, ObservationId, ObservationKind};
use vesper_bridge::operation::OperationSpec;
use vesper_bridge::session::{BridgeSession, StopOutcome};

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "release",
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

fn ready_session_with_input_lease() -> BridgeSession {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("release-session").unwrap(),
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
        id: LeaseId("lease-input".into()),
        resource: ResourceKey::SeatInput("seat-0".into()),
        fence: 0,
        remaining_ms: 60_000,
        holds_input: true,
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
        .expect("authorize with an input-holding lease");
    session
}

#[test]
fn stop_reports_unconfirmed_then_confirmation_reaches_released() {
    let mut session = ready_session_with_input_lease();
    let stop: StopOutcome = session.stop(vec![]);
    assert_eq!(stop.input_release, InputLeaseState::Unconfirmed);
    assert!(!stop.inputs_to_release.is_empty());
    // The host confirms the driver released its inputs (BR-17).
    session.confirm_input_release();
    let after = session.stop(vec![]);
    assert_eq!(
        after.input_release,
        InputLeaseState::Released,
        "confirmed release must be reported as Released, not re-warned forever"
    );
    assert!(
        after.inputs_to_release.is_empty(),
        "confirmed inputs must not be re-reported for emergency release: {:?}",
        after.inputs_to_release
    );
}

#[test]
fn confirmation_without_a_prior_stop_is_harmless_and_truthful() {
    let mut session = ready_session_with_input_lease();
    // Confirming with no stop in flight must not fabricate a Released
    // history or panic; the next stop still reports truthfully.
    session.confirm_input_release();
    let stop = session.stop(vec![]);
    assert_eq!(stop.input_release, InputLeaseState::Released);
}
