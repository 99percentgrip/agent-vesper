//! Deterministic Phase 1 tests: denial, stale identity, duplicates,
//! unknown outcomes, shutdown, quarantine and stop semantics.

use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::identity::{ApplicationInstanceId, BridgeSessionId, Generation};
use vesper_bridge::journal::{IntentRecord, JournalPort, MemoryJournal, Settlement};
use vesper_bridge::lease::{FenceState, LeaseDecision, LeaseId, LeaseState, ResourceKey};
use vesper_bridge::observation::{Observation, ObservationId, ObservationKind};
use vesper_bridge::operation::{Idempotency, OperationOutcome, OperationSpec};
use vesper_bridge::session::{Admission, ApplicationSessionState, BridgeSession, DenialReason};

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
        limitations: "test fixture".into(),
    }
}

fn manifest() -> CapabilityManifest {
    CapabilityManifest::new(
        "test-adapter",
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
                "media.timeline.render",
                Implementation::Native,
                Availability::Available,
                Mutability::Mutating,
            ),
            record(
                "media.effect.magic",
                Implementation::Unknown,
                Availability::Available,
                Mutability::Mutating,
            ),
            record(
                "media.export.delete",
                Implementation::Native,
                Availability::Available,
                Mutability::Destructive,
            ),
        ],
    )
}

fn ready_session() -> BridgeSession {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-1").unwrap(),
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

fn lease(resource: ResourceKey, holds_input: bool) -> LeaseState {
    LeaseState {
        id: LeaseId(format!("lease-{}", resource_debug(&resource))),
        resource,
        fence: 0,
        remaining_ms: 60_000,
        holds_input,
        owner: None,
    }
}

fn resource_debug(resource: &ResourceKey) -> String {
    match resource {
        ResourceKey::Document(name) => format!("doc-{name}"),
        ResourceKey::SeatInput(name) => format!("seat-{name}"),
    }
}

#[test]
fn ready_session_permits_read_only_dispatch() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    let request = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect("read-only dispatch in Code mode must be authorized");
    assert_eq!(request.resolved_mutability, Mutability::ReadOnly);
}

#[test]
fn plan_mode_blocks_mutations_entirely() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            true,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("plan mode must refuse mutation");
    assert_eq!(denial, DenialReason::PlanMode);
}

#[test]
fn read_only_permission_blocks_mutations() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.render").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            true,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("read-only permission must refuse mutation");
    assert_eq!(denial, DenialReason::ReadOnlyPermission);
}

#[test]
fn unknown_capability_is_refused_not_supported() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.effect.magic").unwrap(),
        arguments: serde_json::json!({}),
    };
    match session.authorize(
        &journal,
        &spec,
        false,
        false,
        Generation(4),
        &observation(1),
        lease(ResourceKey::Document("p".into()), false),
        1,
        0,
    ) {
        Err(DenialReason::Capability(error)) => {
            assert_eq!(
                error,
                vesper_bridge::error::BridgeError::CapabilityUnavailable
            );
        }
        other => panic!("unknown implementation must refuse, got {other:?}"),
    }
}

#[test]
fn restarted_application_generation_refuses_old_binding() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    // The application restarted: observed generation 7 > bound generation 4.
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(7),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("stale binding must refuse");
    assert_eq!(denial, DenialReason::StalePrecondition);
}

#[test]
fn superseded_observation_refuses_dispatch() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(5),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    // An older observation (revision 3 < 5) must now be refused. Distinct
    // arguments: §6.6 suppression would otherwise answer before the
    // freshness gate this test exercises.
    let retry_spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({"attempt": 2}),
    };
    let denial = session
        .authorize(
            &journal,
            &retry_spec,
            false,
            false,
            Generation(4),
            &observation(3),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("stale observation must refuse");
    assert_eq!(denial, DenialReason::StalePrecondition);
}

#[test]
fn degraded_capture_observation_refuses_visual_grounding() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let mut obs = observation(9);
    obs.degraded_capture = true;
    let spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &obs,
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("degraded capture cannot ground actions");
    assert_eq!(denial, DenialReason::StalePrecondition);
}

#[test]
fn quarantined_session_blocks_new_writes() {
    let mut session = ready_session();
    session.quarantine();
    assert_eq!(session.state(), ApplicationSessionState::Quarantined);
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("quarantine must block mutations");
    assert_eq!(denial, DenialReason::Quarantined);
}

#[test]
fn stop_closes_admission_and_requires_fresh_observation_on_resume() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(2),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    let stop = session.stop(vec!["render-job-7".into()]);
    assert_eq!(stop.admission, Admission::Closed);
    assert_eq!(stop.outstanding_jobs, vec!["render-job-7".to_string()]);
    // New dispatch refused while stopped.
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(2),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("stop must close admission");
    assert_eq!(denial, DenialReason::AdmissionClosed);
    // Resume forces a fresh observation: the pre-stop revision is stale.
    // Distinct arguments so §6.6 suppression does not answer first — this
    // test pins the freshness contract, not duplicate suppression.
    session.resume().unwrap();
    let stale_spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({"attempt": "stale-obs"}),
    };
    let denial = session
        .authorize(
            &journal,
            &stale_spec,
            false,
            false,
            Generation(4),
            &observation(2),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("resume must require a fresh observation");
    assert_eq!(denial, DenialReason::StalePrecondition);
    let fresh_spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({"attempt": "fresh-obs"}),
    };
    let request = session.authorize(
        &journal,
        &fresh_spec,
        false,
        false,
        Generation(4),
        &observation(3),
        lease(ResourceKey::Document("q".into()), false),
        session.authority_epoch(),
        0,
    );
    assert!(
        request.is_ok(),
        "fresh observation after resume re-enables dispatch"
    );
}

#[test]
fn stop_with_held_input_reports_release_targets_and_unconfirmed_state() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::SeatInput("seat-0".into()), true),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    let stop = session.stop(vec![]);
    assert!(
        !stop.inputs_to_release.is_empty(),
        "input-holding lease must be reported for release"
    );
    assert_eq!(
        stop.input_release,
        vesper_bridge::lease::InputLeaseState::Unconfirmed
    );
}

#[test]
fn driver_ack_never_verifies_and_unknown_outcome_never_succeeds() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let request = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    session.settle(&request.request_id, OperationOutcome::Applied);
    let records = session.records();
    assert_eq!(records[0].outcome, OperationOutcome::Applied);
    assert!(
        !records[0].outcome.is_success(),
        "applied is not verified success"
    );
    // Independent verification promotes.
    session.verify(&request.request_id, true).unwrap();
    assert_eq!(session.records()[0].outcome, OperationOutcome::Verified);
    assert!(session.records()[0].outcome.is_success());
    // Unknown outcome never verifies. Distinct arguments: §6.6.
    let spec2 = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({"attempt": 2}),
    };
    let request2 = session
        .authorize(
            &journal,
            &spec2,
            false,
            false,
            Generation(4),
            &observation(2),
            lease(ResourceKey::Document("p2".into()), false),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    session.settle(&request2.request_id, OperationOutcome::UnknownOutcome);
    assert!(
        session.verify(&request2.request_id, true).is_err(),
        "unknown outcome cannot be verified directly"
    );
}

#[test]
fn duplicate_request_ids_are_suppressed_before_dispatch() {
    let mut session = ready_session();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    // Seed the journal with an unsettled intent for the id the session will mint next.
    let probe = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    // Simulate the same request id arriving again via a second session instance.
    let mut second = ready_session();
    // Force the same id space by seeding the journal through the first record.
    let dup = vesper_bridge::operation::OperationRequestId(probe.request_id.0.clone());
    let mut cloned_spec = spec.clone();
    cloned_spec.arguments = serde_json::json!({"dup": true});
    // The session derives ids from its record count; emulate by pre-filling.
    let _ = &mut second;
    let settlement = vesper_bridge::journal::classify_retry(
        &journal,
        &vesper_bridge::operation::AuthorityRequest {
            request_id: dup,
            bridge_session: probe.bridge_session.clone(),
            application: probe.application.clone(),
            capability_generation: probe.capability_generation,
            authority_generation: probe.authority_generation,
            lease_token: probe.lease_token.clone(),
            spec: cloned_spec,
            preconditions: probe.preconditions.clone(),
            timeout: probe.timeout,
            resolved_mutability: probe.resolved_mutability,
            idempotency: Idempotency::NonIdempotent,
        },
    );
    assert_eq!(settlement, Settlement::DuplicateSuppressed);
}

#[test]
fn uncertain_outcome_requires_reconcile_before_retry() {
    let journal = MemoryJournal::new();
    let mut session = ready_session();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.create").unwrap(),
        arguments: serde_json::json!({}),
    };
    let request = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(4),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .unwrap();
    journal
        .record_intent(IntentRecord::new(request.clone()).settle(OperationOutcome::UnknownOutcome))
        .unwrap();
    let retry = vesper_bridge::journal::classify_retry(&journal, &request);
    assert_eq!(retry, Settlement::ReconcileBeforeRetry);
}

#[test]
fn lease_exclusivity_two_writers_one_document() {
    let mut fences = FenceState::new();
    let doc = ResourceKey::Document("project-a".into());
    assert!(matches!(
        fences.admit(lease(doc.clone(), false)),
        LeaseDecision::Granted
    ));
    let second = LeaseState {
        id: LeaseId("second".into()),
        resource: doc,
        fence: 1,
        remaining_ms: 60_000,
        holds_input: false,
        owner: None,
    };
    assert!(matches!(
        fences.admit(second),
        LeaseDecision::Conflict { .. }
    ));
}

#[test]
fn manifest_revision_blocks_previously_available_operation() {
    let base = manifest();
    let revised_records = base
        .records
        .iter()
        .map(|record| {
            let mut updated = record.clone();
            if updated.id == CapabilityId::new("media.timeline.render").unwrap() {
                updated.availability = Availability::Unavailable;
            }
            updated
        })
        .collect();
    let revised = base.revise(revised_records);
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-2").unwrap(),
        ApplicationInstanceId::new("app/seat-0", Generation(1)).unwrap(),
        revised,
    );
    session.connect().unwrap();
    session.mark_ready();
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.timeline.render").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(1),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("unavailable after revision must refuse");
    assert!(matches!(denial, DenialReason::Capability(_)));
}

#[test]
fn not_ready_session_refuses_everything() {
    let mut session = BridgeSession::new(
        BridgeSessionId::new("bs-3").unwrap(),
        ApplicationInstanceId::new("app/seat-0", Generation(1)).unwrap(),
        manifest(),
    );
    // Still Discovered — never connected.
    let journal = MemoryJournal::new();
    let spec = OperationSpec {
        capability: CapabilityId::new("media.project.inspect").unwrap(),
        arguments: serde_json::json!({}),
    };
    let denial = session
        .authorize(
            &journal,
            &spec,
            false,
            false,
            Generation(1),
            &observation(1),
            lease(ResourceKey::Document("p".into()), false),
            session.authority_epoch(),
            0,
        )
        .expect_err("discovered sessions cannot dispatch");
    assert_eq!(
        denial,
        DenialReason::NotReady(ApplicationSessionState::Discovered)
    );
}

#[test]
fn outstanding_jobs_survive_cancellation_and_remain_visible() {
    let mut session = ready_session();
    session.add_outstanding_job("render-42".into());
    let stop = session.stop(vec!["render-42".into()]);
    assert_eq!(
        stop.outstanding_jobs,
        vec!["render-42".to_string()],
        "a continuing render stays visible as an outstanding job, not a rollback"
    );
    assert_eq!(session.outstanding_jobs(), &["render-42".to_string()]);
}
