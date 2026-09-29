use chrono::Utc;
use vesper_harness::release_recovery::{
    EvidenceConfidence, FailureFingerprint, FailureRecord, FocusedProofStatus, GateRecord,
    JobSnapshot, JobState, ReleaseControllerEvent, ReleaseFailureClass, ReleaseMutationKind,
    ReleaseRecoveryState, RepairAttempt, admit_release_mutation, apply_controller_event,
    start_release,
};

fn classified_record() -> vesper_harness::release_recovery::ReleaseRecoveryRecord {
    let original = "1111111111111111111111111111111111111111";
    let mut record = start_release("repo", "patch", "main", original).unwrap();
    record.state = ReleaseRecoveryState::ClassifyingFailure;
    record.mutation.candidate_committed = true;
    record.mutation.candidate_pushed = true;
    record.mutation.candidate_push_ref = Some(format!("origin/main@{original}"));
    record.required_gates = [
        "pull-request-validation",
        "msrv",
        "five-target-foundation",
        "web-driver",
    ]
    .into_iter()
    .map(str::to_owned)
    .enumerate()
    .map(|(index, name)| GateRecord {
        name: name.clone(),
        head_sha: original.into(),
        run_id: Some(index as u64 + 1),
        run_attempt: Some(1),
        jobs: vec![JobSnapshot {
            workflow_id: index as u64 + 10,
            run_id: index as u64 + 1,
            attempt: 1,
            job_id: index as u64 + 100,
            workflow_name: name,
            job_name: "fixture".into(),
            platform: Some("linux".into()),
            state: if index == 0 {
                JobState::Failure
            } else {
                JobState::Success
            },
            failed_step: Some("test".into()),
            url: "https://example.invalid/job".into(),
        }],
        url: None,
    })
    .collect();
    record.failures.push(FailureRecord {
        workflow_id: 10,
        run_id: 1,
        attempt: 1,
        job_id: 100,
        workflow_name: "pull-request-validation".into(),
        job_name: "fixture".into(),
        platform: Some("linux".into()),
        step_name: Some("test".into()),
        fingerprint: FailureFingerprint("fingerprint".into()),
        class: ReleaseFailureClass::TestRegression,
        confidence: EvidenceConfidence::Proven,
        causal_excerpt: "assertion failed".into(),
        source_commit: original.into(),
        observed_at: Utc::now(),
        other_platforms_passed: true,
        exists_on_last_green: Some(false),
        related_source_touched: Some(true),
    });
    record
}

#[test]
fn verified_repair_requires_a_fresh_candidate_push_before_remote_retry() {
    let mut record = classified_record();
    apply_controller_event(
        &mut record,
        ReleaseControllerEvent::VerifiedRepair(RepairAttempt {
            fingerprint: FailureFingerprint("fingerprint".into()),
            causal_family: "fixture".into(),
            hypothesis: "repair the assertion".into(),
            source_commit_before: "1111111111111111111111111111111111111111".into(),
            source_commit_after: Some("2222222222222222222222222222222222222222".into()),
            focused_proof: "cargo test fixture".into(),
            focused_status: FocusedProofStatus::Passed,
            evidence_refs: vec!["local:focused-proof".into()],
            disproven_or_insufficient: false,
        }),
    )
    .unwrap();

    assert_eq!(record.state, ReleaseRecoveryState::RetryAdmissible);
    assert!(!record.mutation.candidate_pushed);
    assert!(record.mutation.candidate_push_ref.is_none());
    assert!(admit_release_mutation(&record, ReleaseMutationKind::PushCandidate).is_ok());
}
