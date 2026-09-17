//! VB-PRD-001 §8.4 / NF-03 / BR-17: emergency input release must survive
//! ordinary lease revocation. A stopped session whose input-holding lease
//! was revoked normally — but whose driver never confirmed release — must
//! still be able to reach that input for emergency release. Revocation
//! bookkeeping may never strand a held key/button.
//!
//! Red-first: the revoke-then-emergency-release cases fail on the
//! pre-repair fence state, which forgets revoked input holders.

use vesper_bridge::lease::{FenceState, InputLeaseState, LeaseId, LeaseState};
use vesper_bridge::session::BridgeSession;

fn input_lease(resource: &str, fence: u64) -> LeaseState {
    LeaseState {
        id: LeaseId(format!("lease-{fence}")),
        resource: vesper_bridge::ResourceKey::SeatInput(resource.into()),
        fence,
        remaining_ms: 60_000,
        holds_input: true,
        owner: None,
    }
}

#[test]
fn emergency_release_survives_ordinary_revocation() {
    // §8.4: revocation must not strand held inputs. Revoke an
    // input-holding lease normally, then an emergency stop must still
    // report it for release.
    let mut fences = FenceState::new();
    let lease = input_lease("seat-0", 1);
    assert!(matches!(
        fences.admit(input_lease("seat-0", 1)),
        vesper_bridge::lease::LeaseDecision::Granted
    ));
    let revoked = fences.revoke(&lease.id);
    assert!(revoked.is_some(), "ordinary revocation removes the lease");
    let to_release = fences.emergency_release_inputs(u64::MAX);
    assert!(
        to_release.iter().any(|state| state.holds_input),
        "emergency release after ordinary revocation must still reach the held input; got {to_release:?}"
    );
}

#[test]
fn emergency_release_covers_both_live_and_revoked_input_holders() {
    let mut fences = FenceState::new();
    fences.admit(input_lease("seat-live", 1));
    let revoked = input_lease("seat-revoked", 2);
    fences.admit(input_lease("seat-revoked", 2));
    fences.revoke(&revoked.id);
    let to_release = fences.emergency_release_inputs(u64::MAX);
    let seats: Vec<String> = to_release
        .iter()
        .filter(|state| state.holds_input)
        .map(|state| match &state.resource {
            vesper_bridge::ResourceKey::SeatInput(seat) => seat.clone(),
            other => resource_debug(other),
        })
        .collect();
    assert!(
        seats.contains(&"seat-live".to_owned()) && seats.contains(&"seat-revoked".to_owned()),
        "emergency release must cover live AND revoked input holders: {seats:?}"
    );
}

#[test]
fn emergency_release_still_runs_after_ordinary_stop_revoked_the_lease() {
    // End-to-end session shape (AT-21/AT-22 core route): authorize with a
    // held-input lease, ordinary stop (which revokes), then a second stop
    // must still surface the input for emergency release with
    // Unconfirmed state.
    let mut session = ready_session_with_input_lease();
    let first = session.stop(vec![]);
    assert!(
        !first.inputs_to_release.is_empty(),
        "first stop must report the held input"
    );
    assert_eq!(first.input_release, InputLeaseState::Unconfirmed);
    // Second stop after the lease is already revoked: emergency release
    // must remain available, not silently forget the input.
    let second = session.stop(vec![]);
    assert!(
        !second.inputs_to_release.is_empty(),
        "emergency release must survive the ordinary revocation in the first stop: {:?}",
        second.inputs_to_release
    );
}

fn ready_session_with_input_lease() -> BridgeSession {
    use vesper_bridge::capability::{
        Availability, CapabilityId, CapabilityRecord, DeliveryMode, Implementation, Mutability,
        RouteKind, VerificationMethod,
    };
    use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
    use vesper_bridge::session::BridgeSession;
    let manifest = vesper_bridge::capability::CapabilityManifest::new(
        "emergency",
        1,
        vec![CapabilityRecord {
            id: CapabilityId::new("media.timeline.create").unwrap(),
            schema_version: 1,
            availability: Availability::Available,
            implementation: Implementation::Native,
            mutability: Mutability::Mutating,
            route: RouteKind::NativeApi,
            delivery: DeliveryMode::Background,
            verification: VerificationMethod::Independent,
            limitations: "test".into(),
        }],
    );
    let mut session = BridgeSession::new(
        BridgeSessionId::new("emergency-session").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(1)).unwrap(),
        manifest,
    );
    session.connect().unwrap();
    session.mark_ready();
    // Take the foreground-input lease through the real admission path so
    // the fence state actually holds a live input-holding lease.
    let journal = vesper_bridge::journal::MemoryJournal::new();
    let spec = vesper_bridge::operation::OperationSpec {
        capability: vesper_bridge::capability::CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(1),
            &vesper_bridge::observation::Observation {
                id: vesper_bridge::observation::ObservationId("obs-1".into()),
                kind: vesper_bridge::observation::ObservationKind::Semantic,
                revision: 1,
                captured_at_ms: 0,
                semantic: serde_json::json!({"state": "fixture"}),
                images: vec![],
                transforms: vec![],
                bounds: vec![],
                degraded_capture: false,
            },
            input_lease("seat-0", 0),
            session.authority_epoch(),
            0,
        )
        .expect("authorize with an input-holding lease");
    session
}

fn resource_debug(resource: &vesper_bridge::ResourceKey) -> String {
    format!("{resource:?}")
}
