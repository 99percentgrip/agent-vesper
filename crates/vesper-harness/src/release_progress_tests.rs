#![forbid(unsafe_code)]

use super::release_executor::{ReleaseWorkerSnapshot, render_active_worker_status};
use super::release_recovery::{
    GateRecord, JobSnapshot, JobState, LocalGateRecord, MAX_PROGRESS_MILESTONES, ReleaseObjective,
    ReleaseProgressState, ReleaseRecoveryRecord, ReleaseRecoveryState, SettlementState,
};

fn record() -> ReleaseRecoveryRecord {
    ReleaseRecoveryRecord::new(
        "progress-test-repository".into(),
        "progress-test-epoch".into(),
        ReleaseObjective {
            bump: "patch".into(),
            branch_ref: "main".into(),
            request: Some("release completed progress work".into()),
            post_release_main_epoch: false,
            required_gate_names: vec!["canonical".into()],
        },
    )
}

fn local_verification_record() -> ReleaseRecoveryRecord {
    let mut record = record();
    record
        .transition(
            ReleaseRecoveryState::Preparing,
            "source-commit",
            "isolated workspace is ready",
            Vec::new(),
            None,
        )
        .expect("preparing transition");
    record
        .transition(
            ReleaseRecoveryState::LocalVerification,
            "source-commit",
            "local verification begins",
            Vec::new(),
            None,
        )
        .expect("local verification transition");
    record
}

#[test]
fn typed_progress_tracks_local_gate_and_survives_ledger_serialization() {
    let mut record = local_verification_record();
    record.mutation.local_gates = vec![
        LocalGateRecord {
            name: "workspace-verify".into(),
            state: SettlementState::Running,
            command: "cargo xtask verify".into(),
            evidence_ref: None,
        },
        LocalGateRecord {
            name: "acceptance".into(),
            state: SettlementState::NotStarted,
            command: "cargo xtask acceptance".into(),
            evidence_ref: None,
        },
    ];
    record.note_progress_milestone("Running local gate 1/2: workspace-verify");

    assert_eq!(record.progress.phase.label(), "Local verification");
    assert_eq!(
        record.progress.current_gate.as_deref(),
        Some("workspace-verify")
    );
    assert_eq!(record.progress.completed_local_gates, 0);
    assert_eq!(record.progress.total_local_gates, 2);
    assert!(
        record
            .progress
            .milestones
            .last()
            .is_some_and(|milestone| milestone.summary.contains("workspace-verify"))
    );

    let restored: ReleaseRecoveryRecord =
        serde_json::from_slice(&serde_json::to_vec(&record).expect("serialize progress"))
            .expect("deserialize progress");
    assert_eq!(restored.progress, record.progress);
    let status = restored.render_status();
    assert!(
        status.contains("Progress              Local verification"),
        "{status}"
    );
    assert!(status.contains("Recent milestones"), "{status}");
}

#[test]
fn persisted_hierarchy_uses_typed_gate_and_job_states_without_percentages() {
    let mut record = local_verification_record();
    record.mutation.local_gates = vec![
        LocalGateRecord {
            name: "workspace-verify".into(),
            state: SettlementState::Succeeded,
            command: "cargo xtask verify".into(),
            evidence_ref: Some("local:workspace".into()),
        },
        LocalGateRecord {
            name: "acceptance".into(),
            state: SettlementState::Running,
            command: "cargo xtask acceptance".into(),
            evidence_ref: None,
        },
    ];
    record.required_gates = vec![GateRecord {
        name: "canonical".into(),
        head_sha: "source-commit".into(),
        run_id: Some(10),
        run_attempt: Some(1),
        url: None,
        jobs: vec![
            JobSnapshot {
                workflow_id: 1,
                run_id: 10,
                attempt: 1,
                job_id: 11,
                workflow_name: "canonical".into(),
                job_name: "linux".into(),
                platform: Some("linux".into()),
                state: JobState::Success,
                failed_step: None,
                url: "https://example.invalid/11".into(),
            },
            JobSnapshot {
                workflow_id: 1,
                run_id: 10,
                attempt: 1,
                job_id: 12,
                workflow_name: "canonical".into(),
                job_name: "windows".into(),
                platform: Some("windows".into()),
                state: JobState::InProgress,
                failed_step: None,
                url: "https://example.invalid/12".into(),
            },
        ],
    }];
    record.refresh_progress();
    record
        .progress
        .update_local_subtask("acceptance", "Exact acceptance cases", 18, 41);

    let local = record
        .progress
        .tasks
        .iter()
        .find(|task| task.name == "Local verification")
        .expect("local branch");
    assert_eq!(local.state, ReleaseProgressState::Running);
    assert_eq!(local.units.render().as_deref(), Some("1/2"));
    let acceptance = local
        .children
        .iter()
        .find(|task| task.name == "acceptance")
        .expect("acceptance gate");
    assert_eq!(acceptance.state, ReleaseProgressState::Running);
    let exact_cases = acceptance
        .children
        .iter()
        .find(|task| task.name == "Exact acceptance cases")
        .expect("acceptance child");
    assert_eq!(exact_cases.state, ReleaseProgressState::Running);
    assert_eq!(exact_cases.units.render().as_deref(), Some("18/41"));

    let ci = record
        .progress
        .tasks
        .iter()
        .find(|task| task.name == "Required CI")
        .expect("CI branch");
    assert_eq!(ci.state, ReleaseProgressState::Running);
    assert_eq!(ci.children[0].units.render().as_deref(), Some("1/2"));
    assert_eq!(
        ci.children[0].children[0].state,
        ReleaseProgressState::Passed
    );
    assert_eq!(
        ci.children[0].children[1].state,
        ReleaseProgressState::Running
    );

    let round_trip: ReleaseRecoveryRecord =
        serde_json::from_slice(&serde_json::to_vec(&record).expect("serialize hierarchy"))
            .expect("deserialize hierarchy");
    assert_eq!(round_trip.progress.tasks, record.progress.tasks);
}

#[test]
fn text_host_run_status_uses_the_same_persisted_progress_snapshot() {
    let mut record = local_verification_record();
    record.mutation.local_gates = vec![LocalGateRecord {
        name: "workspace-verify".into(),
        state: SettlementState::Running,
        command: "cargo xtask verify".into(),
        evidence_ref: None,
    }];
    record.note_progress_milestone("Running local gate 1/1: workspace-verify");

    let status = render_active_worker_status(&ReleaseWorkerSnapshot {
        repo_identity: record.repo_identity.clone(),
        epoch_id: record.epoch_id.clone(),
        stage: "Release recovery".into(),
        detail: record.progress.headline.clone(),
        cancellable: true,
        current_gate: record.progress.current_gate.clone(),
        current_command: Some("cargo xtask verify".into()),
        current_child: Some("cargo".into()),
        gate_elapsed_secs: Some(1),
        last_activity_ago_secs: 0,
        completed_gates: record.progress.completed_local_gates,
        total_gates: record.progress.total_local_gates,
        version_before: None,
        version_after: None,
        candidate_sha: None,
        retry_budget: "full 0/1".into(),
        failure_fingerprint: None,
        recent_output: Vec::new(),
        process_alive: true,
        progress: record.progress.clone(),
        resource_telemetry: None,
    });
    assert!(
        status.contains("Progress             Local verification"),
        "{status}"
    );
    assert!(status.contains("Milestone #3"), "{status}");
    assert!(status.contains("workspace-verify"), "{status}");
    assert!(status.contains("Tasks"), "{status}");
    assert!(
        status.contains("Running · workspace-verify · 0/1"),
        "{status}"
    );
}

#[test]
fn progress_history_is_bounded_and_monotonic() {
    let mut record = record();
    for number in 0..(MAX_PROGRESS_MILESTONES + 5) {
        record.note_progress_milestone(format!("milestone {number}"));
    }
    assert_eq!(record.progress.milestones.len(), MAX_PROGRESS_MILESTONES);
    assert_eq!(
        record
            .progress
            .milestones
            .first()
            .map(|milestone| milestone.sequence),
        Some(6)
    );
    assert_eq!(
        record
            .progress
            .milestones
            .last()
            .map(|milestone| milestone.sequence),
        Some((MAX_PROGRESS_MILESTONES + 5) as u64)
    );
}
