use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use chrono::Utc;
use vesper_harness::{
    release_executor::{
        ExternalHealthPort, OfficialStatusSnapshot, PublicationReceipt, ReleaseAdvanceContext,
        ReleaseExecutionPort, VersionBumpReceipt, advance_release,
    },
    release_recovery::{
        EvidenceConfidence, FailureFingerprint, FailureRecord, FocusedProofStatus, GateRecord,
        GitHubEvidencePort, JobSnapshot, JobState, LocalGateRecord, ReleaseControllerEvent,
        ReleaseFailureClass, ReleaseLedger, ReleaseMutationAdmission, ReleaseMutationKind,
        ReleaseRecoveryRecord, ReleaseRecoveryState, RepairAttempt, RrcError,
        apply_controller_event, start_release,
    },
};

fn retry_admissible_record() -> ReleaseRecoveryRecord {
    let original = "1111111111111111111111111111111111111111";
    let mut record = start_release("repair-repo", "patch", "main", original).unwrap();
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
    .enumerate()
    .map(|(index, name)| GateRecord {
        name: name.into(),
        head_sha: original.into(),
        run_id: Some(index as u64 + 1),
        run_attempt: Some(1),
        run_state: Some(JobState::Success),
        jobs: vec![JobSnapshot {
            workflow_id: index as u64 + 10,
            run_id: index as u64 + 1,
            attempt: 1,
            job_id: index as u64 + 100,
            workflow_name: name.into(),
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
    apply_controller_event(
        &mut record,
        ReleaseControllerEvent::VerifiedRepair(RepairAttempt {
            fingerprint: FailureFingerprint("fingerprint".into()),
            causal_family: "fixture".into(),
            hypothesis: "repair the assertion".into(),
            source_commit_before: original.into(),
            source_commit_after: Some("2222222222222222222222222222222222222222".into()),
            focused_proof: "cargo test fixture".into(),
            focused_status: FocusedProofStatus::Passed,
            evidence_refs: vec!["local:focused-proof".into()],
            disproven_or_insufficient: false,
        }),
    )
    .unwrap();
    record
}

struct RepairExecutor(Arc<AtomicUsize>);
impl ReleaseExecutionPort for RepairExecutor {
    fn prepare_version_bump(
        &self,
        _: &str,
        _: ReleaseMutationAdmission,
    ) -> Result<VersionBumpReceipt, RrcError> {
        unreachable!()
    }
    fn run_local_gate(&self, _: &LocalGateRecord) -> Result<String, RrcError> {
        unreachable!()
    }
    fn commit_candidate(&self, _: &str, _: ReleaseMutationAdmission) -> Result<String, RrcError> {
        unreachable!()
    }
    fn push_candidate(&self, admission: ReleaseMutationAdmission) -> Result<String, RrcError> {
        assert_eq!(admission.kind(), ReleaseMutationKind::PushCandidate);
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("origin/main@2222222222222222222222222222222222222222".into())
    }
    fn create_and_push_tag(
        &self,
        _: &str,
        _: &str,
        _: ReleaseMutationAdmission,
    ) -> Result<(String, String), RrcError> {
        unreachable!()
    }
    fn publication(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<Option<PublicationReceipt>, RrcError> {
        unreachable!()
    }
}

struct NoRerunGithub(Arc<AtomicUsize>);
impl GitHubEvidencePort for NoRerunGithub {
    fn matrix_for_sha(&self, _: &str, _: &str) -> Result<Vec<GateRecord>, RrcError> {
        unreachable!()
    }
    fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
        unreachable!()
    }
    fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct UnusedHealth;
impl ExternalHealthPort for UnusedHealth {
    fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
        unreachable!()
    }
}

#[test]
fn advancing_verified_repair_pushes_new_sha_without_actions_rerun() {
    let temp = tempfile::tempdir().unwrap();
    let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repair-repo").unwrap();
    let mut record = retry_admissible_record();
    ledger.save(&record).unwrap();
    let pushes = Arc::new(AtomicUsize::new(0));
    let reruns = Arc::new(AtomicUsize::new(0));

    advance_release(
        &mut record,
        ReleaseAdvanceContext {
            workspace: temp.path(),
            repository: "owner/repo",
            ledger: &ledger,
            executor: &RepairExecutor(Arc::clone(&pushes)),
            github: &NoRerunGithub(Arc::clone(&reruns)),
            health: &UnusedHealth,
            repair_factory: None,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        },
    )
    .unwrap();

    assert_eq!(pushes.load(Ordering::SeqCst), 1);
    assert_eq!(reruns.load(Ordering::SeqCst), 0);
    assert_eq!(record.retry_budget.full_gate_used, 1);
    assert_eq!(record.state, ReleaseRecoveryState::RemoteGateRunning);
    assert!(record.required_gates.is_empty());
}
