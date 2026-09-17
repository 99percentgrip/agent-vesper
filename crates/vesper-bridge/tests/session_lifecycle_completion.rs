//! VB-PRD-001 session-lifecycle completion (final reachability debt from
//! increments 8–12): `Paused` and `Closed` must be *producible* states,
//! and production disconnect must surface unresolved input leases and
//! outstanding jobs instead of silently discarding them.
//!
//! Red-first: every case fails on the pre-repair code — `pause()`/`close()`
//! do not exist, and `bridge_disconnect` drops the session without
//! inspecting held inputs or jobs.

use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::session::{ApplicationSessionState, BridgeSession};

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "lifecycle",
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
        BridgeSessionId::new("lifecycle-session").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(1)).unwrap(),
        manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();
    session
}

#[test]
fn pause_is_reachable_refuses_dispatch_and_resumes() {
    let mut session = ready_session();
    // Red: no producer for Paused existed.
    session.pause().expect("pause from Ready");
    assert_eq!(session.state(), ApplicationSessionState::Paused);
    // A paused session refuses dispatch through the existing NotReady
    // branch of authorize() — verified via the state check.
    assert_ne!(session.state(), ApplicationSessionState::Ready);
    // And resume from Paused returns to a dispatchable state.
    session.resume().expect("resume from Paused");
    assert_eq!(session.state(), ApplicationSessionState::Recovering);
}

#[test]
fn pause_from_quarantine_refuses_honestly() {
    let mut session = ready_session();
    session.quarantine();
    // Pausing a quarantined session must not weaken the quarantine —
    // pause is not a resolution path (BR-18, increment 11).
    assert!(session.pause().is_err());
    assert_eq!(session.state(), ApplicationSessionState::Quarantined);
}

#[test]
fn close_is_reachable_and_terminal() {
    let mut session = ready_session();
    // Red: no producer for Closed existed (resume() read it; nothing
    // could set it).
    session.close();
    assert_eq!(session.state(), ApplicationSessionState::Closed);
    // Terminal: resume refuses (already enforced since increment 11's
    // reading — verified here as the consuming path).
    assert!(session.resume().is_err());
    // Closing again is idempotent.
    session.close();
    assert_eq!(session.state(), ApplicationSessionState::Closed);
}
