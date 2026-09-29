#![forbid(unsafe_code)]
//! Deterministic Release Recovery Controller (RRC).
//!
//! This module owns release lifecycle authority. Providers and frontends may
//! diagnose or render its records, but cannot admit retries or mutate release
//! progression without passing these typed guards.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fs2::FileExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: u32 = 1;
pub const NORMAL_POLL_INTERVAL: Duration = Duration::from_secs(20);
pub const UNCHANGED_POLL_INTERVAL: Duration = Duration::from_secs(120);
pub const STAGNATION_ACTION_LIMIT: u8 = 6;
pub const STAGNATION_TIME_LIMIT: Duration = Duration::from_secs(20 * 60);
pub const MAX_CAUSAL_EXCERPT_BYTES: usize = 4096;
pub const EXTERNAL_HEALTH_BUDGET: Duration = Duration::from_secs(15);
pub const MAX_EXTERNAL_HEALTH_REQUESTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseRecoveryState {
    Idle,
    Preparing,
    LocalVerification,
    DiagnosingLocalFailure,
    CandidateReady,
    RemoteGateRunning,
    WaitingForMatrix,
    CollectingFailureEvidence,
    ClassifyingFailure,
    NeedMoreEvidence,
    FocusedRepair,
    FocusedVerification,
    DiagnosingRepair,
    RetryAdmissible,
    ExternalHealthCheck,
    PausedExternal,
    Escalated,
    RemoteGatesGreen,
    Tagging,
    Publishing,
    Published,
    PostReleaseCloseout,
    PostReleaseMainDegraded,
    Complete,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseFailureClass {
    SourceRegression,
    TestRegression,
    FlakyOrTimingSensitiveTest,
    CompileFailure,
    PlatformSpecificFailure,
    PackagingFailure,
    ReleaseMetadataFailure,
    WorkflowConfigurationFailure,
    DependencyFailure,
    CredentialOrPermissionFailure,
    RateLimit,
    RunnerInfrastructureFailure,
    ArtifactInfrastructureFailure,
    ExternalServiceOutage,
    Timeout,
    Cancelled,
    Unknown,
}

impl ReleaseFailureClass {
    #[must_use]
    pub fn infrastructure_like(self) -> bool {
        matches!(
            self,
            Self::RateLimit
                | Self::RunnerInfrastructureFailure
                | Self::ArtifactInfrastructureFailure
                | Self::ExternalServiceOutage
                | Self::Timeout
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidence {
    Proven,
    StronglySupported,
    Tentative,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    InProgress,
    Success,
    Failure,
    TimedOut,
    Cancelled,
    Skipped,
}

impl JobState {
    #[must_use]
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Success | Self::Failure | Self::TimedOut | Self::Cancelled | Self::Skipped
        )
    }

    #[must_use]
    pub fn failed(self) -> bool {
        matches!(self, Self::Failure | Self::TimedOut | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub workflow_id: u64,
    pub run_id: u64,
    pub attempt: u32,
    pub job_id: u64,
    pub workflow_name: String,
    pub job_name: String,
    pub platform: Option<String>,
    pub state: JobState,
    pub failed_step: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRecord {
    pub name: String,
    pub head_sha: String,
    pub run_id: Option<u64>,
    pub run_attempt: Option<u32>,
    pub jobs: Vec<JobSnapshot>,
    pub url: Option<String>,
}

impl GateRecord {
    #[must_use]
    pub fn matrix_complete(&self) -> bool {
        !self.jobs.is_empty() && self.jobs.iter().all(|job| job.state.terminal())
    }

    #[must_use]
    pub fn has_failure(&self) -> bool {
        self.jobs.iter().any(|job| job.state.failed())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FailureFingerprint(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRecord {
    pub workflow_id: u64,
    pub run_id: u64,
    pub attempt: u32,
    pub job_id: u64,
    pub workflow_name: String,
    pub job_name: String,
    pub platform: Option<String>,
    pub step_name: Option<String>,
    pub fingerprint: FailureFingerprint,
    pub class: ReleaseFailureClass,
    pub confidence: EvidenceConfidence,
    pub causal_excerpt: String,
    pub source_commit: String,
    pub observed_at: DateTime<Utc>,
    pub other_platforms_passed: bool,
    pub exists_on_last_green: Option<bool>,
    pub related_source_touched: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelevantStateChangeKind {
    Source,
    WorkflowConfiguration,
    DependencyOrToolchain,
    CredentialOrPermission,
    ExternalServiceRecovered,
    MatrixCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelevantStateChange {
    pub kind: RelevantStateChangeKind,
    pub description: String,
    pub evidence_refs: Vec<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusedProofStatus {
    NotRun,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairAttempt {
    pub fingerprint: FailureFingerprint,
    pub causal_family: String,
    pub hypothesis: String,
    pub source_commit_before: String,
    pub source_commit_after: Option<String>,
    pub focused_proof: String,
    pub focused_status: FocusedProofStatus,
    pub evidence_refs: Vec<String>,
    pub disproven_or_insufficient: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryBudget {
    pub full_gate_limit: u8,
    pub full_gate_used: u8,
    pub infrastructure_limit: u8,
    pub infrastructure_used: u8,
    #[serde(default = "default_targeted_diagnostic_limit")]
    pub targeted_diagnostic_limit: u8,
    #[serde(default)]
    pub targeted_diagnostic_used: u8,
    pub repair_attempt_limit_per_family: u8,
}

const fn default_targeted_diagnostic_limit() -> u8 {
    2
}

impl Default for RetryBudget {
    fn default() -> Self {
        Self {
            full_gate_limit: 1,
            full_gate_used: 0,
            infrastructure_limit: 1,
            infrastructure_used: 0,
            targeted_diagnostic_limit: default_targeted_diagnostic_limit(),
            targeted_diagnostic_used: 0,
            repair_attempt_limit_per_family: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub sequence: u64,
    pub at: DateTime<Utc>,
    pub from: ReleaseRecoveryState,
    pub to: ReleaseRecoveryState,
    pub source_commit: String,
    pub workflow_id: Option<u64>,
    pub run_id: Option<u64>,
    pub job_id: Option<u64>,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub repair_attempts: usize,
    pub full_gate_retries: u8,
    pub infrastructure_retries: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalBlockRecord {
    pub service: String,
    pub official_status: String,
    pub direct_api_evidence: Vec<String>,
    pub cross_job_evidence: Vec<String>,
    pub community_corroboration: Vec<String>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SettlementState {
    #[default]
    NotStarted,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LocalGateRecord {
    pub name: String,
    pub state: SettlementState,
    pub command: String,
    pub evidence_ref: Option<String>,
}

/// Immutable provenance and settled side-effect state for the production
/// release path. A state is recorded only after the native adapter has
/// independently observed the corresponding repository/GitHub fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseMutationRecord {
    pub source_commit: Option<String>,
    pub version_before: Option<String>,
    pub version_after: Option<String>,
    pub version_files: Vec<String>,
    pub local_gates: Vec<LocalGateRecord>,
    pub candidate_committed: bool,
    pub candidate_pushed: bool,
    pub candidate_push_ref: Option<String>,
    pub tag_name: Option<String>,
    pub tag_object: Option<String>,
    pub tag_pushed: bool,
    pub publication_run_id: Option<u64>,
    pub publication_verified: bool,
    pub published_asset_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseObjective {
    pub bump: String,
    pub branch_ref: String,
    pub post_release_main_epoch: bool,
    #[serde(default = "default_pre_release_gates")]
    pub required_gate_names: Vec<String>,
}

pub(crate) fn default_pre_release_gates() -> Vec<String> {
    vec![
        "pull-request-validation".into(),
        "msrv".into(),
        "five-target-foundation".into(),
        "web-driver".into(),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRecoveryRecord {
    pub schema_version: u32,
    pub repo_identity: String,
    pub epoch_id: String,
    pub objective: ReleaseObjective,
    pub state: ReleaseRecoveryState,
    pub release_version: Option<String>,
    pub release_commit: Option<String>,
    pub current_main: Option<String>,
    pub required_gates: Vec<GateRecord>,
    pub failures: Vec<FailureRecord>,
    pub repair_attempts: Vec<RepairAttempt>,
    pub state_changes: Vec<RelevantStateChange>,
    pub retry_budget: RetryBudget,
    pub external_block: Option<ExternalBlockRecord>,
    #[serde(default)]
    pub mutation: ReleaseMutationRecord,
    pub transitions: Vec<TransitionRecord>,
    pub consecutive_stagnant_actions: u8,
    pub last_progress_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ReleaseRecoveryRecord {
    #[must_use]
    pub fn new(repo_identity: String, epoch_id: String, objective: ReleaseObjective) -> Self {
        let now = Utc::now();
        Self {
            schema_version: SCHEMA_VERSION,
            repo_identity,
            epoch_id,
            objective,
            state: ReleaseRecoveryState::Idle,
            release_version: None,
            release_commit: None,
            current_main: None,
            required_gates: Vec::new(),
            failures: Vec::new(),
            repair_attempts: Vec::new(),
            state_changes: Vec::new(),
            retry_budget: RetryBudget::default(),
            external_block: None,
            mutation: ReleaseMutationRecord::default(),
            transitions: Vec::new(),
            consecutive_stagnant_actions: 0,
            last_progress_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn transition(
        &mut self,
        to: ReleaseRecoveryState,
        source_commit: &str,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
        ids: Option<(u64, u64, u64)>,
    ) -> Result<(), RrcError> {
        if !legal_transition(self.state, to) {
            return Err(RrcError::IllegalTransition {
                from: self.state,
                to,
            });
        }
        if source_commit.trim().is_empty() {
            return Err(RrcError::Invalid(
                "transition source commit is empty".into(),
            ));
        }
        let now = Utc::now();
        let (workflow_id, run_id, job_id) = ids
            .map(|(workflow, run, job)| (Some(workflow), Some(run), Some(job)))
            .unwrap_or((None, None, None));
        self.transitions.push(TransitionRecord {
            sequence: self.transitions.last().map_or(1, |row| row.sequence + 1),
            at: now,
            from: self.state,
            to,
            source_commit: source_commit.to_owned(),
            workflow_id,
            run_id,
            job_id,
            reason: bounded(reason.into(), 1024),
            evidence_refs: bounded_refs(evidence_refs),
            repair_attempts: self.repair_attempts.len(),
            full_gate_retries: self.retry_budget.full_gate_used,
            infrastructure_retries: self.retry_budget.infrastructure_used,
        });
        self.state = to;
        self.updated_at = now;
        Ok(())
    }

    #[must_use]
    pub fn active_commit(&self) -> Option<&str> {
        if self.objective.post_release_main_epoch
            || self.state == ReleaseRecoveryState::PostReleaseMainDegraded
        {
            self.current_main.as_deref()
        } else {
            self.release_commit
                .as_deref()
                .or(self.current_main.as_deref())
        }
    }

    pub fn apply_matrix(&mut self, gate: GateRecord) -> Result<bool, RrcError> {
        let candidate = self
            .active_commit()
            .ok_or_else(|| RrcError::Invalid("candidate SHA is not recorded".into()))?;
        if gate.head_sha != candidate {
            return Err(RrcError::StaleWorkflow {
                expected: candidate.to_owned(),
                observed: gate.head_sha,
            });
        }
        let changed = if let Some(existing) = self
            .required_gates
            .iter_mut()
            .find(|row| row.name == gate.name)
        {
            if existing.run_attempt.unwrap_or(0) > gate.run_attempt.unwrap_or(0) {
                return Err(RrcError::Invalid(format!(
                    "stale workflow attempt for {}: current {:?}, observed {:?}",
                    gate.name, existing.run_attempt, gate.run_attempt
                )));
            }
            if *existing == gate {
                false
            } else {
                *existing = gate;
                true
            }
        } else {
            self.required_gates.push(gate);
            true
        };
        Ok(changed)
    }

    pub fn note_progress(&mut self, progressed: bool) {
        self.updated_at = Utc::now();
        if progressed {
            self.consecutive_stagnant_actions = 0;
            self.last_progress_at = self.updated_at;
        } else {
            self.consecutive_stagnant_actions = self.consecutive_stagnant_actions.saturating_add(1);
        }
    }

    #[must_use]
    pub fn watchdog_triggered(&self, now: DateTime<Utc>) -> bool {
        self.consecutive_stagnant_actions >= STAGNATION_ACTION_LIMIT
            || now
                .signed_duration_since(self.last_progress_at)
                .to_std()
                .is_ok_and(|elapsed| elapsed >= STAGNATION_TIME_LIMIT)
    }

    #[must_use]
    pub fn matrix_complete(&self) -> bool {
        !self.objective.required_gate_names.is_empty()
            && self.objective.required_gate_names.iter().all(|required| {
                self.required_gates
                    .iter()
                    .find(|gate| gate.name.eq_ignore_ascii_case(required))
                    .is_some_and(GateRecord::matrix_complete)
            })
    }

    #[must_use]
    pub fn retry_admission(&self, kind: RetryKind) -> RetryDecision {
        let Some(current) = self.failures.last() else {
            return RetryDecision::blocked("no captured causal failure evidence");
        };
        if !self.matrix_complete() {
            return RetryDecision::blocked("required CI matrix is incomplete");
        }
        let repeated = self.failures.iter().rev().skip(1).any(|prior| {
            prior.fingerprint == current.fingerprint
                && (prior.run_id != current.run_id || prior.attempt != current.attempt)
        });
        let changed_after_current = self
            .state_changes
            .iter()
            .any(|change| change.observed_at >= current.observed_at);
        if repeated && !changed_after_current {
            return RetryDecision::blocked(
                "identical failure fingerprint with no relevant state change",
            );
        }
        if current.confidence == EvidenceConfidence::Tentative
            || current.confidence == EvidenceConfidence::Unknown
        {
            return RetryDecision::blocked(
                "classification is not strong enough for mutation or retry",
            );
        }
        match kind {
            RetryKind::FullGate => {
                if self.retry_budget.full_gate_used >= self.retry_budget.full_gate_limit {
                    return RetryDecision::blocked("full-gate retry budget exhausted");
                }
                if self.state != ReleaseRecoveryState::RetryAdmissible {
                    return RetryDecision::blocked("controller state is not retry-admissible");
                }
                let Some(repair) = self
                    .repair_attempts
                    .iter()
                    .rev()
                    .find(|repair| repair.fingerprint == current.fingerprint)
                else {
                    return RetryDecision::blocked("no evidence-backed repair attempt recorded");
                };
                if repair.focused_status != FocusedProofStatus::Passed {
                    return RetryDecision::blocked("focused verification has not passed");
                }
                if repair.hypothesis.trim().is_empty()
                    || repair.focused_proof.trim().is_empty()
                    || repair.evidence_refs.is_empty()
                    || repair.source_commit_after.is_none()
                {
                    return RetryDecision::blocked(
                        "repair hypothesis, changed commit, focused proof, or evidence is missing",
                    );
                }
            }
            RetryKind::Infrastructure => {
                if self.retry_budget.infrastructure_used >= self.retry_budget.infrastructure_limit {
                    return RetryDecision::blocked("infrastructure retry budget exhausted");
                }
                if !current.class.infrastructure_like() {
                    return RetryDecision::blocked("failure is not infrastructure-class");
                }
                if !self.state_changes.iter().any(|change| {
                    change.kind == RelevantStateChangeKind::ExternalServiceRecovered
                        && change.observed_at >= current.observed_at
                }) {
                    return RetryDecision::blocked("external service recovery is not established");
                }
            }
            RetryKind::TargetedDiagnostic => {
                if self.retry_budget.targeted_diagnostic_used
                    >= self.retry_budget.targeted_diagnostic_limit
                {
                    return RetryDecision::blocked("targeted diagnostic retry budget exhausted");
                }
            }
        }
        RetryDecision {
            admitted: true,
            reason: "retry admitted by deterministic policy".into(),
        }
    }

    pub fn consume_retry(&mut self, kind: RetryKind) -> Result<(), RrcError> {
        let decision = self.retry_admission(kind);
        if !decision.admitted {
            return Err(RrcError::RetryBlocked(decision.reason));
        }
        match kind {
            RetryKind::FullGate => self.retry_budget.full_gate_used += 1,
            RetryKind::Infrastructure => self.retry_budget.infrastructure_used += 1,
            RetryKind::TargetedDiagnostic => {
                self.retry_budget.targeted_diagnostic_used += 1;
            }
        }
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Adds the immutable last-green/source comparison once. Re-observation
    /// must append a new failure record rather than rewriting established
    /// context.
    pub fn record_failure_context(
        &mut self,
        fingerprint: &FailureFingerprint,
        exists_on_last_green: bool,
        related_source_touched: bool,
    ) -> Result<(), RrcError> {
        let failure = self
            .failures
            .iter_mut()
            .rev()
            .find(|failure| &failure.fingerprint == fingerprint)
            .ok_or_else(|| RrcError::Invalid("failure fingerprint is not recorded".into()))?;
        if failure.exists_on_last_green.is_some() || failure.related_source_touched.is_some() {
            return Err(RrcError::Invalid(
                "failure comparison context is immutable once recorded".into(),
            ));
        }
        failure.exists_on_last_green = Some(exists_on_last_green);
        failure.related_source_touched = Some(related_source_touched);
        self.note_progress(true);
        Ok(())
    }

    pub fn record_repair(&mut self, repair: RepairAttempt) -> Result<(), RrcError> {
        if repair.hypothesis.trim().is_empty() || repair.evidence_refs.is_empty() {
            return Err(RrcError::Invalid(
                "repair requires a hypothesis and evidence reference".into(),
            ));
        }
        let family_count = self
            .repair_attempts
            .iter()
            .filter(|row| row.causal_family == repair.causal_family)
            .count();
        if family_count >= usize::from(self.retry_budget.repair_attempt_limit_per_family) {
            return Err(RrcError::RepairBudgetExhausted(repair.causal_family));
        }
        let change = repair
            .source_commit_after
            .as_ref()
            .map(|commit| RelevantStateChange {
                kind: RelevantStateChangeKind::Source,
                description: bounded(format!("focused repair promoted as commit {commit}"), 1024),
                evidence_refs: bounded_refs(repair.evidence_refs.clone()),
                observed_at: Utc::now(),
            });
        self.repair_attempts.push(repair);
        if let Some(change) = change {
            self.state_changes.push(change);
        }
        self.note_progress(true);
        Ok(())
    }

    #[must_use]
    pub fn render_status(&self) -> String {
        let terminal = self
            .required_gates
            .iter()
            .flat_map(|gate| &gate.jobs)
            .filter(|job| job.state.terminal())
            .count();
        let total = self
            .required_gates
            .iter()
            .map(|gate| gate.jobs.len())
            .sum::<usize>();
        let retry = self.retry_admission(RetryKind::FullGate);
        let active_gate = self
            .required_gates
            .iter()
            .find(|gate| !gate.matrix_complete() || gate.has_failure())
            .map(|gate| gate.name.as_str())
            .unwrap_or("none");
        let local_complete = self
            .mutation
            .local_gates
            .iter()
            .filter(|gate| gate.state == SettlementState::Succeeded)
            .count();
        let local_active = self
            .mutation
            .local_gates
            .iter()
            .find(|gate| gate.state != SettlementState::Succeeded)
            .map(|gate| format!("{} ({:?})", gate.name, gate.state))
            .unwrap_or_else(|| "none".into());
        let failed_jobs = self
            .required_gates
            .iter()
            .flat_map(|gate| &gate.jobs)
            .filter(|job| job.state.failed())
            .map(|job| job.job_name.as_str())
            .take(4)
            .collect::<Vec<_>>()
            .join(", ");
        let failure = self.failures.last();
        let fingerprint = failure
            .map(|row| short_sha(&row.fingerprint.0))
            .unwrap_or("none");
        let focused = self
            .repair_attempts
            .last()
            .map(|repair| format!("{:?}", repair.focused_status))
            .unwrap_or_else(|| "NotRun".into());
        let published = if self.mutation.publication_verified
            || matches!(
                self.state,
                ReleaseRecoveryState::Published
                    | ReleaseRecoveryState::PostReleaseCloseout
                    | ReleaseRecoveryState::PostReleaseMainDegraded
                    | ReleaseRecoveryState::Complete
            ) {
            self.release_version
                .as_deref()
                .map(|version| format!("{version} PUBLISHED / VERIFIED"))
                .unwrap_or_else(|| "PUBLISHED / VERIFIED".into())
        } else {
            "not published".into()
        };
        let main_health = if self.state == ReleaseRecoveryState::PostReleaseMainDegraded {
            "DEGRADED — release artifacts: NO EVIDENCE OF IMPACT"
        } else if self.current_main.is_some() {
            "green / monitoring"
        } else {
            "not assessed"
        };
        let external = self.external_block.as_ref().map_or_else(
            || "none".into(),
            |block| format!("{} — {}", block.service, block.official_status),
        );
        format!(
            "RELEASE RECOVERY\nState                 {:?}\nCandidate SHA         {}\nCurrent main SHA      {}\nVersion provenance    {} -> {}\nLocal gates           {local_complete}/{} · {local_active}\nRemote gate           {active_gate}\nJobs                  {terminal}/{total} terminal\nFailed jobs           {}\nFailure fingerprint   {fingerprint}\nFocused verification  {focused}\nRetry                 {} — {}\nRetry budget          full {}/{} · infrastructure {}/{} · diagnostic {}/{}\nMutation state        commit={} push={} tag={} publish={}\nPublished release     {published}\nCurrent main health   {main_health}\nExternal block        {external}\nNext                  {:?}",
            self.state,
            self.release_commit
                .as_deref()
                .map(short_sha)
                .unwrap_or("unknown"),
            self.current_main
                .as_deref()
                .map(short_sha)
                .unwrap_or("unknown"),
            self.mutation.version_before.as_deref().unwrap_or("pending"),
            self.mutation.version_after.as_deref().unwrap_or("pending"),
            self.mutation.local_gates.len(),
            if failed_jobs.is_empty() {
                "none"
            } else {
                &failed_jobs
            },
            if retry.admitted {
                "admissible"
            } else {
                "blocked"
            },
            retry.reason,
            self.retry_budget.full_gate_used,
            self.retry_budget.full_gate_limit,
            self.retry_budget.infrastructure_used,
            self.retry_budget.infrastructure_limit,
            self.retry_budget.targeted_diagnostic_used,
            self.retry_budget.targeted_diagnostic_limit,
            self.mutation.candidate_committed,
            self.mutation.candidate_pushed,
            self.mutation.tag_pushed,
            self.mutation.publication_verified,
            next_directive(self, 0),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    FullGate,
    Infrastructure,
    TargetedDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMutationKind {
    VersionBump,
    CommitCandidate,
    PushCandidate,
    CreateTag,
    Publish,
}

/// Unforgeable capability for one release mutation. Construction remains in
/// the controller so native adapters cannot turn API/process access into
/// lifecycle authority.
#[derive(Debug)]
pub struct ReleaseMutationAdmission {
    kind: ReleaseMutationKind,
}

impl ReleaseMutationAdmission {
    #[must_use]
    pub fn kind(&self) -> ReleaseMutationKind {
        self.kind
    }
}

pub fn admit_release_mutation(
    record: &ReleaseRecoveryRecord,
    kind: ReleaseMutationKind,
) -> Result<ReleaseMutationAdmission, RrcError> {
    let allowed = match kind {
        ReleaseMutationKind::VersionBump => {
            record.state == ReleaseRecoveryState::LocalVerification
                && record.mutation.version_after.is_none()
        }
        ReleaseMutationKind::CommitCandidate => {
            record.state == ReleaseRecoveryState::LocalVerification
                && record.mutation.version_after.is_some()
                && !record.mutation.local_gates.is_empty()
                && record
                    .mutation
                    .local_gates
                    .iter()
                    .all(|gate| gate.state == SettlementState::Succeeded)
                && !record.mutation.candidate_committed
        }
        ReleaseMutationKind::PushCandidate => {
            matches!(
                record.state,
                ReleaseRecoveryState::CandidateReady | ReleaseRecoveryState::RetryAdmissible
            ) && record.mutation.candidate_committed
                && !record.mutation.candidate_pushed
                && (record.state != ReleaseRecoveryState::RetryAdmissible
                    || record.retry_admission(RetryKind::FullGate).admitted)
        }
        ReleaseMutationKind::CreateTag => {
            record.state == ReleaseRecoveryState::RemoteGatesGreen
                && record.matrix_complete()
                && !record.required_gates.iter().any(GateRecord::has_failure)
                && record.mutation.candidate_pushed
                && !record.mutation.tag_pushed
        }
        ReleaseMutationKind::Publish => {
            record.state == ReleaseRecoveryState::Publishing
                && record.mutation.tag_pushed
                && !record.mutation.publication_verified
        }
    };
    if !allowed {
        return Err(RrcError::MutationBlocked(format!(
            "{kind:?} is not admissible from {:?}",
            record.state
        )));
    }
    Ok(ReleaseMutationAdmission { kind })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDecision {
    pub admitted: bool,
    pub reason: String,
}

impl RetryDecision {
    fn blocked(reason: impl Into<String>) -> Self {
        Self {
            admitted: false,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalHealthVerdict {
    Confirmed(ExternalBlockRecord),
    Unconfirmed,
    NotAdmissible,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExternalHealthEvidence {
    pub repository_checks_green: bool,
    pub source_explanation_absent: bool,
    pub infrastructure_failures: usize,
    pub official_degraded: bool,
    pub direct_api_failure: bool,
    pub community_reports: bool,
    pub official_summary: String,
    pub direct_evidence: Vec<String>,
    pub cross_job_evidence: Vec<String>,
    pub community_evidence: Vec<String>,
}

/// The one controller-authorized next operation. Adapters execute directives;
/// they do not infer progression from prose or provider output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseDirective {
    RunLocalVerification,
    DiagnoseLocalFailure,
    DispatchRequiredGates,
    RefreshMatrix { after: Duration },
    CollectMoreEvidence,
    RequestFocusedRepair { fingerprint: FailureFingerprint },
    RunExternalHealthCheck,
    DispatchAdmittedRetry { kind: RetryKind },
    VerifyAndCreateImmutableTag,
    PublishAndVerifyAssets,
    RunPostReleaseCloseout,
    PauseExternal,
    Escalate,
    None,
}

#[must_use]
pub fn next_directive(record: &ReleaseRecoveryRecord, unchanged_polls: u8) -> ReleaseDirective {
    use ReleaseRecoveryState as S;
    match record.state {
        S::LocalVerification => ReleaseDirective::RunLocalVerification,
        S::DiagnosingLocalFailure => ReleaseDirective::DiagnoseLocalFailure,
        S::CandidateReady => ReleaseDirective::DispatchRequiredGates,
        S::RemoteGateRunning | S::WaitingForMatrix => ReleaseDirective::RefreshMatrix {
            after: poll_interval(unchanged_polls),
        },
        S::CollectingFailureEvidence | S::NeedMoreEvidence => ReleaseDirective::CollectMoreEvidence,
        S::ClassifyingFailure => {
            record
                .failures
                .last()
                .map_or(ReleaseDirective::CollectMoreEvidence, |failure| {
                    if failure.class.infrastructure_like() {
                        ReleaseDirective::RunExternalHealthCheck
                    } else if matches!(
                        failure.confidence,
                        EvidenceConfidence::Proven | EvidenceConfidence::StronglySupported
                    ) {
                        ReleaseDirective::RequestFocusedRepair {
                            fingerprint: failure.fingerprint.clone(),
                        }
                    } else {
                        ReleaseDirective::CollectMoreEvidence
                    }
                })
        }
        S::FocusedRepair | S::FocusedVerification | S::DiagnosingRepair => {
            ReleaseDirective::CollectMoreEvidence
        }
        S::RetryAdmissible => ReleaseDirective::DispatchAdmittedRetry {
            kind: RetryKind::FullGate,
        },
        S::ExternalHealthCheck => ReleaseDirective::RunExternalHealthCheck,
        S::PausedExternal => ReleaseDirective::PauseExternal,
        S::RemoteGatesGreen => ReleaseDirective::VerifyAndCreateImmutableTag,
        S::Tagging | S::Publishing => ReleaseDirective::PublishAndVerifyAssets,
        S::Published | S::PostReleaseCloseout => ReleaseDirective::RunPostReleaseCloseout,
        S::PostReleaseMainDegraded | S::Escalated => ReleaseDirective::Escalate,
        S::Idle | S::Preparing | S::Complete | S::Cancelled => ReleaseDirective::None,
    }
}

/// Settled evidence accepted by the deterministic controller reducer. Hosts
/// and models may produce evidence, but only this reducer advances lifecycle
/// state and retry counters.
#[derive(Debug, Clone)]
pub enum ReleaseControllerEvent {
    LocalVerificationPassed {
        version: String,
        candidate_commit: String,
        evidence_refs: Vec<String>,
    },
    LocalVerificationFailed(Vec<String>),
    RemoteGateDispatched(Vec<String>),
    VerifiedRepair(RepairAttempt),
    FailedRepair(RepairAttempt),
    RetryDispatched {
        kind: RetryKind,
        evidence_refs: Vec<String>,
    },
    ExternalHealth(ExternalHealthEvidence),
    TagVerified(Vec<String>),
    PublicationStarted(Vec<String>),
    PublicationVerified {
        version: String,
        evidence_refs: Vec<String>,
    },
    PostReleaseCloseoutStarted(Vec<String>),
    PostReleaseMainGreen {
        main_commit: String,
        evidence_refs: Vec<String>,
    },
    PostReleaseMainRed {
        main_commit: String,
        evidence_refs: Vec<String>,
    },
}

pub fn apply_controller_event(
    record: &mut ReleaseRecoveryRecord,
    event: ReleaseControllerEvent,
) -> Result<(), RrcError> {
    let commit = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("controller commit is not recorded".into()))?;
    match event {
        ReleaseControllerEvent::LocalVerificationPassed {
            version,
            candidate_commit,
            evidence_refs,
        } => {
            record.release_version = Some(bounded(version, 128));
            record.release_commit = Some(candidate_commit.clone());
            record.transition(
                ReleaseRecoveryState::CandidateReady,
                &candidate_commit,
                "local release verification passed",
                evidence_refs,
                None,
            )
        }
        ReleaseControllerEvent::LocalVerificationFailed(evidence_refs) => record.transition(
            ReleaseRecoveryState::DiagnosingLocalFailure,
            &commit,
            "local release verification failed",
            evidence_refs,
            None,
        ),
        ReleaseControllerEvent::RemoteGateDispatched(evidence_refs) => record.transition(
            ReleaseRecoveryState::RemoteGateRunning,
            &commit,
            "exact-commit remote gates dispatched",
            evidence_refs,
            None,
        ),
        ReleaseControllerEvent::VerifiedRepair(repair) => {
            validate_repair_target(record, &repair)?;
            if repair.focused_status != FocusedProofStatus::Passed {
                return Err(RrcError::Invalid(
                    "verified repair must contain a passed focused proof".into(),
                ));
            }
            let repaired_commit = repair.source_commit_after.clone().ok_or_else(|| {
                RrcError::Invalid("verified repair must identify its changed commit".into())
            })?;
            record.transition(
                ReleaseRecoveryState::FocusedRepair,
                &commit,
                "evidence-backed repair prepared",
                repair.evidence_refs.clone(),
                None,
            )?;
            record.transition(
                ReleaseRecoveryState::FocusedVerification,
                &commit,
                "smallest credible focused proof executed",
                repair.evidence_refs.clone(),
                None,
            )?;
            record.record_repair(repair)?;
            record.release_commit = Some(repaired_commit.clone());
            // GitHub reruns preserve the original GITHUB_SHA. A source repair
            // therefore invalidates the prior push receipt and must be pushed
            // as a fresh exact-SHA candidate before another full gate set.
            record.mutation.candidate_pushed = false;
            record.mutation.candidate_push_ref = None;
            record.transition(
                ReleaseRecoveryState::RetryAdmissible,
                &repaired_commit,
                "focused proof passed after a relevant source change",
                vec!["rrc:focused-proof".into()],
                None,
            )
        }
        ReleaseControllerEvent::FailedRepair(mut repair) => {
            validate_repair_target(record, &repair)?;
            repair.focused_status = FocusedProofStatus::Failed;
            repair.disproven_or_insufficient = true;
            record.transition(
                ReleaseRecoveryState::FocusedRepair,
                &commit,
                "evidence-backed repair prepared",
                repair.evidence_refs.clone(),
                None,
            )?;
            record.transition(
                ReleaseRecoveryState::FocusedVerification,
                &commit,
                "focused proof executed",
                repair.evidence_refs.clone(),
                None,
            )?;
            let family = repair.causal_family.clone();
            record.record_repair(repair)?;
            record.transition(
                ReleaseRecoveryState::DiagnosingRepair,
                &commit,
                "focused proof failed; repair hypothesis disproven or insufficient",
                vec!["rrc:focused-proof-failed".into()],
                None,
            )?;
            let attempts = record
                .repair_attempts
                .iter()
                .filter(|attempt| attempt.causal_family == family)
                .count();
            if attempts >= usize::from(record.retry_budget.repair_attempt_limit_per_family) {
                record.transition(
                    ReleaseRecoveryState::Escalated,
                    &commit,
                    "repair-attempt budget exhausted for causal family",
                    vec!["rrc:repair-budget".into()],
                    None,
                )?;
            }
            Ok(())
        }
        ReleaseControllerEvent::RetryDispatched {
            kind,
            evidence_refs,
        } => {
            record.consume_retry(kind)?;
            record.required_gates.clear();
            record.transition(
                ReleaseRecoveryState::RemoteGateRunning,
                &commit,
                "admitted retry dispatched",
                evidence_refs,
                None,
            )
        }
        ReleaseControllerEvent::ExternalHealth(evidence) => {
            let failure = record.failures.last().ok_or_else(|| {
                RrcError::Invalid("external check has no captured failure".into())
            })?;
            if !failure.class.infrastructure_like() && failure.class != ReleaseFailureClass::Unknown
            {
                return Err(RrcError::Invalid(
                    "repository failure classification does not admit external-health diagnosis"
                        .into(),
                ));
            }
            record.transition(
                ReleaseRecoveryState::ExternalHealthCheck,
                &commit,
                "late external-health diagnostic admitted",
                vec!["rrc:external-health-check".into()],
                None,
            )?;
            match classify_external_health(evidence) {
                ExternalHealthVerdict::Confirmed(block) => {
                    record.external_block = Some(block);
                    record.transition(
                        ReleaseRecoveryState::PausedExternal,
                        &commit,
                        "confirmed GitHub service degradation",
                        vec!["github:official-status".into()],
                        None,
                    )
                }
                ExternalHealthVerdict::Unconfirmed | ExternalHealthVerdict::NotAdmissible => record
                    .transition(
                        ReleaseRecoveryState::NeedMoreEvidence,
                        &commit,
                        "external status unconfirmed",
                        vec!["rrc:external-unconfirmed".into()],
                        None,
                    ),
            }
        }
        ReleaseControllerEvent::TagVerified(evidence_refs) => {
            if record.state != ReleaseRecoveryState::RemoteGatesGreen
                || !record.matrix_complete()
                || record.required_gates.iter().any(GateRecord::has_failure)
                || !record.mutation.candidate_pushed
                || !record.mutation.tag_pushed
            {
                return Err(RrcError::MutationBlocked(
                    "tagging requires pushed candidate plus complete green exact-commit gates"
                        .into(),
                ));
            }
            record.transition(
                ReleaseRecoveryState::Tagging,
                &commit,
                "annotated immutable tag verified at exact green commit",
                evidence_refs,
                None,
            )
        }
        ReleaseControllerEvent::PublicationStarted(evidence_refs) => {
            if record.state != ReleaseRecoveryState::Tagging || !record.mutation.tag_pushed {
                return Err(RrcError::MutationBlocked(
                    "publication requires the immutable tag to be pushed".into(),
                ));
            }
            record.transition(
                ReleaseRecoveryState::Publishing,
                &commit,
                "release publication started after exact-commit gates",
                evidence_refs,
                None,
            )
        }
        ReleaseControllerEvent::PublicationVerified {
            version,
            evidence_refs,
        } => {
            if record.state != ReleaseRecoveryState::Publishing
                || !record.mutation.publication_verified
                || record.mutation.publication_run_id.is_none()
                || record.mutation.published_asset_names.is_empty()
            {
                return Err(RrcError::MutationBlocked(
                    "publication is not independently verified with a run and assets".into(),
                ));
            }
            record.release_version = Some(bounded(version, 128));
            record.transition(
                ReleaseRecoveryState::Published,
                &commit,
                "release assets and metadata published and verified",
                evidence_refs,
                None,
            )
        }
        ReleaseControllerEvent::PostReleaseCloseoutStarted(evidence_refs) => record.transition(
            ReleaseRecoveryState::PostReleaseCloseout,
            &commit,
            "post-release main-health closeout started",
            evidence_refs,
            None,
        ),
        ReleaseControllerEvent::PostReleaseMainGreen {
            main_commit,
            evidence_refs,
        } => {
            record.current_main = Some(main_commit.clone());
            record.transition(
                ReleaseRecoveryState::Complete,
                &main_commit,
                "post-release main is green",
                evidence_refs,
                None,
            )
        }
        ReleaseControllerEvent::PostReleaseMainRed {
            main_commit,
            evidence_refs,
        } => {
            record.current_main = Some(main_commit.clone());
            record.objective.post_release_main_epoch = true;
            record.required_gates.clear();
            record.transition(
                ReleaseRecoveryState::PostReleaseMainDegraded,
                &main_commit,
                "published release remains verified; later main commit is degraded",
                evidence_refs,
                None,
            )
        }
    }
}

fn validate_repair_target(
    record: &ReleaseRecoveryRecord,
    repair: &RepairAttempt,
) -> Result<(), RrcError> {
    if !matches!(
        record.state,
        ReleaseRecoveryState::ClassifyingFailure | ReleaseRecoveryState::DiagnosingRepair
    ) {
        return Err(RrcError::Invalid(
            "repair is allowed only after complete-matrix classification or repair diagnosis"
                .into(),
        ));
    }
    let failure = record
        .failures
        .last()
        .ok_or_else(|| RrcError::Invalid("repair has no captured failure".into()))?;
    if repair.fingerprint != failure.fingerprint {
        return Err(RrcError::Invalid(
            "repair fingerprint does not target the current failure".into(),
        ));
    }
    if !matches!(
        failure.confidence,
        EvidenceConfidence::Proven | EvidenceConfidence::StronglySupported
    ) {
        return Err(RrcError::Invalid(
            "tentative or unknown classification cannot authorize repair".into(),
        ));
    }
    if !record.matrix_complete() {
        return Err(RrcError::Invalid(
            "repair promotion is blocked until the required matrix is complete".into(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn classify_external_health(evidence: ExternalHealthEvidence) -> ExternalHealthVerdict {
    let admitted = (evidence.repository_checks_green
        && evidence.source_explanation_absent
        && evidence.infrastructure_failures >= 2)
        || evidence.direct_api_failure;
    if !admitted {
        return ExternalHealthVerdict::NotAdmissible;
    }
    if evidence.official_degraded
        && (evidence.direct_api_failure || evidence.infrastructure_failures >= 2)
        && evidence.repository_checks_green
    {
        return ExternalHealthVerdict::Confirmed(ExternalBlockRecord {
            service: "GitHub Actions".into(),
            official_status: bounded(evidence.official_summary, 512),
            direct_api_evidence: bounded_refs(evidence.direct_evidence),
            cross_job_evidence: bounded_refs(evidence.cross_job_evidence),
            community_corroboration: bounded_refs(evidence.community_evidence),
            checked_at: Utc::now(),
        });
    }
    ExternalHealthVerdict::Unconfirmed
}

#[must_use]
pub fn poll_interval(unchanged_polls: u8) -> Duration {
    if unchanged_polls >= 3 {
        UNCHANGED_POLL_INTERVAL
    } else {
        NORMAL_POLL_INTERVAL
    }
}

#[must_use]
pub fn legal_transition(from: ReleaseRecoveryState, to: ReleaseRecoveryState) -> bool {
    use ReleaseRecoveryState as S;
    if to == S::Cancelled && !matches!(from, S::Complete | S::Cancelled) {
        return true;
    }
    if to == S::Escalated
        && !matches!(
            from,
            S::Idle | S::Published | S::PostReleaseCloseout | S::Complete | S::Cancelled
        )
    {
        return true;
    }
    matches!(
        (from, to),
        (S::Idle, S::Preparing)
            | (S::Preparing, S::LocalVerification)
            | (
                S::LocalVerification,
                S::DiagnosingLocalFailure | S::CandidateReady
            )
            | (
                S::DiagnosingLocalFailure,
                S::LocalVerification | S::Escalated
            )
            | (S::CandidateReady, S::RemoteGateRunning)
            | (
                S::RemoteGateRunning,
                S::WaitingForMatrix | S::CollectingFailureEvidence | S::RemoteGatesGreen
            )
            | (
                S::WaitingForMatrix,
                S::WaitingForMatrix
                    | S::CollectingFailureEvidence
                    | S::RemoteGatesGreen
                    | S::Complete
            )
            | (S::CollectingFailureEvidence, S::ClassifyingFailure)
            | (
                S::ClassifyingFailure,
                S::FocusedRepair | S::ExternalHealthCheck | S::NeedMoreEvidence
            )
            | (
                S::NeedMoreEvidence,
                S::CollectingFailureEvidence | S::Escalated
            )
            | (S::FocusedRepair, S::FocusedVerification)
            | (
                S::FocusedVerification,
                S::RetryAdmissible | S::DiagnosingRepair
            )
            | (S::DiagnosingRepair, S::FocusedRepair | S::Escalated)
            | (S::RetryAdmissible, S::RemoteGateRunning)
            | (
                S::ExternalHealthCheck,
                S::PausedExternal | S::NeedMoreEvidence
            )
            | (
                S::PausedExternal,
                S::RemoteGateRunning | S::ExternalHealthCheck
            )
            | (S::RemoteGatesGreen, S::Tagging)
            | (S::Tagging, S::Publishing)
            | (S::Publishing, S::Published)
            | (S::Published, S::PostReleaseCloseout | S::Complete)
            | (
                S::PostReleaseCloseout,
                S::Complete | S::PostReleaseMainDegraded
            )
            | (
                S::PostReleaseMainDegraded,
                S::CollectingFailureEvidence | S::WaitingForMatrix | S::Complete
            )
    )
}

#[derive(Debug, thiserror::Error)]
pub enum RrcError {
    #[error("illegal release transition {from:?} -> {to:?}")]
    IllegalTransition {
        from: ReleaseRecoveryState,
        to: ReleaseRecoveryState,
    },
    #[error("invalid release recovery input: {0}")]
    Invalid(String),
    #[error("stale workflow result: expected {expected}, observed {observed}")]
    StaleWorkflow { expected: String, observed: String },
    #[error("retry blocked: {0}")]
    RetryBlocked(String),
    #[error("release mutation blocked: {0}")]
    MutationBlocked(String),
    #[error("repair budget exhausted for causal family {0}")]
    RepairBudgetExhausted(String),
    #[error("release ledger I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("release ledger is malformed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("release ledger lock is busy")]
    Busy,
}

#[must_use]
pub fn failure_fingerprint(
    workflow: &str,
    job: &str,
    step: Option<&str>,
    platform: Option<&str>,
    tool_identity: Option<&str>,
    causal_line: &str,
) -> FailureFingerprint {
    let normalized = normalize_failure_text(causal_line);
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        workflow.trim().to_ascii_lowercase(),
        job.trim().to_ascii_lowercase(),
        step.unwrap_or_default().trim().to_ascii_lowercase(),
        platform.unwrap_or_default().trim().to_ascii_lowercase(),
        tool_identity
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
        normalized,
    );
    FailureFingerprint(sha256_hex(canonical.as_bytes()))
}

#[must_use]
pub fn normalize_failure_text(input: &str) -> String {
    let mut text = redact_secrets(input);
    let patterns = [
        (
            r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b",
            "<uuid>",
        ),
        (
            r"(?i)\b(?:runner|request|trace)[-_ ]?id[=: ]+[a-z0-9._-]+",
            "id=<volatile>",
        ),
        (r"(?m)^\d{4}-\d{2}-\d{2}[tT ][0-9:.+-]+[zZ]?\s*", ""),
        (
            r"(?:/tmp|/var/folders|[A-Za-z]:\\(?:Users\\)?[^\\\s]+\\AppData\\Local\\Temp)[^\s:]*",
            "<temp-path>",
        ),
        (
            r"\b(?:127\.0\.0\.1|localhost):\d{2,5}\b",
            "localhost:<port>",
        ),
        (r"(?i)\battempt\s+#?\d+\b", "attempt <n>"),
    ];
    for (pattern, replacement) in patterns {
        if let Ok(regex) = Regex::new(pattern) {
            text = regex.replace_all(&text, replacement).into_owned();
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[must_use]
pub fn redact_secrets(input: &str) -> String {
    let mut text = input.to_owned();
    let patterns = [
        r"(?i)(authorization\s*:\s*(?:bearer|token)\s+)[^\s]+",
        r"(?i)((?:github_token|gh_token|api[_-]?key|access[_-]?token|client[_-]?secret)\s*[=:]\s*)[^\s]+",
        r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b",
        r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
    ];
    for pattern in patterns {
        if let Ok(regex) = Regex::new(pattern) {
            text = regex.replace_all(&text, "$1[REDACTED]").into_owned();
        }
    }
    bounded(text, MAX_CAUSAL_EXCERPT_BYTES)
}

#[must_use]
pub fn classify_failure(
    excerpt: &str,
    platform: Option<&str>,
) -> (ReleaseFailureClass, EvidenceConfidence) {
    let normalized = normalize_failure_text(excerpt);
    let class = if normalized.contains("panicked at") || normalized.contains("assertion") {
        ReleaseFailureClass::TestRegression
    } else if normalized.contains("error[e") || normalized.contains("could not compile") {
        ReleaseFailureClass::CompileFailure
    } else if normalized.contains("permission denied")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
    {
        ReleaseFailureClass::CredentialOrPermissionFailure
    } else if normalized.contains("rate limit") || normalized.contains("http 429") {
        ReleaseFailureClass::RateLimit
    } else if normalized.contains("runner")
        && (normalized.contains("lost") || normalized.contains("provision"))
    {
        ReleaseFailureClass::RunnerInfrastructureFailure
    } else if normalized.contains("artifact")
        && (normalized.contains("service") || normalized.contains("upload failed"))
    {
        ReleaseFailureClass::ArtifactInfrastructureFailure
    } else if normalized.contains("timed out") || normalized.contains("timeout") {
        ReleaseFailureClass::Timeout
    } else if normalized.contains("package") || normalized.contains("archive") {
        ReleaseFailureClass::PackagingFailure
    } else if platform.is_some() {
        ReleaseFailureClass::PlatformSpecificFailure
    } else {
        ReleaseFailureClass::Unknown
    };
    let confidence = if class == ReleaseFailureClass::Unknown {
        EvidenceConfidence::Unknown
    } else if matches!(
        class,
        ReleaseFailureClass::TestRegression
            | ReleaseFailureClass::CompileFailure
            | ReleaseFailureClass::CredentialOrPermissionFailure
            | ReleaseFailureClass::RateLimit
            | ReleaseFailureClass::RunnerInfrastructureFailure
    ) {
        EvidenceConfidence::StronglySupported
    } else {
        EvidenceConfidence::Tentative
    };
    (class, confidence)
}

#[must_use]
pub fn first_causal_excerpt(log: &str) -> String {
    let causal = Regex::new(
        r"(?i)(panicked at|assertion [`']?(?:left|right)|error\[e\d+\]|error:|fatal:|permission denied|unauthorized|rate limit|runner .* (?:lost|failed)|timed out|process exited while answering)",
    )
    .expect("static causal regex");
    let lines = log.lines().collect::<Vec<_>>();
    let index = lines
        .iter()
        .position(|line| causal.is_match(line))
        .unwrap_or(0);
    let start = index.saturating_sub(1);
    let end = (index + 4).min(lines.len());
    redact_secrets(&lines[start..end].join("\n"))
}

#[derive(Debug, Clone)]
pub struct ReleaseLedger {
    root: PathBuf,
    repo_key: String,
}

impl ReleaseLedger {
    pub fn open(root: impl Into<PathBuf>, repo_identity: &str) -> Result<Self, RrcError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(RrcError::Invalid(
                "release ledger root must be absolute".into(),
            ));
        }
        fs::create_dir_all(&root)?;
        let repo_key = sha256_hex(repo_identity.as_bytes());
        Ok(Self { root, repo_key })
    }

    fn path(&self) -> PathBuf {
        self.root.join(format!("{}.json", self.repo_key))
    }
    fn lock_path(&self) -> PathBuf {
        self.root.join(format!("{}.lock", self.repo_key))
    }

    pub fn load(&self) -> Result<Option<ReleaseRecoveryRecord>, RrcError> {
        let path = self.path();
        match fs::read(&path) {
            Ok(bytes) => {
                if bytes.len() > 4 * 1024 * 1024 {
                    return Err(RrcError::Invalid(
                        "release ledger exceeds the 4 MiB safety bound".into(),
                    ));
                }
                let record: ReleaseRecoveryRecord = serde_json::from_slice(&bytes)?;
                if record.schema_version != SCHEMA_VERSION {
                    return Err(RrcError::Invalid(format!(
                        "unsupported schema version {}",
                        record.schema_version
                    )));
                }
                Ok(Some(record))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, record: &ReleaseRecoveryRecord) -> Result<(), RrcError> {
        if sha256_hex(record.repo_identity.as_bytes()) != self.repo_key {
            return Err(RrcError::Invalid(
                "record repository does not match ledger".into(),
            ));
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())?;
        lock.try_lock_exclusive().map_err(|_| RrcError::Busy)?;
        let bytes = serde_json::to_vec_pretty(record)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(self.path()).map_err(|error| error.error)?;
        if let Ok(directory) = OpenOptions::new().read(true).open(&self.root) {
            let _ = directory.sync_all();
        }
        FileExt::unlock(&lock)?;
        Ok(())
    }
}

/// Refreshes exact-SHA remote evidence and applies the complete-matrix rule.
/// Failed-job logs are captured only after every required gate is terminal.
pub fn refresh_remote_evidence(
    record: &mut ReleaseRecoveryRecord,
    repository: &str,
    adapter: &dyn GitHubEvidencePort,
) -> Result<(), RrcError> {
    let candidate = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("candidate SHA is not recorded".into()))?;
    let mut gates = adapter.matrix_for_sha(repository, &candidate)?;
    gates.retain(|gate| {
        record
            .objective
            .required_gate_names
            .iter()
            .any(|required| gate.name.eq_ignore_ascii_case(required))
    });
    if gates.is_empty() {
        record.note_progress(false);
        return Err(RrcError::Invalid(
            "no required workflow runs exist for the exact candidate SHA".into(),
        ));
    }
    let mut changed = false;
    for gate in gates {
        changed |= record.apply_matrix(gate)?;
    }
    record.note_progress(changed);
    if record.watchdog_triggered(Utc::now()) {
        let from = record.state;
        if legal_transition(from, ReleaseRecoveryState::Escalated) {
            record.transition(
                ReleaseRecoveryState::Escalated,
                &candidate,
                "stagnation watchdog exhausted without new CI evidence",
                vec!["rrc:stagnation-watchdog".into()],
                None,
            )?;
            return Ok(());
        }
    }
    if !record.matrix_complete() {
        if record.state != ReleaseRecoveryState::WaitingForMatrix {
            record.transition(
                ReleaseRecoveryState::WaitingForMatrix,
                &candidate,
                "required workflow matrix is incomplete",
                vec!["github:exact-sha-matrix".into()],
                None,
            )?;
        }
        return Ok(());
    }
    if !record.required_gates.iter().any(GateRecord::has_failure) {
        let (state, reason) = if record.objective.post_release_main_epoch {
            (
                ReleaseRecoveryState::Complete,
                "post-release main is green; published release remains unchanged",
            )
        } else {
            (
                ReleaseRecoveryState::RemoteGatesGreen,
                "all exact-commit required gates are terminal and green",
            )
        };
        record.transition(
            state,
            &candidate,
            reason,
            vec!["github:exact-sha-matrix".into()],
            None,
        )?;
        return Ok(());
    }
    record.transition(
        ReleaseRecoveryState::CollectingFailureEvidence,
        &candidate,
        "complete matrix contains failed jobs",
        vec!["github:exact-sha-matrix".into()],
        None,
    )?;
    let jobs = record
        .required_gates
        .iter()
        .flat_map(|gate| gate.jobs.iter())
        .filter(|job| job.state.failed())
        .cloned()
        .collect::<Vec<_>>();
    for job in jobs {
        let log = adapter.job_log(repository, job.job_id)?;
        let excerpt = first_causal_excerpt(&log);
        let (class, confidence) = classify_failure(&excerpt, job.platform.as_deref());
        let fingerprint = failure_fingerprint(
            &job.workflow_name,
            &job.job_name,
            job.failed_step.as_deref(),
            job.platform.as_deref(),
            None,
            &excerpt,
        );
        let already_captured = record.failures.iter().any(|failure| {
            failure.run_id == job.run_id
                && failure.attempt == job.attempt
                && failure.job_id == job.job_id
                && failure.fingerprint == fingerprint
        });
        if !already_captured {
            record.failures.push(FailureRecord {
                workflow_id: job.workflow_id,
                run_id: job.run_id,
                attempt: job.attempt,
                job_id: job.job_id,
                workflow_name: job.workflow_name,
                job_name: job.job_name,
                platform: job.platform,
                step_name: job.failed_step,
                fingerprint,
                class,
                confidence,
                causal_excerpt: excerpt,
                source_commit: candidate.clone(),
                observed_at: Utc::now(),
                other_platforms_passed: record
                    .required_gates
                    .iter()
                    .flat_map(|gate| &gate.jobs)
                    .any(|row| row.state == JobState::Success),
                exists_on_last_green: None,
                related_source_touched: None,
            });
        }
    }
    record.transition(
        ReleaseRecoveryState::ClassifyingFailure,
        &candidate,
        "first causal evidence captured and fingerprinted",
        record
            .failures
            .iter()
            .rev()
            .take(32)
            .map(|failure| format!("github:job:{}", failure.job_id))
            .collect(),
        None,
    )?;
    Ok(())
}

/// Compares one captured failure with an exact last-known-green commit and
/// records whether the same normalized cause existed there. The caller
/// supplies source-diff evidence because Git history access belongs to the
/// composition boundary.
pub fn compare_with_last_green(
    record: &mut ReleaseRecoveryRecord,
    repository: &str,
    last_green_commit: &str,
    related_source_touched: bool,
    adapter: &dyn GitHubEvidencePort,
) -> Result<(), RrcError> {
    let current = record
        .failures
        .last()
        .cloned()
        .ok_or_else(|| RrcError::Invalid("no captured failure to compare".into()))?;
    let gates = adapter.matrix_for_sha(repository, last_green_commit)?;
    let mut existed = false;
    for job in gates.iter().flat_map(|gate| &gate.jobs).filter(|job| {
        job.state.failed()
            && job.workflow_name == current.workflow_name
            && job.job_name == current.job_name
            && job.platform == current.platform
    }) {
        let excerpt = first_causal_excerpt(&adapter.job_log(repository, job.job_id)?);
        let fingerprint = failure_fingerprint(
            &job.workflow_name,
            &job.job_name,
            job.failed_step.as_deref(),
            job.platform.as_deref(),
            None,
            &excerpt,
        );
        if fingerprint == current.fingerprint {
            existed = true;
            break;
        }
    }
    record.record_failure_context(&current.fingerprint, existed, related_source_touched)
}

pub trait GitHubEvidencePort: Send + Sync {
    fn matrix_for_sha(&self, repository: &str, head_sha: &str)
    -> Result<Vec<GateRecord>, RrcError>;
    fn job_log(&self, repository: &str, job_id: u64) -> Result<String, RrcError>;
    fn rerun_job(&self, repository: &str, job_id: u64) -> Result<(), RrcError>;
    fn rerun_failed(&self, repository: &str, run_id: u64) -> Result<(), RrcError>;
    fn rerun_workflow(&self, repository: &str, run_id: u64) -> Result<(), RrcError>;
}

/// A mutation capability issued only after [`ReleaseRecoveryRecord::consume_retry`].
/// Keeping construction private prevents adapters from treating API access as policy authority.
#[derive(Debug)]
pub struct RetryAdmissionToken {
    kind: RetryKind,
}

impl RetryAdmissionToken {
    #[must_use]
    pub fn kind(&self) -> RetryKind {
        self.kind
    }
}

pub fn admit_retry(
    record: &mut ReleaseRecoveryRecord,
    kind: RetryKind,
) -> Result<RetryAdmissionToken, RrcError> {
    record.consume_retry(kind)?;
    Ok(RetryAdmissionToken { kind })
}

pub fn dispatch_admitted_full_retry(
    record: &mut ReleaseRecoveryRecord,
    repository: &str,
    adapter: &GhCliEvidenceAdapter,
) -> Result<(), RrcError> {
    let run_ids = record
        .required_gates
        .iter()
        .filter_map(|gate| gate.run_id)
        .collect::<Vec<_>>();
    let commit = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("controller commit is not recorded".into()))?;
    let token = admit_retry(record, RetryKind::FullGate)?;
    adapter.rerun_gate_set_with_admission(repository, &run_ids, token)?;
    record.required_gates.clear();
    record.transition(
        ReleaseRecoveryState::RemoteGateRunning,
        &commit,
        "controller-admitted complete gate retry dispatched",
        run_ids
            .into_iter()
            .map(|run_id| format!("github:run:{run_id}"))
            .collect(),
        None,
    )
}

/// GitHub CLI implementation of the structured evidence port. It uses JSON
/// fields rather than terminal text and never accepts a token argument.
#[derive(Debug, Clone, Default)]
pub struct GhCliEvidenceAdapter;

impl GhCliEvidenceAdapter {
    fn gh(repository: &str, args: &[&str]) -> Result<String, RrcError> {
        if !matches!(
            github_repository_slug(&format!("https://github.com/{repository}")),
            Ok(parsed) if parsed == repository
        ) {
            return Err(RrcError::Invalid(
                "unsafe GitHub repository identity".into(),
            ));
        }
        let output = std::process::Command::new("gh")
            .arg("api")
            .args(args)
            .arg("-H")
            .arg("Accept: application/vnd.github+json")
            .env_remove("GH_DEBUG")
            .output()?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "GitHub API command failed with status {}: {}",
                output.status,
                bounded(
                    redact_secrets(&String::from_utf8_lossy(&output.stderr)),
                    512
                )
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| RrcError::Invalid("GitHub response was not UTF-8".into()))
    }

    pub fn rerun_with_admission(
        &self,
        repository: &str,
        run_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        match token.kind {
            RetryKind::FullGate | RetryKind::Infrastructure => {
                self.rerun_workflow(repository, run_id)
            }
            RetryKind::TargetedDiagnostic => Err(RrcError::Invalid(
                "targeted admission requires rerun_job".into(),
            )),
        }
    }

    pub fn rerun_gate_set_with_admission(
        &self,
        repository: &str,
        run_ids: &[u64],
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        if !matches!(token.kind, RetryKind::FullGate | RetryKind::Infrastructure) {
            return Err(RrcError::Invalid(
                "gate-set admission requires a full or infrastructure retry".into(),
            ));
        }
        if run_ids.is_empty() {
            return Err(RrcError::Invalid("retry gate set is empty".into()));
        }
        for run_id in run_ids {
            self.rerun_workflow(repository, *run_id)?;
        }
        Ok(())
    }
}

impl GitHubEvidencePort for GhCliEvidenceAdapter {
    fn matrix_for_sha(
        &self,
        repository: &str,
        head_sha: &str,
    ) -> Result<Vec<GateRecord>, RrcError> {
        if head_sha.len() != 40 || !head_sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(RrcError::Invalid(
                "GitHub evidence requires an exact 40-character commit SHA".into(),
            ));
        }
        if github_repository_slug(&format!("https://github.com/{repository}")).is_err() {
            return Err(RrcError::Invalid(
                "unsafe GitHub repository identity".into(),
            ));
        }
        let endpoint = format!("repos/{repository}/actions/runs?head_sha={head_sha}&per_page=100");
        let body = Self::gh(repository, &[&endpoint])?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let mut gates = Vec::new();
        for run in value
            .get("workflow_runs")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if run.get("head_sha").and_then(serde_json::Value::as_str) != Some(head_sha) {
                continue;
            }
            let workflow_name = bounded(
                run.get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                256,
            );
            // The API returns newest runs first. Keep one authoritative latest
            // attempt per required workflow instead of allowing an older run
            // later in the page to overwrite it.
            if gates
                .iter()
                .any(|gate: &GateRecord| gate.name == workflow_name)
            {
                continue;
            }
            let run_id = run
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| RrcError::Invalid("run id missing".into()))?;
            let jobs_endpoint =
                format!("repos/{repository}/actions/runs/{run_id}/jobs?filter=all&per_page=100");
            let jobs_body = Self::gh(repository, &[&jobs_endpoint])?;
            let jobs_value: serde_json::Value = serde_json::from_str(&jobs_body)?;
            let mut jobs = Vec::new();
            for job in jobs_value
                .get("jobs")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let status = job
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("queued");
                let conclusion = job.get("conclusion").and_then(serde_json::Value::as_str);
                let state = github_job_state(status, conclusion);
                let failed_step = job
                    .get("steps")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|steps| {
                        steps.iter().find(|step| {
                            matches!(
                                step.get("conclusion").and_then(serde_json::Value::as_str),
                                Some("failure" | "timed_out" | "cancelled")
                            )
                        })
                    })
                    .and_then(|step| step.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let workflow_id = run
                    .get("workflow_id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| RrcError::Invalid("workflow id missing".into()))?;
                let job_id = job
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| RrcError::Invalid("job id missing".into()))?;
                let attempt = bounded_u32(
                    job.get("run_attempt")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(1),
                    "job run attempt",
                )?;
                jobs.push(JobSnapshot {
                    workflow_id,
                    run_id,
                    attempt,
                    job_id,
                    workflow_name: workflow_name.clone(),
                    job_name: bounded(
                        job.get("name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                        256,
                    ),
                    platform: infer_platform(job.get("labels")),
                    state,
                    failed_step: failed_step.map(|value| bounded(value, 256)),
                    url: bounded(
                        redact_secrets(
                            job.get("html_url")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default(),
                        ),
                        512,
                    ),
                });
            }
            gates.push(GateRecord {
                name: workflow_name,
                head_sha: head_sha.to_owned(),
                run_id: Some(run_id),
                run_attempt: run
                    .get("run_attempt")
                    .and_then(serde_json::Value::as_u64)
                    .map(|value| bounded_u32(value, "workflow run attempt"))
                    .transpose()?,
                jobs,
                url: run
                    .get("html_url")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| bounded(redact_secrets(value), 512)),
            });
        }
        Ok(gates)
    }

    fn job_log(&self, repository: &str, job_id: u64) -> Result<String, RrcError> {
        let endpoint = format!("repos/{repository}/actions/jobs/{job_id}/logs");
        Self::gh(repository, &[&endpoint])
    }

    fn rerun_job(&self, repository: &str, job_id: u64) -> Result<(), RrcError> {
        let endpoint = format!("repos/{repository}/actions/jobs/{job_id}/rerun");
        Self::gh(repository, &["--method", "POST", &endpoint]).map(|_| ())
    }

    fn rerun_failed(&self, repository: &str, run_id: u64) -> Result<(), RrcError> {
        let endpoint = format!("repos/{repository}/actions/runs/{run_id}/rerun-failed-jobs");
        Self::gh(repository, &["--method", "POST", &endpoint]).map(|_| ())
    }

    fn rerun_workflow(&self, repository: &str, run_id: u64) -> Result<(), RrcError> {
        let endpoint = format!("repos/{repository}/actions/runs/{run_id}/rerun");
        Self::gh(repository, &["--method", "POST", &endpoint]).map(|_| ())
    }
}

fn bounded_u32(value: u64, field: &str) -> Result<u32, RrcError> {
    u32::try_from(value).map_err(|_| RrcError::Invalid(format!("{field} exceeds u32")))
}

fn github_job_state(status: &str, conclusion: Option<&str>) -> JobState {
    if status != "completed" {
        return if status == "in_progress" {
            JobState::InProgress
        } else {
            JobState::Queued
        };
    }
    match conclusion.unwrap_or("failure") {
        "success" => JobState::Success,
        "timed_out" => JobState::TimedOut,
        "cancelled" => JobState::Cancelled,
        "skipped" | "neutral" => JobState::Skipped,
        _ => JobState::Failure,
    }
}

fn infer_platform(labels: Option<&serde_json::Value>) -> Option<String> {
    labels
        .and_then(serde_json::Value::as_array)
        .and_then(|labels| {
            labels
                .iter()
                .filter_map(serde_json::Value::as_str)
                .find(|label| {
                    let lower = label.to_ascii_lowercase();
                    lower.contains("windows")
                        || lower.contains("macos")
                        || lower.contains("ubuntu")
                        || lower.contains("linux")
                })
        })
        .map(str::to_owned)
}

#[must_use]
pub fn default_release_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AGENT_VESPER_RELEASE_ROOT") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))?;
    Some(home.join("agent-vesper").join("release-recovery"))
}

pub fn start_release(
    repo_identity: &str,
    bump: &str,
    branch_ref: &str,
    source_commit: &str,
) -> Result<ReleaseRecoveryRecord, RrcError> {
    if !matches!(bump, "patch" | "minor" | "major") {
        return Err(RrcError::Invalid(
            "release bump must be patch, minor, or major".into(),
        ));
    }
    let mut record = ReleaseRecoveryRecord::new(
        repo_identity.to_owned(),
        format!(
            "{}-{}",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            short_sha(source_commit)
        ),
        ReleaseObjective {
            bump: bump.into(),
            branch_ref: branch_ref.into(),
            post_release_main_epoch: false,
            required_gate_names: default_pre_release_gates(),
        },
    );
    record.release_commit = Some(source_commit.to_owned());
    record.transition(
        ReleaseRecoveryState::Preparing,
        source_commit,
        "release intent accepted by RRC",
        vec![],
        None,
    )?;
    record.transition(
        ReleaseRecoveryState::LocalVerification,
        source_commit,
        "local release verification required before candidate creation",
        vec![],
        None,
    )?;
    Ok(record)
}

pub fn release_command(
    repo_identity: &str,
    argument: &str,
    source_commit: &str,
    branch_ref: &str,
) -> Result<String, RrcError> {
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    let ledger = ReleaseLedger::open(root, repo_identity)?;
    let arg = argument.trim();
    match arg {
        "status" | "" if ledger.load()?.is_some() => {
            Ok(ledger.load()?.expect("checked").render_status())
        }
        "resume" => {
            let record = ledger
                .load()?
                .ok_or_else(|| RrcError::Invalid("no release checkpoint exists".into()))?;
            if record.state == ReleaseRecoveryState::Cancelled {
                return Err(RrcError::Invalid(
                    "cancelled epochs are immutable; start a new release".into(),
                ));
            }
            Ok(format!(
                "{}\nResume               host must refresh remote state before progression",
                record.render_status()
            ))
        }
        "cancel" => {
            crate::release_executor::cancel_release_worker(repo_identity);
            let mut record = ledger
                .load()?
                .ok_or_else(|| RrcError::Invalid("no release checkpoint exists".into()))?;
            let commit = record
                .active_commit()
                .map(str::to_owned)
                .unwrap_or_else(|| source_commit.into());
            record.transition(
                ReleaseRecoveryState::Cancelled,
                &commit,
                "user cancelled local recovery; remote workflows were not claimed cancelled",
                vec![],
                None,
            )?;
            ledger.save(&record)?;
            Ok("Release recovery cancelled locally. Checkpoint preserved; already-running GitHub workflows were not claimed cancelled.".into())
        }
        "evidence" => {
            let record = ledger
                .load()?
                .ok_or_else(|| RrcError::Invalid("no release checkpoint exists".into()))?;
            if record.failures.is_empty() {
                return Ok("release evidence: no failures captured".into());
            }
            let mut lines = vec![format!(
                "release evidence: {} failure(s)",
                record.failures.len()
            )];
            for failure in &record.failures {
                lines.push(format!(
                    "  {} / {} / {} — {:?} ({:?}) — {}",
                    failure.workflow_name,
                    failure.job_name,
                    failure.step_name.as_deref().unwrap_or("unknown step"),
                    failure.class,
                    failure.confidence,
                    &failure.fingerprint.0[..12.min(failure.fingerprint.0.len())]
                ));
            }
            Ok(lines.join("\n"))
        }
        "retry" => {
            let record = ledger
                .load()?
                .ok_or_else(|| RrcError::Invalid("no release checkpoint exists".into()))?;
            let decision = record.retry_admission(RetryKind::FullGate);
            Ok(format!(
                "release retry: {} — {}",
                if decision.admitted {
                    "admissible; explicit Actions write permission still required"
                } else {
                    "blocked"
                },
                decision.reason
            ))
        }
        "status" => Ok("release: no active or persisted epoch".into()),
        value => {
            if let Some(existing) = ledger.load()?
                && !matches!(
                    existing.state,
                    ReleaseRecoveryState::Complete | ReleaseRecoveryState::Cancelled
                )
            {
                return Err(RrcError::Invalid(format!(
                    "release epoch {} is still {:?}; use /release status, resume, or cancel",
                    existing.epoch_id, existing.state
                )));
            }
            let bump = if value.is_empty() { "patch" } else { value };
            let record = start_release(repo_identity, bump, branch_ref, source_commit)?;
            ledger.save(&record)?;
            Ok(format!(
                "{}\nNext                  local verification (controller-owned)",
                record.render_status()
            ))
        }
    }
}

/// Resolves repository identity and current ref/commit before executing a
/// shared host command. No remote URL or credential-bearing value is stored.
pub fn release_command_for_workspace(workspace: &Path, argument: &str) -> Result<String, RrcError> {
    release_command_for_workspace_with_factory(workspace, argument, None)
}

pub fn release_command_for_workspace_with_factory(
    workspace: &Path,
    argument: &str,
    repair_factory: Option<crate::WorkerFactory>,
) -> Result<String, RrcError> {
    let canonical = workspace.canonicalize()?;
    let git = |args: &[&str]| -> Result<String, RrcError> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&canonical)
            .output()?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "git {} failed with status {}",
                args.join(" "),
                output.status
            )));
        }
        Ok(bounded(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            512,
        ))
    };
    let source_commit = git(&["rev-parse", "HEAD"])?;
    let branch_ref = git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let repo_identity = canonical.to_string_lossy().into_owned();
    let action = argument.trim();
    if matches!(action, "resume" | "retry") {
        let remote = git(&["config", "--get", "remote.origin.url"])?;
        let repository = github_repository_slug(&remote)?;
        let root = default_release_root().ok_or_else(|| {
            RrcError::Invalid("no user-owned release state root is available".into())
        })?;
        let ledger = ReleaseLedger::open(root, &repo_identity)?;
        let record = ledger
            .load()?
            .ok_or_else(|| RrcError::Invalid("no release checkpoint exists".into()))?;
        if record.state == ReleaseRecoveryState::Cancelled {
            return Err(RrcError::Invalid(
                "cancelled epochs are immutable; start a new release".into(),
            ));
        }
        if action == "retry" {
            let decision = record.retry_admission(RetryKind::FullGate);
            if !decision.admitted {
                return Err(RrcError::RetryBlocked(decision.reason));
            }
            // The native executor pushes the verified repaired commit and
            // waits for fresh push workflows. Actions reruns are deliberately
            // not used because GitHub preserves the original GITHUB_SHA.
        }
        crate::release_executor::spawn_release_worker_with_factory(
            canonical.clone(),
            repository,
            repo_identity.clone(),
            repair_factory.clone(),
        )?;
        return Ok(format!(
            "{}\nController            active in background; use /release status for the persisted stage",
            record.render_status()
        ));
    }
    let body = release_command(&repo_identity, action, &source_commit, &branch_ref)?;
    if matches!(action, "patch" | "minor" | "major" | "") {
        let remote = git(&["config", "--get", "remote.origin.url"])?;
        let repository = github_repository_slug(&remote)?;
        crate::release_executor::spawn_release_worker_with_factory(
            canonical,
            repository,
            repo_identity,
            repair_factory,
        )?;
    }
    Ok(body)
}

fn github_repository_slug(remote: &str) -> Result<String, RrcError> {
    let trimmed = remote.trim().trim_end_matches(".git");
    let path = if let Some(path) = trimmed.strip_prefix("git@github.com:") {
        path
    } else if let Some(path) = trimmed.strip_prefix("ssh://git@github.com/") {
        path
    } else if let Some(path) = trimmed.strip_prefix("https://github.com/") {
        path
    } else {
        return Err(RrcError::Invalid(
            "remote.origin.url is not a supported GitHub repository URL".into(),
        ));
    };
    let mut segments = path.split('/');
    let owner = segments.next().unwrap_or_default();
    let repository = segments.next().unwrap_or_default();
    let safe = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if !safe(owner) || !safe(repository) || segments.next().is_some() {
        return Err(RrcError::Invalid(
            "remote.origin.url does not identify exactly one safe GitHub owner/repository".into(),
        ));
    }
    Ok(format!("{owner}/{repository}"))
}

/// Read-only RRC status for `/ci`; never creates a state root.
#[must_use]
pub fn release_status_for_workspace(workspace: &Path) -> Option<String> {
    let canonical = workspace.canonicalize().ok()?;
    let root = default_release_root()?;
    if !root.is_dir() {
        return None;
    }
    let ledger = ReleaseLedger {
        root,
        repo_key: sha256_hex(canonical.to_string_lossy().as_bytes()),
    };
    ledger
        .load()
        .ok()
        .flatten()
        .map(|record| record.render_status())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn bounded(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push('…');
    text
}

fn bounded_refs(refs: Vec<String>) -> Vec<String> {
    refs.into_iter()
        .take(32)
        .map(|value| bounded(redact_secrets(&value), 512))
        .collect()
}

fn short_sha(sha: &str) -> &str {
    sha.get(..sha.len().min(12)).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> ReleaseRecoveryRecord {
        let mut record = start_release("repo", "patch", "main", "abcdef123456").unwrap();
        record.objective.required_gate_names = vec!["gate".into()];
        record
            .transition(
                ReleaseRecoveryState::CandidateReady,
                "abcdef123456",
                "green",
                vec![],
                None,
            )
            .unwrap();
        record
            .transition(
                ReleaseRecoveryState::RemoteGateRunning,
                "abcdef123456",
                "remote",
                vec![],
                None,
            )
            .unwrap();
        record
    }

    fn failed_job(state: JobState) -> JobSnapshot {
        JobSnapshot {
            workflow_id: 1,
            run_id: 2,
            attempt: 1,
            job_id: 3,
            workflow_name: "gate".into(),
            job_name: "windows".into(),
            platform: Some("windows".into()),
            state,
            failed_step: Some("test".into()),
            url: "https://example.invalid/job/3".into(),
        }
    }

    #[test]
    fn partial_matrix_blocks_retry() {
        let mut record = record();
        record
            .apply_matrix(GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                jobs: vec![
                    failed_job(JobState::Failure),
                    JobSnapshot {
                        state: JobState::InProgress,
                        job_id: 4,
                        job_name: "macos".into(),
                        ..failed_job(JobState::Failure)
                    },
                ],
                url: None,
            })
            .unwrap();
        assert!(!record.retry_admission(RetryKind::FullGate).admitted);
    }

    #[test]
    fn github_remote_parser_accepts_common_forms_and_rejects_endpoint_injection() {
        assert_eq!(
            github_repository_slug("git@github.com:owner/repo.git").unwrap(),
            "owner/repo"
        );
        assert_eq!(
            github_repository_slug("https://github.com/owner/repo.git").unwrap(),
            "owner/repo"
        );
        assert!(github_repository_slug("https://example.com/owner/repo").is_err());
        assert!(github_repository_slug("https://github.com/owner/repo?x=/evil").is_err());
    }

    #[test]
    fn equivalent_volatile_logs_have_same_fingerprint() {
        let one = failure_fingerprint(
            "CI",
            "test",
            Some("run"),
            Some("linux"),
            Some("cargo test"),
            "2026-01-01T01:02:03Z panicked at /tmp/a-123 request-id=abc 127.0.0.1:49123",
        );
        let two = failure_fingerprint(
            "CI",
            "test",
            Some("run"),
            Some("linux"),
            Some("cargo test"),
            "2027-02-02T03:04:05Z panicked at /tmp/b-999 request-id=xyz 127.0.0.1:59234",
        );
        assert_eq!(one, two);
    }

    #[test]
    fn secret_canaries_are_redacted() {
        let text = first_causal_excerpt(
            "noise\nerror: authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz123456\nmore",
        );
        assert!(!text.contains("ghp_"));
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn community_reports_alone_never_confirm_outage() {
        let verdict = classify_external_health(ExternalHealthEvidence {
            community_reports: true,
            community_evidence: vec!["reports".into()],
            ..Default::default()
        });
        assert_eq!(verdict, ExternalHealthVerdict::NotAdmissible);
    }

    #[test]
    fn official_degradation_needs_repository_exclusion_and_infrastructure_evidence() {
        let verdict = classify_external_health(ExternalHealthEvidence {
            repository_checks_green: true,
            source_explanation_absent: true,
            infrastructure_failures: 2,
            official_degraded: true,
            official_summary: "Actions degraded".into(),
            ..Default::default()
        });
        assert!(matches!(verdict, ExternalHealthVerdict::Confirmed(_)));
    }

    #[test]
    fn stale_sha_cannot_advance_candidate() {
        let mut record = record();
        let error = record
            .apply_matrix(GateRecord {
                name: "gate".into(),
                head_sha: "old".into(),
                run_id: None,
                run_attempt: None,
                jobs: vec![failed_job(JobState::Success)],
                url: None,
            })
            .unwrap_err();
        assert!(matches!(error, RrcError::StaleWorkflow { .. }));
    }

    #[test]
    fn ledger_round_trip_preserves_job_ids() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().canonicalize().unwrap(), "repo").unwrap();
        let mut record = record();
        record
            .apply_matrix(GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Failure)],
                url: None,
            })
            .unwrap();
        ledger.save(&record).unwrap();
        let loaded = ledger.load().unwrap().unwrap();
        assert_eq!(loaded.required_gates[0].jobs[0].job_id, 3);
    }

    #[test]
    fn published_cannot_transition_back_to_publishing() {
        assert!(!legal_transition(
            ReleaseRecoveryState::PostReleaseMainDegraded,
            ReleaseRecoveryState::Publishing
        ));
    }

    struct FakeGitHub {
        gates: Vec<GateRecord>,
        log: String,
    }

    impl GitHubEvidencePort for FakeGitHub {
        fn matrix_for_sha(&self, _: &str, _: &str) -> Result<Vec<GateRecord>, RrcError> {
            Ok(self.gates.clone())
        }
        fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
            Ok(self.log.clone())
        }
        fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
            unreachable!()
        }
        fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
            unreachable!()
        }
        fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
            unreachable!()
        }
    }

    #[test]
    fn complete_matrix_collects_first_causal_log_and_classifies_it() {
        let mut record = record();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Failure)],
                url: None,
            }],
            log:
                "cache warning\nthread 'x' panicked at tests/a.rs:4: assertion failed\nexit code 1"
                    .into(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::ClassifyingFailure);
        assert_eq!(
            record.failures[0].class,
            ReleaseFailureClass::TestRegression
        );
        assert!(record.failures[0].causal_excerpt.contains("panicked at"));
    }

    fn classified_record() -> ReleaseRecoveryRecord {
        let mut record = record();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Failure)],
                url: None,
            }],
            log: "thread 'x' panicked at tests/a.rs:4: assertion failed".into(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        record
    }

    fn repair_for(record: &ReleaseRecoveryRecord, status: FocusedProofStatus) -> RepairAttempt {
        RepairAttempt {
            fingerprint: record.failures.last().unwrap().fingerprint.clone(),
            causal_family: "settlement".into(),
            hypothesis: "the terminal event raced process settlement".into(),
            source_commit_before: "abcdef123456".into(),
            source_commit_after: Some("fedcba654321".into()),
            focused_proof: "cargo test exact_settlement_regression".into(),
            focused_status: status,
            evidence_refs: vec!["test:exact_settlement_regression".into()],
            disproven_or_insufficient: false,
        }
    }

    #[test]
    fn directives_are_typed_and_back_off_without_model_step_polling() {
        let mut record = record();
        assert_eq!(
            next_directive(&record, 0),
            ReleaseDirective::RefreshMatrix {
                after: NORMAL_POLL_INTERVAL
            }
        );
        record.state = ReleaseRecoveryState::WaitingForMatrix;
        assert_eq!(
            next_directive(&record, 3),
            ReleaseDirective::RefreshMatrix {
                after: UNCHANGED_POLL_INTERVAL
            }
        );
        let classified = classified_record();
        assert!(matches!(
            next_directive(&classified, 0),
            ReleaseDirective::RequestFocusedRepair { .. }
        ));
    }

    #[test]
    fn verified_repair_is_the_only_path_to_one_full_gate_retry() {
        let mut record = classified_record();
        let repair = repair_for(&record, FocusedProofStatus::Passed);
        apply_controller_event(&mut record, ReleaseControllerEvent::VerifiedRepair(repair))
            .unwrap();
        assert!(record.retry_admission(RetryKind::FullGate).admitted);
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::RetryDispatched {
                kind: RetryKind::FullGate,
                evidence_refs: vec!["github:run:2".into()],
            },
        )
        .unwrap();
        assert_eq!(record.retry_budget.full_gate_used, 1);
        assert!(!record.retry_admission(RetryKind::FullGate).admitted);
    }

    #[test]
    fn changed_fingerprint_after_retry_opens_a_new_diagnosis() {
        let mut record = classified_record();
        let original = record.failures.last().unwrap().fingerprint.clone();
        let repair = repair_for(&record, FocusedProofStatus::Passed);
        apply_controller_event(&mut record, ReleaseControllerEvent::VerifiedRepair(repair))
            .unwrap();
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::RetryDispatched {
                kind: RetryKind::FullGate,
                evidence_refs: vec!["github:retry".into()],
            },
        )
        .unwrap();
        let mut job = failed_job(JobState::Failure);
        job.attempt = 2;
        job.job_id = 30;
        job.failed_step = Some("compile".into());
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "fedcba654321".into(),
                run_id: Some(2),
                run_attempt: Some(2),
                jobs: vec![job],
                url: None,
            }],
            log: "error[E0308]: mismatched types".into(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::ClassifyingFailure);
        assert_ne!(record.failures.last().unwrap().fingerprint, original);
        assert_eq!(
            record.failures.last().unwrap().class,
            ReleaseFailureClass::CompileFailure
        );
    }

    #[test]
    fn tentative_classification_cannot_authorize_source_repair() {
        let mut record = classified_record();
        record.failures.last_mut().unwrap().confidence = EvidenceConfidence::Tentative;
        let repair = repair_for(&record, FocusedProofStatus::Passed);
        let error =
            apply_controller_event(&mut record, ReleaseControllerEvent::VerifiedRepair(repair))
                .unwrap_err();
        assert!(error.to_string().contains("tentative or unknown"));
    }

    #[test]
    fn two_failed_focused_repairs_exhaust_the_causal_family_budget() {
        let mut record = classified_record();
        let first = repair_for(&record, FocusedProofStatus::Failed);
        apply_controller_event(&mut record, ReleaseControllerEvent::FailedRepair(first)).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::DiagnosingRepair);
        let mut second = repair_for(&record, FocusedProofStatus::Failed);
        second.hypothesis = "the cleanup child survives the first signal".into();
        second.source_commit_after = Some("111111111111".into());
        second.evidence_refs = vec!["test:cleanup-child-regression".into()];
        apply_controller_event(&mut record, ReleaseControllerEvent::FailedRepair(second)).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::Escalated);
        assert_eq!(record.repair_attempts.len(), 2);
    }

    #[test]
    fn identical_failure_on_new_attempt_without_change_hard_blocks_retry() {
        let mut record = classified_record();
        let mut repeated = record.failures[0].clone();
        repeated.attempt = 2;
        repeated.observed_at = Utc::now();
        record.failures.push(repeated);
        record.state = ReleaseRecoveryState::RetryAdmissible;
        let decision = record.retry_admission(RetryKind::FullGate);
        assert!(!decision.admitted);
        assert!(decision.reason.contains("identical failure fingerprint"));
    }

    #[test]
    fn last_green_comparison_is_exact_and_immutable() {
        let mut record = classified_record();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "lastgreen".into(),
                run_id: Some(1),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Failure)],
                url: None,
            }],
            log: "error[E0308]: mismatched types".into(),
        };
        compare_with_last_green(&mut record, "owner/repo", "lastgreen", true, &adapter).unwrap();
        let failure = record.failures.last().unwrap();
        assert_eq!(failure.exists_on_last_green, Some(false));
        assert_eq!(failure.related_source_touched, Some(true));
        let fingerprint = failure.fingerprint.clone();
        assert!(
            record
                .record_failure_context(&fingerprint, true, false)
                .is_err()
        );
    }

    #[test]
    fn missing_required_gate_never_counts_as_complete_matrix() {
        let mut record = record();
        record
            .objective
            .required_gate_names
            .push("missing-gate".into());
        record
            .apply_matrix(GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Success)],
                url: None,
            })
            .unwrap();
        assert!(!record.matrix_complete());
    }

    #[test]
    fn paused_epoch_reopens_with_exact_identity_and_resumes_through_remote_refresh() {
        let mut record = classified_record();
        record.failures.last_mut().unwrap().class =
            ReleaseFailureClass::RunnerInfrastructureFailure;
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::ExternalHealth(ExternalHealthEvidence {
                repository_checks_green: true,
                source_explanation_absent: true,
                infrastructure_failures: 2,
                official_degraded: true,
                official_summary: "Actions degraded".into(),
                ..Default::default()
            }),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().canonicalize().unwrap(), "repo").unwrap();
        ledger.save(&record).unwrap();
        drop(record);
        let mut reopened = ledger.load().unwrap().unwrap();
        assert_eq!(reopened.state, ReleaseRecoveryState::PausedExternal);
        assert_eq!(reopened.required_gates[0].jobs[0].job_id, 3);
        let epoch = reopened.epoch_id.clone();
        reopened
            .transition(
                ReleaseRecoveryState::RemoteGateRunning,
                "abcdef123456",
                "service recovery requires exact-SHA refresh",
                vec!["github:status-recovered".into()],
                None,
            )
            .unwrap();
        assert_eq!(reopened.epoch_id, epoch);
        assert!(matches!(
            next_directive(&reopened, 0),
            ReleaseDirective::RefreshMatrix { .. }
        ));
    }

    #[test]
    fn green_release_then_red_closeout_keeps_publication_immutable() {
        let mut record = start_release("repo", "patch", "main", "abcdef123456").unwrap();
        record.objective.required_gate_names = vec!["gate".into()];
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::LocalVerificationPassed {
                version: "0.24.5".into(),
                candidate_commit: "abcdef123456".into(),
                evidence_refs: vec!["local:verify".into()],
            },
        )
        .unwrap();
        record.mutation.candidate_committed = true;
        record.mutation.candidate_pushed = true;
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::RemoteGateDispatched(vec!["github:push".into()]),
        )
        .unwrap();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(9),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Success)],
                url: None,
            }],
            log: String::new(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        record.mutation.tag_name = Some("v0.24.5".into());
        record.mutation.tag_object = Some("tag-object".into());
        record.mutation.tag_pushed = true;
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::TagVerified(vec!["git:tag:v0.24.5".into()]),
        )
        .unwrap();
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::PublicationStarted(vec!["github:release".into()]),
        )
        .unwrap();
        record.mutation.publication_run_id = Some(10);
        record.mutation.publication_verified = true;
        record.mutation.published_asset_names = vec!["agent-vesper.tar.gz".into()];
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::PublicationVerified {
                version: "0.24.5".into(),
                evidence_refs: vec!["github:asset-checksums".into()],
            },
        )
        .unwrap();
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::PostReleaseCloseoutStarted(vec!["git:docs-closeout".into()]),
        )
        .unwrap();
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::PostReleaseMainRed {
                main_commit: "dddddddddddd".into(),
                evidence_refs: vec!["github:job:macos-intel".into()],
            },
        )
        .unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::PostReleaseMainDegraded);
        assert_eq!(record.release_version.as_deref(), Some("0.24.5"));
        assert!(!legal_transition(
            record.state,
            ReleaseRecoveryState::Publishing
        ));
        assert!(record.render_status().contains("PUBLISHED / VERIFIED"));
        assert!(record.render_status().contains("NO EVIDENCE OF IMPACT"));
        let tag = record.mutation.tag_name.clone();
        let publication_run = record.mutation.publication_run_id;
        let main_adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "dddddddddddd".into(),
                run_id: Some(11),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::Success)],
                url: None,
            }],
            log: String::new(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &main_adapter).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::Complete);
        assert_eq!(record.mutation.tag_name, tag);
        assert_eq!(record.mutation.publication_run_id, publication_run);
        assert_eq!(record.release_version.as_deref(), Some("0.24.5"));
    }

    #[test]
    fn deterministic_test_failure_cannot_be_relabelled_as_outage() {
        let mut record = classified_record();
        let error = apply_controller_event(
            &mut record,
            ReleaseControllerEvent::ExternalHealth(ExternalHealthEvidence {
                repository_checks_green: true,
                source_explanation_absent: true,
                infrastructure_failures: 3,
                official_degraded: true,
                community_reports: true,
                official_summary: "Actions degraded".into(),
                ..Default::default()
            }),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not admit external-health diagnosis")
        );
        assert_eq!(record.state, ReleaseRecoveryState::ClassifyingFailure);
    }

    #[test]
    fn official_degradation_pauses_only_from_late_diagnostic_branch() {
        let mut record = classified_record();
        record.failures.last_mut().unwrap().class =
            ReleaseFailureClass::RunnerInfrastructureFailure;
        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::ExternalHealth(ExternalHealthEvidence {
                repository_checks_green: true,
                source_explanation_absent: true,
                infrastructure_failures: 2,
                official_degraded: true,
                official_summary: "Actions degraded".into(),
                cross_job_evidence: vec!["jobs:2".into()],
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::PausedExternal);
        assert!(record.external_block.is_some());
    }

    #[test]
    fn tag_and_publication_mutations_fail_closed_without_settled_prerequisites() {
        let mut record = record();
        record.state = ReleaseRecoveryState::RemoteGatesGreen;
        assert!(matches!(
            admit_release_mutation(&record, ReleaseMutationKind::CreateTag),
            Err(RrcError::MutationBlocked(_))
        ));
        record.mutation.candidate_pushed = true;
        record.required_gates = vec![GateRecord {
            name: "gate".into(),
            head_sha: "abcdef123456".into(),
            run_id: Some(1),
            run_attempt: Some(1),
            jobs: vec![failed_job(JobState::Success)],
            url: None,
        }];
        assert!(admit_release_mutation(&record, ReleaseMutationKind::CreateTag).is_ok());
        assert!(matches!(
            admit_release_mutation(&record, ReleaseMutationKind::Publish),
            Err(RrcError::MutationBlocked(_))
        ));
    }

    #[test]
    fn status_projection_exposes_product_fields_without_causal_log_content() {
        let mut record = classified_record();
        record.failures.last_mut().unwrap().causal_excerpt =
            "authorization: Bearer ghp_secret-canary".into();
        let status = record.render_status();
        for label in [
            "Candidate SHA",
            "Current main SHA",
            "Local gates",
            "Remote gate",
            "Failure fingerprint",
            "Focused verification",
            "Retry budget",
            "Published release",
            "Current main health",
            "External block",
        ] {
            assert!(status.contains(label), "missing {label}");
        }
        assert!(!status.contains("secret-canary"));
        assert!(!status.contains("authorization"));
    }

    #[test]
    fn watchdog_triggers_after_six_stagnant_actions() {
        let mut record = record();
        for _ in 0..6 {
            record.note_progress(false);
        }
        assert!(record.watchdog_triggered(Utc::now()));
    }
}
