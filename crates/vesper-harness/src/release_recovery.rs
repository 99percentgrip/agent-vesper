#![forbid(unsafe_code)]
//! Deterministic Release Recovery Controller (RRC).
//!
//! This module owns release lifecycle authority. Providers and frontends may
//! diagnose or render its records, but cannot admit retries or mutate release
//! progression without passing these typed guards.

use std::fs::{self, OpenOptions};

use std::io::Write;

use std::path::{Path, PathBuf};

use std::sync::atomic::AtomicBool;

use std::time::Duration;

use chrono::{DateTime, Utc};

use fs2::FileExt;

use regex::Regex;

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use crate::host_resources::ResourceTelemetry;

pub const SCHEMA_VERSION: u32 = 1;

pub const INITIAL_POLL_INTERVAL: Duration = Duration::from_secs(20);

pub const SECOND_POLL_INTERVAL: Duration = Duration::from_secs(40);

pub const THIRD_POLL_INTERVAL: Duration = Duration::from_secs(80);

pub const MAX_POLL_INTERVAL: Duration = Duration::from_secs(120);

pub const STAGNATION_ACTION_LIMIT: u8 = 6;

pub const STAGNATION_TIME_LIMIT: Duration = Duration::from_secs(20 * 60);

pub const MAX_CAUSAL_EXCERPT_BYTES: usize = 4096;

pub const EXTERNAL_HEALTH_BUDGET: Duration = Duration::from_secs(15);

pub const MAX_EXTERNAL_HEALTH_REQUESTS: u8 = 3;

/// Bounded, persisted user-facing events shared by TUI, ACP and a restarted
/// controller. This is deliberately separate from immutable transition
/// evidence: it is a concise progress projection, not a second authority.
pub const MAX_PROGRESS_MILESTONES: usize = 32;

/// Typed settlement state for one user-visible RRC task. This is a display
/// projection only: controller transitions and immutable evidence still own
/// release authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseProgressState {
    #[default]
    Pending,
    Running,
    Passed,
    Failed,
    Paused,
    Cancelled,
    Skipped,
}

impl ReleaseProgressState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Paused => "Paused",
            Self::Cancelled => "Cancelled",
            Self::Skipped => "Skipped",
        }
    }
}

/// Counted work reported by a task. A missing denominator deliberately means
/// that a host must show the task state rather than inventing a percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseProgressUnits {
    pub completed: u64,
    pub total: Option<u64>,
}

impl ReleaseProgressUnits {
    #[must_use]
    pub const fn counted(completed: u64, total: u64) -> Self {
        Self {
            completed,
            total: Some(total),
        }
    }

    #[must_use]
    pub fn render(&self) -> Option<String> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| format!("{}/{}", self.completed.min(total), total))
    }
}

/// One bounded branch of the shared RRC progress hierarchy. The controller
/// creates this from settled local/remote records; hosts only render it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseProgressTask {
    pub name: String,
    pub state: ReleaseProgressState,
    #[serde(default)]
    pub units: ReleaseProgressUnits,
    #[serde(default)]
    pub children: Vec<ReleaseProgressTask>,
}

impl ReleaseProgressTask {
    fn new(
        name: impl Into<String>,
        state: ReleaseProgressState,
        units: ReleaseProgressUnits,
        children: Vec<Self>,
    ) -> Self {
        Self {
            name: bounded(name.into(), 128),
            state,
            units,
            children,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseRecoveryState {
    Idle,
    Preparing,
    LocalVerification,
    ResourceDeferred,
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

/// Coarse, user-facing lifecycle grouping derived only from RRC state and
/// settled gate records. Hosts render this type directly; they never invent a
/// phase from an elapsed timer or from a command admission acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseProgressPhase {
    #[default]
    Preparing,
    LocalVerification,
    RemoteVerification,
    FailureDiagnosis,
    Repair,
    Publication,
    Paused,
    Settled,
}

impl ReleaseProgressPhase {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::LocalVerification => "Local verification",
            Self::RemoteVerification => "Remote verification",
            Self::FailureDiagnosis => "Failure diagnosis",
            Self::Repair => "Focused repair",
            Self::Publication => "Publication",
            Self::Paused => "Paused",
            Self::Settled => "Settled",
        }
    }

    /// Short truthful phase label for width-constrained host surfaces. It is
    /// deliberately a display abbreviation only; serialized state remains the
    /// typed enum and text-only status uses [`Self::label`].
    #[must_use]
    pub const fn compact_label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::LocalVerification => "Local",
            Self::RemoteVerification => "Remote",
            Self::FailureDiagnosis => "Diagnosing",
            Self::Repair => "Repair",
            Self::Publication => "Publishing",
            Self::Paused => "Paused",
            Self::Settled => "Settled",
        }
    }
}

/// One concise RRC-owned event suitable for a chat milestone and RUN history.
/// `sequence` is monotonically increasing for the epoch, so a host can resume
/// without duplicating prior chat lines after a redraw or process restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseProgressMilestone {
    pub sequence: u64,
    pub at: DateTime<Utc>,
    pub phase: ReleaseProgressPhase,
    pub summary: String,
}

/// Persisted, typed progress projection. It does not authorize a transition:
/// `ReleaseRecoveryRecord::state`, gate records, and immutable transition
/// evidence remain the sole lifecycle authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseProgress {
    pub phase: ReleaseProgressPhase,
    pub headline: String,
    pub current_gate: Option<String>,
    pub completed_local_gates: usize,
    pub total_local_gates: usize,
    pub terminal_remote_jobs: usize,
    pub total_remote_jobs: usize,
    /// Bounded persisted hierarchy for local gates and exact-SHA CI jobs.
    /// Absence in older ledgers is refreshed before their next save.
    #[serde(default)]
    pub tasks: Vec<ReleaseProgressTask>,
    pub next_sequence: u64,
    pub milestones: Vec<ReleaseProgressMilestone>,
}

impl ReleaseProgress {
    /// Reflects a controller-owned local command after it has actually been
    /// spawned. The gate does not become passed until the ledger records its
    /// settled result.
    pub fn mark_local_gate_running(&mut self, gate_name: &str) {
        let Some(local) = self
            .tasks
            .iter_mut()
            .find(|task| task.name == "Local verification")
        else {
            return;
        };
        local.state = ReleaseProgressState::Running;
        if let Some(gate) = local
            .children
            .iter_mut()
            .find(|task| task.name == gate_name)
        {
            gate.state = ReleaseProgressState::Running;
        }
    }

    /// Applies a bounded, controller-parsed live subtask count to the already
    /// derived hierarchy. This never changes RRC authority or completion: the
    /// parent gate remains Running until its command settles and is persisted.
    pub fn update_local_subtask(
        &mut self,
        gate_name: &str,
        subtask_name: &str,
        completed: u64,
        total: u64,
    ) {
        if total == 0 || completed > total {
            return;
        }
        let Some(gate) = self.tasks.iter_mut().find_map(|task| {
            (task.name == "Local verification")
                .then(|| {
                    task.children
                        .iter_mut()
                        .find(|child| child.name == gate_name)
                })
                .flatten()
        }) else {
            return;
        };
        let state = if completed == total {
            ReleaseProgressState::Passed
        } else {
            ReleaseProgressState::Running
        };
        if let Some(subtask) = gate
            .children
            .iter_mut()
            .find(|child| child.name == subtask_name)
        {
            subtask.state = state;
            subtask.units = ReleaseProgressUnits::counted(completed, total);
        } else {
            gate.children.push(ReleaseProgressTask::new(
                subtask_name,
                state,
                ReleaseProgressUnits::counted(completed, total),
                Vec::new(),
            ));
        }
    }
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
    #[serde(default)]
    pub run_state: Option<JobState>,
    pub jobs: Vec<JobSnapshot>,
    pub url: Option<String>,
}

impl GateRecord {
    #[must_use]
    pub fn matrix_complete(&self) -> bool {
        self.run_state.is_some_and(JobState::terminal)
            && !self.jobs.is_empty()
            && self.jobs.iter().all(|job| job.state.terminal())
    }

    #[must_use]
    pub fn has_failure(&self) -> bool {
        self.run_state
            .is_some_and(|state| state != JobState::Success && state.terminal())
            || self
                .jobs
                .iter()
                .any(|job| job.state != JobState::Success && job.state.terminal())
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

/// Persisted passive-watch state for a release whose next local gate is
/// withheld by current host risk. This is not source-failure evidence and does
/// not consume any retry or repair budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDeferredRecord {
    pub gate_name: String,
    pub deferred_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub telemetry: ResourceTelemetry,
    pub consecutive_normal_observations: u8,
    pub unchanged_observations: u8,
    pub next_check_seconds: u64,
}

/// Persisted controller liveness is separate from release evidence. A stalled
/// local process or repair agent must never be presented as a source/CI
/// failure, and a restart must retain the reason the worker stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseLivenessState {
    #[default]
    Idle,
    Active,
    Stalled,
    DeadlineExceeded,
    Failed,
    /// The owning controller process is gone. The epoch stays recoverable and
    /// this is not source, CI, or publication evidence.
    OwnerExited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseLivenessRecord {
    pub state: ReleaseLivenessState,
    pub operation: String,
    pub detail: String,
    pub observed_at: Option<DateTime<Utc>>,
    /// Process that last claimed `Active`. Absent means Active is not proof
    /// that a controller still owns the epoch.
    #[serde(default)]
    pub owner_pid: Option<u32>,
}

static RELEASE_OWNER_RELINQUISHED: AtomicBool = AtomicBool::new(false);

/// Stops this process from persisting `Active` ownership. Later ledger saves
/// for this process's owner pid become `OwnerExited` instead.
pub fn relinquish_release_owner_writes() {
    RELEASE_OWNER_RELINQUISHED.store(true, std::sync::atomic::Ordering::Release);
}

#[must_use]
pub fn release_owner_writes_relinquished() -> bool {
    RELEASE_OWNER_RELINQUISHED.load(std::sync::atomic::Ordering::Acquire)
}

impl ReleaseLivenessRecord {
    #[must_use]
    pub fn blocked(&self) -> bool {
        matches!(
            self.state,
            ReleaseLivenessState::Stalled
                | ReleaseLivenessState::DeadlineExceeded
                | ReleaseLivenessState::Failed
        )
    }
}

/// Immutable provenance and settled side-effect state for the production
/// release path. A state is recorded only after the native adapter has
/// independently observed the corresponding repository/GitHub fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseMutationRecord {
    pub source_commit: Option<String>,
    /// Workspace where the admitted completed implementation was observed.
    pub source_workspace: Option<String>,
    /// Stable completed-objective identity from the repository provenance marker.
    pub objective_id: Option<String>,
    /// Human-facing objective label; raw Git topology stays in evidence only.
    pub objective_label: Option<String>,
    /// Human-facing completed variant label used only if same-objective variants remain.
    pub source_variant_label: Option<String>,
    /// Stable completed-variant identity from the repository provenance marker.
    #[serde(default)]
    pub source_variant_id: Option<String>,
    /// Whether this epoch was admitted from the authoritative integrated source.
    #[serde(default)]
    pub canonical_release_source: bool,
    /// Historical objective/variant identities explicitly replaced by this source.
    #[serde(default)]
    pub supersedes: Vec<String>,
    /// Evidence reports bound by the completed-objective provenance marker.
    pub objective_evidence_reports: Vec<String>,
    /// Clean isolated worktree owned by this release epoch.
    pub release_workspace: Option<String>,
    /// Active checkout HEAD from which intended completed commits were derived.
    pub base_sha: Option<String>,
    /// Ordered implementation/evidence commits admitted into this release.
    pub intended_commits: Vec<String>,
    /// SHA-256 of the exact admitted base..source binary diff.
    pub intended_diff_sha256: Option<String>,
    /// Exact clean source/candidate identity selected before version preparation.
    pub final_candidate_commit: Option<String>,
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
    #[serde(default)]
    pub in_flight_operation: Option<String>,
    #[serde(default)]
    pub repair_files: Vec<String>,
    /// SHA-256 causal-family identities counted before model dispatch. Includes
    /// interrupted/failed turns which never produced a focused proof receipt.
    #[serde(default)]
    pub repair_admissions: std::collections::BTreeMap<String, u8>,
    #[serde(default)]
    pub closeout_registry: Option<crate::release_closeout::RegistryTarget>,
    #[serde(default)]
    pub closeout_receipt: Option<crate::release_closeout::CloseoutReceipt>,
}

/// A normalized release target selected by ordinary-language or slash-command
/// admission. Its string representation remains compatible with schema-v1
/// ledger `bump` fields while making exact stable versions explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ReleaseVersionSelector {
    Patch,
    Minor,
    Major,
    Exact(String),
}

impl ReleaseVersionSelector {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "patch" => Ok(Self::Patch),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            _ => {
                let [major, minor, patch] = stable_semver_components(value).ok_or_else(|| {
                    "release target must be patch, minor, major, or a stable x.y.z version"
                        .to_owned()
                })?;
                Ok(Self::Exact(format!("{major}.{minor}.{patch}")))
            }
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Exact(version) => version,
        }
    }

    #[must_use]
    pub fn target_label(&self) -> String {
        match self {
            Self::Exact(version) => version.clone(),
            selector => format!("next {}", selector.as_str()),
        }
    }
}

impl TryFrom<String> for ReleaseVersionSelector {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ReleaseVersionSelector> for String {
    fn from(value: ReleaseVersionSelector) -> Self {
        value.as_str().to_owned()
    }
}

pub(crate) fn stable_semver_components(value: &str) -> Option<[u64; 3]> {
    let trimmed = value.trim();
    let value = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    let mut components = value.split('.').map(|component| {
        if component.is_empty()
            || (component.len() > 1 && component.starts_with('0'))
            || !component.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        component.parse::<u64>().ok()
    });
    let parsed = [
        components.next()??,
        components.next()??,
        components.next()??,
    ];
    components.next().is_none().then_some(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseObjective {
    /// Serialized as `bump` for schema-v1 ledger compatibility.
    #[serde(rename = "bump")]
    pub version: ReleaseVersionSelector,
    pub branch_ref: String,
    /// Original bounded user objective; provenance only, never executed as a prompt.
    #[serde(default)]
    pub request: Option<String>,
    pub post_release_main_epoch: bool,
    #[serde(default = "default_pre_release_gates")]
    pub required_gate_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseIntentDecision {
    NotRelease,
    Admit {
        version: ReleaseVersionSelector,
        scope: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NaturalReleaseAdmission {
    NotRelease,
    Started(String),
    Clarification(String),
}

/// Conservative, provider-neutral admission for an ordinary conversation
/// objective. It recognizes direct imperatives only; questions, planning,
/// negation, status discussion and deferred intent remain ordinary chat.
fn explicit_release_version(text: &str) -> Result<Option<ReleaseVersionSelector>, ()> {
    for raw in text.split_whitespace() {
        let token = raw
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    ',' | ';'
                        | ':'
                        | '!'
                        | '?'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '"'
                        | '\''
                        | '`'
                )
            })
            .trim_end_matches('.');
        if token.is_empty() {
            continue;
        }
        if let Ok(selector @ ReleaseVersionSelector::Exact(_)) =
            ReleaseVersionSelector::parse(token)
        {
            return Ok(Some(selector));
        }
        let candidate = token
            .strip_prefix('v')
            .or_else(|| token.strip_prefix('V'))
            .unwrap_or(token);
        if candidate.contains('.') && candidate.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Err(());
        }
    }
    Ok(None)
}

#[must_use]
pub fn classify_release_intent(text: &str) -> ReleaseIntentDecision {
    let normalized = text
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`'))
        .to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.contains('?')
        || [
            "how ",
            "what ",
            "when ",
            "why ",
            "should ",
            "could ",
            "can ",
            "would ",
            "explain ",
            "describe ",
            "document ",
            "discuss ",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
        || [
            "do not release",
            "don't release",
            "dont release",
            "not release",
            "release later",
            "ship later",
            "publish later",
            "can release later",
            "release system",
            "release process",
            "release workflow",
        ]
        .iter()
        .any(|phrase| normalized.contains(phrase))
    {
        return ReleaseIntentDecision::NotRelease;
    }
    let imperative = [
        "release ",
        "release it",
        "ship ",
        "ship it",
        "publish ",
        "publish it",
        "please release ",
        "please ship ",
        "please publish ",
        "go ahead and release ",
        "go ahead and ship ",
        "go ahead and publish ",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix));
    if !imperative {
        return ReleaseIntentDecision::NotRelease;
    }
    let version = match explicit_release_version(&normalized) {
        Ok(Some(version)) => version,
        Err(()) => return ReleaseIntentDecision::NotRelease,
        Ok(None) => {
            if normalized
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|word| word == "major")
            {
                ReleaseVersionSelector::Major
            } else if normalized
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|word| word == "minor")
            {
                ReleaseVersionSelector::Minor
            } else {
                // Repository policy makes an unspecified imperative a patch.
                // This avoids needless clarification for "ship this" while
                // keeping the typed objective explicit and auditable.
                ReleaseVersionSelector::Patch
            }
        }
    };
    ReleaseIntentDecision::Admit {
        version,
        scope: bounded(text.trim().to_owned(), 2048),
    }
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
    #[serde(default)]
    pub cancelled_from: Option<ReleaseRecoveryState>,
    pub release_version: Option<String>,
    pub release_commit: Option<String>,
    pub current_main: Option<String>,
    #[serde(default)]
    pub last_green_release_commit: Option<String>,
    pub required_gates: Vec<GateRecord>,
    #[serde(default)]
    pub retry_run_floors: Vec<(u64, u32)>,
    pub failures: Vec<FailureRecord>,
    pub repair_attempts: Vec<RepairAttempt>,
    pub state_changes: Vec<RelevantStateChange>,
    pub retry_budget: RetryBudget,
    pub external_block: Option<ExternalBlockRecord>,
    #[serde(default)]
    pub resource_deferred: Option<ResourceDeferredRecord>,
    /// Last controller-operation liveness observation, shared by TUI/ACP and
    /// retained across restart.
    #[serde(default)]
    pub liveness: ReleaseLivenessRecord,
    #[serde(default)]
    pub mutation: ReleaseMutationRecord,
    /// Bounded host-neutral progress projection. Backward-compatible ledger
    /// loads derive an empty default and refresh it before the next save.
    #[serde(default)]
    pub progress: ReleaseProgress,
    pub transitions: Vec<TransitionRecord>,
    #[serde(default)]
    pub metrics: ReleaseMetrics,
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
            cancelled_from: None,
            release_version: None,
            release_commit: None,
            current_main: None,
            last_green_release_commit: None,
            required_gates: Vec::new(),
            retry_run_floors: Vec::new(),
            failures: Vec::new(),
            repair_attempts: Vec::new(),
            state_changes: Vec::new(),
            retry_budget: RetryBudget::default(),
            external_block: None,
            resource_deferred: None,
            liveness: ReleaseLivenessRecord::default(),
            mutation: ReleaseMutationRecord::default(),
            progress: ReleaseProgress::default(),
            transitions: Vec::new(),
            metrics: ReleaseMetrics::default(),
            consecutive_stagnant_actions: 0,
            last_progress_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    /// Recomputes the host-neutral progress projection from RRC-owned state.
    /// This function has no release authority and intentionally performs no
    /// I/O; callers persist it with the same ledger save as the settled state.
    pub fn refresh_progress(&mut self) {
        let completed_local_gates = self
            .mutation
            .local_gates
            .iter()
            .filter(|gate| gate.state == SettlementState::Succeeded)
            .count();
        let current_gate = self
            .mutation
            .local_gates
            .iter()
            .find(|gate| gate.state == SettlementState::Running)
            .or_else(|| {
                matches!(
                    self.state,
                    ReleaseRecoveryState::LocalVerification
                        | ReleaseRecoveryState::ResourceDeferred
                )
                .then(|| {
                    self.mutation
                        .local_gates
                        .iter()
                        .find(|gate| gate.state == SettlementState::NotStarted)
                })
                .flatten()
            })
            .map(|gate| gate.name.clone());
        let total_remote_jobs = self.required_gates.iter().map(|gate| gate.jobs.len()).sum();
        let terminal_remote_jobs = self
            .required_gates
            .iter()
            .flat_map(|gate| gate.jobs.iter())
            .filter(|job| job.state.terminal())
            .count();
        let phase = match self.state {
            ReleaseRecoveryState::Idle | ReleaseRecoveryState::Preparing => {
                ReleaseProgressPhase::Preparing
            }
            ReleaseRecoveryState::LocalVerification => ReleaseProgressPhase::LocalVerification,
            ReleaseRecoveryState::ResourceDeferred => ReleaseProgressPhase::Paused,
            ReleaseRecoveryState::CandidateReady
            | ReleaseRecoveryState::RemoteGateRunning
            | ReleaseRecoveryState::WaitingForMatrix
            | ReleaseRecoveryState::RemoteGatesGreen
            | ReleaseRecoveryState::PostReleaseMainDegraded => {
                ReleaseProgressPhase::RemoteVerification
            }
            ReleaseRecoveryState::DiagnosingLocalFailure
            | ReleaseRecoveryState::CollectingFailureEvidence
            | ReleaseRecoveryState::ClassifyingFailure
            | ReleaseRecoveryState::NeedMoreEvidence
            | ReleaseRecoveryState::DiagnosingRepair
            | ReleaseRecoveryState::ExternalHealthCheck => ReleaseProgressPhase::FailureDiagnosis,
            ReleaseRecoveryState::FocusedRepair
            | ReleaseRecoveryState::FocusedVerification
            | ReleaseRecoveryState::RetryAdmissible => ReleaseProgressPhase::Repair,
            ReleaseRecoveryState::Tagging
            | ReleaseRecoveryState::Publishing
            | ReleaseRecoveryState::PostReleaseCloseout => ReleaseProgressPhase::Publication,
            ReleaseRecoveryState::PausedExternal | ReleaseRecoveryState::Escalated => {
                ReleaseProgressPhase::Paused
            }
            ReleaseRecoveryState::Published => ReleaseProgressPhase::Publication,
            ReleaseRecoveryState::Complete
                if self.mutation.publication_verified
                    && self.mutation.closeout_receipt.is_none() =>
            {
                ReleaseProgressPhase::Publication
            }
            ReleaseRecoveryState::Complete | ReleaseRecoveryState::Cancelled => {
                ReleaseProgressPhase::Settled
            }
        };
        let headline = match self.state {
            ReleaseRecoveryState::Idle => "Awaiting a release request".into(),
            ReleaseRecoveryState::Preparing => "Preparing the isolated candidate workspace".into(),
            ReleaseRecoveryState::LocalVerification => match current_gate.as_deref() {
                Some(gate) => format!(
                    "Local verification {completed_local_gates}/{} · {gate}",
                    self.mutation.local_gates.len()
                ),
                None => format!(
                    "Preparing release target {} and local verification",
                    self.objective.version.target_label()
                ),
            },
            ReleaseRecoveryState::ResourceDeferred => {
                let gate = self
                    .resource_deferred
                    .as_ref()
                    .map(|deferred| deferred.gate_name.as_str())
                    .unwrap_or("next local gate");
                format!(
                    "Paused — resource pressure · Local verification {completed_local_gates}/{} · {gate} waiting",
                    self.mutation.local_gates.len()
                )
            }
            ReleaseRecoveryState::CandidateReady => {
                "Local verification passed; pushing the exact candidate".into()
            }
            ReleaseRecoveryState::RemoteGateRunning => {
                "Exact candidate pushed; locating required CI gates".into()
            }
            ReleaseRecoveryState::WaitingForMatrix => {
                format!("Required CI matrix {terminal_remote_jobs}/{total_remote_jobs} terminal")
            }
            ReleaseRecoveryState::CollectingFailureEvidence => {
                "Collecting complete-matrix failure evidence".into()
            }
            ReleaseRecoveryState::ClassifyingFailure => {
                "Classifying the first causal failure".into()
            }
            ReleaseRecoveryState::DiagnosingLocalFailure => {
                "Local verification failed; preserving causal evidence".into()
            }
            ReleaseRecoveryState::NeedMoreEvidence => {
                "More evidence is required before a retry or repair".into()
            }
            ReleaseRecoveryState::FocusedRepair => {
                "Running one focused evidence-backed repair".into()
            }
            ReleaseRecoveryState::FocusedVerification => {
                "Running focused verification for the repair".into()
            }
            ReleaseRecoveryState::DiagnosingRepair => {
                "Focused repair needs additional diagnosis".into()
            }
            ReleaseRecoveryState::RetryAdmissible => {
                "Focused proof passed; preparing a fresh exact-SHA candidate".into()
            }
            ReleaseRecoveryState::ExternalHealthCheck => {
                "Checking official GitHub health evidence".into()
            }
            ReleaseRecoveryState::PausedExternal => {
                "Paused: official external degradation remains confirmed".into()
            }
            ReleaseRecoveryState::Escalated => {
                "Paused safely; controller escalation is required".into()
            }
            ReleaseRecoveryState::RemoteGatesGreen => {
                "Exact-SHA remote gates are green; verifying immutable tag".into()
            }
            ReleaseRecoveryState::Tagging => "Immutable tag verified; starting publication".into(),
            ReleaseRecoveryState::Publishing => "Waiting for publication assets to settle".into(),
            ReleaseRecoveryState::Published => "Release assets are published and verified".into(),
            ReleaseRecoveryState::PostReleaseCloseout => "Checking post-release main health".into(),
            ReleaseRecoveryState::PostReleaseMainDegraded => {
                "Published release remains verified; later main is degraded".into()
            }
            ReleaseRecoveryState::Complete
                if self.mutation.publication_verified
                    && self.mutation.closeout_receipt.is_none() =>
            {
                "Main health verified; registry and execution report closeout pending".into()
            }
            ReleaseRecoveryState::Complete => "Release recovery is complete".into(),
            ReleaseRecoveryState::Cancelled => "Release recovery was cancelled".into(),
        };
        let headline = if self.liveness.state == ReleaseLivenessState::OwnerExited {
            "Recoverable — controller owner exited; release epoch is not running".into()
        } else if self.liveness.blocked() {
            format!(
                "Paused — {} · {}",
                self.liveness.operation, self.liveness.detail
            )
        } else {
            headline
        };
        self.progress.phase = phase;
        self.progress.headline = headline;
        self.progress.current_gate = current_gate;
        self.progress.completed_local_gates = completed_local_gates;
        self.progress.total_local_gates = self.mutation.local_gates.len();
        self.progress.terminal_remote_jobs = terminal_remote_jobs;
        self.progress.total_remote_jobs = total_remote_jobs;
        self.progress.tasks = progress_tasks(self);
    }

    /// Appends one bounded display event after a state or gate settlement.
    /// The immutable RRC record remains authoritative; milestones only let a
    /// host show concise chat progress and resume without duplicate output.
    pub fn note_progress_milestone(&mut self, summary: impl Into<String>) {
        self.refresh_progress();
        let at = Utc::now();
        let sequence = self.progress.next_sequence.saturating_add(1);
        self.progress.next_sequence = sequence;
        self.progress.milestones.push(ReleaseProgressMilestone {
            sequence,
            at,
            phase: self.progress.phase,
            summary: bounded(redact_secrets(&summary.into()), 512),
        });
        if self.progress.milestones.len() > MAX_PROGRESS_MILESTONES {
            let excess = self.progress.milestones.len() - MAX_PROGRESS_MILESTONES;
            self.progress.milestones.drain(..excess);
        }
        self.updated_at = at;
    }

    pub fn note_liveness_active(&mut self, operation: impl Into<String>) {
        let now = Utc::now();
        self.liveness = ReleaseLivenessRecord {
            state: ReleaseLivenessState::Active,
            operation: bounded(operation.into(), 128),
            detail: "controller operation is active".into(),
            observed_at: Some(now),
            owner_pid: Some(std::process::id()),
        };
        self.updated_at = now;
        self.refresh_progress();
    }

    pub fn note_liveness_failure(&mut self, error: &RrcError) {
        let (state, operation, detail) = match error {
            RrcError::WatchdogStalled { operation, .. } => (
                ReleaseLivenessState::Stalled,
                operation.clone(),
                error.to_string(),
            ),
            RrcError::WatchdogDeadline { operation, .. } => (
                ReleaseLivenessState::DeadlineExceeded,
                operation.clone(),
                error.to_string(),
            ),
            RrcError::Cancelled | RrcError::ResourceConstrained(_) => return,
            _ => (
                ReleaseLivenessState::Failed,
                if self.liveness.operation.is_empty() {
                    "release controller step".into()
                } else {
                    self.liveness.operation.clone()
                },
                error.to_string(),
            ),
        };
        self.liveness = ReleaseLivenessRecord {
            state,
            operation: bounded(operation, 128),
            detail: bounded(detail, 512),
            observed_at: Some(Utc::now()),
            owner_pid: self.liveness.owner_pid,
        };
        self.note_progress_milestone(self.liveness.detail.clone());
    }

    pub fn transition(
        &mut self,
        to: ReleaseRecoveryState,
        source_commit: &str,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
        ids: Option<(u64, u64, u64)>,
    ) -> Result<(), RrcError> {
        if !legal_transition(self.state, to)
            || (self.state == ReleaseRecoveryState::Cancelled && self.cancelled_from != Some(to))
        {
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
        let ids = ids.or_else(|| {
            self.required_gates
                .iter()
                .filter(|gate| gate.head_sha == source_commit)
                .flat_map(|gate| &gate.jobs)
                .find(|job| job.state.failed())
                .or_else(|| {
                    self.required_gates
                        .iter()
                        .filter(|gate| gate.head_sha == source_commit)
                        .flat_map(|gate| &gate.jobs)
                        .next()
                })
                .map(|job| (job.workflow_id, job.run_id, job.job_id))
        });
        let reason = bounded(reason.into(), 1024);
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
            reason: reason.clone(),
            evidence_refs: bounded_refs(evidence_refs),
            repair_attempts: self.repair_attempts.len(),
            full_gate_retries: self.retry_budget.full_gate_used,
            infrastructure_retries: self.retry_budget.infrastructure_used,
        });
        if to == ReleaseRecoveryState::Cancelled {
            self.cancelled_from = Some(self.state);
        }
        if self.state == ReleaseRecoveryState::Cancelled {
            self.cancelled_from = None;
        }
        self.state = to;
        self.updated_at = now;
        self.refresh_progress();
        self.note_progress_milestone(format!("{} — {reason}", self.progress.phase.label()));
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
            .find(|row| row.name.eq_ignore_ascii_case(&gate.name))
        {
            if existing.run_id > gate.run_id
                || (existing.run_id == gate.run_id
                    && existing.run_attempt.unwrap_or(0) > gate.run_attempt.unwrap_or(0))
            {
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
        if changed {
            self.refresh_progress();
        }
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
    fn remote_matrix_watch_timed_out(&self, now: DateTime<Utc>) -> bool {
        // Waiting on known queued/running GitHub jobs is passive CI time, not
        // active model recovery. Allow the repository's 60-minute platform jobs
        // to settle, while bounding unavailable/indefinitely queued CI evidence.
        let active_jobs = self.required_gates.iter().any(|gate| {
            gate.run_state.is_some_and(|state| !state.terminal())
                || gate.jobs.iter().any(|job| !job.state.terminal())
        });
        let limit = if active_jobs {
            Duration::from_secs(2 * 60 * 60)
        } else {
            STAGNATION_TIME_LIMIT
        };
        now.signed_duration_since(self.last_progress_at)
            .to_std()
            .is_ok_and(|elapsed| elapsed >= limit)
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
        if matches!(
            self.state,
            ReleaseRecoveryState::Cancelled
                | ReleaseRecoveryState::Escalated
                | ReleaseRecoveryState::Complete
                | ReleaseRecoveryState::Published
                | ReleaseRecoveryState::PausedExternal
        ) {
            return RetryDecision::blocked("controller epoch is stopped");
        }
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
                if repair.focused_status != FocusedProofStatus::Passed
                    || repair.disproven_or_insufficient
                {
                    return RetryDecision::blocked("focused verification has not passed");
                }
                if repair.hypothesis.trim().is_empty()
                    || repair.focused_proof.trim().is_empty()
                    || repair.evidence_refs.is_empty()
                    || repair.source_commit_after.as_deref() != self.active_commit()
                    || repair.source_commit_after.as_deref()
                        == Some(repair.source_commit_before.as_str())
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
                if self.state != ReleaseRecoveryState::ClassifyingFailure {
                    return RetryDecision::blocked(
                        "infrastructure retry requires fresh classification",
                    );
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
        // Old schema-v1 checkpoints may predate the persisted projection. Do
        // not write during a read-only `/release status`; derive a local view.
        let mut progress_record = self.clone();
        progress_record.refresh_progress();
        let progress = &progress_record.progress;
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
        let recent_progress = progress
            .milestones
            .iter()
            .rev()
            .take(4)
            .rev()
            .map(|milestone| format!("#{} {}", milestone.sequence, milestone.summary))
            .collect::<Vec<_>>()
            .join(" | ");
        let release_target = self.objective.version.target_label();
        let ci_wait = self.metrics.ci_wait_millis;
        let model_active = self.metrics.model_active_millis;
        let refusals = self.metrics.retries_rejected;
        format!(
            "RELEASE RECOVERY\nState                 {:?}\nProgress              {} — {}\nProgress gates        {}/{} local · {}/{} remote jobs\nRecent milestones     {}\nCandidate SHA         {}\nCurrent main SHA      {}\nRelease target         {release_target}\nVersion provenance    {} -> {}\nLocal gates           {local_complete}/{} · {local_active}\nRemote gate           {active_gate}\nJobs                  {terminal}/{total} terminal\nFailed jobs           {}\nFailure fingerprint   {fingerprint}\nFocused verification  {focused}\nRetry                 {} — {}\nRetry budget          full {}/{} · infrastructure {}/{} · diagnostic {}/{}\nMutation state        commit={} push={} tag={} publish={}\nPublished release     {published}\nCurrent main health   {main_health}\nExternal block        {external}\nMeasured CI wait      {ci_wait} ms\nModel repair active   {model_active} ms\nAutonomous refusals   {refusals}\nNext                  {:?}",
            self.state,
            progress.phase.label(),
            progress.headline,
            progress.completed_local_gates,
            progress.total_local_gates,
            progress.terminal_remote_jobs,
            progress.total_remote_jobs,
            if recent_progress.is_empty() {
                "none"
            } else {
                &recent_progress
            },
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

    pub fn resume_cancelled(&mut self) -> Result<(), RrcError> {
        if self.state != ReleaseRecoveryState::Cancelled {
            return Ok(());
        }
        let from = self.cancelled_from.ok_or_else(|| {
            RrcError::Invalid("legacy cancelled checkpoint has no resumable stage".into())
        })?;
        let commit = self
            .active_commit()
            .ok_or_else(|| RrcError::Invalid("cancelled checkpoint commit missing".into()))?
            .to_owned();
        self.transition(
            from,
            &commit,
            "user resumed cancelled checkpoint; refresh remote evidence before progression",
            vec![],
            None,
        )
    }
}

fn progress_tasks(record: &ReleaseRecoveryRecord) -> Vec<ReleaseProgressTask> {
    let local_children = record
        .mutation
        .local_gates
        .iter()
        .map(|gate| {
            let state = if (record.liveness.blocked()
                || record.liveness.state == ReleaseLivenessState::OwnerExited)
                && gate.state == SettlementState::Running
            {
                ReleaseProgressState::Paused
            } else {
                progress_state_from_settlement(gate.state)
            };
            let completed = u64::from(state == ReleaseProgressState::Passed);
            let children = (gate.name == "acceptance").then(|| {
                vec![ReleaseProgressTask::new(
                    "Exact acceptance cases",
                    state,
                    ReleaseProgressUnits::default(),
                    Vec::new(),
                )]
            });
            ReleaseProgressTask::new(
                gate.name.clone(),
                state,
                ReleaseProgressUnits::counted(completed, 1),
                children.unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let local_state = if record.mutation.local_gates.is_empty() {
        stage_state(record, ReleaseProgressPhase::LocalVerification)
    } else {
        aggregate_progress_state(local_children.iter().map(|task| task.state))
    };
    let local_completed = local_children
        .iter()
        .filter(|task| task.state == ReleaseProgressState::Passed)
        .count() as u64;

    let remote_children = record
        .objective
        .required_gate_names
        .iter()
        .map(|required| {
            let Some(gate) = record
                .required_gates
                .iter()
                .find(|gate| gate.name.eq_ignore_ascii_case(required))
            else {
                return ReleaseProgressTask::new(
                    required.clone(),
                    stage_state(record, ReleaseProgressPhase::RemoteVerification),
                    ReleaseProgressUnits::default(),
                    Vec::new(),
                );
            };
            let job_children = gate
                .jobs
                .iter()
                .map(|job| {
                    ReleaseProgressTask::new(
                        bounded(
                            format!(
                                "{}{}",
                                job.job_name,
                                job.platform
                                    .as_deref()
                                    .map(|platform| format!(" · {platform}"))
                                    .unwrap_or_default()
                            ),
                            128,
                        ),
                        progress_state_from_job(job.state),
                        ReleaseProgressUnits::counted(u64::from(job.state.terminal()), 1),
                        Vec::new(),
                    )
                })
                .collect::<Vec<_>>();
            let terminal = job_children
                .iter()
                .filter(|job| {
                    matches!(
                        job.state,
                        ReleaseProgressState::Passed
                            | ReleaseProgressState::Failed
                            | ReleaseProgressState::Cancelled
                            | ReleaseProgressState::Skipped
                    )
                })
                .count() as u64;
            ReleaseProgressTask::new(
                gate.name.clone(),
                aggregate_progress_state(job_children.iter().map(|job| job.state)),
                if job_children.is_empty() {
                    ReleaseProgressUnits::default()
                } else {
                    ReleaseProgressUnits::counted(terminal, job_children.len() as u64)
                },
                job_children,
            )
        })
        .collect::<Vec<_>>();
    let remote_state = if remote_children.is_empty() {
        stage_state(record, ReleaseProgressPhase::RemoteVerification)
    } else {
        aggregate_progress_state(remote_children.iter().map(|task| task.state))
    };
    let remote_terminal = remote_children
        .iter()
        .filter(|task| {
            matches!(
                task.state,
                ReleaseProgressState::Passed
                    | ReleaseProgressState::Failed
                    | ReleaseProgressState::Cancelled
                    | ReleaseProgressState::Skipped
            )
        })
        .count() as u64;

    let mut tasks = vec![ReleaseProgressTask::new(
        "Local verification",
        local_state,
        if local_children.is_empty() {
            ReleaseProgressUnits::default()
        } else {
            ReleaseProgressUnits::counted(local_completed, local_children.len() as u64)
        },
        local_children,
    )];
    if !remote_children.is_empty()
        || matches!(
            record.progress.phase,
            ReleaseProgressPhase::RemoteVerification | ReleaseProgressPhase::Publication
        )
    {
        tasks.push(ReleaseProgressTask::new(
            "Required CI",
            remote_state,
            if remote_children.is_empty() {
                ReleaseProgressUnits::default()
            } else {
                ReleaseProgressUnits::counted(remote_terminal, remote_children.len() as u64)
            },
            remote_children,
        ));
    }
    if matches!(
        record.progress.phase,
        ReleaseProgressPhase::FailureDiagnosis | ReleaseProgressPhase::Repair
    ) {
        tasks.push(ReleaseProgressTask::new(
            record.progress.phase.label(),
            stage_state(record, record.progress.phase),
            ReleaseProgressUnits::default(),
            Vec::new(),
        ));
    }
    if matches!(
        record.progress.phase,
        ReleaseProgressPhase::Publication | ReleaseProgressPhase::Settled
    ) {
        tasks.push(ReleaseProgressTask::new(
            "Publication",
            publication_progress_state(record),
            ReleaseProgressUnits::default(),
            Vec::new(),
        ));
    }
    tasks
}

fn progress_state_from_settlement(state: SettlementState) -> ReleaseProgressState {
    match state {
        SettlementState::NotStarted => ReleaseProgressState::Pending,
        SettlementState::Running => ReleaseProgressState::Running,
        SettlementState::Succeeded => ReleaseProgressState::Passed,
        SettlementState::Failed => ReleaseProgressState::Failed,
    }
}

fn progress_state_from_job(state: JobState) -> ReleaseProgressState {
    match state {
        JobState::Queued => ReleaseProgressState::Pending,
        JobState::InProgress => ReleaseProgressState::Running,
        JobState::Success => ReleaseProgressState::Passed,
        JobState::Failure | JobState::TimedOut => ReleaseProgressState::Failed,
        JobState::Cancelled => ReleaseProgressState::Cancelled,
        JobState::Skipped => ReleaseProgressState::Skipped,
    }
}

fn aggregate_progress_state(
    states: impl Iterator<Item = ReleaseProgressState>,
) -> ReleaseProgressState {
    let states = states.collect::<Vec<_>>();
    if states.is_empty()
        || states
            .iter()
            .all(|state| *state == ReleaseProgressState::Pending)
    {
        ReleaseProgressState::Pending
    } else if states.contains(&ReleaseProgressState::Failed) {
        ReleaseProgressState::Failed
    } else if states.contains(&ReleaseProgressState::Cancelled) {
        ReleaseProgressState::Cancelled
    } else if states.contains(&ReleaseProgressState::Running) {
        ReleaseProgressState::Running
    } else if states.contains(&ReleaseProgressState::Paused) {
        ReleaseProgressState::Paused
    } else if states
        .iter()
        .all(|state| *state == ReleaseProgressState::Skipped)
    {
        ReleaseProgressState::Skipped
    } else if states.iter().all(|state| {
        matches!(
            state,
            ReleaseProgressState::Passed | ReleaseProgressState::Skipped
        )
    }) {
        ReleaseProgressState::Passed
    } else {
        ReleaseProgressState::Pending
    }
}

fn stage_state(
    record: &ReleaseRecoveryRecord,
    phase: ReleaseProgressPhase,
) -> ReleaseProgressState {
    if record.state == ReleaseRecoveryState::Cancelled {
        return ReleaseProgressState::Cancelled;
    }
    if record.liveness.blocked() || record.liveness.state == ReleaseLivenessState::OwnerExited {
        return ReleaseProgressState::Paused;
    }
    if matches!(
        record.state,
        ReleaseRecoveryState::PausedExternal | ReleaseRecoveryState::Escalated
    ) {
        return ReleaseProgressState::Paused;
    }
    if record.progress.phase == phase {
        return ReleaseProgressState::Running;
    }
    if matches!(record.progress.phase, ReleaseProgressPhase::Settled)
        && matches!(
            record.state,
            ReleaseRecoveryState::Complete | ReleaseRecoveryState::Published
        )
    {
        return ReleaseProgressState::Passed;
    }
    ReleaseProgressState::Pending
}

fn publication_progress_state(record: &ReleaseRecoveryRecord) -> ReleaseProgressState {
    if record.state == ReleaseRecoveryState::Cancelled {
        ReleaseProgressState::Cancelled
    } else if record.mutation.publication_verified
        || matches!(
            record.state,
            ReleaseRecoveryState::Published | ReleaseRecoveryState::Complete
        )
    {
        ReleaseProgressState::Passed
    } else if record.liveness.blocked() {
        ReleaseProgressState::Paused
    } else if matches!(record.progress.phase, ReleaseProgressPhase::Publication) {
        ReleaseProgressState::Running
    } else {
        ReleaseProgressState::Pending
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
    Closeout,
}

/// Unforgeable capability for one release mutation. Construction remains in
/// the controller so native adapters cannot turn API/process access into
/// lifecycle authority.
#[derive(Debug)]
pub struct ReleaseMutationAdmission {
    kind: ReleaseMutationKind,
    candidate_commit: Option<String>,
    candidate_paths: Vec<String>,
    preparation_baseline_digest: Option<String>,
}

impl ReleaseMutationAdmission {
    pub(crate) fn preparation_baseline_digest(&self) -> Option<&str> {
        self.preparation_baseline_digest.as_deref()
    }
    pub(crate) fn candidate_paths(&self) -> &[String] {
        &self.candidate_paths
    }
    pub(crate) fn candidate_commit(&self) -> Option<&str> {
        self.candidate_commit.as_deref()
    }
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
            !record.objective.post_release_main_epoch
                && record.state == ReleaseRecoveryState::RemoteGatesGreen
                && record.matrix_complete()
                && record
                    .required_gates
                    .iter()
                    .all(|gate| Some(gate.head_sha.as_str()) == record.active_commit())
                && !record.required_gates.iter().any(GateRecord::has_failure)
                && record.mutation.candidate_pushed
                && !record.mutation.tag_pushed
        }
        ReleaseMutationKind::Publish => {
            record.state == ReleaseRecoveryState::Publishing
                && record.mutation.tag_pushed
                && !record.mutation.publication_verified
        }
        ReleaseMutationKind::Closeout => {
            record.state == ReleaseRecoveryState::Complete
                && record.mutation.publication_verified
                && record.mutation.closeout_receipt.is_none()
                && record.current_main.as_deref() == record.active_commit()
                && record.matrix_complete()
                && !record.required_gates.iter().any(GateRecord::has_failure)
        }
    };
    if !allowed {
        return Err(RrcError::MutationBlocked(format!(
            "{kind:?} is not admissible from {:?}",
            record.state
        )));
    }
    Ok(ReleaseMutationAdmission {
        kind,
        preparation_baseline_digest: if kind == ReleaseMutationKind::VersionBump
            && !record.mutation.repair_files.is_empty()
        {
            record
                .repair_attempts
                .last()
                .filter(|attempt| {
                    attempt.focused_status == FocusedProofStatus::Passed
                        && !attempt.disproven_or_insufficient
                        && attempt.source_commit_after.is_none()
                        && Some(attempt.source_commit_before.as_str()) == record.active_commit()
                })
                .and_then(|attempt| {
                    attempt.evidence_refs.iter().find_map(|value| {
                        value
                            .strip_prefix("repair:preparation-baseline:")
                            .filter(|digest| {
                                digest.len() == 64
                                    && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                            })
                            .map(str::to_owned)
                    })
                })
        } else {
            None
        },
        candidate_commit: record.active_commit().map(str::to_owned),
        candidate_paths: [
            vec![
                "Cargo.toml".into(),
                "Cargo.lock".into(),
                "registry/agent.json".into(),
            ],
            record.mutation.version_files.clone(),
            record.mutation.repair_files.clone(),
        ]
        .concat(),
    })
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
    WatchResourcePressure { after: Duration },
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
        S::ResourceDeferred => ReleaseDirective::WatchResourcePressure {
            after: Duration::from_secs(
                record
                    .resource_deferred
                    .as_ref()
                    .map_or(5, |deferred| deferred.next_check_seconds.max(1)),
            ),
        },
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
                    if is_account_execution_restriction(&normalize_failure_text(
                        &failure.causal_excerpt,
                    )) {
                        ReleaseDirective::Escalate
                    } else if failure.class.infrastructure_like() {
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
        S::Complete
            if record.mutation.publication_verified
                && record.mutation.closeout_receipt.is_none() =>
        {
            ReleaseDirective::RunPostReleaseCloseout
        }
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
    VerifiedRepairBatch(Vec<RepairAttempt>),
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

fn apply_verified_repair_batch(
    record: &mut ReleaseRecoveryRecord,
    commit: &str,
    repairs: Vec<RepairAttempt>,
) -> Result<(), RrcError> {
    if repairs.is_empty() {
        return Err(RrcError::Invalid(
            "verified repair batch must contain at least one repair".into(),
        ));
    }

    // Apply the batch to a clone so that a malformed later family cannot leave
    // the durable reducer partially advanced.
    let mut next = record.clone();
    let repaired_commit = repairs[0].source_commit_after.clone().ok_or_else(|| {
        RrcError::Invalid("verified repair must identify its changed commit".into())
    })?;
    let mut evidence_refs = Vec::new();
    for repair in &repairs {
        validate_repair_target(&next, repair)?;
        if repair.focused_status != FocusedProofStatus::Passed {
            return Err(RrcError::Invalid(
                "verified repair must contain a passed focused proof".into(),
            ));
        }
        if repair.source_commit_after.as_deref() != Some(repaired_commit.as_str()) {
            return Err(RrcError::Invalid(
                "verified repair batch must identify one shared changed commit".into(),
            ));
        }
        evidence_refs.extend(repair.evidence_refs.iter().cloned());
    }
    evidence_refs.sort();
    evidence_refs.dedup();

    next.transition(
        ReleaseRecoveryState::FocusedRepair,
        commit,
        "evidence-backed matrix repair prepared",
        evidence_refs.clone(),
        None,
    )?;
    next.transition(
        ReleaseRecoveryState::FocusedVerification,
        commit,
        "smallest credible focused proof executed for all classified failure families",
        evidence_refs,
        None,
    )?;
    for repair in repairs {
        next.record_repair(repair)?;
    }
    if next.objective.post_release_main_epoch {
        next.current_main = Some(repaired_commit.clone());
    } else {
        next.release_commit = Some(repaired_commit.clone());
    }
    // GitHub reruns preserve the original GITHUB_SHA. A source repair
    // therefore invalidates the prior push receipt and must be pushed
    // as a fresh exact-SHA candidate before another full gate set.
    next.mutation.candidate_pushed = false;
    next.mutation.candidate_push_ref = None;
    next.transition(
        ReleaseRecoveryState::RetryAdmissible,
        &repaired_commit,
        "focused proof passed after repairs for every classified failure family",
        vec!["rrc:focused-proof".into()],
        None,
    )?;
    *record = next;
    Ok(())
}

pub fn apply_controller_event(
    record: &mut ReleaseRecoveryRecord,
    event: ReleaseControllerEvent,
) -> Result<(), RrcError> {
    let mut next = record.clone();
    apply_controller_event_inner(&mut next, event)?;
    *record = next;
    Ok(())
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
    if repair.hypothesis.trim().is_empty()
        || repair.focused_proof.trim().is_empty()
        || repair.evidence_refs.is_empty()
        || repair.causal_family.trim().is_empty()
    {
        return Err(RrcError::Invalid(
            "repair hypothesis, focused proof, causal family and evidence are required".into(),
        ));
    }
    if record.active_commit() != Some(repair.source_commit_before.as_str())
        || (repair.focused_status == FocusedProofStatus::Passed
            && (repair.source_commit_after.as_deref().is_none_or(|after| {
                after.trim().is_empty() || after == repair.source_commit_before
            }) || repair.disproven_or_insufficient))
    {
        return Err(RrcError::Invalid(
            "repair must change the current source commit".into(),
        ));
    }
    let active_commit = record.active_commit();
    let failure = record
        .failures
        .iter()
        .rev()
        .find(|failure| {
            failure.fingerprint == repair.fingerprint
                && Some(failure.source_commit.as_str()) == active_commit
        })
        .ok_or_else(|| {
            RrcError::Invalid("repair fingerprint does not target a current failure".into())
        })?;
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
        && evidence.source_explanation_absent
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
    match unchanged_polls {
        0 => INITIAL_POLL_INTERVAL,
        1 => SECOND_POLL_INTERVAL,
        2 => THIRD_POLL_INTERVAL,
        _ => MAX_POLL_INTERVAL,
    }
}

#[must_use]
pub fn legal_transition(from: ReleaseRecoveryState, to: ReleaseRecoveryState) -> bool {
    use ReleaseRecoveryState as S;
    if to == S::RemoteGateRunning
        && matches!(
            from,
            S::ClassifyingFailure
                | S::DiagnosingRepair
                | S::CollectingFailureEvidence
                | S::RemoteGatesGreen
        )
    {
        return true;
    }
    if from == S::Cancelled && !matches!(to, S::Idle | S::Cancelled | S::Preparing) {
        return true;
    }
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
                S::ResourceDeferred | S::DiagnosingLocalFailure | S::CandidateReady
            )
            | (S::ResourceDeferred, S::LocalVerification)
            | (
                S::DiagnosingLocalFailure,
                S::LocalVerification | S::Escalated | S::NeedMoreEvidence
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
                S::Complete | S::PostReleaseMainDegraded | S::WaitingForMatrix
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
    /// Source validation failed after the version transaction was restored and
    /// the exact approved preparation baseline was reobserved.
    #[error("version preparation failed after verified rollback: {0}")]
    VersionPreparationFailed(String),
    #[error("stale workflow result: expected {expected}, observed {observed}")]
    StaleWorkflow { expected: String, observed: String },
    #[error("retry blocked: {0}")]
    RetryBlocked(String),
    #[error("release mutation blocked: {0}")]
    MutationBlocked(String),
    /// Local host resource safety is neither a source failure nor external
    /// CI evidence. The epoch remains locally resumable after recovery.
    #[error("local resource governor deferred verification: {0}")]
    ResourceConstrained(String),
    #[error("repair budget exhausted for causal family {0}")]
    RepairBudgetExhausted(String),
    /// A controller-owned operation stopped because it produced no observable
    /// progress inside its state-specific inactivity budget. This is a
    /// liveness failure, not source, CI, or publication evidence.
    #[error("release watchdog stopped {operation} after {limit_seconds}s without progress")]
    WatchdogStalled {
        operation: String,
        limit_seconds: u64,
    },
    /// A controller-owned operation exceeded its absolute safety ceiling even
    /// though it may have continued to emit output.
    #[error("release watchdog stopped {operation} after its {limit_seconds}s execution deadline")]
    WatchdogDeadline {
        operation: String,
        limit_seconds: u64,
    },
    /// User cancellation is typed separately from watchdog and command
    /// failure so persistence and both host projections cannot mislabel it.
    #[error("release recovery was cancelled by the user")]
    Cancelled,
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
    // Context before the actual error is useful evidence but not causal identity.
    let lines = causal_line.lines().collect::<Vec<_>>();
    let signature = lines
        .iter()
        .position(|line| is_causal_diagnostic(line))
        .map(|index| {
            lines[index..]
                .iter()
                .filter(|line| {
                    let normalized = normalize_failure_text(line);
                    // Interleaving stderr/stdout may reorder unrelated authorization
                    // output and generic runner exit wrappers after the same error.
                    !normalized.contains("process completed with exit code")
                        && (is_causal_diagnostic(line)
                            || normalized.starts_with("left:")
                            || normalized.starts_with("right:")
                            || normalized.starts_with("expected:")
                            || normalized.starts_with("actual:"))
                })
                .copied()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| causal_line.to_owned());
    let normalized = normalize_failure_text(&signature);
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
    static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
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
        (r"(\.rs):\d+(?::\d+)?", "$1:<line>"),
        (r"(thread '[^']+') \(\d+\)", "$1 (<pid>)"),
        (
            r"(?i)\bworker_\d{8}-\d{6}-utc\.log\b",
            "worker_<timestamp>.log",
        ),
    ].into_iter().map(|(pattern, replacement)| (Regex::new(pattern).expect("static normalization regex"), replacement)).collect()
    });
    for (regex, replacement) in PATTERNS.iter() {
        text = regex.replace_all(&text, *replacement).into_owned();
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[must_use]
pub fn redact_secrets(input: &str) -> String {
    bounded(redact_secret_text(input), MAX_CAUSAL_EXCERPT_BYTES)
}

#[must_use]
pub fn classify_failure(
    excerpt: &str,
    _platform: Option<&str>,
) -> (ReleaseFailureClass, EvidenceConfidence) {
    let normalized = normalize_failure_text(excerpt);
    let class = if normalized.contains("panicked at") || normalized.contains("assertion") {
        ReleaseFailureClass::TestRegression
    } else if normalized.contains("error[e")
        || normalized.contains("could not compile")
        || normalized.contains("linking with")
        || normalized.contains("undefined reference")
        || normalized.contains("undefined symbol")
        || normalized.contains("ld terminated with signal")
        || normalized.contains("llvm error:")
    {
        ReleaseFailureClass::CompileFailure
    } else if normalized.contains("permission denied")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
        || is_account_execution_restriction(&normalized)
    {
        ReleaseFailureClass::CredentialOrPermissionFailure
    } else if normalized.contains("rate limit") || normalized.contains("http 429") {
        ReleaseFailureClass::RateLimit
    } else if (normalized.contains("package") && normalized.contains("is not available"))
        || (normalized.contains("e: version") && normalized.contains("was not found"))
    {
        ReleaseFailureClass::DependencyFailure
    } else if normalized.contains("the operation was canceled")
        || normalized.contains("the operation was cancelled")
    {
        ReleaseFailureClass::Cancelled
    } else if (normalized.contains("runner")
        && (normalized.contains("lost") || normalized.contains("provision")))
        || normalized.contains("job was not acquired by runner")
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
    } else {
        ReleaseFailureClass::Unknown
    };
    let confidence = if class == ReleaseFailureClass::Unknown {
        EvidenceConfidence::Unknown
    } else if matches!(
        class,
        ReleaseFailureClass::TestRegression
            | ReleaseFailureClass::CompileFailure
            | ReleaseFailureClass::DependencyFailure
            | ReleaseFailureClass::CredentialOrPermissionFailure
            | ReleaseFailureClass::RateLimit
            | ReleaseFailureClass::RunnerInfrastructureFailure
            | ReleaseFailureClass::ArtifactInfrastructureFailure
    ) {
        EvidenceConfidence::StronglySupported
    } else {
        EvidenceConfidence::Tentative
    };
    (class, confidence)
}

#[must_use]
pub fn first_causal_excerpt(log: &str) -> String {
    let clean = redact_secret_text(log);
    let lines = clean.lines().collect::<Vec<_>>();
    let mut command_header = false;
    let index = lines
        .iter()
        .position(|line| {
            if line.contains("##[group]Run ") {
                command_header = true;
                return false;
            }
            if command_header {
                if line.contains("##[endgroup]") {
                    command_header = false;
                }
                return false;
            }
            is_causal_diagnostic(line)
        })
        .unwrap_or(0);
    // Reserve the excerpt for the diagnostic: a linker command immediately
    // before it can exceed the entire evidence cap.
    let start = if index > 0 && lines[index - 1].len() <= 512 {
        index - 1
    } else {
        index
    };
    let end = (index + 4).min(lines.len());
    redact_secrets(&lines[start..end].join("\n"))
}

#[derive(Debug, Clone)]
pub struct ReleaseLedger {
    root: PathBuf,
    repo_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SupersededEpochEvidence {
    superseded_at: DateTime<Utc>,
    reason: String,
    replacement_epoch_id: String,
    replacement_source_commit: String,
    record: ReleaseRecoveryRecord,
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

    fn superseded_path(&self, epoch_id: &str) -> PathBuf {
        self.root
            .join("superseded")
            .join(&self.repo_key)
            .join(format!("{epoch_id}.json"))
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
                let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
                redact_persisted_value(&mut value);
                let record: ReleaseRecoveryRecord = serde_json::from_value(value)?;
                if sha256_hex(record.repo_identity.as_bytes()) != self.repo_key {
                    return Err(RrcError::Invalid(
                        "checkpoint repository identity does not match ledger".into(),
                    ));
                }
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
        self.save_inner(record, true)
    }

    /// Publishes a checkpoint that the running controller will read before it
    /// spawns the next cargo child. The rename is visible immediately and this
    /// path does not flush the file or its directory, in this thread or another.
    /// A background `sync_all` still occupies the directory inode and stalled
    /// the next governed Cargo spawn on saturated CI disks. Authority
    /// transitions keep [`Self::save`].
    pub(crate) fn save_for_scheduling(
        &self,
        record: &ReleaseRecoveryRecord,
    ) -> Result<(), RrcError> {
        self.save_inner(record, false)
    }

    fn save_inner(
        &self,
        record: &ReleaseRecoveryRecord,
        sync_before_return: bool,
    ) -> Result<(), RrcError> {
        if sha256_hex(record.repo_identity.as_bytes()) != self.repo_key {
            return Err(RrcError::Invalid(
                "record repository does not match ledger".into(),
            ));
        }
        let mut owned = record.clone();
        if release_owner_writes_relinquished()
            && owned.liveness.state == ReleaseLivenessState::Active
            && owned.liveness.owner_pid == Some(std::process::id())
        {
            owned.liveness.state = ReleaseLivenessState::OwnerExited;
            owned.liveness.detail = "controller owner exited; epoch remains recoverable".into();
            owned.liveness.observed_at = Some(Utc::now());
            owned.refresh_progress();
        }
        let record = &owned;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())?;
        lock.try_lock_exclusive().map_err(|_| RrcError::Busy)?;
        if let Some(current) = self.load()? {
            if current.epoch_id != record.epoch_id {
                if record.created_at <= current.created_at {
                    return Err(RrcError::Invalid("superseded release epoch writer".into()));
                }
                let archive = self.root.join(format!(
                    "{}-{}.json",
                    self.repo_key,
                    sha256_hex(current.epoch_id.as_bytes())
                ));
                if !archive.exists() {
                    let mut value = serde_json::to_value(&current)?;
                    redact_persisted_value(&mut value);
                    let mut snapshot = tempfile::NamedTempFile::new_in(&self.root)?;
                    snapshot.write_all(&serde_json::to_vec_pretty(&value)?)?;
                    snapshot.as_file().sync_all()?;
                    snapshot
                        .persist_noclobber(archive)
                        .map_err(|error| error.error)?;
                }
            }
            if current.epoch_id == record.epoch_id
                && (current.transitions.len() > record.transitions.len()
                    || (current.state == ReleaseRecoveryState::Cancelled
                        && record.state != current.state
                        && record.transitions.len() <= current.transitions.len())
                    || current.updated_at > record.updated_at)
            {
                return Err(RrcError::Invalid("stale release checkpoint writer".into()));
            }
        }
        let mut value = serde_json::to_value(record)?;
        redact_persisted_value(&mut value);
        let bytes = serde_json::to_vec_pretty(&value)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(RrcError::Invalid(
                "release ledger exceeds the 4 MiB safety bound".into(),
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        temp.write_all(&bytes)?;
        if sync_before_return {
            temp.as_file().sync_all()?;
        }
        temp.persist(self.path()).map_err(|error| error.error)?;
        if sync_before_return && let Ok(directory) = OpenOptions::new().read(true).open(&self.root)
        {
            let _ = directory.sync_all();
        }
        FileExt::unlock(&lock)?;
        Ok(())
    }

    fn supersede_and_save(
        &self,
        previous: &ReleaseRecoveryRecord,
        reason: &str,
        replacement: &ReleaseRecoveryRecord,
    ) -> Result<(), RrcError> {
        if sha256_hex(previous.repo_identity.as_bytes()) != self.repo_key
            || sha256_hex(replacement.repo_identity.as_bytes()) != self.repo_key
        {
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
        let current: ReleaseRecoveryRecord = serde_json::from_slice(&fs::read(self.path())?)?;
        if current.epoch_id != previous.epoch_id || current.updated_at != previous.updated_at {
            FileExt::unlock(&lock)?;
            return Err(RrcError::Busy);
        }

        let directory = self.root.join("superseded").join(&self.repo_key);
        fs::create_dir_all(&directory)?;
        let archive_path = self.superseded_path(&previous.epoch_id);
        if !archive_path.exists() {
            let evidence = SupersededEpochEvidence {
                superseded_at: Utc::now(),
                reason: bounded(reason.to_owned(), 512),
                replacement_epoch_id: replacement.epoch_id.clone(),
                replacement_source_commit: replacement
                    .mutation
                    .source_commit
                    .clone()
                    .unwrap_or_default(),
                record: previous.clone(),
            };
            let mut archive = tempfile::NamedTempFile::new_in(&directory)?;
            let mut value = serde_json::to_value(&evidence)?;
            redact_persisted_value(&mut value);
            archive.write_all(&serde_json::to_vec_pretty(&value)?)?;
            archive.as_file().sync_all()?;
            archive
                .persist(&archive_path)
                .map_err(|error| error.error)?;
        }

        let mut active = tempfile::NamedTempFile::new_in(&self.root)?;
        let mut value = serde_json::to_value(replacement)?;
        redact_persisted_value(&mut value);
        active.write_all(&serde_json::to_vec_pretty(&value)?)?;
        active.as_file().sync_all()?;
        active.persist(self.path()).map_err(|error| error.error)?;
        if let Ok(directory) = OpenOptions::new().read(true).open(&directory) {
            let _ = directory.sync_all();
        }
        if let Ok(directory) = OpenOptions::new().read(true).open(&self.root) {
            let _ = directory.sync_all();
        }
        FileExt::unlock(&lock)?;
        Ok(())
    }

    pub(crate) fn acquire_owner(&self) -> Result<std::fs::File, RrcError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(format!("{}.owner", self.repo_key)))?;
        file.try_lock_exclusive().map_err(|_| RrcError::Busy)?;
        Ok(file)
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
        record.required_gates.clear();
        record.note_progress(false);
        if record.state != ReleaseRecoveryState::WaitingForMatrix {
            record.transition(
                ReleaseRecoveryState::WaitingForMatrix,
                &candidate,
                "required workflow runs have not appeared for the exact candidate SHA",
                vec!["github:exact-sha-matrix".into()],
                None,
            )?;
        }
        if record.remote_matrix_watch_timed_out(Utc::now()) {
            record.transition(
                ReleaseRecoveryState::Escalated,
                &candidate,
                "remote matrix watch timed out before required workflow runs appeared",
                vec!["rrc:stagnation-watchdog".into()],
                None,
            )?;
        }
        return Ok(());
    }
    let mut changed = false;
    record.required_gates.retain(|old| {
        gates
            .iter()
            .any(|gate| gate.name.eq_ignore_ascii_case(&old.name))
    });
    for gate in gates {
        if record
            .retry_run_floors
            .iter()
            .any(|(run, floor)| Some(*run) == gate.run_id && gate.run_attempt.unwrap_or(0) < *floor)
        {
            continue;
        }
        changed |= record.apply_matrix(gate)?;
    }
    if changed {
        record.refresh_progress();
        record.note_progress_milestone(format!(
            "Remote matrix update: {}/{} jobs terminal",
            record.progress.terminal_remote_jobs, record.progress.total_remote_jobs
        ));
    }
    record.note_progress(changed);
    // Passive GitHub refreshes are not substantial recovery actions. Their
    // poll count drives bounded backoff, but must not exhaust the six-action
    // repair watchdog while ordinary matrix jobs are still running.
    if record.remote_matrix_watch_timed_out(Utc::now()) {
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
        .filter(|job| job.state != JobState::Success && job.state.terminal())
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
            for repair in record
                .repair_attempts
                .iter_mut()
                .filter(|repair| repair.fingerprint == fingerprint)
            {
                repair.disproven_or_insufficient = true;
            }
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
        .iter()
        .filter(|failure| {
            Some(failure.source_commit.as_str()) == record.active_commit()
                && failure.exists_on_last_green.is_none()
        })
        .cloned()
        .collect::<Vec<_>>();
    if current.is_empty() {
        return Ok(());
    }
    let gates = adapter.matrix_for_sha(repository, last_green_commit)?;
    record.last_green_release_commit = Some(last_green_commit.to_owned());
    if gates.iter().any(|gate| gate.head_sha != last_green_commit) {
        return Err(RrcError::Invalid(
            "last-green evidence has a stale SHA".into(),
        ));
    }
    for failure in current {
        let gate = gates
            .iter()
            .find(|gate| gate.name == failure.workflow_name)
            .filter(|gate| gate.matrix_complete())
            .ok_or_else(|| {
                RrcError::Invalid("last-green workflow evidence missing or incomplete".into())
            })?;
        let matching_jobs = gate
            .jobs
            .iter()
            .filter(|job| job.job_name == failure.job_name && job.platform == failure.platform)
            .collect::<Vec<_>>();
        if matching_jobs.is_empty()
            || matching_jobs
                .iter()
                .any(|job| job.state == JobState::Skipped)
        {
            return Err(RrcError::Invalid(
                "last-green matching platform/job evidence missing or skipped; comparison remains unknown".into(),
            ));
        }
        let mut existed = false;
        for job in matching_jobs.into_iter().filter(|job| {
            job.state.failed()
                && job.job_name == failure.job_name
                && job.platform == failure.platform
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
            if fingerprint == failure.fingerprint {
                existed = true;
                break;
            }
        }
        record.record_failure_context(&failure.fingerprint, existed, related_source_touched)?;
    }
    Ok(())
}

pub trait GitHubEvidencePort: Send + Sync {
    fn current_main_commit(&self, _repository: &str) -> Result<Option<String>, RrcError> {
        Ok(None)
    }
    fn matrix_for_sha(&self, repository: &str, head_sha: &str)
    -> Result<Vec<GateRecord>, RrcError>;
    fn job_log(&self, repository: &str, job_id: u64) -> Result<String, RrcError>;
    fn rerun_job(&self, repository: &str, job_id: u64) -> Result<(), RrcError>;
    fn rerun_failed(&self, repository: &str, run_id: u64) -> Result<(), RrcError>;
    fn rerun_workflow(&self, repository: &str, run_id: u64) -> Result<(), RrcError>;
    fn rerun_failed_admitted(
        &self,
        _repository: &str,
        _run_id: u64,
        _token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        Err(RrcError::MutationBlocked(
            "failed-job retry requires an admitted adapter implementation".into(),
        ))
    }
    fn rerun_admitted(&self, repository: &str, token: RetryAdmissionToken) -> Result<(), RrcError> {
        for run in token.run_ids {
            self.rerun_workflow(repository, run)?;
        }
        Ok(())
    }
}

/// A mutation capability issued only after [`ReleaseRecoveryRecord::consume_retry`].
/// Keeping construction private prevents adapters from treating API access as policy authority.
#[derive(Debug)]
pub struct RetryAdmissionToken {
    kind: RetryKind,
    run_ids: Vec<u64>,
    job_ids: Vec<u64>,
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
    let gates = record
        .required_gates
        .iter()
        .filter(|gate| kind != RetryKind::Infrastructure || gate.has_failure())
        .collect::<Vec<_>>();
    Ok(RetryAdmissionToken {
        kind,
        run_ids: gates.iter().filter_map(|gate| gate.run_id).collect(),
        job_ids: gates
            .iter()
            .flat_map(|gate| &gate.jobs)
            .filter(|job| job.state.failed())
            .map(|job| job.job_id)
            .collect(),
    })
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
    if record
        .required_gates
        .iter()
        .any(|gate| gate.head_sha != commit)
    {
        return Err(RrcError::RetryBlocked(
            "Actions reruns cannot verify a changed source SHA; push the repaired candidate".into(),
        ));
    }
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
    pub fn with_cancellation(
        &self,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> CancellableGhCliEvidenceAdapter {
        CancellableGhCliEvidenceAdapter { cancelled }
    }

    pub fn rerun_with_admission(
        &self,
        repository: &str,
        run_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        self.standalone()
            .rerun_with_admission(repository, run_id, token)
    }

    pub fn rerun_gate_set_with_admission(
        &self,
        repository: &str,
        run_ids: &[u64],
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        self.standalone()
            .rerun_gate_set_with_admission(repository, run_ids, token)
    }

    fn standalone(&self) -> CancellableGhCliEvidenceAdapter {
        self.with_cancellation(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
    }

    pub fn rerun_job_with_admission(
        &self,
        repository: &str,
        job_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        self.standalone()
            .rerun_job_with_admission(repository, job_id, token)
    }
}

impl GitHubEvidencePort for GhCliEvidenceAdapter {
    fn matrix_for_sha(
        &self,
        repository: &str,
        head_sha: &str,
    ) -> Result<Vec<GateRecord>, RrcError> {
        self.standalone().matrix_for_sha(repository, head_sha)
    }

    fn job_log(&self, repository: &str, job_id: u64) -> Result<String, RrcError> {
        self.standalone().job_log(repository, job_id)
    }

    fn rerun_job(&self, repository: &str, job_id: u64) -> Result<(), RrcError> {
        self.standalone().rerun_job(repository, job_id)
    }

    fn rerun_failed(&self, repository: &str, run_id: u64) -> Result<(), RrcError> {
        self.standalone().rerun_failed(repository, run_id)
    }

    fn rerun_workflow(&self, repository: &str, run_id: u64) -> Result<(), RrcError> {
        self.standalone().rerun_workflow(repository, run_id)
    }

    fn current_main_commit(&self, repository: &str) -> Result<Option<String>, RrcError> {
        self.standalone().current_main_commit(repository)
    }

    fn rerun_failed_admitted(
        &self,
        repository: &str,
        run_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        self.standalone()
            .rerun_failed_admitted(repository, run_id, token)
    }

    fn rerun_admitted(&self, repository: &str, token: RetryAdmissionToken) -> Result<(), RrcError> {
        self.standalone().rerun_admitted(repository, token)
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

pub fn repository_identity_for_workspace(workspace: &Path) -> Result<String, RrcError> {
    repository_identity(&workspace.canonicalize()?)
}

pub fn start_release(
    repo_identity: &str,
    target: &str,
    branch_ref: &str,
    source_commit: &str,
) -> Result<ReleaseRecoveryRecord, RrcError> {
    let version = ReleaseVersionSelector::parse(target).map_err(RrcError::Invalid)?;
    let mut record = ReleaseRecoveryRecord::new(
        repo_identity.to_owned(),
        format!(
            "{}-{}",
            Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
            short_sha(source_commit)
        ),
        ReleaseObjective {
            version,
            branch_ref: branch_ref.into(),
            request: None,
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
            let persisted = ledger.load()?.expect("checked").render_status();
            let live = crate::release_executor::active_release_worker(repo_identity)
                .map(|worker| crate::release_executor::render_active_worker_status(&worker));
            Ok(live.map_or(persisted.clone(), |live| format!("{persisted}\n{live}")))
        }
        "resume" => {
            let _owner = ledger.acquire_owner()?;
            let mut record = ledger
                .load()?
                .ok_or_else(|| RrcError::Invalid("no release checkpoint exists".into()))?;
            record.resume_cancelled()?;
            ledger.save(&record)?;
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
            record.liveness = ReleaseLivenessRecord::default();
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
            let mut lines = vec![format!(
                "release evidence: {} failure(s)",
                record.failures.len()
            )];
            if let Some(objective) = record.mutation.objective_label.as_deref() {
                lines.push(format!("  objective: {objective}"));
            }
            if let Some(objective_id) = record.mutation.objective_id.as_deref() {
                lines.push(format!("  objective id: {objective_id}"));
            }
            if let Some(workspace) = record.mutation.source_workspace.as_deref() {
                lines.push(format!("  source workspace: {workspace}"));
            }
            if let Some(workspace) = record.mutation.release_workspace.as_deref() {
                lines.push(format!("  release workspace: {workspace}"));
            }
            if let Some(base) = record.mutation.base_sha.as_deref() {
                lines.push(format!("  base SHA: {base}"));
            }
            if let Some(candidate) = record.mutation.final_candidate_commit.as_deref() {
                lines.push(format!("  candidate SHA: {candidate}"));
            }
            if let Some(diff) = record.mutation.intended_diff_sha256.as_deref() {
                lines.push(format!("  intended diff SHA-256: {diff}"));
            }
            for report in &record.mutation.objective_evidence_reports {
                lines.push(format!("  objective evidence: {report}"));
            }
            if record.failures.is_empty() {
                return Ok(lines.join("\n"));
            }
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
                lines.push(format!(
                    "    source {} · last-green {:?} · related source {:?}\n{}",
                    short_sha(&failure.source_commit),
                    failure.exists_on_last_green,
                    failure.related_source_touched,
                    redact_secrets(&failure.causal_excerpt)
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
            let _owner = ledger.acquire_owner()?;
            if let Some(existing) = ledger.load()?
                && !matches!(
                    existing.state,
                    ReleaseRecoveryState::Complete
                        | ReleaseRecoveryState::Cancelled
                        | ReleaseRecoveryState::Published
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

#[derive(Debug, Clone)]
struct ReleaseSourceChoice {
    source_workspace: PathBuf,
    source_commit: String,
    objective_id: Option<String>,
    objective_label: Option<String>,
    variant_label: Option<String>,
    variant_id: Option<String>,
    canonical_release_source: bool,
    supersedes: Vec<String>,
    evidence_reports: Vec<String>,
    base_sha: String,
    intended_commits: Vec<String>,
    intended_diff_sha256: String,
    branch_ref: String,
}

fn git_output(workspace: &Path, args: &[&str]) -> Result<String, RrcError> {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(workspace);
    let output = crate::release_executor::run_bounded_external_command(
        &mut command,
        &AtomicBool::new(false),
        crate::release_executor::MUTATION_WATCHDOG,
    )?;
    if !output.status.success() {
        return Err(RrcError::Invalid(format!(
            "git {} failed with status {}: {}",
            args.join(" "),
            output.status,
            bounded(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                512
            )
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_output_bytes(workspace: &Path, args: &[&str]) -> Result<Vec<u8>, RrcError> {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(workspace);
    let output = crate::release_executor::run_bounded_external_command(
        &mut command,
        &AtomicBool::new(false),
        crate::release_executor::MUTATION_WATCHDOG,
    )?;
    if !output.status.success() {
        return Err(RrcError::Invalid(format!(
            "git {} failed with status {}: {}",
            args.join(" "),
            output.status,
            bounded(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                512
            )
        )));
    }
    Ok(output.stdout)
}

fn git_succeeds(workspace: &Path, args: &[&str]) -> bool {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(workspace);
    crate::release_executor::run_bounded_external_command(
        &mut command,
        &AtomicBool::new(false),
        crate::release_executor::MUTATION_WATCHDOG,
    )
    .is_ok_and(|output| output.status.success())
}

fn exact_git_sha(value: &str) -> Result<String, RrcError> {
    let value = value.trim();
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(RrcError::Invalid(format!(
            "expected an exact 40-character git SHA, got {value:?}"
        )))
    }
}

fn repository_identity(workspace: &Path) -> Result<String, RrcError> {
    let common = PathBuf::from(git_output(workspace, &["rev-parse", "--git-common-dir"])?);
    let common = if common.is_absolute() {
        common
    } else {
        workspace.join(common)
    };
    Ok(common.canonicalize()?.to_string_lossy().into_owned())
}

fn worktree_paths(workspace: &Path) -> Result<Vec<PathBuf>, RrcError> {
    let output = git_output(workspace, &["worktree", "list", "--porcelain"])?;
    Ok(output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect())
}

const RELEASE_OBJECTIVE_PROVENANCE_PATH: &str = "docs/foundation/release-objective-provenance.json";

#[derive(Debug, Clone, Deserialize)]
struct CompletedObjectiveProvenance {
    version: u32,
    objective_id: String,
    objective_label: String,
    variant_label: String,
    #[serde(default)]
    variant_id: Option<String>,
    completed_at: String,
    #[serde(default)]
    evidence_reports: Vec<String>,
    #[serde(default)]
    canonical_release_source: bool,
    #[serde(default)]
    supersedes: Vec<String>,
}

impl CompletedObjectiveProvenance {
    fn load(candidate: &Path) -> Option<Self> {
        let marker_spec = format!("HEAD:{RELEASE_OBJECTIVE_PROVENANCE_PATH}");
        let bytes = git_output_bytes(candidate, &["show", &marker_spec]).ok()?;
        if bytes.len() > 32 * 1024 {
            return None;
        }
        let value: Self = serde_json::from_slice(&bytes).ok()?;
        if value.version != 1
            || value.objective_id.is_empty()
            || value.objective_id.len() > 128
            || value.objective_label.is_empty()
            || value.objective_label.len() > 160
            || value.variant_label.is_empty()
            || value.variant_label.len() > 200
            || value
                .variant_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 128)
            || value.supersedes.len() > 32
            || value
                .supersedes
                .iter()
                .any(|id| id.is_empty() || id.len() > 128)
            || chrono::DateTime::parse_from_rfc3339(&value.completed_at).is_err()
            || value.evidence_reports.is_empty()
            || value.evidence_reports.len() > 16
        {
            return None;
        }
        let safe_report = |report: &str| {
            report.starts_with("docs/foundation/")
                && report.ends_with(".md")
                && !report.contains("..")
                && !report.contains('\\')
                && git_succeeds(candidate, &["cat-file", "-e", &format!("HEAD:{report}")])
        };
        value
            .evidence_reports
            .iter()
            .all(|report| safe_report(report))
            .then_some(value)
    }

    fn identity(&self) -> &str {
        self.variant_id.as_deref().unwrap_or(&self.variant_label)
    }

    fn explicitly_supersedes(&self, older: &Self) -> bool {
        self.supersedes
            .iter()
            .any(|id| id == older.identity() || id == &older.objective_id)
    }
}

#[derive(Debug, Clone)]
struct ProvenanceCandidate {
    path: PathBuf,
    head: String,
    tree: String,
    subject: String,
    implementation_diff_sha256: String,
    provenance: CompletedObjectiveProvenance,
}

fn implementation_diff_identity(
    active: &Path,
    base_sha: &str,
    head: &str,
    provenance_paths: &std::collections::BTreeSet<String>,
) -> Result<String, RrcError> {
    let mut args = vec![
        "diff".to_owned(),
        "--binary".to_owned(),
        base_sha.to_owned(),
        head.to_owned(),
        "--".to_owned(),
        ".".to_owned(),
        format!(":(exclude){RELEASE_OBJECTIVE_PROVENANCE_PATH}"),
        ":(exclude)docs/foundation/evidence-index.md".to_owned(),
        ":(exclude)docs/foundation/AGENTS.md".to_owned(),
    ];
    args.extend(
        provenance_paths
            .iter()
            .map(|path| format!(":(exclude){path}")),
    );
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    Ok(sha256_hex(&git_output_bytes(active, &refs)?))
}

fn resolve_release_source(
    active: &Path,
) -> Result<Result<ReleaseSourceChoice, Vec<String>>, RrcError> {
    let active = active.canonicalize()?;
    let base_sha = exact_git_sha(git_output(&active, &["rev-parse", "HEAD"])?.trim())?;
    let active_clean = git_output(&active, &["status", "--porcelain"])?.is_empty();
    let mut candidates = Vec::<ProvenanceCandidate>::new();
    for path in worktree_paths(&active)? {
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        if !git_output(&path, &["status", "--porcelain"])?.is_empty() {
            continue;
        }
        let head = exact_git_sha(git_output(&path, &["rev-parse", "HEAD"])?.trim())?;
        if head != base_sha
            && !git_succeeds(&active, &["merge-base", "--is-ancestor", &base_sha, &head])
        {
            continue;
        }
        let Some(provenance) = CompletedObjectiveProvenance::load(&path) else {
            // A clean checkout without an objective binding is repository
            // topology, not an objective release candidate.
            continue;
        };
        let tree = exact_git_sha(git_output(&path, &["rev-parse", "HEAD^{tree}"])?.trim())?;
        let subject = bounded(
            redact_secrets(&git_output(&path, &["show", "-s", "--format=%s", "HEAD"])?),
            120,
        );
        candidates.push(ProvenanceCandidate {
            path,
            head,
            tree,
            subject,
            implementation_diff_sha256: String::new(),
            provenance,
        });
    }
    let provenance_paths = candidates
        .iter()
        .flat_map(|candidate| candidate.provenance.evidence_reports.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    for candidate in &mut candidates {
        candidate.implementation_diff_sha256 =
            implementation_diff_identity(&active, &base_sha, &candidate.head, &provenance_paths)?;
    }
    let (source_workspace, provenance) = if candidates.is_empty() {
        if active_clean {
            (active.clone(), CompletedObjectiveProvenance::load(&active))
        } else {
            return Ok(Err(Vec::new()));
        }
    } else {
        // A final-integration marker is an explicit authoritative binding. It
        // prevents retained historical worktrees from competing merely because
        // they still carry an older completed-objective record.
        if candidates
            .iter()
            .any(|candidate| candidate.provenance.canonical_release_source)
        {
            candidates.retain(|candidate| candidate.provenance.canonical_release_source);
        }
        let latest_completed_at = candidates
            .iter()
            .map(|candidate| candidate.provenance.completed_at.as_str())
            .max()
            .expect("non-empty objective candidates")
            .to_owned();
        candidates.retain(|candidate| candidate.provenance.completed_at == latest_completed_at);
        let objective_ids = candidates
            .iter()
            .map(|candidate| candidate.provenance.objective_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if objective_ids.len() != 1 {
            return Err(RrcError::Invalid(
                "completed-objective provenance has multiple current objective identities; repair the provenance binding before release"
                    .into(),
            ));
        }

        let candidate_snapshot = candidates.clone();
        candidates.retain(|candidate| {
            !candidate_snapshot.iter().any(|other| {
                candidate.head != other.head
                    && other
                        .provenance
                        .explicitly_supersedes(&candidate.provenance)
                    && git_succeeds(
                        &active,
                        &["merge-base", "--is-ancestor", &candidate.head, &other.head],
                    )
            })
        });
        let candidate_heads = candidates
            .iter()
            .map(|candidate| candidate.head.clone())
            .collect::<Vec<_>>();
        candidates.retain(|candidate| {
            !candidate_heads.iter().any(|other| {
                candidate.head != *other
                    && git_succeeds(
                        &active,
                        &["merge-base", "--is-ancestor", &candidate.head, other],
                    )
            })
        });

        // Commit identity is not content identity. Collapse equal trees first,
        // then equal implementation diffs after removing only bounded provenance
        // bookkeeping paths. This keeps real package/version/source differences.
        let mut by_tree = std::collections::BTreeMap::<String, Vec<ProvenanceCandidate>>::new();
        for candidate in candidates {
            by_tree
                .entry(candidate.tree.clone())
                .or_default()
                .push(candidate);
        }
        let mut tree_representatives = Vec::new();
        for (_, mut variants) in by_tree {
            variants.sort_by(|left, right| left.path.cmp(&right.path));
            tree_representatives.push(variants.remove(0));
        }
        let mut by_implementation =
            std::collections::BTreeMap::<String, Vec<ProvenanceCandidate>>::new();
        for candidate in tree_representatives {
            by_implementation
                .entry(candidate.implementation_diff_sha256.clone())
                .or_default()
                .push(candidate);
        }
        let mut representatives = Vec::new();
        for (_, mut variants) in by_implementation {
            variants.sort_by(|left, right| left.path.cmp(&right.path));
            representatives.push(variants.remove(0));
        }
        representatives.sort_by(|left, right| left.path.cmp(&right.path));
        if representatives.len() != 1 {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for candidate in &representatives {
                *counts
                    .entry(candidate.provenance.variant_label.clone())
                    .or_default() += 1;
            }
            let choices = representatives
                .iter()
                .map(|candidate| {
                    let label = &candidate.provenance.variant_label;
                    if counts.get(label).copied().unwrap_or_default() > 1 {
                        format!("{label} — {}", candidate.subject)
                    } else {
                        label.clone()
                    }
                })
                .collect::<Vec<_>>();
            return Ok(Err(choices));
        }
        let candidate = representatives.remove(0);
        (candidate.path, Some(candidate.provenance))
    };
    let source_commit =
        exact_git_sha(git_output(&source_workspace, &["rev-parse", "HEAD"])?.trim())?;
    let branch_ref = git_output(&source_workspace, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let intended_commits = if source_commit == base_sha {
        Vec::new()
    } else {
        git_output(
            &source_workspace,
            &[
                "rev-list",
                "--reverse",
                &format!("{base_sha}..{source_commit}"),
            ],
        )?
        .lines()
        .map(str::to_owned)
        .collect()
    };
    let diff = git_output_bytes(
        &source_workspace,
        &["diff", "--binary", &base_sha, &source_commit],
    )?;
    Ok(Ok(ReleaseSourceChoice {
        source_workspace,
        source_commit,
        objective_id: provenance.as_ref().map(|value| value.objective_id.clone()),
        objective_label: provenance
            .as_ref()
            .map(|value| value.objective_label.clone()),
        variant_label: provenance.as_ref().map(|value| value.variant_label.clone()),
        variant_id: provenance
            .as_ref()
            .and_then(|value| value.variant_id.clone()),
        canonical_release_source: provenance
            .as_ref()
            .is_some_and(|value| value.canonical_release_source),
        supersedes: provenance
            .as_ref()
            .map(|value| value.supersedes.clone())
            .unwrap_or_default(),
        evidence_reports: provenance
            .as_ref()
            .map(|value| value.evidence_reports.clone())
            .unwrap_or_default(),
        base_sha,
        intended_commits,
        intended_diff_sha256: sha256_hex(&diff),
        branch_ref,
    }))
}

fn create_release_worktree(
    source: &ReleaseSourceChoice,
    state_root: &Path,
    repo_identity: &str,
) -> Result<PathBuf, RrcError> {
    let parent = state_root
        .join("worktrees")
        .join(&sha256_hex(repo_identity.as_bytes())[..16]);
    fs::create_dir_all(&parent)?;
    let destination = parent.join(format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%3fZ"),
        short_sha(&source.source_commit)
    ));
    let destination_text = destination.to_string_lossy().into_owned();
    git_output(
        &source.source_workspace,
        &[
            "worktree",
            "add",
            "--detach",
            &destination_text,
            &source.source_commit,
        ],
    )?;
    let destination = destination.canonicalize()?;
    let head = exact_git_sha(git_output(&destination, &["rev-parse", "HEAD"])?.trim())?;
    if head != source.source_commit
        || !git_output(&destination, &["status", "--porcelain"])?.is_empty()
    {
        return Err(RrcError::Invalid(
            "isolated release worktree did not settle at the intended clean source".into(),
        ));
    }
    Ok(destination)
}

#[derive(Debug)]
enum ActiveEpochReconciliation {
    Resume {
        record: ReleaseRecoveryRecord,
        workspace: PathBuf,
    },
    Supersede {
        record: ReleaseRecoveryRecord,
        reason: String,
    },
    Clarify(String),
}

fn has_publication_boundary(record: &ReleaseRecoveryRecord) -> bool {
    record.mutation.tag_pushed
        || record.mutation.publication_verified
        || !record.mutation.published_asset_names.is_empty()
        || matches!(
            record.state,
            ReleaseRecoveryState::Published
                | ReleaseRecoveryState::PostReleaseCloseout
                | ReleaseRecoveryState::PostReleaseMainDegraded
        )
}

fn has_historical_candidate_or_ci_evidence(record: &ReleaseRecoveryRecord) -> bool {
    let persisted_remote_gate_evidence = record.required_gates.iter().any(|gate| {
        gate.run_id.is_some()
            || gate.run_attempt.is_some()
            || !gate.jobs.is_empty()
            || gate.url.is_some()
    });
    record.mutation.candidate_pushed
        || record.mutation.candidate_push_ref.is_some()
        || record.mutation.publication_run_id.is_some()
        || persisted_remote_gate_evidence
        || !record.failures.is_empty()
        || matches!(
            record.state,
            ReleaseRecoveryState::RemoteGateRunning
                | ReleaseRecoveryState::WaitingForMatrix
                | ReleaseRecoveryState::CollectingFailureEvidence
                | ReleaseRecoveryState::ClassifyingFailure
                | ReleaseRecoveryState::NeedMoreEvidence
                | ReleaseRecoveryState::FocusedRepair
                | ReleaseRecoveryState::FocusedVerification
                | ReleaseRecoveryState::DiagnosingRepair
                | ReleaseRecoveryState::RetryAdmissible
                | ReleaseRecoveryState::ExternalHealthCheck
                | ReleaseRecoveryState::PausedExternal
                | ReleaseRecoveryState::RemoteGatesGreen
                | ReleaseRecoveryState::Tagging
                | ReleaseRecoveryState::Publishing
        )
}

fn same_release_objective(
    record: &ReleaseRecoveryRecord,
    source: &ReleaseSourceChoice,
) -> Option<bool> {
    match (
        record.mutation.objective_id.as_deref(),
        source.objective_id.as_deref(),
    ) {
        (Some(previous), Some(current)) => Some(previous == current),
        (None, None)
            if record.mutation.source_commit.as_deref() == Some(source.source_commit.as_str())
                && record.mutation.intended_diff_sha256.as_deref()
                    == Some(source.intended_diff_sha256.as_str()) =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn source_explicitly_supersedes_epoch(
    record: &ReleaseRecoveryRecord,
    source: &ReleaseSourceChoice,
) -> bool {
    if !source.canonical_release_source {
        return false;
    }
    [
        record.mutation.objective_id.as_deref(),
        record.mutation.source_variant_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|identity| source.supersedes.iter().any(|older| older == identity))
}

fn active_objective_description(record: &ReleaseRecoveryRecord) -> String {
    let label = record
        .mutation
        .objective_label
        .as_deref()
        .unwrap_or("an earlier release objective");
    let transition = match (
        record.mutation.version_before.as_deref(),
        record.mutation.version_after.as_deref(),
    ) {
        (Some(before), Some(after)) => format!("{before} -> {after}"),
        _ => record.objective.version.target_label(),
    };
    bounded(
        redact_secrets(&format!("{transition} release for {label}")),
        240,
    )
}

fn workspace_version_for_release_source(workspace: &Path) -> Option<String> {
    let manifest = fs::read_to_string(workspace.join("Cargo.toml")).ok()?;
    let mut workspace_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            workspace_package = trimmed == "[workspace.package]";
        } else if workspace_package && trimmed.starts_with("version = ") {
            return trimmed.split('"').nth(1).map(str::to_owned);
        }
    }
    None
}

fn resolved_target_from_selector(
    version: &ReleaseVersionSelector,
    workspace_version: Option<&str>,
) -> Option<String> {
    if let ReleaseVersionSelector::Exact(target) = version {
        return Some(target.clone());
    }
    let [major, minor, patch] = stable_semver_components(workspace_version?)?;
    match version {
        ReleaseVersionSelector::Patch => Some(format!("{major}.{minor}.{}", patch.checked_add(1)?)),
        ReleaseVersionSelector::Minor => Some(format!("{major}.{}.0", minor.checked_add(1)?)),
        ReleaseVersionSelector::Major => Some(format!("{}.0.0", major.checked_add(1)?)),
        ReleaseVersionSelector::Exact(_) => unreachable!("handled above"),
    }
}

fn resolved_record_release_target(record: &ReleaseRecoveryRecord) -> Option<String> {
    if let Some(target) = record.mutation.version_after.as_deref() {
        let [major, minor, patch] = stable_semver_components(target)?;
        return Some(format!("{major}.{minor}.{patch}"));
    }
    let workspace_version = record
        .mutation
        .version_before
        .clone()
        .or_else(|| {
            record
                .mutation
                .source_workspace
                .as_deref()
                .and_then(|path| workspace_version_for_release_source(Path::new(path)))
        })
        .or_else(|| {
            record
                .mutation
                .release_workspace
                .as_deref()
                .and_then(|path| workspace_version_for_release_source(Path::new(path)))
        });
    resolved_target_from_selector(&record.objective.version, workspace_version.as_deref())
}

fn resolved_source_release_target(
    source: &ReleaseSourceChoice,
    version: &ReleaseVersionSelector,
) -> Option<String> {
    let workspace_version = workspace_version_for_release_source(&source.source_workspace);
    resolved_target_from_selector(version, workspace_version.as_deref())
}

fn source_version_transition(
    source: &ReleaseSourceChoice,
    version: &ReleaseVersionSelector,
) -> Option<String> {
    let before = workspace_version_for_release_source(&source.source_workspace)?;
    let after = resolved_target_from_selector(version, Some(&before))?;
    Some(format!("{before} -> {after}"))
}

fn current_objective_description(
    source: &ReleaseSourceChoice,
    version: &ReleaseVersionSelector,
) -> String {
    let label = source
        .objective_label
        .as_deref()
        .unwrap_or("the current completed implementation");
    let transition =
        source_version_transition(source, version).unwrap_or_else(|| version.target_label());
    bounded(
        redact_secrets(&format!("{transition} release for {label}")),
        240,
    )
}

fn source_repository_matches_epoch(
    record: &ReleaseRecoveryRecord,
    source: &ReleaseSourceChoice,
) -> Result<bool, RrcError> {
    Ok(repository_identity(&source.source_workspace)? == record.repo_identity)
}

fn source_is_strict_descendant(
    record: &ReleaseRecoveryRecord,
    source: &ReleaseSourceChoice,
) -> bool {
    let Some(previous) = record
        .mutation
        .final_candidate_commit
        .as_deref()
        .or(record.release_commit.as_deref())
        .or(record.mutation.source_commit.as_deref())
    else {
        return false;
    };
    previous != source.source_commit
        && git_succeeds(
            &source.source_workspace,
            &[
                "merge-base",
                "--is-ancestor",
                previous,
                &source.source_commit,
            ],
        )
}

fn irreversible_state_description(record: &ReleaseRecoveryRecord) -> &'static str {
    if record.mutation.publication_verified
        || !record.mutation.published_asset_names.is_empty()
        || matches!(
            record.state,
            ReleaseRecoveryState::Published
                | ReleaseRecoveryState::PostReleaseCloseout
                | ReleaseRecoveryState::PostReleaseMainDegraded
        )
    {
        "an actually published target release"
    } else {
        "a pushed target release tag"
    }
}

fn release_workspace_matches_epoch(
    record: &ReleaseRecoveryRecord,
    state_root: &Path,
) -> Result<Option<PathBuf>, RrcError> {
    let Some(path) = record.mutation.release_workspace.as_deref() else {
        return Ok(None);
    };
    let Ok(workspace) = PathBuf::from(path).canonicalize() else {
        return Ok(None);
    };
    let worktree_root = state_root.join("worktrees");
    let Ok(worktree_root) = worktree_root.canonicalize() else {
        return Ok(None);
    };
    if !workspace.starts_with(&worktree_root)
        || repository_identity(&workspace)? != record.repo_identity
    {
        return Ok(None);
    }
    let expected_head = record
        .release_commit
        .as_deref()
        .or(record.mutation.source_commit.as_deref());
    if expected_head.is_none()
        || git_output(&workspace, &["rev-parse", "HEAD"])?.trim() != expected_head.unwrap()
    {
        return Ok(None);
    }
    let status = git_output(
        &workspace,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    if status.is_empty() {
        return Ok(Some(workspace));
    }
    if record.mutation.version_after.is_none() || record.mutation.candidate_committed {
        return Ok(None);
    }
    let mut allowed = record
        .mutation
        .version_files
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    allowed.insert("Cargo.lock".into());
    let status_is_planned = status.lines().all(|line| {
        let Some(path) = line.get(3..) else {
            return false;
        };
        !path.contains(" -> ") && allowed.contains(path)
    });
    Ok(status_is_planned.then_some(workspace))
}

fn reconcile_active_epoch(
    mut record: ReleaseRecoveryRecord,
    source: &ReleaseSourceChoice,
    version: &ReleaseVersionSelector,
    state_root: &Path,
) -> Result<ActiveEpochReconciliation, RrcError> {
    if record.mutation.publication_verified
        && source_repository_matches_epoch(&record, source)?
        && same_release_objective(&record, source) == Some(true)
        && resolved_record_release_target(&record)
            == resolved_source_release_target(source, version)
        && record.mutation.closeout_receipt.is_none()
        && matches!(
            record.state,
            ReleaseRecoveryState::Published
                | ReleaseRecoveryState::PostReleaseCloseout
                | ReleaseRecoveryState::Complete
        )
        && let Some(workspace) = release_workspace_matches_epoch(&record, state_root)?
    {
        record.liveness = Default::default();
        record.refresh_progress();
        return Ok(ActiveEpochReconciliation::Resume { record, workspace });
    }
    if has_publication_boundary(&record) {
        return Ok(ActiveEpochReconciliation::Clarify(format!(
            "An unfinished {} already has {}. Your new request targets {}. Which release should RRC continue?",
            active_objective_description(&record),
            irreversible_state_description(&record),
            current_objective_description(source, version),
        )));
    }
    if !source_repository_matches_epoch(&record, source)? {
        return Ok(ActiveEpochReconciliation::Clarify(
            "The current completed source belongs to a different repository identity. I preserved the active release and made no new release changes. Which release should RRC continue?"
                .into(),
        ));
    }

    let same_objective = same_release_objective(&record, source) == Some(true);
    let explicitly_superseded = source_explicitly_supersedes_epoch(&record, source);
    if !same_objective && !explicitly_superseded {
        return Ok(ActiveEpochReconciliation::Clarify(format!(
            "An unfinished {} is still active. Your new request targets {}. Which release should RRC continue?",
            active_objective_description(&record),
            current_objective_description(source, version),
        )));
    }

    let previous_target = resolved_record_release_target(&record);
    let requested_target = resolved_source_release_target(source, version);
    let (Some(previous_target), Some(requested_target)) =
        (previous_target.as_deref(), requested_target.as_deref())
    else {
        return Ok(ActiveEpochReconciliation::Clarify(
            "RRC could not resolve both release targets safely. I preserved the active release and made no new release changes. Which release should RRC continue?"
                .into(),
        ));
    };
    let historical_remote_evidence = has_historical_candidate_or_ci_evidence(&record);
    if previous_target != requested_target {
        if historical_remote_evidence {
            return Ok(ActiveEpochReconciliation::Clarify(format!(
                "An unfinished {} has historical candidate or CI evidence for target {previous_target}, while the new request targets {}. Which release should RRC continue?",
                active_objective_description(&record),
                current_objective_description(source, version),
            )));
        }
        return Ok(ActiveEpochReconciliation::Supersede {
            record,
            reason: "same completed objective now targets a different release version before remote mutation"
                .into(),
        });
    }

    let source_changed = record.mutation.source_commit.as_deref()
        != Some(source.source_commit.as_str())
        || record.mutation.intended_diff_sha256.as_deref()
            != Some(source.intended_diff_sha256.as_str());
    if source_changed {
        if !source_is_strict_descendant(&record, source) {
            return Ok(ActiveEpochReconciliation::Clarify(format!(
                "The replacement source for target {requested_target} is not a strict descendant of the historical candidate. I preserved the active release and made no new release changes. Which release should RRC continue?"
            )));
        }
        if historical_remote_evidence && !source.canonical_release_source {
            return Ok(ActiveEpochReconciliation::Clarify(format!(
                "The replacement source for target {requested_target} is not bound as the canonical release source. I preserved the active release and its historical evidence. Which release should RRC continue?"
            )));
        }
        return Ok(ActiveEpochReconciliation::Supersede {
            record,
            reason: if historical_remote_evidence {
                "same release target adopted a newer canonical descendant while preserving historical candidate and CI evidence"
                    .into()
            } else if explicitly_superseded {
                "canonical integrated objective explicitly supersedes the historical prerelease objective"
                    .into()
            } else {
                "same release objective has a newer completed descendant source identity".into()
            },
        });
    }

    let release_workspace = release_workspace_matches_epoch(&record, state_root)?;
    if release_workspace.is_none() {
        if historical_remote_evidence {
            return Ok(ActiveEpochReconciliation::Clarify(format!(
                "Replacing the workspace for target {requested_target} requires a newer canonical strict descendant of the historical candidate. I preserved the active release and its historical evidence. Which release should RRC continue?"
            )));
        }
        return Ok(ActiveEpochReconciliation::Supersede {
            record,
            reason: "release workspace no longer matches the persisted objective provenance".into(),
        });
    }
    if record.state == ReleaseRecoveryState::Escalated {
        return Ok(ActiveEpochReconciliation::Clarify(
            "The current release reached a safety stop that needs one decision. I preserved it and made no new release changes. Should I continue this release?"
                .into(),
        ));
    }
    if record.state == ReleaseRecoveryState::Preparing {
        let commit = record
            .release_commit
            .clone()
            .ok_or_else(|| RrcError::Invalid("release source commit is missing".into()))?;
        record.transition(
            ReleaseRecoveryState::LocalVerification,
            &commit,
            "natural release admission reconciled the recoverable checkpoint",
            vec!["local:admission-reconciliation".into()],
            None,
        )?;
    } else if record.state == ReleaseRecoveryState::DiagnosingLocalFailure {
        let commit = record
            .release_commit
            .clone()
            .ok_or_else(|| RrcError::Invalid("release source commit is missing".into()))?;
        record.transition(
            ReleaseRecoveryState::LocalVerification,
            &commit,
            "natural release admission resumed the persisted safe local stage",
            vec!["local:admission-reconciliation".into()],
            None,
        )?;
    }
    Ok(ActiveEpochReconciliation::Resume {
        record,
        workspace: release_workspace.expect("checked above"),
    })
}

/// Resolves and reconciles the canonical release source before invoking a host launcher.
/// The launcher owns progression, process lifetime and permission composition;
/// this admission port never grants mutation permission or changes retry policy.
pub fn admit_natural_release_with_launcher<F>(
    workspace: &Path,
    objective: &str,
    state_root: &Path,
    launcher: F,
) -> Result<NaturalReleaseAdmission, RrcError>
where
    F: FnOnce(PathBuf, String, String) -> Result<(), RrcError>,
{
    let ReleaseIntentDecision::Admit { version, scope } = classify_release_intent(objective) else {
        return Ok(NaturalReleaseAdmission::NotRelease);
    };
    let canonical = workspace.canonicalize()?;
    let repo_identity = repository_identity(&canonical)?;
    let source = match resolve_release_source(&canonical)? {
        Ok(source) => source,
        Err(choices) => {
            let detail = if choices.is_empty() {
                "No clean completed implementation is provenance-linked to this dirty checkout."
                    .to_owned()
            } else {
                format!(
                    "I found two or more completed variants of the current objective:\n{}",
                    choices
                        .iter()
                        .enumerate()
                        .map(|(index, label)| format!("{}. {label}", index + 1))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            return Ok(NaturalReleaseAdmission::Clarification(format!(
                "Release needs one clarification: {detail} Which completed implementation should be released? No workspace or release state was changed."
            )));
        }
    };
    let remote = git_output(&canonical, &["config", "--get", "remote.origin.url"])?;
    let repository = github_repository_slug(&remote)?;
    let ledger = ReleaseLedger::open(state_root.to_path_buf(), &repo_identity)?;
    let owner = ledger.acquire_owner()?;
    let mut superseded = None;
    if let Some(existing) = ledger.load()?
        && existing.state == ReleaseRecoveryState::Complete
        && source_repository_matches_epoch(&existing, &source)?
        && resolved_record_release_target(&existing)
            == resolved_source_release_target(&source, &version)
        && let Some(summary) = crate::release_closeout::summary(&existing)
    {
        if same_release_objective(&existing, &source) != Some(true) {
            return Ok(NaturalReleaseAdmission::Clarification(format!(
                "Version {} is already published for {}. Your new request names a different completed objective. Which new version should RRC release?",
                existing.release_version.as_deref().unwrap_or("unknown"),
                active_objective_description(&existing),
            )));
        }
        return Ok(NaturalReleaseAdmission::Started(summary));
    }
    if let Some(existing) = ledger.load()?
        && !matches!(existing.state, ReleaseRecoveryState::Cancelled)
        && !(existing.state == ReleaseRecoveryState::Complete
            && existing.mutation.closeout_receipt.is_some())
    {
        match reconcile_active_epoch(existing, &source, &version, state_root)? {
            ActiveEpochReconciliation::Resume {
                mut record,
                workspace,
            } => {
                record.objective.request = Some(scope.clone());
                ledger.save(&record)?;
                drop(owner);
                launcher(workspace, repository, repo_identity)?;
                return Ok(NaturalReleaseAdmission::Started(
                    "Continuing the matching release automatically from its last safe stage; no manual resume command was required."
                        .into(),
                ));
            }
            ActiveEpochReconciliation::Supersede { record, reason } => {
                superseded = Some((record, reason));
            }
            ActiveEpochReconciliation::Clarify(message) => {
                return Ok(NaturalReleaseAdmission::Clarification(message));
            }
        }
    }
    let release_workspace = create_release_worktree(&source, state_root, &repo_identity)?;
    let mut record = start_release(
        &repo_identity,
        version.as_str(),
        &source.branch_ref,
        &source.source_commit,
    )?;
    record.objective.request = Some(scope);
    record.mutation.source_commit = Some(source.source_commit.clone());
    record.mutation.source_workspace = Some(source.source_workspace.to_string_lossy().into_owned());
    record.mutation.objective_id = source.objective_id;
    record.mutation.objective_label = source.objective_label;
    record.mutation.source_variant_id = source.variant_id;
    record.mutation.canonical_release_source = source.canonical_release_source;
    record.mutation.supersedes = source.supersedes;
    record.mutation.source_variant_label = source.variant_label;
    record.mutation.objective_evidence_reports = source.evidence_reports;
    record.mutation.release_workspace = Some(release_workspace.to_string_lossy().into_owned());
    record.mutation.base_sha = Some(source.base_sha);
    record.mutation.intended_commits = source.intended_commits;
    record.mutation.intended_diff_sha256 = Some(source.intended_diff_sha256);
    record.mutation.final_candidate_commit = Some(source.source_commit);
    let superseded_previous = superseded.is_some();
    if let Some((previous, reason)) = superseded {
        ledger.supersede_and_save(&previous, &reason, &record)?;
    } else {
        ledger.save(&record)?;
    }
    drop(owner);
    launcher(release_workspace, repository, repo_identity)?;
    let admission_note = if superseded_previous {
        "An obsolete failed candidate was preserved as superseded evidence, and the current completed objective was admitted automatically."
    } else {
        "The completed objective was admitted automatically."
    };
    Ok(NaturalReleaseAdmission::Started(format!(
        "{admission_note}\nRelease controller started and is preparing the release target."
    )))
}

pub fn admit_natural_release_for_workspace_with_factory(
    workspace: &Path,
    objective: &str,
    repair_factory: Option<crate::WorkerFactory>,
) -> Result<NaturalReleaseAdmission, RrcError> {
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    admit_natural_release_with_launcher(
        workspace,
        objective,
        &root,
        move |release_workspace, repository, repo_identity| {
            crate::release_executor::spawn_release_worker_with_factory(
                release_workspace,
                repository,
                repo_identity,
                repair_factory,
            )
            .map(|_| ())
        },
    )
}

/// Resolves repository identity and current ref/commit before executing a
/// shared host command. No remote URL or credential-bearing value is stored.
pub fn release_command_for_workspace(workspace: &Path, argument: &str) -> Result<String, RrcError> {
    release_command_for_workspace_with_factory(workspace, argument, None)
}

fn release_start_selector(action: &str, checkpoint_exists: bool) -> Option<ReleaseVersionSelector> {
    if action.is_empty() {
        return (!checkpoint_exists).then_some(ReleaseVersionSelector::Patch);
    }
    ReleaseVersionSelector::parse(action).ok()
}

/// Typed admission prevents hosts from waiting on an unrelated active release
/// when a command instead returned clarification or a read-only reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseCommandOutcome {
    Started(String),
    Reply(String),
}

pub fn release_command_for_workspace_with_factory(
    workspace: &Path,
    argument: &str,
    repair_factory: Option<crate::WorkerFactory>,
) -> Result<String, RrcError> {
    release_command_outcome_for_workspace_with_factory(workspace, argument, repair_factory).map(
        |outcome| match outcome {
            ReleaseCommandOutcome::Started(body) | ReleaseCommandOutcome::Reply(body) => body,
        },
    )
}

pub fn release_command_outcome_for_workspace_with_factory(
    workspace: &Path,
    argument: &str,
    repair_factory: Option<crate::WorkerFactory>,
) -> Result<ReleaseCommandOutcome, RrcError> {
    let canonical = workspace.canonicalize()?;
    let repo_identity = repository_identity(&canonical)?;
    let action = argument.trim();
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    let ledger = ReleaseLedger::open(root, &repo_identity)?;
    if let Some(version) = release_start_selector(action, ledger.load()?.is_some()) {
        let objective = match &version {
            ReleaseVersionSelector::Exact(target) => format!("Release version {target}."),
            _ => format!(
                "Release the completed implementation as the next {} release.",
                version.as_str()
            ),
        };
        return match admit_natural_release_for_workspace_with_factory(
            &canonical,
            &objective,
            repair_factory,
        )? {
            NaturalReleaseAdmission::Started(body) => Ok(ReleaseCommandOutcome::Started(body)),
            NaturalReleaseAdmission::Clarification(body) => Ok(ReleaseCommandOutcome::Reply(body)),
            NaturalReleaseAdmission::NotRelease => Err(RrcError::Invalid(
                "explicit release command was not admitted".into(),
            )),
        };
    }
    if action == "status" {
        return Ok(ReleaseCommandOutcome::Reply(
            release_status_for_workspace(&canonical)
                .unwrap_or_else(|| "release: no active or persisted epoch".into()),
        ));
    }
    let source_commit = git_output(&canonical, &["rev-parse", "HEAD"])?;
    let branch_ref = git_output(&canonical, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if matches!(action, "resume" | "retry") {
        let remote = git_output(&canonical, &["config", "--get", "remote.origin.url"])?;
        let repository = github_repository_slug(&remote)?;
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
        }
        let release_workspace = record
            .mutation
            .release_workspace
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| canonical.clone());
        crate::release_executor::spawn_release_worker_with_factory(
            release_workspace,
            repository,
            repo_identity,
            repair_factory,
        )?;
        return Ok(ReleaseCommandOutcome::Started(format!(
            "{}\nController            active in background; use /release status for the persisted stage",
            record.render_status()
        )));
    }
    release_command(&repo_identity, action, &source_commit, &branch_ref)
        .map(ReleaseCommandOutcome::Reply)
}

pub(crate) fn github_repository_slug(remote: &str) -> Result<String, RrcError> {
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
            && value != "."
            && value != ".."
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

/// Clears `Active` liveness when no live owner process can be shown.
/// The release state, gates, and evidence are unchanged.
pub fn reconcile_unowned_release(
    workspace: &Path,
) -> Result<Option<ReleaseRecoveryRecord>, RrcError> {
    let identity = repository_identity_for_workspace(workspace)?;
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    let ledger = ReleaseLedger::open(root, &identity)?;
    let Some(mut record) = ledger.load()? else {
        return Ok(None);
    };
    let owner_live = release_owner_is_live(&identity, record.liveness.owner_pid);
    if settle_absent_owner(&mut record, owner_live) {
        ledger.save(&record)?;
    }
    Ok(Some(record))
}

pub(crate) fn settle_absent_owner(record: &mut ReleaseRecoveryRecord, owner_live: bool) -> bool {
    if record.liveness.state != ReleaseLivenessState::Active || owner_live {
        return false;
    }
    record.liveness.state = ReleaseLivenessState::OwnerExited;
    if record.liveness.operation.is_empty() {
        record.liveness.operation = "release controller".into();
    }
    record.liveness.detail = "controller owner is gone; epoch remains recoverable".into();
    record.liveness.observed_at = Some(Utc::now());
    record.refresh_progress();
    true
}

fn release_owner_is_live(repo_identity: &str, owner_pid: Option<u32>) -> bool {
    let Some(pid) = owner_pid else {
        return false;
    };
    if pid == std::process::id() {
        return crate::release_executor::active_release_worker(repo_identity).is_some();
    }
    foreign_process_alive(pid)
}

fn foreign_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Read-only RRC status for `/ci`; never creates a state root.
#[must_use]
pub fn release_status_for_workspace(workspace: &Path) -> Option<String> {
    let canonical = workspace.canonicalize().ok()?;
    let root = default_release_root()?;
    if !root.is_dir() {
        return None;
    }
    let record = reconcile_unowned_release(&canonical).ok().flatten()?;
    let _ = root;
    Some({
        let mut status = record.render_status();
        if let Some(worker) =
            crate::release_executor::active_release_worker_for_workspace(&canonical)
        {
            status.push_str("\n\n");
            status.push_str(&crate::release_executor::render_active_worker_status(
                &worker,
            ));
        }
        status
    })
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
    use std::thread;

    use super::*;

    #[test]
    fn natural_release_intent_is_imperative_and_fail_closed() {
        for (prompt, bump) in [
            ("Release all completed RRC work as the next patch.", "patch"),
            ("release everything we finished as the next patch", "patch"),
            ("ship this", "patch"),
            ("release these fixes", "patch"),
            ("publish the next patch", "patch"),
            ("Release this as a minor version.", "minor"),
        ] {
            assert!(matches!(
                classify_release_intent(prompt),
                ReleaseIntentDecision::Admit { version: admitted, .. } if admitted.as_str() == bump
            ));
        }
        for prompt in [
            "How does the release system work?",
            "How should we release this?",
            "We can release later.",
            "Document the release process.",
            "Do not release this yet.",
            "The release build is green.",
        ] {
            assert_eq!(
                classify_release_intent(prompt),
                ReleaseIntentDecision::NotRelease,
                "false positive for {prompt:?}"
            );
        }
    }

    #[test]
    fn natural_release_intent_accepts_and_normalizes_explicit_stable_versions() {
        for prompt in [
            "Release version 0.25.0.",
            "release v0.25.0",
            "Please publish version v0.25.0.",
        ] {
            assert!(matches!(
                classify_release_intent(prompt),
                ReleaseIntentDecision::Admit {
                    version: ReleaseVersionSelector::Exact(ref version),
                    ..
                } if version == "0.25.0"
            ));
        }
        for prompt in [
            "Release version 0.25.",
            "Release v0.25.",
            "Release version 01.25.0.",
            "Release version 0.25.0-beta.1.",
        ] {
            assert_eq!(
                classify_release_intent(prompt),
                ReleaseIntentDecision::NotRelease,
                "malformed or unstable version must fail closed for {prompt:?}"
            );
        }
    }

    #[test]
    fn explicit_release_command_routes_exact_versions_through_natural_admission() {
        assert_eq!(
            release_start_selector("0.25.0", false),
            Some(ReleaseVersionSelector::Exact("0.25.0".into()))
        );
        assert_eq!(
            release_start_selector("v0.25.0", false),
            Some(ReleaseVersionSelector::Exact("0.25.0".into()))
        );
        assert_eq!(release_start_selector("status", false), None);
        assert_eq!(release_start_selector("", true), None);
    }

    #[test]
    fn exact_release_target_is_visible_before_version_mutation() {
        let record = start_release("repo", "v0.25.0", "main", "abcdef123456").unwrap();
        assert_eq!(
            record.objective.version,
            ReleaseVersionSelector::Exact("0.25.0".into())
        );
        assert!(
            record
                .render_status()
                .contains("Release target         0.25.0")
        );
        assert!(record.progress.headline.contains("release target 0.25.0"));
        assert!(record.mutation.version_after.is_none());
    }

    #[test]
    fn natural_release_from_dirty_primary_selects_completed_source_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("repository");
        let completion = temp.path().join("completion");
        let state = temp.path().join("state");
        fs::create_dir_all(primary.join("docs/foundation")).unwrap();
        run_git(&primary, &["init", "-b", "main"]);
        run_git(&primary, &["config", "user.email", "rrc@example.invalid"]);
        run_git(&primary, &["config", "user.name", "RRC Test"]);
        run_git(
            &primary,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ],
        );
        fs::write(primary.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(primary.join("user.txt"), "base\n").unwrap();
        run_git(&primary, &["add", "."]);
        run_git(&primary, &["commit", "-m", "base"]);
        let old_head = git_text(&primary, &["rev-parse", "HEAD"]);
        run_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "rrc-completion-test",
                completion.to_str().unwrap(),
            ],
        );
        fs::write(completion.join("rrc.rs"), "final controller\n").unwrap();
        fs::create_dir_all(completion.join("docs/foundation")).unwrap();
        fs::write(
            completion.join("docs/foundation/rrc-final-completion.md"),
            "# COMPLETE\n",
        )
        .unwrap();
        run_git(&completion, &["add", "."]);
        run_git(&completion, &["commit", "-m", "feat: complete RRC"]);
        write_objective_marker(
            &completion,
            "rrc-autonomy-repair",
            "RRC autonomy repair",
            "Completed autonomy repair after release admission",
            "2026-09-30T00:00:00Z",
        );
        run_git(&completion, &["add", "."]);
        run_git(
            &completion,
            &["commit", "-m", "docs: bind RRC autonomy objective"],
        );
        let intended_head = git_text(&completion, &["rev-parse", "HEAD"]);
        let historical = temp.path().join("red-main-ci");
        run_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "red-main-ci-test",
                historical.to_str().unwrap(),
            ],
        );
        fs::write(historical.join("red-main.txt"), "historical repair\n").unwrap();
        write_objective_marker(
            &historical,
            "red-main-ci",
            "Historical red main CI repair",
            "Red main repair after CI recovery",
            "2026-09-29T00:00:00Z",
        );
        run_git(&historical, &["add", "."]);
        run_git(
            &historical,
            &["commit", "-m", "complete red main CI repair"],
        );
        fs::write(primary.join("user.txt"), "private dirty bytes\n").unwrap();
        let before = fs::read(primary.join("user.txt")).unwrap();
        let mut launched = None;
        let outcome = admit_natural_release_with_launcher(
            &primary,
            "Release all completed RRC work as the next patch.",
            &state,
            |workspace, repository, identity| {
                launched = Some((workspace, repository, identity));
                Ok(())
            },
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
        let (release_workspace, repository, identity) =
            launched.expect("worker launched automatically");
        assert_ne!(release_workspace, primary);
        assert_eq!(repository, "test/repo");
        assert_eq!(
            git_text(
                &release_workspace,
                &["config", "--get", "remote.origin.url"]
            ),
            "https://github.com/test/repo.git"
        );
        assert_eq!(
            git_text(&release_workspace, &["rev-parse", "HEAD"]),
            intended_head
        );
        assert_ne!(
            git_text(&release_workspace, &["rev-parse", "HEAD"]),
            old_head
        );
        assert!(git_text(&release_workspace, &["status", "--porcelain"]).is_empty());
        assert_eq!(fs::read(primary.join("user.txt")).unwrap(), before);
        let ledger = ReleaseLedger::open(state, &identity).unwrap();
        let record = ledger.load().unwrap().unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            record
                .mutation
                .source_workspace
                .as_deref()
                .map(Path::new)
                .map(Path::canonicalize)
                .transpose()
                .unwrap(),
            Some(completion.canonicalize().unwrap())
        );
        assert_eq!(
            record.mutation.objective_id.as_deref(),
            Some("rrc-autonomy-repair")
        );
        assert_eq!(
            record.mutation.objective_label.as_deref(),
            Some("RRC autonomy repair")
        );
        assert_ne!(
            record.mutation.source_workspace.as_deref(),
            Some(historical.to_string_lossy().as_ref())
        );
        assert_eq!(
            record.mutation.release_workspace.as_deref(),
            Some(release_workspace.to_string_lossy().as_ref())
        );
        assert_eq!(record.mutation.base_sha.as_deref(), Some(old_head.as_str()));
        assert_eq!(
            record.mutation.final_candidate_commit.as_deref(),
            Some(intended_head.as_str())
        );
        assert!(!record.mutation.intended_commits.is_empty());
    }

    #[test]
    fn ambiguous_completed_candidates_clarify_once_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("repository");
        let state = temp.path().join("state");
        fs::create_dir_all(&primary).unwrap();
        run_git(&primary, &["init", "-b", "main"]);
        run_git(&primary, &["config", "user.email", "rrc@example.invalid"]);
        run_git(&primary, &["config", "user.name", "RRC Test"]);
        run_git(
            &primary,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ],
        );
        fs::write(primary.join("base.txt"), "base\n").unwrap();
        run_git(&primary, &["add", "."]);
        run_git(&primary, &["commit", "-m", "base"]);
        for name in ["completion-a", "completion-b"] {
            let path = temp.path().join(name);
            run_git(
                &primary,
                &["worktree", "add", "-b", name, path.to_str().unwrap()],
            );
            fs::create_dir_all(path.join("docs/foundation")).unwrap();
            fs::write(path.join(format!("{name}.txt")), name).unwrap();
            fs::write(
                path.join(format!("docs/foundation/{name}-completion.md")),
                "# COMPLETE\n",
            )
            .unwrap();
            write_objective_marker(
                &path,
                "rrc-autonomy-repair",
                "RRC autonomy repair",
                if name == "completion-a" {
                    "Variant completed after CI recovery"
                } else {
                    "Earlier variant before CI recovery"
                },
                "2026-09-30T00:00:00Z",
            );
            run_git(&path, &["add", "."]);
            run_git(&path, &["commit", "-m", &format!("complete {name}")]);
        }
        fs::write(primary.join("base.txt"), "dirty\n").unwrap();
        let outcome = admit_natural_release_with_launcher(
            &primary,
            "Release this as the next patch.",
            &state,
            |_, _, _| panic!("ambiguous provenance must not launch"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("same-objective variants must clarify");
        };
        assert!(message.contains("Variant completed after CI recovery"));
        assert!(message.contains("Earlier variant before CI recovery"));
        assert!(!message.contains("completion-a"));
        assert!(!message.contains("completion-b"));
        assert!(!message.contains(temp.path().to_string_lossy().as_ref()));
        assert!(
            !state.exists(),
            "clarification must not create release state"
        );
        assert_eq!(
            fs::read_to_string(primary.join("base.txt")).unwrap(),
            "dirty\n"
        );
    }

    #[test]
    fn same_tree_under_two_provenance_records_collapses_to_one_candidate() {
        let fixture = provenance_resolution_fixture();
        let first = add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "same-tree-a",
            ProvenanceVariant::new(
                "shared implementation\n",
                "Shared completed implementation",
                "same-tree-a",
            ),
        );
        let second = add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "same-tree-b",
            ProvenanceVariant::new(
                "shared implementation\n",
                "Shared completed implementation",
                "same-tree-b",
            ),
        );
        let second_marker = second.join("docs/foundation/release-objective-provenance.json");
        let marker = fs::read_to_string(&second_marker)
            .unwrap()
            .replace("same-tree-b", "same-tree-a");
        fs::write(&second_marker, marker).unwrap();
        run_git(&second, &["add", "."]);
        run_git(
            &second,
            &["commit", "--amend", "-m", "same-tree-b distinct commit"],
        );
        assert_ne!(
            git_text(&first, &["rev-parse", "HEAD"]),
            git_text(&second, &["rev-parse", "HEAD"])
        );
        assert_eq!(
            git_text(&first, &["rev-parse", "HEAD^{tree}"]),
            git_text(&second, &["rev-parse", "HEAD^{tree}"])
        );
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
    }

    #[test]
    fn equivalent_implementation_diffs_collapse_despite_distinct_provenance() {
        let fixture = provenance_resolution_fixture();
        add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "equivalent-a",
            ProvenanceVariant::new(
                "equivalent implementation\n",
                "Equivalent evidence record A",
                "equivalent-a",
            ),
        );
        add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "equivalent-b",
            ProvenanceVariant::new(
                "equivalent implementation\n",
                "Equivalent evidence record B",
                "equivalent-b",
            ),
        );
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
    }

    #[test]
    fn canonical_final_integration_outranks_historical_autonomy_variants() {
        let fixture = provenance_resolution_fixture();
        add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "historical-autonomy",
            ProvenanceVariant::new(
                "historical autonomy source\n",
                "Completed autonomy repair with objective-linked source selection",
                "historical-autonomy",
            ),
        );
        add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "rejected-integration",
            ProvenanceVariant::new(
                "version=3.3.1\n",
                "Completed autonomy repair with objective-linked source selection",
                "rejected-integration",
            ),
        );
        let corrected = add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "corrected-integration",
            ProvenanceVariant {
                implementation: "version=0.24.4\n",
                variant_label: "Canonical integrated v0.24.4 release source",
                commit_subject: "corrected-integration",
                objective_id: "integrated-v0.24.4-release-source-20261001",
                canonical_release_source: true,
                supersedes: &["rrc-source-resolution-ux-repair-20260930"],
            },
        );
        fs::write(fixture.primary.join("base.txt"), "private dirty bytes\n").unwrap();
        let before = fs::read(fixture.primary.join("base.txt")).unwrap();
        let mut launched = None;
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |workspace, _, _| {
                launched = Some(workspace);
                Ok(())
            },
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
        let release_workspace = launched.expect("canonical integration launched");
        assert_eq!(
            git_text(&release_workspace, &["rev-parse", "HEAD"]),
            git_text(&corrected, &["rev-parse", "HEAD"])
        );
        assert_eq!(fs::read(fixture.primary.join("base.txt")).unwrap(), before);
        assert!(git_text(&release_workspace, &["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn genuinely_different_duplicate_labels_explain_the_difference() {
        let fixture = provenance_resolution_fixture();
        add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "different-a",
            ProvenanceVariant::new(
                "implementation alpha\n",
                "Same human label",
                "alpha transport implementation",
            ),
        );
        add_provenance_variant(
            &fixture.primary,
            fixture._temp.path(),
            "different-b",
            ProvenanceVariant::new(
                "implementation beta\n",
                "Same human label",
                "beta transport implementation",
            ),
        );
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |_, _, _| panic!("genuinely different variants must clarify"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("genuinely different variants must clarify");
        };
        assert!(message.contains("alpha transport implementation"));
        assert!(message.contains("beta transport implementation"));
        assert_eq!(message.matches("Same human label").count(), 2);
    }

    struct ActiveEpochFixture {
        _temp: tempfile::TempDir,
        primary: PathBuf,
        completion: PathBuf,
        state: PathBuf,
        identity: String,
    }

    fn active_epoch_fixture() -> ActiveEpochFixture {
        // This fixture exercises an actual controller-owned Cargo gate. Keep
        // its ephemeral release root on the repository filesystem rather
        // than `/tmp`, whose small tmpfs is correctly rejected by the RRC
        // disk reserve + expected-growth policy.
        let temp = tempfile::Builder::new()
            .prefix(".rrc-active-epoch-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let primary = temp.path().join("repository");
        let completion = temp.path().join("completion-old");
        let state = temp.path().join("state");
        fs::create_dir_all(&primary).unwrap();
        run_git(&primary, &["init", "-b", "main"]);
        run_git(&primary, &["config", "user.email", "rrc@example.invalid"]);
        run_git(&primary, &["config", "user.name", "RRC Test"]);
        run_git(
            &primary,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ],
        );
        fs::write(primary.join("base.txt"), "base\n").unwrap();
        fs::create_dir_all(primary.join(".cargo")).unwrap();
        fs::create_dir_all(primary.join("xtask/src")).unwrap();
        fs::write(
            primary.join(".cargo/config.toml"),
            "[alias]\nxtask = \"run --quiet -p xtask --\"\n",
        )
        .unwrap();
        fs::write(
            primary.join("Cargo.toml"),
            "[workspace]\nmembers = [\"xtask\"]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"0.24.4\"\n",
        )
        .unwrap();
        fs::write(
            primary.join("xtask/Cargo.toml"),
            "[package]\nname = \"xtask\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            primary.join("xtask/src/main.rs"),
            "use std::{fs, thread, time::Duration};\nfn main() {\n    let gate = std::env::args().nth(1).unwrap();\n    if gate == \"verify\" {\n        fs::write(\"local-gate-started\", b\"started\").unwrap();\n        eprintln!(\"Running tests/test_a.rs (target/debug/deps/test_a-0123456789abcdef)\");\n        thread::sleep(Duration::from_secs(2));\n        eprintln!(\"Running tests/test_b.rs (target/debug/deps/test_b-fedcba9876543210)\");\n        thread::sleep(Duration::from_secs(2));\n    } else {\n        assert_eq!(gate, \"acceptance\");\n        eprintln!(\"Running tests/acceptance.rs (target/debug/deps/acceptance-0123456789abcdef)\");\n        thread::sleep(Duration::from_secs(30));\n    }\n}\n",
        )
        .unwrap();
        run_git(&primary, &["add", "."]);
        run_git(&primary, &["commit", "-m", "base"]);
        run_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "completion-old",
                completion.to_str().unwrap(),
            ],
        );
        fs::write(completion.join("repair.txt"), "old candidate\n").unwrap();
        write_objective_marker_options(
            &completion,
            "rrc-autonomy-repair",
            "RRC autonomy repair",
            "Older prerelease repair candidate",
            "2026-09-30T00:00:00Z",
            ObjectiveMarkerOptions {
                variant_id: Some("historical-prerelease-source"),
                ..ObjectiveMarkerOptions::default()
            },
        );
        run_git(&completion, &["add", "."]);
        run_git(&completion, &["commit", "-m", "complete old candidate"]);
        fs::write(primary.join("base.txt"), "Alex's dirty primary bytes\n").unwrap();
        let outcome = admit_natural_release_with_launcher(
            &primary,
            "Release all completed work as the next patch.",
            &state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
        let identity = repository_identity(&primary).unwrap();
        let ledger = ReleaseLedger::open(state.clone(), &identity).unwrap();
        let mut record = ledger.load().unwrap().unwrap();
        let commit = record.release_commit.clone().unwrap();
        record
            .transition(
                ReleaseRecoveryState::DiagnosingLocalFailure,
                &commit,
                "persisted older prerelease local failure",
                vec!["local:old-candidate-failure".into()],
                None,
            )
            .unwrap();
        ledger.save(&record).unwrap();
        ActiveEpochFixture {
            _temp: temp,
            primary,
            completion,
            state,
            identity,
        }
    }

    fn add_same_objective_descendant_completion(
        fixture: &ActiveEpochFixture,
        workspace_version: Option<&str>,
    ) -> PathBuf {
        add_same_objective_descendant_completion_with_canonical(fixture, workspace_version, true)
    }

    fn add_same_objective_descendant_completion_with_canonical(
        fixture: &ActiveEpochFixture,
        workspace_version: Option<&str>,
        canonical_release_source: bool,
    ) -> PathBuf {
        let current = fixture._temp.path().join(if canonical_release_source {
            "completion-descendant"
        } else {
            "completion-descendant-noncanonical"
        });
        let old_head = git_text(&fixture.completion, &["rev-parse", "HEAD"]);
        run_git(
            &fixture.primary,
            &[
                "worktree",
                "add",
                "-b",
                "completion-descendant",
                current.to_str().unwrap(),
                &old_head,
            ],
        );
        fs::write(current.join("repair.txt"), "newer canonical descendant\n").unwrap();
        if let Some(version) = workspace_version {
            let manifest = fs::read_to_string(current.join("Cargo.toml")).unwrap();
            fs::write(
                current.join("Cargo.toml"),
                manifest.replace("version = \"0.24.4\"", &format!("version = \"{version}\"")),
            )
            .unwrap();
        }
        write_objective_marker_options(
            &current,
            "rrc-autonomy-repair",
            "RRC autonomy repair",
            "Newer canonical descendant",
            "2026-10-03T12:00:00Z",
            ObjectiveMarkerOptions {
                variant_id: Some(if canonical_release_source {
                    "same-target-canonical-descendant"
                } else {
                    "same-target-noncanonical-descendant"
                }),
                canonical_release_source,
                ..ObjectiveMarkerOptions::default()
            },
        );
        run_git(&current, &["add", "."]);
        run_git(&current, &["commit", "-m", "advance canonical source"]);
        current
    }

    fn add_same_objective_non_descendant_completion(fixture: &ActiveEpochFixture) -> PathBuf {
        let current = fixture._temp.path().join("completion-non-descendant");
        let base_head = git_text(&fixture.primary, &["rev-parse", "HEAD"]);
        run_git(
            &fixture.primary,
            &[
                "worktree",
                "add",
                "-b",
                "completion-non-descendant",
                current.to_str().unwrap(),
                &base_head,
            ],
        );
        fs::write(current.join("repair.txt"), "unrelated branch replacement\n").unwrap();
        write_objective_marker_options(
            &current,
            "rrc-autonomy-repair",
            "RRC autonomy repair",
            "Non-descendant replacement",
            "2026-10-03T12:00:00Z",
            ObjectiveMarkerOptions {
                variant_id: Some("same-target-non-descendant"),
                canonical_release_source: true,
                ..ObjectiveMarkerOptions::default()
            },
        );
        run_git(&current, &["add", "."]);
        run_git(&current, &["commit", "-m", "prepare non-descendant source"]);
        current
    }

    fn historical_remote_failure(source_commit: &str) -> FailureRecord {
        FailureRecord {
            workflow_id: 41,
            run_id: 7001,
            attempt: 1,
            job_id: 91,
            workflow_name: "canonical".into(),
            job_name: "linux".into(),
            platform: Some("linux-x86_64".into()),
            step_name: Some("test".into()),
            fingerprint: FailureFingerprint("historical-failure".into()),
            class: ReleaseFailureClass::TestRegression,
            confidence: EvidenceConfidence::Proven,
            causal_excerpt: "historical candidate failure".into(),
            source_commit: source_commit.into(),
            observed_at: Utc::now(),
            other_platforms_passed: true,
            exists_on_last_green: Some(false),
            related_source_touched: Some(true),
        }
    }

    fn add_canonical_superseding_completion(fixture: &ActiveEpochFixture) -> PathBuf {
        let current = fixture._temp.path().join("completion-canonical");
        let old_head = git_text(&fixture.completion, &["rev-parse", "HEAD"]);
        run_git(
            &fixture.primary,
            &[
                "worktree",
                "add",
                "-b",
                "completion-canonical",
                current.to_str().unwrap(),
                &old_head,
            ],
        );
        fs::write(current.join("repair.txt"), "canonical integrated source\n").unwrap();
        write_objective_marker_options(
            &current,
            "corrected-v0.24.4-prerelease-integration-20261001",
            "Corrected v0.24.4 prerelease integration",
            "Complete corrected source",
            "2026-10-01T07:15:40Z",
            ObjectiveMarkerOptions {
                variant_id: Some("corrected-v0.24.4-canonical-source-20261001"),
                canonical_release_source: true,
                supersedes: &["rrc-autonomy-repair"],
            },
        );
        run_git(&current, &["add", "."]);
        run_git(
            &current,
            &["commit", "-m", "integrate canonical release source"],
        );
        current
    }

    #[test]
    fn same_recoverable_epoch_resumes_from_natural_language_without_manual_command() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let before = ledger.load().unwrap().unwrap();
        let mut launched = None;
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |workspace, _, _| {
                launched = Some(workspace);
                Ok(())
            },
        )
        .unwrap();
        let NaturalReleaseAdmission::Started(message) = outcome else {
            panic!("matching recoverable epoch must resume");
        };
        assert!(message.contains("Continuing the matching release automatically"));
        assert!(!message.contains("/release"));
        let after = ledger.load().unwrap().unwrap();
        assert_eq!(after.epoch_id, before.epoch_id);
        assert_eq!(after.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            launched.unwrap(),
            PathBuf::from(after.mutation.release_workspace.as_deref().unwrap())
        );
        assert!(!ledger.superseded_path(&before.epoch_id).exists());
    }

    #[test]
    fn obsolete_diagnosing_epoch_is_preserved_and_newer_candidate_is_admitted() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let previous = ledger.load().unwrap().unwrap();
        let previous_workspace = previous.mutation.release_workspace.clone().unwrap();
        let newer = fixture._temp.path().join("completion-new");
        let old_head = git_text(&fixture.completion, &["rev-parse", "HEAD"]);
        run_git(
            &fixture.primary,
            &[
                "worktree",
                "add",
                "-b",
                "completion-new",
                newer.to_str().unwrap(),
                &old_head,
            ],
        );
        fs::write(newer.join("repair.txt"), "new repaired candidate\n").unwrap();
        write_objective_marker(
            &newer,
            "rrc-autonomy-repair",
            "RRC autonomy repair",
            "Newer stale-epoch repair candidate",
            "2026-09-30T01:00:00Z",
        );
        run_git(&newer, &["add", "."]);
        run_git(&newer, &["commit", "-m", "repair stale epoch autonomy"]);
        let newer_head = git_text(&newer, &["rev-parse", "HEAD"]);
        let primary_before = fs::read(fixture.primary.join("base.txt")).unwrap();
        let mut launched = None;
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |workspace, _, _| {
                launched = Some(workspace);
                Ok(())
            },
        )
        .unwrap();
        let NaturalReleaseAdmission::Started(message) = outcome else {
            panic!("newer same-objective candidate must be admitted");
        };
        assert!(message.contains("preserved as superseded evidence"));
        assert!(!message.contains("/release"));
        let current = ledger.load().unwrap().unwrap();
        assert_ne!(current.epoch_id, previous.epoch_id);
        assert_eq!(current.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            current.mutation.source_commit.as_deref(),
            Some(newer_head.as_str())
        );
        assert_ne!(
            current.mutation.release_workspace.as_deref(),
            Some(previous_workspace.as_str())
        );
        assert_eq!(
            launched.unwrap(),
            PathBuf::from(current.mutation.release_workspace.as_deref().unwrap())
        );
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&previous.epoch_id)).unwrap())
                .unwrap();
        assert_eq!(archive.record.epoch_id, previous.epoch_id);
        assert_eq!(archive.replacement_epoch_id, current.epoch_id);
        assert_eq!(
            archive.record.state,
            ReleaseRecoveryState::DiagnosingLocalFailure
        );
        assert_eq!(archive.replacement_source_commit, newer_head);
        assert_eq!(
            fs::read(fixture.primary.join("base.txt")).unwrap(),
            primary_before
        );
    }

    #[test]
    fn canonical_objective_supersedes_historical_active_prerelease_without_clarification() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let previous = ledger.load().unwrap().unwrap();
        let current = add_canonical_superseding_completion(&fixture);
        let current_head = git_text(&current, &["rev-parse", "HEAD"]);
        let primary_before = fs::read(fixture.primary.join("base.txt")).unwrap();
        let mut launched = None;
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |workspace, _, _| {
                launched = Some(workspace);
                Ok(())
            },
        )
        .unwrap();
        let NaturalReleaseAdmission::Started(message) = outcome else {
            panic!("explicitly superseded historical prerelease must not clarify");
        };
        assert!(message.contains("preserved as superseded evidence"));
        let replacement = ledger.load().unwrap().unwrap();
        assert_ne!(replacement.epoch_id, previous.epoch_id);
        assert_eq!(
            replacement.mutation.source_commit.as_deref(),
            Some(current_head.as_str())
        );
        assert_eq!(
            launched.unwrap(),
            PathBuf::from(replacement.mutation.release_workspace.as_deref().unwrap())
        );
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&previous.epoch_id)).unwrap())
                .unwrap();
        assert_eq!(archive.record.epoch_id, previous.epoch_id);
        assert_eq!(
            archive.record.mutation.objective_id.as_deref(),
            Some("rrc-autonomy-repair")
        );
        assert_eq!(
            archive.record.mutation.source_variant_id.as_deref(),
            Some("historical-prerelease-source")
        );
        assert_eq!(archive.replacement_source_commit, current_head);
        assert_eq!(
            replacement.mutation.source_variant_id.as_deref(),
            Some("corrected-v0.24.4-canonical-source-20261001")
        );
        assert!(replacement.mutation.canonical_release_source);
        assert_eq!(replacement.mutation.supersedes, vec!["rrc-autonomy-repair"]);
        assert_eq!(
            fs::read(fixture.primary.join("base.txt")).unwrap(),
            primary_before
        );
    }

    #[test]
    fn tui_natural_release_retains_registered_controller_through_local_gate_progress() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let previous = ledger.load().unwrap().unwrap();
        let newer = fixture._temp.path().join("completion-lifecycle");
        let old_head = git_text(&fixture.completion, &["rev-parse", "HEAD"]);
        run_git(
            &fixture.primary,
            &[
                "worktree",
                "add",
                "-b",
                "completion-lifecycle",
                newer.to_str().unwrap(),
                &old_head,
            ],
        );
        fs::write(newer.join("repair.txt"), "controller lifecycle repair\n").unwrap();
        write_objective_marker(
            &newer,
            "rrc-autonomy-repair",
            "RRC autonomy repair",
            "Controller lifecycle repair candidate",
            "2026-09-30T02:00:00Z",
        );
        run_git(&newer, &["add", "."]);
        run_git(&newer, &["commit", "-m", "repair controller lifecycle"]);
        let primary_before = fs::read(fixture.primary.join("base.txt")).unwrap();

        let (factory, _session, _runtime) = crate::release_executor::tests::repair_test_factory(
            "registered-controller-permission-fixture",
            "unused",
        );
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |workspace, repository, repo_identity| {
                let worker_ledger =
                    ReleaseLedger::open(fixture.state.clone(), &repo_identity).unwrap();
                let mut record = worker_ledger.load().unwrap().unwrap();
                record.mutation.version_before = Some("0.1.0".into());
                record.mutation.version_after = Some("0.1.1".into());
                record.mutation.local_gates = vec![
                    LocalGateRecord {
                        name: "workspace-verify".into(),
                        state: SettlementState::NotStarted,
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
                worker_ledger.save(&record).unwrap();
                crate::release_executor::spawn_release_worker_at_root_with_permissive_policy(
                    workspace,
                    repository,
                    repo_identity,
                    Some(factory.clone()),
                    fixture.state.clone(),
                )
                .map(|_| ())
            },
        )
        .unwrap();
        let NaturalReleaseAdmission::Started(message) = outcome else {
            panic!("production admission must start the controller");
        };
        assert!(
            message.contains("Release controller started and is preparing the release target.")
        );

        let current = ledger.load().unwrap().unwrap();
        assert_ne!(current.epoch_id, previous.epoch_id);
        let release_workspace =
            PathBuf::from(current.mutation.release_workspace.as_deref().unwrap());
        let started = release_workspace.join("local-gate-started");
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !started.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "RunLocalVerification did not start a real subprocess: {}",
                ledger.load().unwrap().unwrap().render_status()
            );
            thread::sleep(Duration::from_millis(25));
        }

        let task = crate::release_executor::active_release_worker(&fixture.identity)
            .expect("one registered RRC task must survive handler return");
        assert_eq!(
            crate::release_executor::active_release_worker_count_for_repo(&fixture.identity),
            1
        );
        assert!(
            matches!(ledger.acquire_owner(), Err(RrcError::Busy)),
            "registered worker must retain the cross-process owner lock"
        );
        assert_eq!(task.epoch_id, current.epoch_id);
        assert_eq!(task.stage, "Release recovery");
        assert!(task.detail.contains("workspace-verify"));
        assert!(task.cancellable);
        assert_eq!(task.current_gate.as_deref(), Some("workspace-verify"));
        assert_eq!(task.current_command.as_deref(), Some("cargo xtask verify"));
        assert_eq!(task.completed_gates, 0);
        assert_eq!(task.total_gates, 2);
        assert_eq!(task.version_before.as_deref(), Some("0.1.0"));
        assert_eq!(task.version_after.as_deref(), Some("0.1.1"));

        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let test_a = loop {
            let snapshot = crate::release_executor::active_release_worker(&fixture.identity)
                .expect("worker remains registered while test A runs");
            if snapshot.current_child.as_deref() == Some("test_a") {
                break snapshot;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "test A telemetry missing"
            );
            thread::sleep(Duration::from_millis(25));
        };
        thread::sleep(Duration::from_millis(1100));
        let ticking = crate::release_executor::active_release_worker(&fixture.identity)
            .expect("worker remains registered while elapsed time ticks");
        assert!(ticking.gate_elapsed_secs > test_a.gate_elapsed_secs);
        assert!(ticking.last_activity_ago_secs >= 1);

        let test_b = loop {
            let snapshot = crate::release_executor::active_release_worker(&fixture.identity)
                .expect("worker remains registered while test B runs");
            if snapshot.current_child.as_deref() == Some("test_b") {
                break snapshot;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "test B telemetry missing"
            );
            thread::sleep(Duration::from_millis(25));
        };
        assert!(test_b.last_activity_ago_secs < ticking.last_activity_ago_secs);
        assert_ne!(test_a.recent_output, test_b.recent_output);
        assert!(test_b.recent_output.len() <= 8);

        let advanced = loop {
            let snapshot = crate::release_executor::active_release_worker(&fixture.identity)
                .expect("worker remains registered while the next gate runs");
            if snapshot.completed_gates == 1
                && snapshot.current_gate.as_deref() == Some("acceptance")
                && snapshot.current_child.as_deref() == Some("acceptance")
            {
                break snapshot;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "completed-gate telemetry did not advance"
            );
            thread::sleep(Duration::from_millis(25));
        };
        assert_eq!(advanced.total_gates, 2);
        assert_eq!(
            advanced.current_command.as_deref(),
            Some("cargo xtask acceptance")
        );
        assert_eq!(advanced.current_child.as_deref(), Some("acceptance"));
        assert!(advanced.process_alive);
        let progressed = ledger.load().unwrap().unwrap();
        assert_eq!(progressed.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            progressed.mutation.local_gates[0].state,
            SettlementState::Succeeded
        );
        assert_eq!(
            progressed.mutation.local_gates[1].state,
            SettlementState::Running
        );
        assert_eq!(
            fs::read(fixture.primary.join("base.txt")).unwrap(),
            primary_before
        );

        // Stop the long-running acceptance fixture through the typed worker-cancellation path;
        // cancellation is controller liveness, not a failed source-verification result.
        crate::release_executor::cancel_release_worker(&fixture.identity);
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while crate::release_executor::active_release_worker(&fixture.identity).is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "cancelled RRC task did not settle"
            );
            thread::sleep(Duration::from_millis(25));
        }
        let settled = ledger.load().unwrap().unwrap();
        assert_eq!(settled.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            settled.mutation.local_gates[0].state,
            SettlementState::Succeeded
        );
        assert_eq!(
            settled.mutation.local_gates[1].state,
            SettlementState::Running
        );
        assert!(settled.mutation.local_gates[1].evidence_ref.is_none());
        assert!(settled.failures.is_empty());
        assert!(!settled.liveness.blocked());
        assert!(ledger.superseded_path(&previous.epoch_id).exists());
    }

    #[test]
    fn matching_published_release_resumes_closeout_without_new_epoch_or_tag() {
        for state in [
            ReleaseRecoveryState::Published,
            ReleaseRecoveryState::Complete,
        ] {
            let fixture = active_epoch_fixture();
            let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
            let mut active = ledger.load().unwrap().unwrap();
            active.state = state;
            active.release_version = Some("0.24.5".into());
            active.mutation.version_after = Some("0.24.5".into());
            active.mutation.publication_verified = true;
            active.mutation.tag_pushed = true;
            active.mutation.tag_name = Some("v0.24.5".into());
            active.mutation.tag_object = Some("immutable-published-tag".into());
            ledger.save(&active).unwrap();
            let mut launches = 0;
            let outcome = admit_natural_release_with_launcher(
                &fixture.primary,
                "Release version 0.24.5.",
                &fixture.state,
                |workspace, _, _| {
                    launches += 1;
                    assert_eq!(
                        workspace.to_string_lossy(),
                        active.mutation.release_workspace.as_deref().unwrap()
                    );
                    Ok(())
                },
            )
            .unwrap();
            assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
            assert_eq!(launches, 1);
            let resumed = ledger.load().unwrap().unwrap();
            assert_eq!(resumed.epoch_id, active.epoch_id);
            assert_eq!(resumed.state, state);
            assert_eq!(resumed.mutation.tag_object, active.mutation.tag_object);
            assert_eq!(resumed.retry_budget, active.retry_budget);
        }
    }

    #[test]
    fn completed_version_does_not_claim_a_different_objective_was_released() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.state = ReleaseRecoveryState::Complete;
        active.release_version = Some("0.24.5".into());
        active.release_commit = active.mutation.source_commit.clone();
        active.mutation.publication_verified = true;
        active.mutation.closeout_receipt = Some(crate::release_closeout::CloseoutReceipt {
            report: "docs/foundation/release-v0.24.5-closeout.md".into(),
            registry_url: None,
            registry_blob: None,
        });
        active.mutation.objective_id = Some("another-objective".into());
        ledger.save(&active).unwrap();
        let before = fs::read(ledger.path()).unwrap();
        let result = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| panic!("published version conflict must not launch"),
        )
        .unwrap();
        assert!(matches!(result, NaturalReleaseAdmission::Clarification(_)));
        assert_eq!(fs::read(ledger.path()).unwrap(), before);
    }

    #[test]
    fn unrelated_active_release_asks_one_human_clarification_without_mutation() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.objective_id = Some("different-completed-objective".into());
        active.mutation.objective_label = Some("Historical unrelated release".into());
        active.mutation.version_before = Some("0.23.9".into());
        active.mutation.version_after = Some("0.24.0".into());
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |_, _, _| panic!("unrelated active release must not launch"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("unrelated objective must clarify");
        };
        assert_eq!(message.matches('?').count(), 1);
        assert!(!message.contains("epoch"));
        assert!(!message.contains("DiagnosingLocalFailure"));
        assert!(message.contains("0.23.9 -> 0.24.0"));
        assert!(message.contains("Historical unrelated release"));
        assert!(message.contains("0.24.4 -> 0.24.5"));
        assert!(message.contains("RRC autonomy repair"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
    }

    #[test]
    fn explicit_version_supersedes_same_objective_before_remote_mutation() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let previous = ledger.load().unwrap().unwrap();
        let mut launched = None;
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.25.0.",
            &fixture.state,
            |workspace, _, _| {
                launched = Some(workspace);
                Ok(())
            },
        )
        .unwrap();
        let NaturalReleaseAdmission::Started(message) = outcome else {
            panic!("a new explicit target must supersede a reversible stale epoch");
        };
        assert!(message.contains("preserved as superseded evidence"));
        let replacement = ledger.load().unwrap().unwrap();
        assert_ne!(replacement.epoch_id, previous.epoch_id);
        assert_eq!(
            replacement.objective.version,
            ReleaseVersionSelector::Exact("0.25.0".into())
        );
        assert_eq!(
            launched.unwrap(),
            PathBuf::from(replacement.mutation.release_workspace.as_deref().unwrap())
        );
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&previous.epoch_id)).unwrap())
                .unwrap();
        assert_eq!(archive.record.epoch_id, previous.epoch_id);
        assert_eq!(
            archive.record.objective.version,
            ReleaseVersionSelector::Patch
        );
    }

    #[test]
    fn same_target_candidate_only_epoch_adopts_newer_canonical_descendant() {
        let fixture = active_epoch_fixture();
        let newer = add_same_objective_descendant_completion(&fixture, None);
        let newer_head = git_text(&newer, &["rev-parse", "HEAD"]);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.mutation.candidate_push_ref = Some("refs/heads/main@historical-candidate".into());
        ledger.save(&active).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release v0.24.5.",
            &fixture.state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
        let replacement = ledger.load().unwrap().unwrap();
        assert_eq!(
            replacement.objective.version,
            ReleaseVersionSelector::Exact("0.24.5".into())
        );
        assert_eq!(
            replacement.mutation.source_commit.as_deref(),
            Some(newer_head.as_str())
        );
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&active.epoch_id)).unwrap())
                .unwrap();
        assert!(archive.record.mutation.candidate_pushed);
        assert_eq!(
            archive.record.mutation.candidate_push_ref,
            active.mutation.candidate_push_ref
        );
    }

    #[test]
    fn same_target_remote_ci_evidence_does_not_force_clarification() {
        let fixture = active_epoch_fixture();
        let newer = add_same_objective_descendant_completion(&fixture, None);
        let newer_head = git_text(&newer, &["rev-parse", "HEAD"]);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.state = ReleaseRecoveryState::Escalated;
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.mutation.publication_run_id = Some(8001);
        active.required_gates.push(GateRecord {
            run_state: Some(crate::release_recovery::JobState::Success),
            name: "canonical".into(),
            head_sha: active.release_commit.clone().unwrap(),
            run_id: Some(7001),
            run_attempt: Some(1),
            jobs: vec![JobSnapshot {
                workflow_id: 41,
                run_id: 7001,
                attempt: 1,
                job_id: 91,
                workflow_name: "canonical".into(),
                job_name: "linux".into(),
                platform: Some("linux-x86_64".into()),
                state: JobState::Failure,
                failed_step: Some("test".into()),
                url: "https://github.example/jobs/91".into(),
            }],
            url: Some("https://github.example/runs/7001".into()),
        });
        ledger.save(&active).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
        assert_eq!(
            ledger
                .load()
                .unwrap()
                .unwrap()
                .mutation
                .source_commit
                .as_deref(),
            Some(newer_head.as_str())
        );
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&active.epoch_id)).unwrap())
                .unwrap();
        assert_eq!(archive.record.required_gates, active.required_gates);
        assert_eq!(
            archive.record.mutation.publication_run_id,
            active.mutation.publication_run_id
        );
    }

    #[test]
    fn same_target_remote_failure_evidence_does_not_force_clarification() {
        let fixture = active_epoch_fixture();
        add_same_objective_descendant_completion(&fixture, None);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.state = ReleaseRecoveryState::Escalated;
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.failures.push(historical_remote_failure(
            active.release_commit.as_deref().unwrap(),
        ));
        ledger.save(&active).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(outcome, NaturalReleaseAdmission::Started(_)));
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&active.epoch_id)).unwrap())
                .unwrap();
        assert_eq!(archive.record.failures, active.failures);
    }

    #[test]
    fn different_release_target_with_pushed_candidate_requires_clarification() {
        let fixture = active_epoch_fixture();
        let canonical = add_canonical_superseding_completion(&fixture);
        let canonical_head = git_text(&canonical, &["rev-parse", "HEAD"]);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.2".into());
        active.mutation.version_after = Some("0.24.3".into());
        active.mutation.candidate_pushed = true;
        active.mutation.candidate_push_ref = Some("refs/heads/main@candidate".into());
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();
        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release all completed work as the next patch.",
            &fixture.state,
            |_, _, _| panic!("irreversible state must not launch a replacement"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("irreversible state must clarify");
        };
        assert_eq!(message.matches('?').count(), 1);
        assert!(message.contains("0.24.2 -> 0.24.3"));
        assert!(message.contains("RRC autonomy repair"));
        assert!(message.contains("historical candidate or CI evidence"));
        assert!(message.contains("Corrected v0.24.4 prerelease integration"));
        assert!(message.contains("0.24.4 -> 0.24.5"));
        assert!(!message.contains("epoch"));
        assert!(!message.contains(&canonical_head));
        assert!(!message.contains("refs/heads"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
        assert!(!ledger.superseded_path(&active.epoch_id).exists());
    }

    #[test]
    fn pushed_tag_still_requires_irreversible_safety() {
        let fixture = active_epoch_fixture();
        add_same_objective_descendant_completion(&fixture, None);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.mutation.tag_name = Some("v0.24.5".into());
        active.mutation.tag_object = Some("tag-object".into());
        active.mutation.tag_pushed = true;
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release v0.24.5.",
            &fixture.state,
            |_, _, _| panic!("pushed target tag must block replacement"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("pushed target tag must clarify");
        };
        assert!(message.contains("pushed target release tag"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
        assert!(!ledger.superseded_path(&active.epoch_id).exists());
    }

    #[test]
    fn publication_still_requires_irreversible_safety() {
        let fixture = active_epoch_fixture();
        add_same_objective_descendant_completion(&fixture, None);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.mutation.publication_run_id = Some(8001);
        active.mutation.publication_verified = true;
        active.mutation.published_asset_names = vec!["agent-vesper.tar.gz".into()];
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| panic!("published target release must block replacement"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("published target release must clarify");
        };
        assert!(message.contains("published target release"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
        assert!(!ledger.superseded_path(&active.epoch_id).exists());
    }

    #[test]
    fn same_target_non_descendant_requires_clarification() {
        let fixture = active_epoch_fixture();
        add_same_objective_non_descendant_completion(&fixture);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| panic!("non-descendant replacement must not launch"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("non-descendant replacement must clarify");
        };
        assert!(message.contains("not a strict descendant"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
        assert!(!ledger.superseded_path(&active.epoch_id).exists());
    }

    #[test]
    fn repository_mismatch_requires_clarification_without_mutation() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let active = ledger.load().unwrap().unwrap();
        let persisted = fs::read(ledger.path()).unwrap();
        let foreign = fixture._temp.path().join("foreign-repository");
        fs::create_dir_all(&foreign).unwrap();
        run_git(&foreign, &["init", "-b", "main"]);
        run_git(&foreign, &["config", "user.email", "rrc@example.invalid"]);
        run_git(&foreign, &["config", "user.name", "RRC Test"]);
        fs::write(
            foreign.join("Cargo.toml"),
            "[workspace]\nmembers = []\n[workspace.package]\nversion = \"0.24.4\"\n",
        )
        .unwrap();
        run_git(&foreign, &["add", "."]);
        run_git(&foreign, &["commit", "-m", "foreign source"]);
        let source_commit = git_text(&foreign, &["rev-parse", "HEAD"]);
        let source = ReleaseSourceChoice {
            source_workspace: foreign,
            source_commit: source_commit.clone(),
            objective_id: Some("rrc-autonomy-repair".into()),
            objective_label: Some("RRC autonomy repair".into()),
            variant_label: Some("Foreign source".into()),
            variant_id: Some("foreign-source".into()),
            canonical_release_source: true,
            supersedes: Vec::new(),
            evidence_reports: Vec::new(),
            base_sha: source_commit.clone(),
            intended_commits: vec![source_commit],
            intended_diff_sha256: "foreign-diff".into(),
            branch_ref: "main".into(),
        };

        let outcome = reconcile_active_epoch(
            active,
            &source,
            &ReleaseVersionSelector::Exact("0.24.5".into()),
            &fixture.state,
        )
        .unwrap();
        let ActiveEpochReconciliation::Clarify(message) = outcome else {
            panic!("repository mismatch must clarify");
        };
        assert!(message.contains("different repository identity"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
    }

    #[test]
    fn same_target_remote_evidence_requires_descendant_for_workspace_replacement() {
        let fixture = active_epoch_fixture();
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.mutation.candidate_push_ref = Some("refs/heads/main@historical-candidate".into());
        let release_workspace = PathBuf::from(
            active
                .mutation
                .release_workspace
                .as_deref()
                .expect("release workspace"),
        );
        fs::write(
            release_workspace.join("unplanned.txt"),
            "workspace no longer matches the persisted epoch\n",
        )
        .unwrap();
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| panic!("remote-history workspace replacement must not launch"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("remote-history workspace replacement must clarify");
        };
        assert!(message.contains("strict descendant"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
        assert!(!ledger.superseded_path(&active.epoch_id).exists());
    }

    #[test]
    fn same_target_noncanonical_descendant_requires_clarification() {
        let fixture = active_epoch_fixture();
        add_same_objective_descendant_completion_with_canonical(&fixture, None, false);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        ledger.save(&active).unwrap();
        let persisted = fs::read(ledger.path()).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release version 0.24.5.",
            &fixture.state,
            |_, _, _| panic!("noncanonical replacement must not launch"),
        )
        .unwrap();
        let NaturalReleaseAdmission::Clarification(message) = outcome else {
            panic!("noncanonical replacement must clarify");
        };
        assert!(message.contains("not bound as the canonical release source"));
        assert_eq!(fs::read(ledger.path()).unwrap(), persisted);
        assert!(!ledger.superseded_path(&active.epoch_id).exists());
    }

    #[test]
    fn exact_live_admission_sentence_adopts_same_unpublished_target() {
        let fixture = active_epoch_fixture();
        let newer = add_same_objective_descendant_completion(&fixture, Some("0.24.5"));
        let newer_head = git_text(&newer, &["rev-parse", "HEAD"]);
        let ledger = ReleaseLedger::open(fixture.state.clone(), &fixture.identity).unwrap();
        let mut active = ledger.load().unwrap().unwrap();
        active.mutation.version_before = Some("0.24.4".into());
        active.mutation.version_after = Some("0.24.5".into());
        active.mutation.candidate_pushed = true;
        active.mutation.candidate_push_ref = Some("refs/heads/main@historical-candidate".into());
        active.required_gates.push(GateRecord {
            run_state: Some(crate::release_recovery::JobState::Success),
            name: "canonical".into(),
            head_sha: active.release_commit.clone().unwrap(),
            run_id: Some(7001),
            run_attempt: Some(1),
            jobs: Vec::new(),
            url: Some("https://github.example/runs/7001".into()),
        });
        assert!(!active.mutation.tag_pushed);
        assert!(!active.mutation.publication_verified);
        ledger.save(&active).unwrap();

        let outcome = admit_natural_release_with_launcher(
            &fixture.primary,
            "Release the current unpublished v0.24.5 from main.",
            &fixture.state,
            |_, _, _| Ok(()),
        )
        .unwrap();
        let NaturalReleaseAdmission::Started(message) = outcome else {
            panic!("the exact live sentence must not clarify");
        };
        assert!(!message.contains("clarif"));
        let replacement = ledger.load().unwrap().unwrap();
        assert_eq!(
            replacement.objective.version,
            ReleaseVersionSelector::Exact("0.24.5".into())
        );
        assert_ne!(replacement.objective.version.target_label(), "0.24.6");
        assert_eq!(
            replacement.mutation.source_commit.as_deref(),
            Some(newer_head.as_str())
        );
        assert!(
            fs::read_to_string(
                PathBuf::from(replacement.mutation.release_workspace.as_deref().unwrap())
                    .join("Cargo.toml")
            )
            .unwrap()
            .contains("version = \"0.24.5\"")
        );
        let archive: SupersededEpochEvidence =
            serde_json::from_slice(&fs::read(ledger.superseded_path(&active.epoch_id)).unwrap())
                .unwrap();
        assert!(archive.record.mutation.candidate_pushed);
        assert_eq!(archive.record.required_gates, active.required_gates);
    }

    fn write_objective_marker(
        root: &Path,
        objective_id: &str,
        objective_label: &str,
        variant_label: &str,
        completed_at: &str,
    ) {
        write_objective_marker_options(
            root,
            objective_id,
            objective_label,
            variant_label,
            completed_at,
            ObjectiveMarkerOptions::default(),
        );
    }

    #[derive(Default)]
    struct ObjectiveMarkerOptions<'a> {
        variant_id: Option<&'a str>,
        canonical_release_source: bool,
        supersedes: &'a [&'a str],
    }

    fn write_objective_marker_options(
        root: &Path,
        objective_id: &str,
        objective_label: &str,
        variant_label: &str,
        completed_at: &str,
        options: ObjectiveMarkerOptions<'_>,
    ) {
        let path = root.join("docs/foundation/release-objective-provenance.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            root.join("docs/foundation/objective-evidence.md"),
            format!("# {objective_label}\n"),
        )
        .unwrap();
        fs::write(
            path,
            serde_json::json!({
                "version": 1,
                "objective_id": objective_id,
                "objective_label": objective_label,
                "variant_label": variant_label,
                "variant_id": options.variant_id,
                "canonical_release_source": options.canonical_release_source,
                "supersedes": options.supersedes,
                "completed_at": completed_at,
                "evidence_reports": ["docs/foundation/objective-evidence.md"]
            })
            .to_string(),
        )
        .unwrap();
    }

    struct ProvenanceResolutionFixture {
        _temp: tempfile::TempDir,
        primary: PathBuf,
        state: PathBuf,
    }

    fn provenance_resolution_fixture() -> ProvenanceResolutionFixture {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("repository");
        let state = temp.path().join("state");
        fs::create_dir_all(&primary).unwrap();
        run_git(&primary, &["init", "-b", "main"]);
        run_git(&primary, &["config", "user.email", "rrc@example.invalid"]);
        run_git(&primary, &["config", "user.name", "RRC Test"]);
        run_git(
            &primary,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ],
        );
        fs::write(primary.join("base.txt"), "base\n").unwrap();
        run_git(&primary, &["add", "."]);
        run_git(&primary, &["commit", "-m", "base"]);
        ProvenanceResolutionFixture {
            _temp: temp,
            primary,
            state,
        }
    }

    struct ProvenanceVariant<'a> {
        implementation: &'a str,
        variant_label: &'a str,
        commit_subject: &'a str,
        objective_id: &'a str,
        canonical_release_source: bool,
        supersedes: &'a [&'a str],
    }

    impl<'a> ProvenanceVariant<'a> {
        fn new(implementation: &'a str, variant_label: &'a str, commit_subject: &'a str) -> Self {
            Self {
                implementation,
                variant_label,
                commit_subject,
                objective_id: "rrc-source-resolution-ux-repair-20260930",
                canonical_release_source: false,
                supersedes: &[],
            }
        }
    }

    fn add_provenance_variant(
        primary: &Path,
        parent: &Path,
        name: &str,
        variant: ProvenanceVariant<'_>,
    ) -> PathBuf {
        let path = parent.join(name);
        run_git(
            primary,
            &["worktree", "add", "-b", name, path.to_str().unwrap()],
        );
        fs::write(path.join("implementation.txt"), variant.implementation).unwrap();
        write_objective_marker_options(
            &path,
            variant.objective_id,
            "Integrated release objective",
            variant.variant_label,
            "2026-10-01T00:00:00Z",
            ObjectiveMarkerOptions {
                variant_id: Some(name),
                canonical_release_source: variant.canonical_release_source,
                supersedes: variant.supersedes,
            },
        );
        run_git(&path, &["add", "."]);
        run_git(&path, &["commit", "-m", variant.commit_subject]);
        path
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {} failed", args.join(" "));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

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
                run_state: Some(crate::release_recovery::JobState::Success),
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
                run_state: None,
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
    fn missing_initial_workflow_runs_remain_an_active_matrix_wait() {
        let mut record = record();
        let adapter = FakeGitHub {
            gates: Vec::new(),
            log: String::new(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::WaitingForMatrix);
        assert!(record.failures.is_empty());
    }

    #[test]
    fn repeated_incomplete_matrix_refreshes_do_not_spend_repair_watchdog_actions() {
        let mut record = record();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                run_state: Some(crate::release_recovery::JobState::Success),
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                jobs: vec![failed_job(JobState::InProgress)],
                url: None,
            }],
            log: String::new(),
        };

        for _ in 0..(STAGNATION_ACTION_LIMIT + 2) {
            refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        }

        assert_eq!(record.state, ReleaseRecoveryState::WaitingForMatrix);
        assert!(record.consecutive_stagnant_actions >= STAGNATION_ACTION_LIMIT);
        assert!(record.failures.is_empty());
    }

    #[test]
    fn a_long_running_platform_matrix_is_not_active_repair_stagnation() {
        let mut record = record();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                run_state: Some(JobState::InProgress),
                jobs: vec![failed_job(JobState::InProgress)],
                url: None,
            }],
            log: String::new(),
        };
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        record.last_progress_at = Utc::now() - chrono::Duration::minutes(40);
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::WaitingForMatrix);
        assert!(record.failures.is_empty());
        assert_eq!(record.retry_budget.full_gate_used, 0);
        record.last_progress_at = Utc::now() - chrono::Duration::hours(3);
        refresh_remote_evidence(&mut record, "owner/repo", &adapter).unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::Escalated);
    }

    #[test]
    fn missing_runs_cannot_reuse_a_stale_running_matrix_to_avoid_timeout() {
        let mut record = record();
        record.state = ReleaseRecoveryState::WaitingForMatrix;
        record.last_progress_at = Utc::now() - chrono::Duration::minutes(21);
        record.required_gates.push(GateRecord {
            name: "gate".into(),
            head_sha: "abcdef123456".into(),
            run_id: Some(2),
            run_attempt: Some(1),
            run_state: Some(JobState::InProgress),
            jobs: vec![failed_job(JobState::InProgress)],
            url: None,
        });
        refresh_remote_evidence(
            &mut record,
            "owner/repo",
            &FakeGitHub {
                gates: Vec::new(),
                log: String::new(),
            },
        )
        .unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::Escalated);
        assert!(record.required_gates.is_empty());
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
        for (unchanged, expected) in [
            (0, Duration::from_secs(20)),
            (1, Duration::from_secs(40)),
            (2, Duration::from_secs(80)),
            (3, Duration::from_secs(120)),
            (u8::MAX, Duration::from_secs(120)),
        ] {
            assert_eq!(
                next_directive(&record, unchanged),
                ReleaseDirective::RefreshMatrix { after: expected }
            );
        }
        record.state = ReleaseRecoveryState::WaitingForMatrix;
        assert_eq!(
            next_directive(&record, 3),
            ReleaseDirective::RefreshMatrix {
                after: MAX_POLL_INTERVAL
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
    fn verified_repair_batch_records_every_failure_family_before_retry() {
        let mut record = classified_record();
        let mut second_failure = record.failures[0].clone();
        second_failure.fingerprint = FailureFingerprint("second-family".into());
        second_failure.job_id = 31;
        second_failure.job_name = "second job".into();
        record.failures.push(second_failure);

        let mut first = repair_for(&record, FocusedProofStatus::Passed);
        first.fingerprint = record.failures[0].fingerprint.clone();
        first.causal_family = "workflow:first job".into();
        let mut second = repair_for(&record, FocusedProofStatus::Passed);
        second.causal_family = "workflow:second job".into();

        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::VerifiedRepairBatch(vec![first, second]),
        )
        .unwrap();

        assert_eq!(record.state, ReleaseRecoveryState::RetryAdmissible);
        assert_eq!(record.repair_attempts.len(), 2);
        assert!(record.retry_admission(RetryKind::FullGate).admitted);
        assert!(
            record
                .repair_attempts
                .iter()
                .any(|repair| repair.causal_family == "workflow:first job")
        );
        assert!(
            record
                .repair_attempts
                .iter()
                .any(|repair| repair.causal_family == "workflow:second job")
        );
    }

    #[test]
    fn malformed_verified_repair_batch_is_transactional() {
        let mut record = classified_record();
        let before = record.clone();
        let first = repair_for(&record, FocusedProofStatus::Passed);
        let mut malformed = repair_for(&record, FocusedProofStatus::Passed);
        malformed.source_commit_after = Some("111111111111".into());

        let error = apply_controller_event(
            &mut record,
            ReleaseControllerEvent::VerifiedRepairBatch(vec![first, malformed]),
        )
        .unwrap_err();

        assert!(error.to_string().contains("one shared changed commit"));
        assert_eq!(record, before);
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
        let mut record = published_fixture();
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
            run_state: Some(crate::release_recovery::JobState::Success),
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

    #[test]
    fn remote_dispatch_starts_a_fresh_matrix_progress_window() {
        let mut record = start_release("repo", "patch", "main", "abcdef123456").unwrap();
        record.objective.required_gate_names = vec!["gate".into()];
        record
            .transition(
                ReleaseRecoveryState::CandidateReady,
                "abcdef123456",
                "local verification passed",
                vec![],
                None,
            )
            .unwrap();
        record.last_progress_at = Utc::now()
            - chrono::Duration::from_std(STAGNATION_TIME_LIMIT + Duration::from_secs(1)).unwrap();

        apply_controller_event(
            &mut record,
            ReleaseControllerEvent::RemoteGateDispatched(vec!["github:push".into()]),
        )
        .unwrap();

        assert_eq!(record.state, ReleaseRecoveryState::RemoteGateRunning);
        assert_eq!(record.consecutive_stagnant_actions, 0);
        assert!(!record.remote_matrix_watch_timed_out(Utc::now()));
    }

    #[test]
    fn watchdog_liveness_survives_ledger_round_trip_and_drives_shared_headline() {
        let temporary = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temporary.path(), "repo").unwrap();
        let mut record = record();
        let error = RrcError::WatchdogStalled {
            operation: "focused repair agent".into(),
            limit_seconds: 600,
        };
        record.note_liveness_failure(&error);
        ledger.save(&record).unwrap();

        let mut loaded = ledger.load().unwrap().expect("persisted record");
        loaded.refresh_progress();
        assert_eq!(loaded.liveness.state, ReleaseLivenessState::Stalled);
        assert_eq!(loaded.liveness.operation, "focused repair agent");
        assert!(loaded.progress.headline.contains("Paused"));
        assert!(
            loaded.progress.headline.contains("without progress"),
            "TUI and ACP shared projection must retain the truthful stop reason"
        );
        assert!(
            loaded
                .progress
                .tasks
                .iter()
                .all(|task| task.state != ReleaseProgressState::Running),
            "a stopped controller stage must not remain Running in the shared projection"
        );
        assert_eq!(
            loaded.state,
            ReleaseRecoveryState::RemoteGateRunning,
            "liveness failure must not be fabricated as CI/source settlement"
        );
    }

    #[test]
    fn dead_owner_is_recoverable_and_not_active_or_a_source_failure() {
        let mut record = start_release("repo", "0.24.5", "main", "abcdef123456").unwrap();
        record.note_liveness_active("preparing release target");
        assert_eq!(record.liveness.owner_pid, Some(std::process::id()));
        assert!(!settle_absent_owner(&mut record, true));
        assert_eq!(record.liveness.state, ReleaseLivenessState::Active);
        assert!(settle_absent_owner(&mut record, false));
        assert_eq!(record.liveness.state, ReleaseLivenessState::OwnerExited);
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        assert!(record.failures.is_empty());
        assert!(
            record.progress.headline.contains("not running"),
            "{}",
            record.progress.headline
        );
        assert!(
            record
                .progress
                .tasks
                .iter()
                .all(|task| task.state != ReleaseProgressState::Running)
        );
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(
            !foreign_process_alive(pid),
            "exited process {pid} still looked alive"
        );
    }

    #[test]
    fn last_green_missing_or_skipped_matching_job_preserves_unknown_context() {
        for skipped in [false, true] {
            let mut record = classified_record();
            let mut job = failed_job(if skipped {
                JobState::Skipped
            } else {
                JobState::Success
            });
            if !skipped {
                job.job_name = "different-platform-lane".into();
            }
            let adapter = FakeGitHub {
                gates: vec![GateRecord {
                    name: "gate".into(),
                    head_sha: "lastgreen".into(),
                    run_id: Some(1),
                    run_attempt: Some(1),
                    run_state: Some(JobState::Success),
                    jobs: vec![job],
                    url: None,
                }],
                log: String::new(),
            };
            assert!(
                compare_with_last_green(&mut record, "owner/repo", "lastgreen", false, &adapter)
                    .is_err()
            );
            assert_eq!(record.failures.last().unwrap().exists_on_last_green, None);
            assert_eq!(record.failures.last().unwrap().related_source_touched, None);
        }
    }

    fn published_fixture() -> ReleaseRecoveryRecord {
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
                run_state: Some(crate::release_recovery::JobState::Success),
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
        record
    }

    #[test]
    fn missing_runs_are_pending_evidence_instead_of_an_executor_error() {
        let mut r = record();
        refresh_remote_evidence(
            &mut r,
            "owner/repo",
            &FakeGitHub {
                gates: vec![],
                log: String::new(),
            },
        )
        .unwrap();
        assert_eq!(r.state, ReleaseRecoveryState::WaitingForMatrix);
    }

    #[test]
    fn required_skipped_job_never_opens_tag_admission() {
        let mut r = record();
        let adapter = FakeGitHub {
            gates: vec![GateRecord {
                name: "gate".into(),
                head_sha: "abcdef123456".into(),
                run_id: Some(2),
                run_attempt: Some(1),
                run_state: Some(crate::release_recovery::JobState::Success),
                jobs: vec![failed_job(JobState::Skipped)],
                url: None,
            }],
            log: String::new(),
        };
        refresh_remote_evidence(&mut r, "owner/repo", &adapter).unwrap();
        assert_ne!(r.state, ReleaseRecoveryState::RemoteGatesGreen);
    }

    #[test]
    fn inconclusive_source_evidence_cannot_confirm_an_outage() {
        assert!(!matches!(
            classify_external_health(ExternalHealthEvidence {
                repository_checks_green: true,
                source_explanation_absent: false,
                infrastructure_failures: 2,
                official_degraded: true,
                direct_api_failure: true,
                ..Default::default()
            }),
            ExternalHealthVerdict::Confirmed(_)
        ));
    }

    #[test]
    fn persisted_strings_are_redacted_at_the_storage_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let mut r = record();
        r.transitions[0].reason =
            "https://alice:canary-password@example.invalid/?token=canary-query".into();
        ledger.save(&r).unwrap();
        let bytes = std::fs::read_to_string(ledger.path()).unwrap();
        assert!(!bytes.contains("canary-password"));
        assert!(!bytes.contains("canary-query"));
        // A schema-v1 checkpoint from an older writer must be sanitized when archived.
        std::fs::write(ledger.path(), serde_json::to_vec(&r).unwrap()).unwrap();
        let mut replacement = r.clone();
        replacement.epoch_id = "replacement-epoch".into();
        replacement.created_at = r.created_at + chrono::Duration::milliseconds(1);
        replacement.updated_at = replacement.created_at;
        ledger.save(&replacement).unwrap();
        let archive = ledger.root.join(format!(
            "{}-{}.json",
            ledger.repo_key,
            sha256_hex(r.epoch_id.as_bytes())
        ));
        let archived = std::fs::read_to_string(archive).unwrap();
        assert!(!archived.contains("canary-password"));
        assert!(!archived.contains("canary-query"));
        let old: ReleaseRecoveryRecord = serde_json::from_str(&archived).unwrap();
        assert_eq!(old.epoch_id, r.epoch_id);
    }

    #[test]
    fn stale_writer_cannot_overwrite_cancelled_checkpoint() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let r = record();
        ledger.save(&r).unwrap();
        let mut cancelled = r.clone();
        cancelled
            .transition(
                ReleaseRecoveryState::Cancelled,
                "abcdef123456",
                "cancel",
                vec![],
                None,
            )
            .unwrap();
        ledger.save(&cancelled).unwrap();
        assert!(ledger.save(&r).is_err());
        assert_eq!(
            ledger.load().unwrap().unwrap().state,
            ReleaseRecoveryState::Cancelled
        );
    }

    #[test]
    fn rejected_repair_event_does_not_partially_mutate_the_record() {
        let mut r = classified_record();
        let before = r.clone();
        let mut repair = repair_for(&r, FocusedProofStatus::Passed);
        repair.hypothesis.clear();
        assert!(
            apply_controller_event(&mut r, ReleaseControllerEvent::VerifiedRepair(repair)).is_err()
        );
        assert_eq!(r, before);
    }

    #[test]
    fn unchanged_commit_is_not_a_verified_source_repair() {
        let mut r = classified_record();
        let mut repair = repair_for(&r, FocusedProofStatus::Passed);
        repair.source_commit_after = Some(repair.source_commit_before.clone());
        assert!(
            apply_controller_event(&mut r, ReleaseControllerEvent::VerifiedRepair(repair)).is_err()
        );
    }

    #[test]
    fn terminal_epoch_cannot_admit_a_diagnostic_rerun() {
        let mut r = classified_record();
        r.state = ReleaseRecoveryState::Cancelled;
        assert!(!r.retry_admission(RetryKind::TargetedDiagnostic).admitted);
        r.state = ReleaseRecoveryState::Escalated;
        assert!(!r.retry_admission(RetryKind::TargetedDiagnostic).admitted);
    }

    #[test]
    fn a_new_workflow_run_can_start_at_attempt_one() {
        let mut r = classified_record();
        r.required_gates[0].run_attempt = Some(3);
        let mut fresh = r.required_gates[0].clone();
        fresh.run_id = Some(99);
        fresh.run_attempt = Some(1);
        assert!(r.apply_matrix(fresh).is_ok());
        let mut stale = r.required_gates[0].clone();
        stale.run_id = Some(2);
        stale.run_attempt = Some(4);
        assert!(r.apply_matrix(stale).is_err());
    }

    #[test]
    fn causal_error_after_large_successful_log_prefix_is_preserved() {
        let log = format!(
            "{}\nerror[E0308]: mismatched types",
            "passing tests\n".repeat(1000)
        );
        assert!(first_causal_excerpt(&log).contains("error[E0308]"));
    }

    #[test]
    fn cancelled_epoch_resumes_the_same_run_and_job_identity() {
        let mut r = classified_record();
        let old = r.clone();
        r.transition(
            ReleaseRecoveryState::Cancelled,
            "abcdef123456",
            "cancel",
            vec![],
            None,
        )
        .unwrap();
        r.resume_cancelled().unwrap();
        assert_eq!(r.epoch_id, old.epoch_id);
        assert_eq!(r.required_gates, old.required_gates);
        assert_eq!(r.state, old.state);
        assert_eq!(r.retry_budget, old.retry_budget);
    }

    #[test]
    fn controller_owner_is_exclusive_across_independent_ledger_handles() {
        let temp = tempfile::tempdir().unwrap();
        let first = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let second = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let owner = first.acquire_owner().unwrap();
        assert!(matches!(second.acquire_owner(), Err(RrcError::Busy)));
        drop(owner);
        assert!(second.acquire_owner().is_ok());
    }

    #[test]
    fn cancelled_checkpoint_can_be_explicitly_resumed_without_erasing_counters() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let mut r = classified_record();
        r.retry_budget.targeted_diagnostic_used = 1;
        r.transition(
            ReleaseRecoveryState::Cancelled,
            "abcdef123456",
            "cancel",
            vec![],
            None,
        )
        .unwrap();
        ledger.save(&r).unwrap();
        let mut resumed = ledger.load().unwrap().unwrap();
        resumed.resume_cancelled().unwrap();
        ledger.save(&resumed).unwrap();
        assert_eq!(
            ledger
                .load()
                .unwrap()
                .unwrap()
                .retry_budget
                .targeted_diagnostic_used,
            1
        );
    }

    #[test]
    fn a_running_workflow_with_only_terminal_visible_jobs_is_incomplete() {
        let mut r = classified_record();
        r.required_gates[0].run_state = Some(JobState::InProgress);
        assert!(!r.matrix_complete());
    }

    #[test]
    fn old_attempt_cannot_satisfy_a_reserved_infrastructure_retry() {
        let mut r = classified_record();
        r.state = ReleaseRecoveryState::RemoteGateRunning;
        r.retry_run_floors = vec![(2, 2)];
        let old_gate = r.required_gates[0].clone();
        r.required_gates[0].run_attempt = Some(2);
        r.required_gates[0].run_state = None;
        r.required_gates[0].jobs.clear();
        refresh_remote_evidence(
            &mut r,
            "owner/repo",
            &FakeGitHub {
                gates: vec![old_gate],
                log: "error".into(),
            },
        )
        .unwrap();
        assert_eq!(r.state, ReleaseRecoveryState::WaitingForMatrix);
        assert!(!r.matrix_complete());
    }

    #[test]
    fn quoted_credentials_basic_auth_and_signed_urls_are_redacted() {
        let text = redact_secrets(
            r#"{"api_key": "quoted-canary"} Authorization: Basic basic-canary https://bob:url-canary@example.test/?X-Amz-Signature=signed-canary"#,
        );
        for canary in [
            "quoted-canary",
            "basic-canary",
            "url-canary",
            "signed-canary",
        ] {
            assert!(!text.contains(canary), "{text}");
        }
    }

    #[test]
    fn context_and_process_numbers_do_not_change_causal_fingerprints() {
        let actual = "2026-10-05T06:44:23.0617838Z thread 'rrc_fixture' panicked at tests/rrc_fixture.rs:42: rrc_fixture_assertion_42\n2026-10-05T06:44:23.0620527Z authorization: ***\n2026-10-05T06:44:23.0621907Z Error: Process completed with exit code 1";
        let reordered = "2026-10-05T06:47:24.7003196Z thread 'rrc_fixture' panicked at tests/rrc_fixture.rs:42: rrc_fixture_assertion_42\n2026-10-05T06:47:24.7005015Z Error: Process completed with exit code 1\n2026-10-05T06:47:24.7007858Z authorization: ***";
        assert_eq!(
            failure_fingerprint("workflow", "job", None, None, None, actual),
            failure_fingerprint("workflow", "job", None, None, None, reordered)
        );
        let first = failure_fingerprint(
            "CI",
            "test",
            None,
            None,
            None,
            "test successful 42\nthread 'same_test' (1234) panicked at src/lib.rs:42:3: assertion failed",
        );
        let second = failure_fingerprint(
            "CI",
            "test",
            None,
            None,
            None,
            "test successful 88\nthread 'same_test' (8765) panicked at src/lib.rs:50:3: assertion failed",
        );
        assert_eq!(first, second);
    }

    #[test]
    fn pagination_reads_all_jobs_and_refuses_duplicate_page_rows() {
        let mut calls = 0;
        let value = paginated_with_fetch("jobs?per_page=100", "jobs", |endpoint| {
            calls += 1;
            assert!(endpoint.ends_with(&format!("page={calls}")));
            Ok(serde_json::json!({"total_count": 2, "jobs": [{"id": calls}]}))
        })
        .unwrap();
        assert_eq!(value["jobs"].as_array().unwrap().len(), 2);
        assert!(
            paginated_with_fetch("jobs?per_page=100", "jobs", |_| Ok(
                serde_json::json!({"total_count": 2, "jobs": [{"id": 1}]})
            ))
            .is_err()
        );
    }

    #[test]
    fn github_adapter_uses_exact_attempt_and_main_push_provenance() {
        let sha = "a".repeat(40);
        let run = serde_json::json!({"id": 99, "workflow_id": 1, "head_sha": sha, "name": "custom evaluated candidate title", "event": "push", "head_branch": "main", "path": ".github/workflows/ci.yml", "run_attempt": 3, "status": "in_progress", "conclusion": null});
        let gates = matrix_with_fetch("owner/repo", &sha, |endpoint, key| {
            if key == "workflow_runs" { Ok(serde_json::json!({"workflow_runs": [run.clone()]})) }
            else if key.is_empty() { Ok(serde_json::json!({"id": 1, "name": "pull-request-validation"})) }
            else { assert!(endpoint.contains("/attempts/3/jobs?")); Ok(serde_json::json!({"jobs": [{"id": 5, "run_attempt": 3, "name": "quality", "status": "completed", "conclusion": "success"}]})) }
        }).unwrap();
        assert!(!gates[0].matrix_complete());
        assert_eq!(gates[0].name, "pull-request-validation");
        let mut wrong = run.clone();
        wrong["event"] = serde_json::json!("pull_request");
        assert!(
            matrix_with_fetch("owner/repo", &sha, |_, key| {
                if key.is_empty() {
                    return Ok(serde_json::json!({"id": 1, "name": "pull-request-validation"}));
                }
                assert_eq!(key, "workflow_runs");
                Ok(serde_json::json!({"workflow_runs": [wrong.clone()]}))
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn published_release_has_a_distinct_main_epoch_and_archived_history() {
        let mut old = classified_record();
        old.state = ReleaseRecoveryState::Published;
        old.release_version = Some("0.24.5".into());
        old.mutation.publication_verified = true;
        old.mutation.tag_name = Some("v0.24.5".into());
        old.mutation.closeout_receipt = Some(crate::release_closeout::CloseoutReceipt {
            report: "previous-release-report".into(),
            registry_url: None,
            registry_blob: None,
        });
        let next = post_release_main_epoch(&old, "fedcba654321").unwrap();
        assert!(next.mutation.closeout_receipt.is_none());
        assert_ne!(next.epoch_id, old.epoch_id);
        assert_eq!(next.release_commit, old.release_commit);
        assert_eq!(next.mutation.tag_name, old.mutation.tag_name);
        assert_eq!(next.state, ReleaseRecoveryState::WaitingForMatrix);
        assert!(admit_release_mutation(&next, ReleaseMutationKind::CreateTag).is_err());
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        ledger.save(&old).unwrap();
        ledger.save(&next).unwrap();
        assert!(ledger.save(&old).is_err());
        assert_eq!(
            std::fs::read_dir(temp.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json"))
                .count(),
            2
        );
    }

    #[test]
    fn github_failed_job_retry_requires_exact_admission_scope() {
        for kind in [RetryKind::FullGate, RetryKind::Infrastructure] {
            let token = RetryAdmissionToken {
                kind,
                run_ids: vec![99],
                job_ids: vec![],
            };
            rerun_failed_with_fetch("owner/repo", 99, token, |endpoint| {
                assert_eq!(
                    endpoint,
                    "repos/owner/repo/actions/runs/99/rerun-failed-jobs"
                );
                Ok(())
            })
            .unwrap();
        }
        for (kind, repository, run) in [
            (RetryKind::TargetedDiagnostic, "owner/repo", 99),
            (RetryKind::FullGate, "owner/repo", 100),
            (RetryKind::Infrastructure, "owner/../repo", 99),
        ] {
            let token = RetryAdmissionToken {
                kind,
                run_ids: vec![99],
                job_ids: vec![99],
            };
            assert!(
                rerun_failed_with_fetch(repository, run, token, |_| panic!(
                    "denied scope dispatched a write"
                ))
                .is_err()
            );
        }
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let port = GhCliEvidenceAdapter.with_cancellation(cancelled);
        let token = RetryAdmissionToken {
            kind: RetryKind::FullGate,
            run_ids: vec![99],
            job_ids: vec![],
        };
        assert!(
            port.rerun_failed_admitted("owner/repo", 99, token)
                .unwrap_err()
                .to_string()
                .contains("cancelled by the user")
        );
    }

    #[test]
    fn native_github_write_ports_cannot_bypass_admission() {
        assert!(
            GhCliEvidenceAdapter
                .rerun_workflow("owner/repo", 99)
                .is_err()
        );
        assert!(GhCliEvidenceAdapter.rerun_job("owner/repo", 99).is_err());
        assert!(GhCliEvidenceAdapter.rerun_failed("owner/repo", 99).is_err());
    }

    #[test]
    fn passing_error_module_tests_cannot_hide_the_real_causal_failure() {
        let cause = "collect2: fatal error: ld terminated with signal 7 [Bus error]";
        let log = format!(
            "test error::tests::ambiguous_mutations_never_retry_automatically ... ok\ntest api_error::tests::safe_retries ... ok\ntest tests::http::grok_session_proxy_headers_refresh_once_on_unauthorized_without_api_fallback ... ok\nerror: linking with `cc` failed: exit status: 1\nnote: {}\n{cause}\nerror: could not compile `fixture`",
            "unrelated linker arguments ".repeat(300)
        );
        let excerpt = first_causal_excerpt(&log);
        assert!(excerpt.contains(cause), "{excerpt}");
        assert!(
            !excerpt.contains("ambiguous_mutations_never_retry"),
            "{excerpt}"
        );
        assert_eq!(
            classify_failure(&excerpt, Some("ubuntu-24.04")),
            (
                ReleaseFailureClass::CompileFailure,
                EvidenceConfidence::StronglySupported
            )
        );
        assert_eq!(
            failure_fingerprint("workflow", "job", None, None, None, &excerpt),
            failure_fingerprint("workflow", "job", None, None, None, cause)
        );
        let changed = "collect2: fatal error: ld terminated with signal 11 [Segmentation fault]";
        assert_ne!(
            failure_fingerprint("workflow", "job", None, None, None, cause),
            failure_fingerprint("workflow", "job", None, None, None, changed)
        );
    }

    #[test]
    fn dependency_errors_and_unrecognized_platform_logs_classify_truthfully() {
        for platform in [
            None,
            Some("ubuntu-24.04"),
            Some("windows-2025"),
            Some("macos-15"),
        ] {
            assert_eq!(
                classify_failure("unrecognized terminal job message", platform),
                (ReleaseFailureClass::Unknown, EvidenceConfidence::Unknown)
            );
            for message in [
                "E: Version '154.0.8037.57-1~deb12u1' was not found",
                "Package chromium-headless-shell is not available",
            ] {
                assert_eq!(
                    classify_failure(message, platform),
                    (
                        ReleaseFailureClass::DependencyFailure,
                        EvidenceConfidence::StronglySupported
                    )
                );
            }
        }
    }

    #[test]
    fn remote_operation_cancellation_is_not_local_user_cancellation() {
        let excerpt = first_causal_excerpt(
            "test error::tests::completed_work ... ok\n##[error]The operation was canceled.",
        );
        assert!(excerpt.contains("The operation was canceled."));
        let (class, confidence) = classify_failure(&excerpt, Some("ubuntu-24.04"));
        assert_eq!(
            (class, confidence),
            (
                ReleaseFailureClass::Cancelled,
                EvidenceConfidence::Tentative
            )
        );
        let mut record = classified_record();
        record.failures[0].class = class;
        record.failures[0].confidence = confidence;
        assert_eq!(record.state, ReleaseRecoveryState::ClassifyingFailure);
        assert_eq!(
            next_directive(&record, 0),
            ReleaseDirective::CollectMoreEvidence
        );
    }

    #[test]
    fn github_account_execution_restriction_requires_owner_action() {
        let message = "The job was not started because recent account payments have failed or your spending limit needs to be increased. Please check the 'Billing & plans' section in your settings";
        let excerpt = first_causal_excerpt(&format!(
            "Job log unavailable (HTTP 404); GitHub failure annotations:\n{message}"
        ));
        let (class, confidence) = classify_failure(&excerpt, Some("windows-x86_64"));
        assert_eq!(class, ReleaseFailureClass::CredentialOrPermissionFailure);
        assert_eq!(confidence, EvidenceConfidence::StronglySupported);
        let mut record = classified_record();
        record.failures[0].class = class;
        record.failures[0].confidence = confidence;
        record.failures[0].causal_excerpt = excerpt;
        assert_eq!(next_directive(&record, 0), ReleaseDirective::Escalate);
        assert!(!record.retry_admission(RetryKind::FullGate).admitted);
        assert!(!record.retry_admission(RetryKind::Infrastructure).admitted);
        assert_eq!(
            classify_failure(
                "billing tests passed; spending limit metadata was read",
                None
            )
            .0,
            ReleaseFailureClass::Unknown
        );
    }

    #[test]
    fn github_command_echo_is_not_the_runtime_causal_error() {
        let log = "##[group]Run echo \"thread 'test' panicked at fixture.rs:42: ${VARIABLE}\"\necho \"thread 'test' panicked at fixture.rs:42: ${VARIABLE}\"\nshell: /usr/bin/bash -e {0}\n##[endgroup]\nthread 'test' panicked at fixture.rs:42: real_runtime_cause\nError: Process completed with exit code 1";
        let excerpt = first_causal_excerpt(log);
        assert!(excerpt.contains("real_runtime_cause"));
        assert!(!excerpt.contains("${VARIABLE}"));
    }

    #[test]
    fn colored_ci_logs_cannot_hide_credentials_or_causal_errors() {
        let log = "\u{1b}[31mauthorization: Bearer github_pat_\u{1b}[0mCOLOREDSECRET000000000000000000\n\u{1b}]0;untrusted title\u{7}thread 'exact_test' panicked at src/lib.rs:9:2: real causal failure";
        let excerpt = first_causal_excerpt(log);
        assert!(excerpt.contains("real causal failure"));
        assert!(!excerpt.contains("COLOREDSECRET"));
        assert!(!excerpt.contains('\u{1b}'));
        assert!(!excerpt.contains("untrusted title"));
    }

    #[test]
    fn cache_warning_is_not_the_cause_of_a_later_compiler_failure() {
        let excerpt = first_causal_excerpt(
            "2026-10-05T01:00:00Z ::warning::cache Error: failed to restore\n2026-10-05T01:00:01Z continue tests\n2026-10-05T01:00:02Z error[E0308]: mismatched types\nexpected u32",
        );
        assert!(excerpt.contains("error[E0308]"));
        assert!(!excerpt.contains("failed to restore"));
        assert_eq!(
            classify_failure(&excerpt, None).0,
            ReleaseFailureClass::CompileFailure
        );
    }

    #[test]
    fn missing_runner_log_uses_only_owned_failure_annotations() {
        fn fetch(endpoint: &str, _: bool) -> Result<String, RrcError> {
            if endpoint.ends_with("/logs") {
                Err(RrcError::Invalid("gh: HTTP 404".into()))
            } else if endpoint.ends_with("/actions/jobs/42") {
                Ok(serde_json::json!({"id":42,"status":"completed","conclusion":"failure","check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77"}).to_string())
            } else {
                assert_eq!(
                    endpoint,
                    "repos/owner/repo/check-runs/77/annotations?per_page=100&page=1"
                );
                Ok(serde_json::json!([{"annotation_level":"warning","message":"unrelated"},{"annotation_level":"failure","message":"error: No space left on device; GH_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz"}]).to_string())
            }
        }
        let evidence = job_log_with_fetch("owner/repo", 42, fetch).unwrap();
        assert!(evidence.contains("No space left on device"));
        assert!(evidence.contains("Job log unavailable"));
        assert!(!evidence.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(!evidence.contains("unrelated"));
        let causal = first_causal_excerpt(
            "wrapper\nSystem.IO.IOException: No space left on device : Worker_20261005-081221-utc.log\n  at Stack.One\n  at Stack.Two",
        );
        assert!(causal.contains("No space left on device"));
        assert_eq!(
            normalize_failure_text(&causal),
            normalize_failure_text(&causal.replace("20261005-081221", "20261006-082222"))
        );
        for job in [
            serde_json::json!({"id":43,"status":"completed","conclusion":"failure","check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77"}),
            serde_json::json!({"id":42,"status":"in_progress","conclusion":"failure","check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77"}),
            serde_json::json!({"id":42,"status":"completed","conclusion":"success","check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77"}),
            serde_json::json!({"id":42,"status":"completed","conclusion":"failure","check_run_url":"https://api.github.com/repos/other/repo/check-runs/77"}),
            serde_json::json!({"id":42,"status":"completed","conclusion":"failure","check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77/../../evil"}),
        ] {
            assert!(
                job_log_with_fetch("owner/repo", 42, |endpoint, _| {
                    if endpoint.ends_with("/logs") {
                        Err(RrcError::Invalid("gh: HTTP 404".into()))
                    } else if endpoint.ends_with("/actions/jobs/42") {
                        Ok(job.to_string())
                    } else {
                        panic!("invalid job cannot read annotations")
                    }
                })
                .is_err()
            );
        }
        let mut calls = 0;
        assert!(
            job_log_with_fetch("owner/repo", 42, |_, _| {
                calls += 1;
                Err(RrcError::Invalid("gh: HTTP 403".into()))
            })
            .is_err()
        );
        assert_eq!(calls, 1, "credential failures cannot fall back");
        assert!(
            job_log_with_fetch("owner/repo", 42, |endpoint, raw| {
                if endpoint.ends_with("/annotations?per_page=100&page=1") {
                    Ok("[]".into())
                } else {
                    fetch(endpoint, raw)
                }
            })
            .is_err(),
            "missing causal evidence stays unknown"
        );
    }

    #[test]
    fn terminal_cancelled_runner_job_uses_owned_failure_annotations() {
        let message =
            "The job was not acquired by Runner of type hosted even after multiple attempts";
        for conclusion in ["cancelled", "timed_out"] {
            let evidence = job_log_with_fetch("owner/repo", 42, |endpoint, _| {
                if endpoint.ends_with("/logs") {
                    Err(RrcError::Invalid("gh: HTTP 404".into()))
                } else if endpoint.ends_with("/actions/jobs/42") {
                    Ok(serde_json::json!({"id":42,"status":"completed","conclusion":conclusion,"check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77"}).to_string())
                } else {
                    assert_eq!(endpoint, "repos/owner/repo/check-runs/77/annotations?per_page=100&page=1");
                    Ok(serde_json::json!([{"annotation_level":"failure","message":message}]).to_string())
                }
            }).expect("terminal runner failures retain repository-owned annotations");
            let causal = first_causal_excerpt(&evidence);
            assert!(
                causal.contains(message),
                "the causal runner annotation must survive bounded extraction"
            );
            assert_eq!(
                classify_failure(&causal, Some("linux")),
                (
                    ReleaseFailureClass::RunnerInfrastructureFailure,
                    EvidenceConfidence::StronglySupported
                )
            );
        }
        assert!(job_log_with_fetch("owner/repo", 42, |endpoint, _| {
            if endpoint.ends_with("/logs") {
                Err(RrcError::Invalid("gh: HTTP 404".into()))
            } else if endpoint.ends_with("/actions/jobs/42") {
                Ok(serde_json::json!({"id":42,"status":"completed","conclusion":"cancelled","check_run_url":"https://api.github.com/repos/owner/repo/check-runs/77"}).to_string())
            } else { Ok("[]".into()) }
        }).is_err(), "cancellation without failure evidence cannot invent a runner outage");
        assert_eq!(
            classify_failure("The operation was canceled.", None).0,
            ReleaseFailureClass::Cancelled
        );
    }

    #[test]
    fn published_docs_red_repair_then_other_platform_red_stops_speculation() {
        let published = published_fixture();
        let publication = published.mutation.clone();
        let original = serde_json::to_vec(&published).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        ledger.save(&published).unwrap();
        let closeout_sha = "d".repeat(40);
        let repaired_sha = "e".repeat(40);
        let mut recovery = post_release_main_epoch(&published, &closeout_sha).unwrap();
        ledger.save(&recovery).unwrap();
        let matrix = |sha: &str, run: u64, failing_platform: &str, log: &str| {
            let jobs = [
                "linux-x86_64",
                "linux-arm64",
                "macos-intel",
                "macos-apple-silicon",
                "windows-x86_64",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, platform)| {
                let mut job = failed_job(if platform == failing_platform {
                    JobState::Failure
                } else {
                    JobState::Success
                });
                job.job_id = run * 10 + index as u64;
                job.run_id = run;
                job.job_name = platform.into();
                job.platform = Some(platform.into());
                job
            })
            .collect();
            FakeGitHub {
                gates: vec![GateRecord {
                    name: "gate".into(),
                    head_sha: sha.into(),
                    run_id: Some(run),
                    run_attempt: Some(1),
                    run_state: Some(JobState::Failure),
                    jobs,
                    url: None,
                }],
                log: log.into(),
            }
        };
        refresh_remote_evidence(
            &mut recovery,
            "owner/repo",
            &matrix(
                &closeout_sha,
                20,
                "macos-intel",
                "thread 'closeout' panicked at tests/a.rs:42: causal_macos_failure",
            ),
        )
        .unwrap();
        assert_eq!(recovery.state, ReleaseRecoveryState::ClassifyingFailure);
        assert!(!recovery.retry_admission(RetryKind::FullGate).admitted);
        let mac_fingerprint = recovery.failures.last().unwrap().fingerprint.clone();
        let mut repair = repair_for(&recovery, FocusedProofStatus::Passed);
        repair.source_commit_before = closeout_sha;
        repair.source_commit_after = Some(repaired_sha.clone());
        apply_controller_event(
            &mut recovery,
            ReleaseControllerEvent::VerifiedRepair(repair),
        )
        .unwrap();
        apply_controller_event(
            &mut recovery,
            ReleaseControllerEvent::RetryDispatched {
                kind: RetryKind::FullGate,
                evidence_refs: vec!["fixture:one-verified-main-push".into()],
            },
        )
        .unwrap();
        assert_eq!(recovery.retry_budget.full_gate_used, 1);
        refresh_remote_evidence(
            &mut recovery,
            "owner/repo",
            &matrix(
                &repaired_sha,
                21,
                "windows-x86_64",
                "error[E0308]: causal_windows_compile_failure",
            ),
        )
        .unwrap();
        let windows_failure = recovery.failures.last().unwrap();
        assert_ne!(windows_failure.fingerprint, mac_fingerprint);
        assert_eq!(windows_failure.platform.as_deref(), Some("windows-x86_64"));
        assert_eq!(recovery.state, ReleaseRecoveryState::ClassifyingFailure);
        assert!(matches!(
            next_directive(&recovery, 0),
            ReleaseDirective::RequestFocusedRepair { .. }
        ));
        assert!(!recovery.retry_admission(RetryKind::FullGate).admitted);
        assert!(admit_release_mutation(&recovery, ReleaseMutationKind::CreateTag).is_err());
        assert_eq!(recovery.release_version, published.release_version);
        assert_eq!(recovery.release_commit, published.release_commit);
        assert_eq!(recovery.mutation.tag_name, publication.tag_name);
        assert_eq!(recovery.mutation.tag_object, publication.tag_object);
        assert_eq!(
            recovery.mutation.publication_run_id,
            publication.publication_run_id
        );
        assert_eq!(
            recovery.mutation.published_asset_names,
            publication.published_asset_names
        );
        assert!(recovery.render_status().contains("PUBLISHED / VERIFIED"));
        ledger.save(&recovery).unwrap();
        assert_eq!(ledger.load().unwrap().unwrap(), recovery);
        assert_eq!(serde_json::to_vec(&published).unwrap(), original);
        assert!(
            temp.path()
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(
                    |entry| entry.path().extension().is_some_and(|ext| ext == "json")
                        && std::fs::read(entry.path()).is_ok_and(|bytes| serde_json::from_slice::<
                            ReleaseRecoveryRecord,
                        >(
                            &bytes
                        )
                        .is_ok_and(|record| record.epoch_id == published.epoch_id
                            && record.mutation.tag_name == published.mutation.tag_name))
                )
        );
    }
}

use std::sync::LazyLock;

/// Measured native worker time and actual denied autonomous operations.
/// Status queries and idle time between processes never count as work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseMetrics {
    pub ci_wait_millis: u64,
    pub model_active_millis: u64,
    pub retries_rejected: u64,
    pub repeated_fingerprints_blocked: u64,
    pub speculative_reruns_prevented: u64,
}

fn apply_controller_event_inner(
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
        ReleaseControllerEvent::RemoteGateDispatched(evidence_refs) => {
            record.transition(
                ReleaseRecoveryState::RemoteGateRunning,
                &commit,
                "exact-commit remote gates dispatched",
                evidence_refs,
                None,
            )?;
            // The remote-matrix watchdog owns a fresh progress window from
            // dispatch. Time spent in local verification must not make the
            // first exact-SHA lookup appear stale.
            record.note_progress(true);
            Ok(())
        }
        ReleaseControllerEvent::VerifiedRepair(repair) => {
            apply_verified_repair_batch(record, &commit, vec![repair])
        }
        ReleaseControllerEvent::VerifiedRepairBatch(repairs) => {
            apply_verified_repair_batch(record, &commit, repairs)
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

fn redact_secret_text(input: &str) -> String {
    static ESCAPES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
            .expect("static terminal escape regex")
    });
    let mut text = ESCAPES
        .replace_all(input, "")
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
        .collect::<String>();
    static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
        r"(?i)(authorization\s*:\s*(?:bearer|token|basic)\s+)[^\s]+",
        r#"(?i)("(?:authorization|github_token|gh_token|api[_-]?key|access[_-]?token|client[_-]?secret)"\s*:\s*")[^"]*"#,
        r"\b(?:sk|xai)-[A-Za-z0-9_-]{16,}\b",
        r"(?i)((?:github_token|gh_token|api[_-]?key|access[_-]?token|client[_-]?secret)\s*[=:]\s*)[^\s]+",
        r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b",
        r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
        r"(?i)(https?://)[^/\s@]+@",
        r"(?i)([?&](?:token|access_token|api_key|key|signature|sig|x-amz-signature)=)[^&\s]+",
    ].into_iter().map(|pattern| Regex::new(pattern).expect("static credential regex")).collect()
    });
    for regex in PATTERNS.iter() {
        text = regex.replace_all(&text, "$1[REDACTED]").into_owned();
    }
    text
}

fn is_account_execution_restriction(normalized: &str) -> bool {
    normalized.contains("the job was not started")
        && (normalized.contains("recent account payments have failed")
            || normalized.contains("spending limit needs to be increased"))
}

fn is_causal_diagnostic(line: &str) -> bool {
    static CAUSAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(panicked at|assertion [`']?(?:left|right)|assertionerror:|error\[e\d+\]|error:(?:\s|$)|fatal:(?:\s|$)|permission denied|unauthorized|rate limit|runner .* (?:lost|failed)|the job was not acquired by runner|timed out|process exited while answering|no space left on device|disk full|package .* is not available|e: version .* was not found|the operation was cancel(?:l)?ed)")
            .expect("static causal regex")
    });
    static TEST_STATUS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|\s)(?:test\s+\S+\s+(?:\.\.\.\s+(?:ok|FAILED|ignored)(?:[\s,]|$)|has been running)|acceptance verified:)")
            .expect("static test status regex")
    });
    let lower = line.to_ascii_lowercase();
    (CAUSAL.is_match(line) || is_account_execution_restriction(&lower))
        && !TEST_STATUS.is_match(line)
        && !line.contains("::warning::")
        && !line.contains("##[warning]")
        && !lower.contains("warning:")
        && !lower.contains("error: linking with")
        && !lower.contains("error: could not compile")
        && !lower.contains("error: test failed")
        && !lower.contains("process completed with exit code")
}

pub(crate) fn redact_persisted_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = redact_secret_text(text),
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_persisted_value),
        serde_json::Value::Object(values) => values.values_mut().for_each(redact_persisted_value),
        _ => {}
    }
}

/// Worker-owned cancellation covers reads and controller-admitted writes.
#[derive(Debug, Clone)]
pub struct CancellableGhCliEvidenceAdapter {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CancellableGhCliEvidenceAdapter {
    fn gh(&self, repository: &str, args: &[&str]) -> Result<String, RrcError> {
        if !matches!(
            github_repository_slug(&format!("https://github.com/{repository}")),
            Ok(parsed) if parsed == repository
        ) {
            return Err(RrcError::Invalid(
                "unsafe GitHub repository identity".into(),
            ));
        }
        let mut command = std::process::Command::new("gh");
        command
            .arg("api")
            .args(args)
            .arg("-H")
            .arg("Accept: application/vnd.github+json")
            .env_remove("GH_DEBUG");
        let output = crate::release_executor::run_bounded_external_command(
            &mut command,
            &self.cancelled,
            crate::release_executor::REMOTE_COMMAND_WATCHDOG,
        )?;
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

    fn paginated(
        &self,
        repository: &str,
        endpoint: &str,
        key: &str,
    ) -> Result<serde_json::Value, RrcError> {
        paginated_with_fetch(endpoint, key, |endpoint| {
            let body = self.gh(repository, &[endpoint])?;
            serde_json::from_str(&body).map_err(RrcError::Json)
        })
    }

    pub fn rerun_job_with_admission(
        &self,
        repository: &str,
        job_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        if token.kind != RetryKind::TargetedDiagnostic || !token.job_ids.contains(&job_id) {
            return Err(RrcError::MutationBlocked(
                "targeted retry token does not authorize this job".into(),
            ));
        }
        self.gh(
            repository,
            &[
                "--method",
                "POST",
                &format!("repos/{repository}/actions/jobs/{job_id}/rerun"),
            ],
        )
        .map(|_| ())
    }

    pub fn rerun_with_admission(
        &self,
        repository: &str,
        run_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        if !token.run_ids.contains(&run_id) {
            return Err(RrcError::MutationBlocked(
                "retry token does not authorize this run".into(),
            ));
        }
        match token.kind {
            RetryKind::FullGate | RetryKind::Infrastructure => self
                .gh(
                    repository,
                    &[
                        "--method",
                        "POST",
                        &format!("repos/{repository}/actions/runs/{run_id}/rerun"),
                    ],
                )
                .map(|_| ()),
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
        if run_ids.iter().any(|run| !token.run_ids.contains(run)) {
            return Err(RrcError::MutationBlocked(
                "retry token does not authorize this gate set".into(),
            ));
        }
        for run_id in run_ids {
            self.gh(
                repository,
                &[
                    "--method",
                    "POST",
                    &format!("repos/{repository}/actions/runs/{run_id}/rerun"),
                ],
            )?;
        }
        Ok(())
    }
}

impl GitHubEvidencePort for CancellableGhCliEvidenceAdapter {
    fn current_main_commit(&self, repository: &str) -> Result<Option<String>, RrcError> {
        let body = self.gh(repository, &[&format!("repos/{repository}/commits/main")])?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let sha = value
            .get("sha")
            .and_then(serde_json::Value::as_str)
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| RrcError::Invalid("remote main SHA missing".into()))?;
        Ok(Some(sha.to_owned()))
    }

    fn matrix_for_sha(
        &self,
        repository: &str,
        head_sha: &str,
    ) -> Result<Vec<GateRecord>, RrcError> {
        matrix_with_fetch(repository, head_sha, |endpoint, key| {
            if key.is_empty() {
                Ok(serde_json::from_str(&self.gh(repository, &[endpoint])?)?)
            } else {
                self.paginated(repository, endpoint, key)
            }
        })
    }

    fn job_log(&self, repository: &str, job_id: u64) -> Result<String, RrcError> {
        job_log_with_fetch(repository, job_id, |endpoint, raw| {
            if raw {
                self.gh(repository, &[endpoint, "--allow-escape-sequences"])
            } else {
                self.gh(repository, &[endpoint])
            }
        })
    }

    fn rerun_job(&self, _repository: &str, _job_id: u64) -> Result<(), RrcError> {
        Err(RrcError::MutationBlocked(
            "GitHub writes require controller admission".into(),
        ))
    }
    fn rerun_failed(&self, _repository: &str, _run_id: u64) -> Result<(), RrcError> {
        Err(RrcError::MutationBlocked(
            "GitHub writes require controller admission".into(),
        ))
    }
    fn rerun_workflow(&self, _repository: &str, _run_id: u64) -> Result<(), RrcError> {
        Err(RrcError::MutationBlocked(
            "GitHub writes require controller admission".into(),
        ))
    }
    fn rerun_failed_admitted(
        &self,
        repository: &str,
        run_id: u64,
        token: RetryAdmissionToken,
    ) -> Result<(), RrcError> {
        rerun_failed_with_fetch(repository, run_id, token, |endpoint| {
            self.gh(repository, &["--method", "POST", endpoint])
                .map(|_| ())
        })
    }
    fn rerun_admitted(&self, repository: &str, token: RetryAdmissionToken) -> Result<(), RrcError> {
        let run_ids = token.run_ids.clone();
        self.rerun_gate_set_with_admission(repository, &run_ids, token)
    }
}

fn rerun_failed_with_fetch(
    repository: &str,
    run_id: u64,
    token: RetryAdmissionToken,
    fetch: impl FnOnce(&str) -> Result<(), RrcError>,
) -> Result<(), RrcError> {
    if !matches!(token.kind, RetryKind::FullGate | RetryKind::Infrastructure)
        || !token.run_ids.contains(&run_id)
    {
        return Err(RrcError::MutationBlocked(
            "retry token does not authorize this failed-job run".into(),
        ));
    }
    // Repository validation remains at the native gh boundary too.
    if !matches!(github_repository_slug(&format!("https://github.com/{repository}")), Ok(parsed) if parsed == repository)
    {
        return Err(RrcError::Invalid(
            "unsafe GitHub repository identity".into(),
        ));
    }
    fetch(&format!(
        "repos/{repository}/actions/runs/{run_id}/rerun-failed-jobs"
    ))
}

fn job_log_with_fetch(
    repository: &str,
    job_id: u64,
    mut fetch: impl FnMut(&str, bool) -> Result<String, RrcError>,
) -> Result<String, RrcError> {
    let endpoint = format!("repos/{repository}/actions/jobs/{job_id}/logs");
    let result = match fetch(&endpoint, false) {
        Err(RrcError::Invalid(reason)) if reason.contains("terminal escape sequences") => {
            fetch(&endpoint, true)
        }
        result => result,
    };
    match result {
        Err(RrcError::Invalid(reason)) if reason.contains("HTTP 404") => {}
        result => return result,
    }
    // A runner may exhaust its disk before uploading logs. Its completed
    // job's repository-owned check annotations can retain the causal error.
    let job: serde_json::Value = serde_json::from_str(&fetch(
        &format!("repos/{repository}/actions/jobs/{job_id}"),
        false,
    )?)?;
    if job.get("id").and_then(serde_json::Value::as_u64) != Some(job_id)
        || job.get("status").and_then(serde_json::Value::as_str) != Some("completed")
        || !matches!(
            job.get("conclusion").and_then(serde_json::Value::as_str),
            Some("failure" | "cancelled" | "timed_out")
        )
    {
        return Err(RrcError::Invalid(
            "missing log has no completed failed job identity".into(),
        ));
    }
    let prefix = format!("https://api.github.com/repos/{repository}/check-runs/");
    let check_id = job
        .get("check_run_url")
        .and_then(serde_json::Value::as_str)
        .and_then(|url| url.strip_prefix(&prefix))
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| {
            RrcError::Invalid("missing log has no repository-owned check identity".into())
        })?;
    let mut messages = Vec::new();
    for page in 1..=10 {
        let body = fetch(
            &format!(
                "repos/{repository}/check-runs/{check_id}/annotations?per_page=100&page={page}"
            ),
            false,
        )?;
        let annotations: Vec<serde_json::Value> = serde_json::from_str(&body)?;
        if annotations.len() > 100 {
            return Err(RrcError::Invalid(
                "failure annotation page exceeds bound".into(),
            ));
        }
        for annotation in &annotations {
            if annotation
                .get("annotation_level")
                .and_then(serde_json::Value::as_str)
                == Some("failure")
                && let Some(message) = annotation
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                && !message.trim().is_empty()
            {
                messages.push(redact_secrets(message));
            }
        }
        if annotations.len() < 100 {
            if messages.is_empty() {
                return Err(RrcError::Invalid(
                    "job log unavailable and no causal failure annotation".into(),
                ));
            }
            return Ok(bounded(
                format!(
                    "Job log unavailable (HTTP 404); GitHub failure annotations:\n{}",
                    messages.join("\n")
                ),
                64 * 1024,
            ));
        }
    }
    Err(RrcError::Invalid(
        "failure annotation inventory exceeds bound".into(),
    ))
}

fn paginated_with_fetch(
    endpoint: &str,
    key: &str,
    mut fetch: impl FnMut(&str) -> Result<serde_json::Value, RrcError>,
) -> Result<serde_json::Value, RrcError> {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for page in 1..=10 {
        let value = fetch(&format!("{endpoint}&page={page}"))?;
        let total = value
            .get("total_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| RrcError::Invalid("GitHub inventory count missing".into()))?;
        let batch = value
            .get(key)
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| RrcError::Invalid("GitHub inventory rows missing".into()))?;
        for row in batch {
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| RrcError::Invalid("GitHub inventory row id missing".into()))?;
            if !seen.insert(id) {
                return Err(RrcError::Invalid(
                    "GitHub paginated inventory changed during observation".into(),
                ));
            }
            rows.push(row.clone());
        }
        if rows.len() as u64 == total {
            return Ok(serde_json::json!({key: rows}));
        }
        if rows.len() as u64 > total || batch.is_empty() {
            break;
        }
    }
    Err(RrcError::Invalid(
        "GitHub inventory incomplete or exceeds 1000-row bound".into(),
    ))
}

fn matrix_with_fetch(
    repository: &str,
    head_sha: &str,
    mut fetch: impl FnMut(&str, &str) -> Result<serde_json::Value, RrcError>,
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
    let value = fetch(&endpoint, "workflow_runs")?;
    let mut gates = Vec::new();
    let mut workflow_names = std::collections::HashMap::<u64, String>::new();
    let mut runs = value
        .get("workflow_runs")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .ok_or_else(|| RrcError::Invalid("workflow run inventory missing".into()))?;
    runs.sort_by_key(|run| {
        std::cmp::Reverse(
            run.get("id")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        )
    });
    for run in &runs {
        if run.get("head_sha").and_then(serde_json::Value::as_str) != Some(head_sha) {
            continue;
        }
        let workflow_id = run
            .get("workflow_id")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| RrcError::Invalid("workflow id missing".into()))?;
        let workflow_name = if let Some(name) = workflow_names.get(&workflow_id) {
            name.clone()
        } else {
            if workflow_names.len() >= 32 {
                return Err(RrcError::Invalid(
                    "workflow identity inventory exceeds bound".into(),
                ));
            }
            // REST run.name may be the evaluated run-name title. Resolve the
            // canonical workflow identity independently before dedup/fingerprint.
            let metadata = fetch(
                &format!("repos/{repository}/actions/workflows/{workflow_id}"),
                "",
            )?;
            if metadata.get("id").and_then(serde_json::Value::as_u64) != Some(workflow_id) {
                return Err(RrcError::Invalid(
                    "workflow metadata identity mismatch".into(),
                ));
            }
            let name = metadata
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| RrcError::Invalid("canonical workflow name missing".into()))?;
            let name = bounded(name.to_owned(), 256);
            workflow_names.insert(workflow_id, name.clone());
            name
        };
        let expected_path = match workflow_name.as_str() {
            "pull-request-validation" => Some(".github/workflows/ci.yml"),
            "msrv" => Some(".github/workflows/msrv.yml"),
            "five-target-foundation" => Some(".github/workflows/platform-foundation.yml"),
            "web-driver" => Some(".github/workflows/web-driver.yml"),
            _ => None,
        };
        if let Some(path) = expected_path
            && (run.get("event").and_then(serde_json::Value::as_str) != Some("push")
                || run.get("head_branch").and_then(serde_json::Value::as_str) != Some("main")
                || run.get("path").and_then(serde_json::Value::as_str) != Some(path))
        {
            continue;
        }
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
        let run_attempt = bounded_u32(
            run.get("run_attempt")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| RrcError::Invalid("run attempt missing".into()))?,
            "workflow run attempt",
        )?;
        let jobs_endpoint = format!(
            "repos/{repository}/actions/runs/{run_id}/attempts/{run_attempt}/jobs?per_page=100"
        );
        let jobs_value = fetch(&jobs_endpoint, "jobs")?;
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
            run_state: Some(github_job_state(
                run.get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown"),
                run.get("conclusion").and_then(serde_json::Value::as_str),
            )),
            jobs,
            url: run
                .get("html_url")
                .and_then(serde_json::Value::as_str)
                .map(|value| bounded(redact_secrets(value), 512)),
        });
    }
    Ok(gates)
}

pub fn post_release_main_epoch(
    published: &ReleaseRecoveryRecord,
    main: &str,
) -> Result<ReleaseRecoveryRecord, RrcError> {
    if !published.mutation.publication_verified || published.release_commit.as_deref() == Some(main)
    {
        return Err(RrcError::Invalid(
            "new main recovery needs a verified release and distinct main SHA".into(),
        ));
    }
    let mut objective = published.objective.clone();
    objective.post_release_main_epoch = true;
    let mut next = ReleaseRecoveryRecord::new(
        published.repo_identity.clone(),
        format!(
            "main-{}-{}",
            Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
            short_sha(main)
        ),
        objective,
    );
    next.release_version = published.release_version.clone();
    next.release_commit = published.release_commit.clone();
    next.mutation = published.mutation.clone();
    next.mutation.in_flight_operation = None;
    next.mutation.closeout_receipt = None;
    next.mutation.repair_admissions.clear();
    next.mutation.local_gates = Vec::new();
    next.current_main = Some(main.to_owned());
    next.state = ReleaseRecoveryState::Published;
    apply_controller_event(
        &mut next,
        ReleaseControllerEvent::PostReleaseCloseoutStarted(vec![
            "rrc:published-parent-epoch".into(),
        ]),
    )?;
    // Health is pending until the exact main matrix is observed; do not invent a red result.
    next.transition(
        ReleaseRecoveryState::WaitingForMatrix,
        main,
        "separate post-release main epoch awaits exact-SHA evidence",
        vec![],
        None,
    )?;
    Ok(next)
}
