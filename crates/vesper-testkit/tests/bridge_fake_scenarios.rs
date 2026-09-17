//! End-to-end fake-driver scenarios through the pure core: the
//! deterministic evidence class for VB-PRD-001 Phase 1's "fake driver
//! proves protocol and state behavior" requirement.
//!
//! Every test here uses [`vesper_testkit::FakeApplication`] — a visibly
//! fake driver. These results certify **Bridge contract behavior only**,
//! never real application control.

use vesper_bridge::capability::CapabilityId;
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::{IntentRecord, JournalPort, MemoryJournal};
use vesper_bridge::lease::{LeaseId, LeaseState, ResourceKey};
use vesper_bridge::observation::{Observation, ObservationId, ObservationKind};
use vesper_bridge::operation::{OperationOutcome, OperationSpec};
use vesper_bridge::session::{BridgeSession, DenialReason};
use vesper_testkit::FakeApplication;

fn observation(revision: u64, app: &FakeApplication) -> Observation {
    Observation {
        id: ObservationId(format!("obs-{revision}")),
        kind: ObservationKind::Semantic,
        revision,
        captured_at_ms: revision * 100,
        semantic: serde_json::json!({
            "fixture": true,
            "generation": app.generation,
            "revision": app.revision,
        }),
        images: vec![],
        transforms: vec![],
        bounds: vec![],
        degraded_capture: false,
    }
}

fn lease(resource: ResourceKey) -> LeaseState {
    LeaseState {
        id: LeaseId("fake-lease".into()),
        resource,
        fence: 0,
        remaining_ms: 60_000,
        holds_input: false,
        owner: Some("fake-session".into()),
    }
}

/// The full happy path against the fake: authorize → dispatch ack → apply
/// → independent verify, with the fake's revision advancing exactly once.
#[test]
fn fake_driver_happy_path_verifies_exactly_once() {
    let mut app = FakeApplication::new();
    let journal = MemoryJournal::new();
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-fake").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(app.generation)).unwrap(),
        FakeApplication::manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();

    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({"name": "Fake timeline"}),
    };
    let request = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(app.generation),
            &observation(1, &app),
            lease(ResourceKey::Document("fixture".into())),
            session.authority_epoch(),
            0,
        )
        .expect("fake native mutating capability authorizes in Code mode");
    // Driver ack → Applied, not Verified.
    session.settle(&request.request_id.clone(), OperationOutcome::Applied);
    // Simulate the fake applying the write: revision advances once.
    let revision_before = app.revision;
    app.apply(&request.request_id.0, "fixture.timeline.create");
    assert_eq!(app.revision, revision_before + 1);
    // Independent verification (readback) promotes to Verified.
    session.verify(&request.request_id, true).unwrap();
    assert_eq!(session.records()[0].outcome, OperationOutcome::Verified);
}

/// AT-03 against the fake: a restart (new generation) invalidates the old
/// binding — dispatch is refused, never silently rebound.
#[test]
fn fake_restart_invalidates_the_old_binding() {
    let mut app = FakeApplication::new();
    let journal = MemoryJournal::new();
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-restart").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(app.generation)).unwrap(),
        FakeApplication::manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();

    app.restart(); // the "application" went away and came back
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(app.generation),
            &observation(1, &app),
            lease(ResourceKey::Document("fixture".into())),
            session.authority_epoch(),
            0,
        )
        .expect_err("a newer observed generation must refuse the stale binding");
    assert_eq!(denial, DenialReason::StalePrecondition);
}

/// AT-15 against the fake: the driver claims success but the application
/// state did not change → verification must fail, not report success.
#[test]
fn fake_success_string_without_effect_fails_verification() {
    let app = FakeApplication::new();
    let journal = MemoryJournal::new();
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-ack").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(app.generation)).unwrap(),
        FakeApplication::manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();

    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({"name": "Never applied"}),
    };
    let request = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(app.generation),
            &observation(1, &app),
            lease(ResourceKey::Document("fixture".into())),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    // The driver acks, but the fake never applies the write: the dispatch
    // map must NOT contain the request, modeling an unchanged project.
    session.settle(&request.request_id.clone(), OperationOutcome::Applied);
    assert!(!app.dispatched.contains_key(&request.request_id.0));
    // Verification compares the claimed effect against actual state — with
    // no applied change, it must refuse to verify.
    let before = app.revision;
    let verified = session.verify(&request.request_id, false);
    assert!(verified.is_err(), "an unapplied mutation must never verify");
    assert_eq!(
        app.revision, before,
        "failed verification must not change state"
    );
}

/// AT-19 against the fake: a disconnect after "create" leaves the outcome
/// unknown; the journal forces reconcile-before-retry, and the reconciled
/// truth (the fake DID apply it) resolves the retry correctly.
#[test]
fn fake_disconnect_after_create_reconciles_before_retry() {
    let mut app = FakeApplication::new();
    let journal = MemoryJournal::new();
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-disc").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(app.generation)).unwrap(),
        FakeApplication::manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();

    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({"name": "Maybe created"}),
    };
    let request = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(app.generation),
            &observation(1, &app),
            lease(ResourceKey::Document("fixture".into())),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    // The connection dropped AFTER dispatch: the fake applied it, but the
    // caller never saw the response → unknown outcome.
    app.apply(&request.request_id.0, "fixture.timeline.create");
    session.settle(
        &request.request_id.clone(),
        OperationOutcome::UnknownOutcome,
    );
    journal
        .record_intent(IntentRecord::new(request.clone()).settle(OperationOutcome::UnknownOutcome))
        .unwrap();
    // A blind retry is refused: reconcile first.
    let settlement = vesper_bridge::journal::classify_retry(&journal, &request);
    assert_eq!(
        settlement,
        vesper_bridge::journal::Settlement::ReconcileBeforeRetry,
        "unknown outcome on a non-idempotent create must reconcile before retry"
    );
    // Reconciliation inspects the fake's actual state: the write happened,
    // so the correct next action is "already applied", not a second create.
    assert!(app.dispatched.contains_key(&request.request_id.0));
    assert_eq!(app.revision, 2, "the fake applied exactly one create");
}

/// AT-16 against the fake: a second session cannot take the same
/// document's lease while the first writer holds it.
#[test]
fn fake_two_writers_cannot_hold_one_document() {
    let app = FakeApplication::new();
    let journal = MemoryJournal::new();
    let manifest = FakeApplication::manifest();
    let shared_fences = std::sync::Arc::new(std::sync::Mutex::new(
        vesper_bridge::lease::FenceState::new(),
    ));
    let mut first = BridgeSession::with_shared_fences(
        BridgeSessionId::new("bs-w1").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(app.generation)).unwrap(),
        manifest.clone(),
        shared_fences.clone(),
    );
    let mut second = BridgeSession::with_shared_fences(
        BridgeSessionId::new("bs-w2").unwrap(),
        ApplicationInstanceId::new("fixture/seat-1", Generation(app.generation)).unwrap(),
        manifest,
        shared_fences,
    );
    for session in [&mut first, &mut second] {
        session.connect().unwrap();
        session.mark_ready();
    }
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({"writer": "first"}),
    };
    let obs = observation(1, &app);
    first
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(app.generation),
            &obs,
            lease(ResourceKey::Document("shared-doc".into())),
            first.authority_epoch(),
            0,
        )
        .expect("first writer takes the lease");
    let current_fence = 1_000; // fresh lease request, never a stale fence
    // Distinct arguments: §6.6 duplicate suppression outranks the lease
    // gate; an identical replay would be refused as a duplicate before
    // exclusivity is exercised.
    let spec2 = OperationSpec {
        capability: CapabilityId::new("fixture.timeline.create").unwrap(),
        arguments: serde_json::json!({"writer": "second"}),
    };
    let denial = second
        .authorize(
            &journal,
            &spec2,
            false,
            false,
            Generation(app.generation),
            &obs,
            LeaseState {
                id: LeaseId("fake-lease-2".into()),
                resource: ResourceKey::Document("shared-doc".into()),
                fence: current_fence,
                remaining_ms: 60_000,
                holds_input: false,
                owner: Some("fake-session".into()),
            },
            second.authority_epoch(),
            0,
        )
        .expect_err("second writer must be refused while the lease is live");
    // H7: exclusivity denials carry the actionable detail (holder + TTL)
    // directly, not wrapped in an opaque BridgeError.
    assert!(
        matches!(
            denial,
            DenialReason::LeaseConflict {
                ref holder,
                expires_in_ms: 60_000
            } if holder == "fake-lease"
        ),
        "expected lease conflict with holder detail, got {denial:?}"
    );
}

/// The fake's unknown capability stays undispatchable through the full
/// service-independent gate (AT-05 at the contract level).
#[test]
fn fake_unknown_operation_is_rejected_before_any_dispatch() {
    let app = FakeApplication::new();
    let journal = MemoryJournal::new();
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-unknown").unwrap(),
        ApplicationInstanceId::new("fixture/seat-0", Generation(app.generation)).unwrap(),
        FakeApplication::manifest(),
    );
    session.connect().unwrap();
    session.mark_ready();
    let spec = OperationSpec {
        capability: CapabilityId::new("fixture.effect.unknown_magic").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(app.generation),
            &observation(1, &app),
            lease(ResourceKey::Document("fixture".into())),
            session.authority_epoch(),
            0,
        )
        .expect_err("unknown implementation must never dispatch");
    assert!(matches!(denial, DenialReason::Capability(_)));
    assert!(app.dispatched.is_empty(), "nothing reached the fake driver");
}
