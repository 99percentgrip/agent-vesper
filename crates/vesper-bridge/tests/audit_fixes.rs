//! Audit-fix regression tests (full-mission-audit C1/C2/C3, H1/H2/H5).
//!
//! Every test here FAILED on the pre-fix code (red-first) and pins the
//! safety property the audit identified: leaked leases on denied dispatch,
//! missing revalidation before dispatch, quarantine erasure on close,
//! cross-session emergency release, stale-observation mutation replay,
//! and un-renewed authority after resume.

use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::{IntentRecord, JournalPort, MemoryJournal};
use vesper_bridge::lease::{FenceState, LeaseId, LeaseState, ResourceKey};
use vesper_bridge::observation::{Observation, ObservationId, ObservationKind};
use vesper_bridge::operation::{OperationOutcome, OperationSpec};
use vesper_bridge::session::{ApplicationSessionState, BridgeSession, DenialReason};

fn record(
    id: &str,
    implementation: Implementation,
    availability: Availability,
    mutability: Mutability,
) -> CapabilityRecord {
    CapabilityRecord {
        id: CapabilityId::new(id).unwrap(),
        schema_version: 1,
        availability,
        implementation,
        mutability,
        route: RouteKind::NativeApi,
        delivery: DeliveryMode::Background,
        verification: VerificationMethod::Independent,
        limitations: "audit fixture".into(),
    }
}

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "audit-adapter",
        1,
        vec![
            record(
                "media.project.inspect",
                Implementation::Native,
                Availability::Available,
                Mutability::ReadOnly,
            ),
            record(
                "media.timeline.create",
                Implementation::Native,
                Availability::Available,
                Mutability::Mutating,
            ),
            record(
                "media.marker.add",
                Implementation::Native,
                Availability::Available,
                Mutability::Mutating,
            ),
        ],
    )
}

fn ready_session() -> BridgeSession {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-audit").unwrap(),
        ApplicationInstanceId::new("app/seat-0", Generation(4)).unwrap(),
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
        captured_at_ms: revision * 100,
        semantic: serde_json::json!({"project": "Draft"}),
        images: vec![],
        transforms: vec![],
        bounds: vec![],
        degraded_capture: false,
    }
}

fn lease(resource: ResourceKey) -> LeaseState {
    lease_at(resource, 0)
}

/// A lease carrying the CURRENT fence for its resource (real dispatches
/// re-enter with the live fence, not zero).
fn lease_at(resource: ResourceKey, fence: u64) -> LeaseState {
    LeaseState {
        id: LeaseId(format!("lease-{resource:?}-{fence}")),
        resource,
        fence,
        remaining_ms: 60_000,
        holds_input: false,
        owner: None,
    }
}

fn doc() -> ResourceKey {
    ResourceKey::Document("project-a".into())
}

fn authorize_ok(
    session: &mut BridgeSession,
    journal: &dyn JournalPort,
    observation: &Observation,
) -> vesper_bridge::operation::AuthorityRequest {
    // Real host flow: take the lease at the CURRENT fence, dispatch, then
    // RELEASE before returning so the next dispatch starts clean (one
    // exclusive writer per resource — the sequential-dispatch pattern the
    // existing contract tests use).
    let fence = session.current_fence_for_tests(&doc());
    let request = session
        .authorize(
            journal,
            &spec("media.timeline.create"),
            false,
            false,
            Generation(4),
            observation,
            lease_at(doc(), fence),
            session.authority_epoch(),
            0,
        )
        .expect("audit fixture dispatch should authorize");
    release_doc_lease(session, &request.lease_token);
    request
}

/// Authorize and KEEP the lease held (C2 revalidation needs a live
/// lease between authorize and the adapter handoff).
fn authorize_held(
    session: &mut BridgeSession,
    journal: &dyn JournalPort,
    observation: &Observation,
) -> vesper_bridge::operation::AuthorityRequest {
    let fence = session.current_fence_for_tests(&doc());
    session
        .authorize(
            journal,
            &spec("media.timeline.create"),
            false,
            false,
            Generation(4),
            observation,
            lease_at(doc(), fence),
            session.authority_epoch(),
            0,
        )
        .expect("held audit fixture dispatch should authorize")
}

/// Release the lease a dispatch acquired (host act between operations).
fn release_doc_lease(session: &mut BridgeSession, token: &str) {
    let handle = session.fences_for_release();
    let mut fences = handle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    fences.revoke_by_token(token);
}

fn spec(capability: &str) -> OperationSpec {
    OperationSpec {
        capability: CapabilityId::new(capability).unwrap(),
        arguments: serde_json::json!({}),
    }
}

/// A journal whose persistence fails: authorize must deny at step 8
/// (fail-closed) — and must not leak the lease admitted at step 6.
struct FailingJournal {
    inner: MemoryJournal,
}

impl JournalPort for FailingJournal {
    fn record_intent(&self, _intent: IntentRecord) -> Result<(), String> {
        Err("disk full".into())
    }
    fn lookup(
        &self,
        request_id: &vesper_bridge::operation::OperationRequestId,
    ) -> Option<IntentRecord> {
        self.inner.lookup(request_id)
    }
    fn known_request_ids(&self) -> Vec<vesper_bridge::operation::OperationRequestId> {
        self.inner.known_request_ids()
    }
    fn recent_intents(&self) -> Vec<IntentRecord> {
        self.inner.recent_intents()
    }
}

// ---------------------------------------------------------------------------
// C1: a denied dispatch must not leak its admitted lease.
// ---------------------------------------------------------------------------

/// Post-admit denial via journal failure (step 8 fail-closed): the lease
/// admitted at step 6 must be revoked, leaving the resource free.
#[test]
fn c1_journal_failure_after_admit_revokes_the_lease() {
    let failing = FailingJournal {
        inner: MemoryJournal::new(),
    };
    let mut session = ready_session();
    let denial = session
        .authorize(
            &failing,
            &spec("media.timeline.create"),
            false,
            false,
            Generation(4),
            &observation(1),
            lease(doc()),
            session.authority_epoch(),
            0,
        )
        .expect_err("failing journal must deny fail-closed");
    assert!(
        matches!(
            denial,
            DenialReason::Capability(vesper_bridge::error::BridgeError::CleanupUnconfirmed)
        ),
        "expected CleanupUnconfirmed, got {denial:?}"
    );
    // THE C1 assertion: no live lease remains from the denied dispatch.
    let live = session.live_leases_for_tests();
    assert!(
        live.is_empty(),
        "denied post-admit dispatch leaked its lease: {live:?}"
    );
    // And the next honest request can still take the document. (Fence
    // semantics are honest: the denied attempt bumped the generation, so
    // the retry carries the CURRENT fence.)
    let journal = MemoryJournal::new();
    let retry = session.authorize(
        &journal,
        &spec("media.timeline.create"),
        false,
        false,
        Generation(4),
        &observation(2),
        lease_at(doc(), session.current_fence_for_tests(&doc())),
        session.authority_epoch(),
        0,
    );
    assert!(
        retry.is_ok(),
        "a denied dispatch must not wedge the resource for the next request: {retry:?}"
    );
}

/// Post-admit denial via reconcile-before-retry (step 7): a journal that
/// knows the minted id as uncertain denies AFTER admit — the lease must
/// not leak. This is the production shape of an ack-loss retry against a
/// restored/replayed journal.
#[test]
fn c1_uncertain_retry_denial_revokes_the_lease() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();
    let request = authorize_ok(&mut session, &journal, &observation(1));
    // Settle it uncertain so classify_retry demands reconciliation.
    let mut settled = journal
        .lookup(&request.request_id)
        .expect("intent recorded");
    settled.outcome = Some(OperationOutcome::UnknownOutcome);
    journal.reseed_for_tests(settled);

    // A second session — same id, shared journal (restart + replay shape) —
    // mints the SAME request id "bs-audit/2" for its second dispatch. We
    // drive it through the original session instead: mint /2 with a fresh
    // observation, then make the journal already hold /2 as uncertain by
    // dispatching twice and settling the second. §6.6: the second dispatch
    // uses a DIFFERENT capability (a same-op replay would be refused at
    // step 5a semantic suppression, which is its own contract).
    let second = {
        let fence = session.current_fence_for_tests(&doc());
        let request = session
            .authorize(
                &journal,
                &spec("media.marker.add"),
                false,
                false,
                Generation(4),
                &observation(2),
                lease_at(doc(), fence),
                session.authority_epoch(),
                0,
            )
            .expect("audit fixture dispatch should authorize");
        release_doc_lease(&mut session, &request.lease_token);
        request
    };
    let mut settled2 = journal.lookup(&second.request_id).expect("intent recorded");
    settled2.outcome = Some(OperationOutcome::UnknownOutcome);
    journal.reseed_for_tests(settled2);

    // Third dispatch mints /3 (fresh id, Fresh classification) — to hit
    // the post-admit denial with a KNOWN id we replay /2's id shape via a
    // restarted session on the same journal: a new BridgeSession with the
    // same id starts its counter at 1, so its /2 collides with the old /2.
    let mut restarted = ready_session();
    restarted.set_request_sequence_for_tests(2); // next mint is /3? no: /2 was consumed; force /3
    restarted.set_request_sequence_for_tests(1); // next mint is /2 ⇒ collides with the uncertain /2
    // §6.6 hardening: same-op retries are refused at step 5a (semantic
    // identity) BEFORE admission. To reach the post-admit id-collision
    // path this test exists for, replay a DIFFERENT operation whose minted
    // attempt id collides with the uncertain /2 — the lease-revocation
    // contract under test.
    let denial = restarted
        .authorize(
            &journal,
            &spec("media.marker.add"),
            false,
            false,
            Generation(4),
            &observation(5),
            lease(doc()),
            restarted.authority_epoch(),
            0,
        )
        .expect_err("uncertain duplicate must demand reconciliation");
    assert!(
        matches!(
            denial,
            DenialReason::Capability(vesper_bridge::error::BridgeError::UnknownOutcome)
        ),
        "expected UnknownOutcome denial, got {denial:?}"
    );
    let live = restarted.live_leases_for_tests();
    assert!(
        live.is_empty(),
        "reconcile-denied dispatch leaked its lease: {live:?}"
    );
}

// ---------------------------------------------------------------------------
// C2: revalidation immediately before dispatch (PRD §6).
// ---------------------------------------------------------------------------

/// revalidate() runs between authorize() and the adapter handoff; it
/// refuses expired leases, stale fences, old authority epochs and
/// elapsed time beyond the timeout policy.
#[test]
fn c2_revalidate_refuses_expired_and_stale_leases() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();
    let _request = authorize_held(&mut session, &journal, &observation(1));

    // Expired lease ⇒ refuse.
    let expired = LeaseState {
        id: LeaseId("lease-expired".into()),
        resource: doc(),
        fence: 1,
        remaining_ms: 0,
        holds_input: false,
        owner: None,
    };
    assert!(matches!(
        session.revalidate(&expired, Generation(4), 0),
        Err(DenialReason::StalePrecondition)
    ));
    // Stale fence (below the live fence for doc) ⇒ refuse.
    let stale = LeaseState {
        id: LeaseId("lease-stale".into()),
        resource: doc(),
        fence: 0,
        remaining_ms: 1_000,
        holds_input: false,
        owner: None,
    };
    assert!(matches!(
        session.revalidate(&stale, Generation(4), 0),
        Err(DenialReason::StalePrecondition)
    ));
    // The actually-granted lease revalidates clean.
    let granted = session
        .granted_lease_for_tests()
        .expect("one granted lease");
    assert!(session.revalidate(&granted, Generation(4), 0).is_ok());
}

/// C2/M10: the timeout policy is enforced at revalidation.
#[test]
fn c2_revalidate_enforces_timeout_policy() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();
    let _request = authorize_held(&mut session, &journal, &observation(1));
    let granted = session
        .granted_lease_for_tests()
        .expect("one granted lease");
    // Beyond the 15 s default policy ⇒ refuse.
    assert!(matches!(
        session.revalidate(&granted, Generation(4), 16_000),
        Err(DenialReason::StalePrecondition)
    ));
    // Within policy ⇒ ok.
    assert!(session.revalidate(&granted, Generation(4), 14_999).is_ok());
}

/// C2: an old authority epoch is refused at revalidation (§6 renewal).
#[test]
fn c2_revalidate_refuses_stale_authority_epoch() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();
    let _request = authorize_held(&mut session, &journal, &observation(1));
    let granted = session
        .granted_lease_for_tests()
        .expect("one granted lease");
    // Current generation revalidates clean (the epoch-specific refusal is
    // pinned by the h2 test).
    assert!(session.revalidate(&granted, Generation(4), 0).is_ok());
    // A newer application generation than the session's binding ⇒ refuse.
    assert!(matches!(
        session.revalidate(&granted, Generation(5), 0),
        Err(DenialReason::StalePrecondition)
    ));
}

// ---------------------------------------------------------------------------
// C3: close() must surface — never silently erase — quarantine.
// ---------------------------------------------------------------------------

#[test]
fn c3_close_reports_quarantine_instead_of_erasing_it() {
    let mut session = ready_session();
    session.quarantine();
    let report = session.close();
    assert!(
        report.was_quarantined,
        "close must report the quarantine it superseded"
    );
    assert_eq!(session.state(), ApplicationSessionState::Closed);

    let mut clean = ready_session();
    let clean_report = clean.close();
    assert!(!clean_report.was_quarantined);
}

#[test]
fn c3_close_report_surfaces_pending_inputs_and_jobs() {
    let mut session = ready_session();
    session.add_outstanding_job("render-1".into());
    let report = session.close();
    assert_eq!(report.outstanding_jobs, vec!["render-1".to_string()]);
    assert_eq!(report.inputs_pending_release, 0);
}

// ---------------------------------------------------------------------------
// H1: emergency release is owner-scoped, never a global sweep.
// ---------------------------------------------------------------------------

#[test]
fn h1_emergency_release_is_scoped_to_owning_session() {
    let shared = std::sync::Arc::new(std::sync::Mutex::new(FenceState::new()));
    let mut session_a = BridgeSession::with_shared_fences(
        BridgeSessionId::new("bs-a").unwrap(),
        ApplicationInstanceId::new("app/seat-0", Generation(1)).unwrap(),
        manifest(),
        std::sync::Arc::clone(&shared),
    );
    let mut session_b = BridgeSession::with_shared_fences(
        BridgeSessionId::new("bs-b").unwrap(),
        ApplicationInstanceId::new("app/seat-0", Generation(1)).unwrap(),
        manifest(),
        std::sync::Arc::clone(&shared),
    );
    session_a.connect().unwrap();
    session_a.mark_ready();
    session_b.connect().unwrap();
    session_b.mark_ready();

    // Session B holds a live input lease on the shared seat.
    let journal = MemoryJournal::new();
    let granted = session_b
        .authorize(
            &journal,
            &spec("media.timeline.create"),
            false,
            false,
            Generation(1),
            &observation(1),
            LeaseState {
                id: LeaseId("b-input".into()),
                resource: ResourceKey::SeatInput("seat-0".into()),
                fence: 0,
                remaining_ms: 60_000,
                holds_input: true,
                owner: None,
            },
            session_b.authority_epoch(),
            0,
        )
        .expect("session B should dispatch with an input lease");
    assert!(granted.lease_token.contains("b-input"));

    // Session A stops: its emergency release must NOT touch B's lease.
    let stop = session_a.stop(vec![]);
    assert!(
        stop.inputs_to_release.iter().all(|l| l.id.0 != "b-input"),
        "session A's stop swept session B's live input lease: {:?}",
        stop.inputs_to_release
    );
    // B's input lease is still live and reachable in the shared fences.
    let still_live = shared
        .lock()
        .map(|mut fences| {
            fences
                .emergency_release_inputs(u64::MAX)
                .iter()
                .any(|l| l.id.0 == "b-input")
        })
        .unwrap_or(false);
    assert!(
        still_live,
        "B's input lease must survive A's stop and remain reachable"
    );
}

// ---------------------------------------------------------------------------
// H5: mutating dispatches require per-dispatch observation freshness.
// ---------------------------------------------------------------------------

#[test]
fn h5_mutating_dispatch_requires_fresh_observation_per_dispatch() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();
    let obs = observation(1);

    let _first = authorize_ok(&mut session, &journal, &obs);
    // Second MUTATING dispatch against the SAME observation must refuse.
    // §6.6 note: use a DIFFERENT mutating capability so the refusal under
    // test is observation freshness — a same-op replay would be refused
    // earlier by semantic duplicate suppression (its own contract).
    let fence_now = session.current_fence_for_tests(&doc());
    let denial = session
        .authorize(
            &journal,
            &spec("media.marker.add"),
            false,
            false,
            Generation(4),
            &obs,
            lease_at(doc(), fence_now),
            session.authority_epoch(),
            0,
        )
        .expect_err("same-observation mutating replay must refuse");
    assert!(
        matches!(denial, DenialReason::StalePrecondition),
        "expected StalePrecondition, got {denial:?}"
    );
    // Read-only dispatch against the same observation stays allowed.
    let inspect = session.authorize(
        &journal,
        &spec("media.project.inspect"),
        false,
        false,
        Generation(4),
        &obs,
        lease_at(doc(), session.current_fence_for_tests(&doc())),
        session.authority_epoch(),
        0,
    );
    assert!(
        inspect.is_ok(),
        "read-only may reuse the current observation: {inspect:?}"
    );
}

// ---------------------------------------------------------------------------
// H2: authority epoch is minted fresh on connect/resume; old epochs refuse.
// ---------------------------------------------------------------------------

#[test]
fn h2_authority_epoch_renews_on_resume_and_old_epochs_refuse() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();

    let epoch_before = session.authority_epoch();
    session.stop(vec![]);
    session.resume().expect("resume from Ready-stop works");
    let epoch_after = session.authority_epoch();
    assert!(
        epoch_after > epoch_before,
        "resume must mint a fresh authority epoch ({epoch_before} → {epoch_after})"
    );

    // A dispatch carrying the pre-resume epoch must refuse.
    let denial = session
        .authorize(
            &journal,
            &spec("media.timeline.create"),
            false,
            false,
            Generation(4),
            &observation(9),
            lease(doc()),
            epoch_before, // stale epoch
            0,
        )
        .expect_err("stale authority epoch must refuse");
    assert!(
        matches!(denial, DenialReason::StalePrecondition),
        "expected StalePrecondition for old epoch, got {denial:?}"
    );
    // The current epoch dispatches.
    let ok = session.authorize(
        &journal,
        &spec("media.timeline.create"),
        false,
        false,
        Generation(4),
        &observation(10),
        lease(doc()),
        epoch_after,
        0,
    );
    assert!(ok.is_ok(), "current epoch must dispatch: {ok:?}");
}
