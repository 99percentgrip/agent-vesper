#![forbid(unsafe_code)]
//! Native, controller-admitted release side effects and bounded external-health
//! checks. The executor follows the existing repository release contract; it
//! does not contain provider logic or a parallel release policy.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use command_group::CommandGroup;
use vesper_domain::{ContentPart, ToolResultStatus};

use crate::host_resources::{
    CargoResourcePolicy, GateCost, HostResourceGovernor, ResourcePolicy, ResourcePressure,
    ResourceTelemetry,
};
use crate::release_recovery::{
    EXTERNAL_HEALTH_BUDGET, ExternalHealthEvidence, ExternalHealthVerdict, GhCliEvidenceAdapter,
    GitHubEvidencePort, LocalGateRecord, ReleaseControllerEvent, ReleaseLedger,
    ReleaseMutationAdmission, ReleaseMutationKind, ReleaseProgress, ReleaseRecoveryRecord,
    ReleaseRecoveryState, RelevantStateChange, RelevantStateChangeKind, RrcError, SettlementState,
    admit_release_mutation, apply_controller_event, classify_external_health, default_release_root,
    redact_secrets, refresh_remote_evidence,
};

const MAX_COMMAND_OUTPUT: usize = 4096;
const MAX_TELEMETRY_LINES: usize = 8;
const MAX_TELEMETRY_LINE_CHARS: usize = 240;
const OFFICIAL_STATUS_URL: &str = "https://www.githubstatus.com/api/v2/summary.json";

struct ActiveReleaseWorker {
    cancelled: Arc<AtomicBool>,
    activity: Arc<Mutex<ReleaseWorkerActivity>>,
    /// Retaining the handle makes process ownership explicit. Dropping a
    /// command handler can no longer be mistaken for owning the worker.
    _handle: thread::JoinHandle<()>,
}

static ACTIVE_RELEASE_WORKERS: OnceLock<Mutex<HashMap<String, ActiveReleaseWorker>>> =
    OnceLock::new();

// Only the integration-only ACP test driver can set this process-local seam.
// It retains real Linux/cgroup/process/disk observation while removing the
// desktop-reserve admission threshold, so the test can hold a fake Cargo child
// deterministically on constrained CI machines. Production binaries neither
// compile this setter nor invoke it.
#[cfg(feature = "test-support")]
static PROCESS_TEST_PERMISSIVE_RESOURCE_POLICY: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "test-support")]
pub fn enable_permissive_resource_governor_for_process_tests() {
    PROCESS_TEST_PERMISSIVE_RESOURCE_POLICY.store(true, Ordering::Release);
}

fn resource_policy_for_worker() -> ResourcePolicy {
    #[cfg(feature = "test-support")]
    if PROCESS_TEST_PERMISSIVE_RESOURCE_POLICY.load(Ordering::Acquire) {
        return ResourcePolicy {
            fixed_reserve_bytes: 0,
            reserve_fraction_numerator: 0,
            reserve_fraction_denominator: 1,
            normal_headroom_min_bytes: 0,
            normal_headroom_fraction_numerator: 0,
            normal_headroom_fraction_denominator: 1,
            estimated_rustc_bytes: 1,
            max_cargo_jobs: 1,
            pressure_swap_percent: 100,
            critical_swap_percent: 100,
            disk_reserve_min_bytes: 0,
            disk_reserve_fraction_numerator: 0,
            disk_reserve_fraction_denominator: 1,
            expected_gate_growth_bytes: 0,
        };
    }
    ResourcePolicy::default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseWorkerActivity {
    epoch_id: String,
    stage: String,
    detail: String,
    current_gate: Option<String>,
    current_command: Option<String>,
    current_child: Option<String>,
    gate_started_at: Option<Instant>,
    last_activity_at: Instant,
    completed_gates: usize,
    total_gates: usize,
    version_before: Option<String>,
    version_after: Option<String>,
    candidate_sha: Option<String>,
    retry_budget: String,
    failure_fingerprint: Option<String>,
    recent_output: VecDeque<String>,
    progress: ReleaseProgress,
    resource_telemetry: Option<ResourceTelemetry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseWorkerSnapshot {
    pub repo_identity: String,
    pub epoch_id: String,
    pub stage: String,
    pub detail: String,
    pub cancellable: bool,
    pub current_gate: Option<String>,
    pub current_command: Option<String>,
    pub current_child: Option<String>,
    pub gate_elapsed_secs: Option<u64>,
    pub last_activity_ago_secs: u64,
    pub completed_gates: usize,
    pub total_gates: usize,
    pub version_before: Option<String>,
    pub version_after: Option<String>,
    pub candidate_sha: Option<String>,
    pub retry_budget: String,
    pub failure_fingerprint: Option<String>,
    pub recent_output: Vec<String>,
    pub process_alive: bool,
    /// Persisted, typed milestones from the release ledger. Both hosts use
    /// this same sequence for their chat/RUN projection.
    pub progress: ReleaseProgress,
    /// Live host/process/disk observation from the controller-owned governor.
    pub resource_telemetry: Option<ResourceTelemetry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseWorkerRegistration {
    pub repo_identity: String,
    pub epoch_id: String,
}

fn active_workers() -> &'static Mutex<HashMap<String, ActiveReleaseWorker>> {
    ACTIVE_RELEASE_WORKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn capture_release_stream<R: Read>(
    mut stream: R,
    activity: Option<Arc<Mutex<ReleaseWorkerActivity>>>,
) -> std::io::Result<Vec<u8>> {
    const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
    let mut captured = Vec::new();
    let mut pending = Vec::new();
    let mut bytes = [0_u8; 4096];
    loop {
        let read = match stream.read(&mut bytes) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(captured.len());
        captured.extend_from_slice(&bytes[..read.min(remaining)]);
        for byte in &bytes[..read] {
            if matches!(byte, b'\n' | b'\r') {
                record_captured_line(activity.as_ref(), &pending);
                pending.clear();
            } else if pending.len() < MAX_COMMAND_OUTPUT {
                pending.push(*byte);
            }
        }
    }
    record_captured_line(activity.as_ref(), &pending);
    Ok(captured)
}

fn join_capture_reader(
    reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stream_name: &str,
) -> Result<Vec<u8>, RrcError> {
    reader
        .join()
        .map_err(|_| RrcError::Invalid(format!("release child {stream_name} reader panicked")))?
        .map_err(RrcError::Io)
}

fn record_captured_line(activity: Option<&Arc<Mutex<ReleaseWorkerActivity>>>, bytes: &[u8]) {
    let Some(activity) = activity else { return };
    let line = sanitize_terminal_text(&String::from_utf8_lossy(bytes));
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Ok(mut activity) = activity.lock() {
        activity.last_activity_at = Instant::now();
        if let Some(child) = child_name_from_output(line) {
            activity.current_child = Some(child);
        }
        if activity.current_gate.as_deref() == Some("acceptance")
            && let Some((completed, total)) = acceptance_progress_from_output(line)
        {
            // Only the controller-owned `xtask acceptance` progress marker is
            // admitted here. Cargo dots, arbitrary test text and timestamps
            // never become a fabricated completion denominator.
            activity.progress.update_local_subtask(
                "acceptance",
                "Exact acceptance cases",
                completed,
                total,
            );
        }
        push_telemetry_line(&mut activity, line);
    }
}

fn acceptance_progress_from_output(line: &str) -> Option<(u64, u64)> {
    let progress = line.trim().strip_prefix("acceptance progress: ")?;
    let (completed, remainder) = progress.split_once('/')?;
    let total = remainder.split_whitespace().next()?;
    let completed = completed.parse::<u64>().ok()?;
    let total = total.parse::<u64>().ok()?;
    (total > 0 && completed <= total).then_some((completed, total))
}

fn sanitize_terminal_text(input: &str) -> String {
    #[derive(Clone, Copy)]
    enum EscapeState {
        Text,
        Escape,
        Csi,
        Osc,
        OscEscape,
        String,
        StringEscape,
    }

    let mut output = String::with_capacity(input.len());
    let mut state = EscapeState::Text;
    for ch in input.chars() {
        state = match state {
            EscapeState::Text if ch == '\u{1b}' => EscapeState::Escape,
            EscapeState::Text => {
                if ch == '\t' || (!ch.is_control() && ch != '\u{7f}') {
                    output.push(ch);
                }
                EscapeState::Text
            }
            EscapeState::Escape if ch == '[' => EscapeState::Csi,
            EscapeState::Escape if ch == ']' => EscapeState::Osc,
            EscapeState::Escape if matches!(ch, 'P' | 'X' | '^' | '_') => EscapeState::String,
            EscapeState::Escape => EscapeState::Text,
            EscapeState::Csi if ('@'..='~').contains(&ch) => EscapeState::Text,
            EscapeState::Csi => EscapeState::Csi,
            EscapeState::Osc if ch == '\u{7}' => EscapeState::Text,
            EscapeState::Osc if ch == '\u{1b}' => EscapeState::OscEscape,
            EscapeState::Osc => EscapeState::Osc,
            EscapeState::OscEscape if ch == '\\' => EscapeState::Text,
            EscapeState::OscEscape => EscapeState::Osc,
            EscapeState::String if ch == '\u{1b}' => EscapeState::StringEscape,
            EscapeState::String => EscapeState::String,
            EscapeState::StringEscape if ch == '\\' => EscapeState::Text,
            EscapeState::StringEscape => EscapeState::String,
        };
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionBumpReceipt {
    pub before: String,
    pub after: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionFileMutation {
    relative_path: String,
    before: Vec<u8>,
    after: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionMutationPlan {
    before: String,
    after: String,
    files: Vec<VersionFileMutation>,
    generated_lockfile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationReceipt {
    pub run_id: u64,
    pub version: String,
    pub assets: Vec<String>,
}

pub trait ReleaseExecutionPort: Send + Sync {
    fn prepare_version_bump(
        &self,
        bump: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<VersionBumpReceipt, RrcError>;
    fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError>;
    fn commit_candidate(
        &self,
        version: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<String, RrcError>;
    fn push_candidate(&self, admission: ReleaseMutationAdmission) -> Result<String, RrcError>;
    fn create_and_push_tag(
        &self,
        version: &str,
        commit: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<(String, String), RrcError>;
    fn publication(
        &self,
        repository: &str,
        tag: &str,
    ) -> Result<Option<PublicationReceipt>, RrcError>;
}

pub trait ExternalHealthPort: Send + Sync {
    fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialStatusSnapshot {
    pub degraded: bool,
    pub summary: String,
    pub evidence_ref: String,
}

#[derive(Debug, Clone)]
pub struct NativeReleaseExecutor {
    workspace: PathBuf,
    cancelled: Arc<AtomicBool>,
    activity: Option<Arc<Mutex<ReleaseWorkerActivity>>>,
    /// Production RRC workers always carry this governor. The public
    /// constructor leaves it absent only for narrow executor tests that do
    /// not represent a controller-admitted local gate.
    resource_governor: Option<HostResourceGovernor>,
}

impl NativeReleaseExecutor {
    pub fn new(workspace: &Path, cancelled: Arc<AtomicBool>) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
            activity: None,
            resource_governor: None,
        })
    }

    fn with_activity(
        workspace: &Path,
        cancelled: Arc<AtomicBool>,
        activity: Arc<Mutex<ReleaseWorkerActivity>>,
    ) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
            activity: Some(activity),
            resource_governor: None,
        })
    }

    fn with_activity_and_governor(
        workspace: &Path,
        cancelled: Arc<AtomicBool>,
        activity: Arc<Mutex<ReleaseWorkerActivity>>,
        resource_governor: HostResourceGovernor,
    ) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
            activity: Some(activity),
            resource_governor: Some(resource_governor),
        })
    }

    fn command(&self, program: &str, args: &[&str]) -> Result<Output, RrcError> {
        self.command_with_env(program, args, &[])
    }

    fn command_with_env(
        &self,
        program: &str,
        args: &[&str],
        environment: &[(&str, &str)],
    ) -> Result<Output, RrcError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(RrcError::Invalid(
                "release recovery was cancelled by the user".into(),
            ));
        }
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&self.workspace)
            .env_remove("GH_DEBUG")
            .envs(environment.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        let mut child = command.group().kill_on_drop(true).spawn()?;
        #[cfg(not(windows))]
        let mut child = command.group_spawn()?;
        self.note_process_activity(Some(program), None);
        let root_pid = child.inner().id();
        let stdout = child
            .inner()
            .stdout
            .take()
            .ok_or_else(|| RrcError::Invalid("release child stdout pipe is missing".into()))?;
        let stderr = child
            .inner()
            .stderr
            .take()
            .ok_or_else(|| RrcError::Invalid("release child stderr pipe is missing".into()))?;
        let stdout_activity = self.activity.clone();
        let stderr_activity = self.activity.clone();
        let stdout_reader = thread::spawn(move || capture_release_stream(stdout, stdout_activity));
        let stderr_reader = thread::spawn(move || capture_release_stream(stderr, stderr_activity));
        let mut cancelled = false;
        let mut resource_stop = None;
        let status = loop {
            if self.cancelled.load(Ordering::Acquire) {
                let _ = child.kill();
                cancelled = true;
            }
            if let Some(governor) = self.resource_governor.as_ref() {
                match governor.snapshot(Some(root_pid)) {
                    Ok(telemetry) => {
                        self.note_resource_telemetry(telemetry.clone());
                        if telemetry.pressure == ResourcePressure::Critical {
                            let _ = child.kill();
                            resource_stop = Some(telemetry);
                        }
                    }
                    Err(error) => {
                        // The gate is already admitted. A transient telemetry
                        // read failure must not pretend it is healthy, but
                        // cannot abandon process ownership either.
                        self.note_resource_action(format!(
                            "live resource telemetry unavailable: {error}"
                        ));
                    }
                }
            }
            if let Some(status) = child.inner().try_wait()? {
                break status;
            }
            thread::sleep(Duration::from_millis(100));
        };
        self.note_process_activity(None, Some("process exited"));
        let stdout_bytes = join_capture_reader(stdout_reader, "stdout")?;
        let stderr_bytes = join_capture_reader(stderr_reader, "stderr")?;
        if cancelled {
            return Err(RrcError::Invalid(
                "release recovery was cancelled by the user".into(),
            ));
        }
        if let Some(telemetry) = resource_stop {
            return Err(RrcError::ResourceConstrained(format!(
                "critical resource pressure stopped the owned process tree: {}",
                telemetry.action
            )));
        }
        Ok(Output {
            status,
            stdout: stdout_bytes,
            stderr: stderr_bytes,
        })
    }

    fn note_process_activity(&self, child: Option<&str>, output: Option<&str>) {
        let Some(activity) = self.activity.as_ref() else {
            return;
        };
        if let Ok(mut activity) = activity.lock() {
            activity.last_activity_at = Instant::now();
            if let Some(child) = child {
                activity.current_child = Some(child.to_owned());
            } else if output == Some("process exited") {
                activity.current_child = None;
            }
            if let Some(output) = output {
                push_telemetry_line(&mut activity, output);
            }
        }
    }

    fn note_resource_telemetry(&self, telemetry: ResourceTelemetry) {
        let Some(activity) = self.activity.as_ref() else {
            return;
        };
        if let Ok(mut activity) = activity.lock() {
            activity.resource_telemetry = Some(telemetry);
        }
    }

    fn note_resource_action(&self, action: String) {
        let Some(activity) = self.activity.as_ref() else {
            return;
        };
        if let Ok(mut activity) = activity.lock()
            && let Some(telemetry) = activity.resource_telemetry.as_mut()
        {
            telemetry.action = action;
        }
    }

    fn admit_local_resources(
        &self,
        cost: GateCost,
    ) -> Result<crate::host_resources::ResourceAdmission, RrcError> {
        let governor = self.resource_governor.as_ref().ok_or_else(|| {
            RrcError::Invalid(
                "controller-owned local verification requires the Host Resource Governor".into(),
            )
        })?;
        match governor.preflight(cost) {
            Ok(admission) => {
                self.note_resource_telemetry(admission.telemetry.clone());
                Ok(admission)
            }
            Err(error) => {
                if let Some(telemetry) = error.telemetry() {
                    self.note_resource_telemetry(telemetry.clone());
                }
                // `run_local_gate` records the requested Cargo child before
                // admission so the RUN panel has context. A rejected gate
                // never spawned that child, so clear it rather than claiming
                // a process is alive.
                if let Some(activity) = self.activity.as_ref()
                    && let Ok(mut activity) = activity.lock()
                {
                    activity.current_child = None;
                    activity.last_activity_at = Instant::now();
                }
                Err(RrcError::ResourceConstrained(error.to_string()))
            }
        }
    }

    fn checked_with_admitted_resources(
        &self,
        program: &str,
        args: &[&str],
        admission: &crate::host_resources::ResourceAdmission,
    ) -> Result<String, RrcError> {
        let environment = admission.cargo.environment();
        let environment = environment
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let output = self.command_with_env(program, args, &environment)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "{} failed with status {}: {}",
                format_command(program, args),
                output.status,
                bounded_output(&output.stderr)
            )));
        }
        Ok(bounded_output(&output.stdout))
    }

    fn checked(&self, program: &str, args: &[&str]) -> Result<String, RrcError> {
        let output = self.command(program, args)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "{} failed with status {}: {}",
                format_command(program, args),
                output.status,
                bounded_output(&output.stderr)
            )));
        }
        Ok(bounded_output(&output.stdout))
    }
}

impl ReleaseExecutionPort for NativeReleaseExecutor {
    fn prepare_version_bump(
        &self,
        bump: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<VersionBumpReceipt, RrcError> {
        require_kind(admission, ReleaseMutationKind::VersionBump)?;
        if !self
            .checked("git", &["status", "--porcelain"])?
            .trim()
            .is_empty()
        {
            return Err(RrcError::Invalid(
                "release candidate preparation requires a clean working tree".into(),
            ));
        }
        let plan = build_version_mutation_plan(&self.workspace, bump)?;
        let lock_path = self.workspace.join(&plan.generated_lockfile);
        let lock_before = fs::read(&lock_path)?;
        apply_version_mutation_plan(&self.workspace, &plan, None)?;
        let resources = match self.admit_local_resources(GateCost::Expensive) {
            Ok(resources) => resources,
            Err(error) => {
                restore_version_mutation(&self.workspace, &plan, &lock_path, &lock_before)?;
                return Err(error);
            }
        };
        if let Err(error) = self
            .checked_with_admitted_resources(
                "cargo",
                &["check", "--workspace", "--all-targets"],
                &resources,
            )
            .and_then(|_| validate_version_mutation(&self.workspace, &plan, &resources.cargo))
        {
            restore_version_mutation(&self.workspace, &plan, &lock_path, &lock_before)?;
            return Err(error);
        }
        Ok(VersionBumpReceipt {
            before: plan.before,
            after: plan.after,
            files: plan
                .files
                .into_iter()
                .map(|file| file.relative_path)
                .chain(std::iter::once(plan.generated_lockfile))
                .collect(),
        })
    }

    fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError> {
        let (program, args) = local_gate_argv(&gate.name).ok_or_else(|| {
            RrcError::Invalid(format!("unknown controller-owned local gate {}", gate.name))
        })?;
        if let Some(activity) = self.activity.as_ref()
            && let Ok(mut activity) = activity.lock()
        {
            activity.current_gate = Some(gate.name.clone());
            activity.current_command = Some(gate.command.clone());
            activity.current_child = Some(program.to_owned());
            activity.progress.mark_local_gate_running(&gate.name);
            activity.gate_started_at = Some(Instant::now());
            activity.last_activity_at = Instant::now();
        }
        let resources = self.admit_local_resources(local_gate_cost(&gate.name))?;
        self.checked_with_admitted_resources(program, args, &resources)
    }

    fn commit_candidate(
        &self,
        version: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<String, RrcError> {
        require_kind(admission, ReleaseMutationKind::CommitCandidate)?;
        let root = fs::read_to_string(self.workspace.join("Cargo.toml"))?;
        let allowed_manifests = workspace_member_manifests(&self.workspace, &root)?
            .into_iter()
            .map(|path| relative_version_path(&self.workspace, &path))
            .collect::<Result<BTreeSet<_>, _>>()?;
        for path in allowed_manifests
            .iter()
            .map(String::as_str)
            .chain(["Cargo.lock", "registry/agent.json"])
        {
            self.checked("git", &["add", "--", path])?;
        }
        let status = self.checked("git", &["status", "--porcelain"])?;
        for line in status.lines() {
            let path = line.get(3..).unwrap_or_default().trim();
            if path != "Cargo.lock"
                && path != "registry/agent.json"
                && !allowed_manifests.contains(path)
            {
                return Err(RrcError::Invalid(format!(
                    "candidate commit contains non-version path {path}"
                )));
            }
        }
        self.checked("git", &["commit", "-m", &format!("Release v{version}")])?;
        let commit = self.checked("git", &["rev-parse", "HEAD"])?;
        exact_sha(commit.trim())
    }

    fn push_candidate(&self, admission: ReleaseMutationAdmission) -> Result<String, RrcError> {
        require_kind(admission, ReleaseMutationKind::PushCandidate)?;
        self.checked("git", &["push", "origin", "HEAD:main"])?;
        let commit = self.checked("git", &["rev-parse", "HEAD"])?;
        Ok(format!("origin/main@{}", exact_sha(commit.trim())?))
    }

    fn create_and_push_tag(
        &self,
        version: &str,
        commit: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<(String, String), RrcError> {
        require_kind(admission, ReleaseMutationKind::CreateTag)?;
        let commit = exact_sha(commit)?;
        let head = exact_sha(self.checked("git", &["rev-parse", "HEAD"])?.trim())?;
        if head != commit {
            return Err(RrcError::Invalid(
                "working-tree HEAD no longer matches the exact green candidate".into(),
            ));
        }
        let tag = format!("v{version}");
        let existing = self.command(
            "git",
            &["rev-parse", "--verify", &format!("refs/tags/{tag}")],
        )?;
        if existing.status.success() {
            return Err(RrcError::Invalid(format!(
                "immutable tag {tag} already exists"
            )));
        }
        self.checked(
            "git",
            &[
                "tag",
                "-a",
                &tag,
                &commit,
                "-m",
                &format!("Agent Vesper {tag}"),
            ],
        )?;
        let object = exact_sha(
            self.checked("git", &["rev-parse", &format!("{tag}^{{tag}}")])?
                .trim(),
        )?;
        self.checked("git", &["push", "origin", &tag])?;
        Ok((tag, object))
    }

    fn publication(
        &self,
        repository: &str,
        tag: &str,
    ) -> Result<Option<PublicationReceipt>, RrcError> {
        let runs = gh_json(
            &self.workspace,
            &[
                "run",
                "list",
                "--repo",
                repository,
                "--workflow",
                "release.yml",
                "--limit",
                "20",
                "--json",
                "databaseId,status,conclusion,headBranch",
            ],
        )?;
        let rows = runs
            .as_array()
            .ok_or_else(|| RrcError::Invalid("release run response is not an array".into()))?;
        let Some(run) = rows
            .iter()
            .find(|row| row.get("headBranch").and_then(serde_json::Value::as_str) == Some(tag))
        else {
            return Ok(None);
        };
        let status = run
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let conclusion = run.get("conclusion").and_then(serde_json::Value::as_str);
        if status != "completed" {
            return Ok(None);
        }
        if conclusion != Some("success") {
            return Err(RrcError::Invalid(format!(
                "release workflow reached terminal conclusion {}",
                conclusion.unwrap_or("unknown")
            )));
        }
        let run_id = run
            .get("databaseId")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| RrcError::Invalid("release workflow run id is missing".into()))?;
        let release = gh_json(
            &self.workspace,
            &[
                "release",
                "view",
                tag,
                "--repo",
                repository,
                "--json",
                "tagName,isDraft,isPrerelease,assets",
            ],
        )?;
        if release.get("tagName").and_then(serde_json::Value::as_str) != Some(tag)
            || release.get("isDraft").and_then(serde_json::Value::as_bool) != Some(false)
        {
            return Err(RrcError::Invalid(
                "published release identity is not settled".into(),
            ));
        }
        let assets = release
            .get("assets")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|asset| asset.get("name").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if assets.is_empty() {
            return Err(RrcError::Invalid(
                "published release has no verified assets".into(),
            ));
        }
        Ok(Some(PublicationReceipt {
            run_id,
            version: tag.trim_start_matches('v').to_owned(),
            assets,
        }))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CurlGitHubStatusAdapter;

impl ExternalHealthPort for CurlGitHubStatusAdapter {
    fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
        let started = Instant::now();
        let output = Command::new("curl")
            .args([
                "--proto",
                "=https",
                "--tlsv1.2",
                "--silent",
                "--show-error",
                "--max-time",
                "10",
                OFFICIAL_STATUS_URL,
            ])
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .output()?;
        if started.elapsed() > EXTERNAL_HEALTH_BUDGET {
            return Err(RrcError::Invalid(
                "official status check exceeded the 15-second budget".into(),
            ));
        }
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "official GitHub status request failed: {}",
                bounded_output(&output.stderr)
            )));
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let indicator = value
            .pointer("/status/indicator")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let description = value
            .pointer("/status/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("status description unavailable");
        Ok(OfficialStatusSnapshot {
            degraded: !matches!(indicator, "none"),
            summary: redact_secrets(&format!("{indicator}: {description}")),
            evidence_ref: OFFICIAL_STATUS_URL.into(),
        })
    }
}

/// Builds late-branch health evidence from settled repository facts plus one
/// official-status request. Production never queries community telemetry.
pub fn external_health_evidence(
    record: &ReleaseRecoveryRecord,
    port: &dyn ExternalHealthPort,
) -> Result<ExternalHealthEvidence, RrcError> {
    if record.state != ReleaseRecoveryState::ClassifyingFailure
        && record.state != ReleaseRecoveryState::PausedExternal
        && record.state != ReleaseRecoveryState::ExternalHealthCheck
    {
        return Err(RrcError::Invalid(
            "external health is not admitted in the current state".into(),
        ));
    }
    let infrastructure = record
        .failures
        .iter()
        .filter(|failure| failure.class.infrastructure_like())
        .count();
    let local_gates_green = !record.mutation.local_gates.is_empty()
        && record
            .mutation
            .local_gates
            .iter()
            .all(|gate| gate.state == SettlementState::Succeeded);
    let repository_checks_green = local_gates_green
        && record
            .failures
            .iter()
            .all(|failure| failure.exists_on_last_green != Some(true));
    let source_explanation_absent = record
        .failures
        .iter()
        .all(|failure| failure.related_source_touched != Some(true));
    let official = port.official_status()?;
    Ok(ExternalHealthEvidence {
        repository_checks_green,
        source_explanation_absent,
        infrastructure_failures: infrastructure,
        official_degraded: official.degraded,
        direct_api_failure: record.failures.iter().any(|failure| {
            matches!(
                failure.class,
                crate::release_recovery::ReleaseFailureClass::RunnerInfrastructureFailure
                    | crate::release_recovery::ReleaseFailureClass::ArtifactInfrastructureFailure
                    | crate::release_recovery::ReleaseFailureClass::RateLimit
            )
        }),
        community_reports: false,
        official_summary: official.summary,
        direct_evidence: vec![official.evidence_ref],
        cross_job_evidence: record
            .failures
            .iter()
            .filter(|failure| failure.class.infrastructure_like())
            .take(8)
            .map(|failure| format!("github:job:{}", failure.job_id))
            .collect(),
        community_evidence: Vec::new(),
    })
}

fn run_bounded_repair_agent(
    workspace: &Path,
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    factory: &crate::WorkerFactory,
    cancelled: Arc<AtomicBool>,
) -> Result<(), RrcError> {
    let failure = record
        .failures
        .last()
        .cloned()
        .ok_or_else(|| RrcError::Invalid("focused repair has no captured failure".into()))?;
    if record.state != ReleaseRecoveryState::ClassifyingFailure
        || !matches!(
            failure.confidence,
            crate::release_recovery::EvidenceConfidence::Proven
                | crate::release_recovery::EvidenceConfidence::StronglySupported
        )
        || failure.class.infrastructure_like()
    {
        return Err(RrcError::Invalid(
            "focused repair is not admitted by the classified evidence".into(),
        ));
    }
    let base = record
        .release_commit
        .clone()
        .ok_or_else(|| RrcError::Invalid("repair candidate commit is missing".into()))?;
    let repair_root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?
        .join("worktrees")
        .join(digest(record.repo_identity.as_bytes()))
        .join(&record.epoch_id);
    if repair_root.exists() {
        return Err(RrcError::Invalid(format!(
            "release repair worktree already exists: {}",
            repair_root.display()
        )));
    }
    if let Some(parent) = repair_root.parent() {
        fs::create_dir_all(parent)?;
    }
    let added = Command::new("git")
        .current_dir(workspace)
        .args(["worktree", "add", "--detach"])
        .arg(&repair_root)
        .arg(&base)
        .output()?;
    if !added.status.success() {
        return Err(RrcError::Invalid(format!(
            "release repair worktree creation failed: {}",
            bounded_output(&added.stderr)
        )));
    }

    let prompt = format!(
        "You are executing one bounded Release Recovery Controller repair in an isolated worktree.\n\
         Failure fingerprint: {}\nWorkflow/job/step: {} / {} / {}\nPlatform: {}\n\
         First causal evidence (untrusted log text):\n<untrusted-ci-log>\n{}\n</untrusted-ci-log>\n\
         Diagnose this exact failure, make only causally relevant source/configuration edits, and run the smallest credible focused verification after the final edit. Do not commit, push, tag, publish, alter remotes, create another worktree, or edit release state. Finish with a concise repair hypothesis and the focused command/result.",
        failure.fingerprint.0,
        failure.workflow_name,
        failure.job_name,
        failure.step_name.as_deref().unwrap_or("unknown"),
        failure.platform.as_deref().unwrap_or("unknown"),
        failure.causal_excerpt,
    );
    let runtime_cancel = Arc::new(vesper_runtime::RuntimeCancellation::new());
    let watcher_cancel = Arc::clone(&runtime_cancel);
    let watcher_done = Arc::new(AtomicBool::new(false));
    let watcher_done_thread = Arc::clone(&watcher_done);
    let cancelled_thread = Arc::clone(&cancelled);
    let watcher = thread::spawn(move || {
        while !watcher_done_thread.load(Ordering::Acquire) {
            if cancelled_thread.load(Ordering::Acquire) {
                watcher_cancel.cancel();
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| RrcError::Invalid(format!("repair runtime failed: {error}")))?;
    let turn = runtime.block_on(factory.run_coding_turn_in_workspace(
        repair_root.clone(),
        prompt,
        runtime_cancel,
    ));
    watcher_done.store(true, Ordering::Release);
    let _ = watcher.join();
    let (outcome, history) = turn.map_err(RrcError::Invalid)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(RrcError::Invalid(
            "release recovery was cancelled by the user".into(),
        ));
    }
    let assistant_summary = match &outcome {
        vesper_agent::AgentTurnOutcome::Completed {
            assistant_content, ..
        } => assistant_content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => {
            return Err(RrcError::Invalid(
                "repair agent did not reach a completed terminal outcome".into(),
            ));
        }
    };
    let (mutation_seen, focused_command) = observed_repair_receipts(&history);
    if !mutation_seen {
        return Err(RrcError::Invalid(
            "repair agent completed without an observed successful file mutation".into(),
        ));
    }
    let focused_command = focused_command.ok_or_else(|| {
        RrcError::Invalid(
            "repair agent did not run a successful focused command after its final edit".into(),
        )
    })?;
    let checked = Command::new("git")
        .current_dir(&repair_root)
        .args(["diff", "--check"])
        .output()?;
    if !checked.status.success() {
        return Err(RrcError::Invalid(format!(
            "repair diff check failed: {}",
            bounded_output(&checked.stderr)
        )));
    }
    let diff = Command::new("git")
        .current_dir(&repair_root)
        .args(["diff", "--binary", "HEAD"])
        .output()?;
    if !diff.status.success() || diff.stdout.is_empty() {
        return Err(RrcError::Invalid(
            "repair worktree produced no promotable diff".into(),
        ));
    }
    let root_status = Command::new("git")
        .current_dir(workspace)
        .args(["status", "--porcelain"])
        .output()?;
    if !root_status.status.success() || !root_status.stdout.is_empty() {
        return Err(RrcError::Invalid(
            "repair promotion requires the controller workspace to remain clean".into(),
        ));
    }
    let mut apply = Command::new("git")
        .current_dir(workspace)
        .args(["apply", "--binary", "--index"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = apply.stdin.take() {
        use std::io::Write as _;
        stdin.write_all(&diff.stdout)?;
    }
    let apply_output = apply.wait_with_output()?;
    if !apply_output.status.success() {
        return Err(RrcError::Invalid(format!(
            "verified repair diff could not be promoted: {}",
            bounded_output(&apply_output.stderr)
        )));
    }
    let fingerprint_short = failure.fingerprint.0.chars().take(12).collect::<String>();
    let committed = Command::new("git")
        .current_dir(workspace)
        .args([
            "commit",
            "-m",
            &format!("fix(release): repair {fingerprint_short}"),
        ])
        .output()?;
    if !committed.status.success() {
        return Err(RrcError::Invalid(format!(
            "verified repair commit failed: {}",
            bounded_output(&committed.stderr)
        )));
    }
    let commit = Command::new("git")
        .current_dir(workspace)
        .args(["rev-parse", "HEAD"])
        .output()?;
    let commit = exact_sha(String::from_utf8_lossy(&commit.stdout).trim())?;
    let patch_digest = digest(&diff.stdout);
    apply_controller_event(
        record,
        ReleaseControllerEvent::VerifiedRepair(crate::release_recovery::RepairAttempt {
            fingerprint: failure.fingerprint,
            causal_family: format!("{}:{}", failure.workflow_name, failure.job_name),
            hypothesis: if assistant_summary.trim().is_empty() {
                "bounded repair agent changed the classified causal failure".into()
            } else {
                assistant_summary.chars().take(1024).collect()
            },
            source_commit_before: base,
            source_commit_after: Some(commit),
            focused_proof: focused_command,
            focused_status: crate::release_recovery::FocusedProofStatus::Passed,
            evidence_refs: vec![
                format!("repair:patch:{patch_digest}"),
                "repair:agent-tool-history".into(),
            ],
            disproven_or_insufficient: false,
        }),
    )?;
    record.mutation.final_candidate_commit = record.release_commit.clone();
    ledger.save(record)?;
    let _ = Command::new("git")
        .current_dir(workspace)
        .args(["worktree", "remove", "--force"])
        .arg(&repair_root)
        .output();
    Ok(())
}

fn observed_repair_receipts(
    history: &[vesper_domain::ConversationMessage],
) -> (bool, Option<String>) {
    let mut calls = HashMap::<String, (String, serde_json::Value, usize)>::new();
    let mut successful_mutation_at = None;
    let mut focused_command = None;
    let mut ordinal = 0usize;
    for message in history {
        for part in &message.content {
            match part {
                ContentPart::ToolCall(call) => {
                    ordinal += 1;
                    calls.insert(
                        call.id.as_str().to_owned(),
                        (
                            call.tool_id.as_str().to_owned(),
                            call.arguments.clone(),
                            ordinal,
                        ),
                    );
                }
                ContentPart::ToolResult(result) if result.status == ToolResultStatus::Succeeded => {
                    if let Some((name, arguments, at)) = calls.get(result.call_id.as_str()) {
                        if matches!(
                            name.as_str(),
                            "write_file" | "edit_file" | "apply_patch" | "apply_patch_set"
                        ) {
                            successful_mutation_at = Some(*at);
                            focused_command = None;
                        } else if name == "run_command"
                            && successful_mutation_at.is_some_and(|mutation| *at > mutation)
                            && let Some(command) =
                                arguments.get("command").and_then(serde_json::Value::as_str)
                        {
                            focused_command = Some(redact_secrets(command));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (successful_mutation_at.is_some(), focused_command)
}

/// Runs one deterministic controller step and persists after every settled
/// side effect. It stops at remote waiting, diagnosis, pause, or a terminal
/// lifecycle boundary; callers may invoke it again through `/release resume`.
pub struct ReleaseAdvanceContext<'a> {
    pub workspace: &'a Path,
    pub repository: &'a str,
    pub ledger: &'a ReleaseLedger,
    pub executor: &'a dyn ReleaseExecutionPort,
    pub github: &'a dyn GitHubEvidencePort,
    pub health: &'a dyn ExternalHealthPort,
    pub repair_factory: Option<&'a crate::WorkerFactory>,
    pub cancelled: Arc<AtomicBool>,
}

pub fn advance_release(
    record: &mut ReleaseRecoveryRecord,
    context: ReleaseAdvanceContext<'_>,
) -> Result<(), RrcError> {
    let ReleaseAdvanceContext {
        workspace,
        repository,
        ledger,
        executor,
        github,
        health,
        repair_factory,
        cancelled,
    } = context;
    match record.state {
        ReleaseRecoveryState::LocalVerification => {
            if record.mutation.version_after.is_none() {
                let receipt = executor.prepare_version_bump(
                    &record.objective.bump,
                    admit_release_mutation(record, ReleaseMutationKind::VersionBump)?,
                )?;
                record.mutation.source_commit = record.release_commit.clone();
                record.mutation.version_before = Some(receipt.before);
                record.mutation.version_after = Some(receipt.after);
                record.mutation.version_files = receipt.files;
                record.mutation.local_gates = production_local_gates();
                record.note_progress_milestone(format!(
                    "Version preparation completed; {} local gates are ready",
                    record.mutation.local_gates.len()
                ));
                ledger.save(record)?;
                return Ok(());
            }
            if let Some(index) = record
                .mutation
                .local_gates
                .iter()
                .position(|gate| gate.state != SettlementState::Succeeded)
            {
                record.mutation.local_gates[index].state = SettlementState::Running;
                let gate = record.mutation.local_gates[index].clone();
                record.note_progress_milestone(format!(
                    "Running local gate {}/{}: {}",
                    index + 1,
                    record.mutation.local_gates.len(),
                    gate.name
                ));
                ledger.save(record)?;
                let outcome = executor.run_local_gate(&gate);
                if let Some(latest) = ledger.load()?
                    && latest.state == ReleaseRecoveryState::Cancelled
                {
                    *record = latest;
                    return Ok(());
                }
                match outcome {
                    Ok(output) => {
                        record.mutation.local_gates[index].state = SettlementState::Succeeded;
                        record.mutation.local_gates[index].evidence_ref =
                            Some(format!("local:{}:{}", gate.name, digest(output.as_bytes())));
                        record.note_progress_milestone(format!(
                            "Passed local gate {}/{}: {}",
                            index + 1,
                            record.mutation.local_gates.len(),
                            gate.name
                        ));
                        ledger.save(record)?;
                    }
                    Err(RrcError::ResourceConstrained(detail)) => {
                        // A safety defer is not evidence of a source or gate
                        // failure. Keep the same exact local-verification
                        // epoch resumable, release any running marker, and
                        // never spend a repair/retry budget diagnosing RAM.
                        record.mutation.local_gates[index].state = SettlementState::NotStarted;
                        record.mutation.local_gates[index].evidence_ref =
                            Some("local:resource-deferred".into());
                        record.note_progress_milestone(format!(
                            "Paused before {}: Host Resource Governor deferred unsafe local work",
                            gate.name
                        ));
                        ledger.save(record)?;
                        return Err(RrcError::ResourceConstrained(detail));
                    }
                    Err(error) => {
                        record.mutation.local_gates[index].state = SettlementState::Failed;
                        record.mutation.local_gates[index].evidence_ref =
                            Some("local:failed".into());
                        record.note_progress_milestone(format!(
                            "Local gate failed: {}; preserving failure evidence",
                            gate.name
                        ));
                        apply_controller_event(
                            record,
                            ReleaseControllerEvent::LocalVerificationFailed(vec![format!(
                                "local:{}",
                                gate.name
                            )]),
                        )?;
                        ledger.save(record)?;
                        return Err(error);
                    }
                }
                return Ok(());
            }
            if !record.mutation.candidate_committed {
                let version = record
                    .mutation
                    .version_after
                    .clone()
                    .ok_or_else(|| RrcError::Invalid("version provenance is missing".into()))?;
                let candidate = executor.commit_candidate(
                    &version,
                    admit_release_mutation(record, ReleaseMutationKind::CommitCandidate)?,
                )?;
                record.mutation.candidate_committed = true;
                apply_controller_event(
                    record,
                    ReleaseControllerEvent::LocalVerificationPassed {
                        version,
                        candidate_commit: candidate,
                        evidence_refs: record
                            .mutation
                            .local_gates
                            .iter()
                            .filter_map(|gate| gate.evidence_ref.clone())
                            .collect(),
                    },
                )?;
                record.mutation.final_candidate_commit = record.release_commit.clone();
                ledger.save(record)?;
            }
        }
        ReleaseRecoveryState::CandidateReady => {
            let pushed = executor.push_candidate(admit_release_mutation(
                record,
                ReleaseMutationKind::PushCandidate,
            )?)?;
            record.mutation.candidate_pushed = true;
            record.mutation.candidate_push_ref = Some(pushed.clone());
            apply_controller_event(
                record,
                ReleaseControllerEvent::RemoteGateDispatched(vec![pushed]),
            )?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::RetryAdmissible => {
            // A source repair changes the candidate SHA. GitHub's Actions
            // rerun APIs preserve the original GITHUB_SHA, so retrying the
            // old runs would verify the wrong commit. Push the independently
            // recorded repair commit and let the normal push workflows create
            // a fresh exact-SHA gate set.
            let admission = admit_release_mutation(record, ReleaseMutationKind::PushCandidate)?;
            let pushed = executor.push_candidate(admission)?;
            record.consume_retry(crate::release_recovery::RetryKind::FullGate)?;
            record.mutation.candidate_pushed = true;
            record.mutation.candidate_push_ref = Some(pushed.clone());
            record.required_gates.clear();
            let commit = record
                .release_commit
                .clone()
                .ok_or_else(|| RrcError::Invalid("repaired candidate commit is missing".into()))?;
            record.transition(
                ReleaseRecoveryState::RemoteGateRunning,
                &commit,
                "verified repair candidate pushed for a fresh exact-SHA gate set",
                vec![pushed],
                None,
            )?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::RemoteGateRunning
        | ReleaseRecoveryState::WaitingForMatrix
        | ReleaseRecoveryState::PostReleaseMainDegraded => {
            refresh_remote_evidence(record, repository, github)?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::ClassifyingFailure
            if record
                .failures
                .last()
                .is_some_and(|failure| !failure.class.infrastructure_like()) =>
        {
            let factory = repair_factory.ok_or_else(|| {
                RrcError::Invalid(
                    "release repair requires an active provider-backed host; resume from TUI or ACP"
                        .into(),
                )
            })?;
            run_bounded_repair_agent(workspace, record, ledger, factory, Arc::clone(&cancelled))?;
        }
        ReleaseRecoveryState::ClassifyingFailure
            if record
                .failures
                .last()
                .is_some_and(|failure| failure.class.infrastructure_like()) =>
        {
            let evidence = external_health_evidence(record, health)?;
            apply_controller_event(record, ReleaseControllerEvent::ExternalHealth(evidence))?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::PausedExternal => {
            let evidence = external_health_evidence(record, health)?;
            match classify_external_health(evidence) {
                ExternalHealthVerdict::Confirmed(block) => {
                    record.external_block = Some(block);
                    record.note_progress(false);
                }
                ExternalHealthVerdict::Unconfirmed | ExternalHealthVerdict::NotAdmissible => {
                    record.external_block = None;
                    record.state_changes.push(RelevantStateChange {
                        kind: RelevantStateChangeKind::ExternalServiceRecovered,
                        description: "official degradation no longer confirmed".into(),
                        evidence_refs: vec!["github:official-status-recheck".into()],
                        observed_at: Utc::now(),
                    });
                    let commit = record
                        .release_commit
                        .clone()
                        .or(record.current_main.clone())
                        .ok_or_else(|| {
                            RrcError::Invalid("controller commit is not recorded".into())
                        })?;
                    record.transition(
                        ReleaseRecoveryState::RemoteGateRunning,
                        &commit,
                        "external service recovery requires fresh exact-SHA evidence",
                        vec!["github:official-status-recheck".into()],
                        None,
                    )?;
                }
            }
            ledger.save(record)?;
        }
        ReleaseRecoveryState::RemoteGatesGreen => {
            let version = record
                .release_version
                .clone()
                .ok_or_else(|| RrcError::Invalid("release version is missing".into()))?;
            let commit = record
                .release_commit
                .clone()
                .ok_or_else(|| RrcError::Invalid("release commit is missing".into()))?;
            let (tag, object) = executor.create_and_push_tag(
                &version,
                &commit,
                admit_release_mutation(record, ReleaseMutationKind::CreateTag)?,
            )?;
            record.mutation.tag_name = Some(tag.clone());
            record.mutation.tag_object = Some(object.clone());
            record.mutation.tag_pushed = true;
            apply_controller_event(
                record,
                ReleaseControllerEvent::TagVerified(vec![format!("git:tag:{object}")]),
            )?;
            apply_controller_event(
                record,
                ReleaseControllerEvent::PublicationStarted(vec![format!("git:tag:{tag}")]),
            )?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::Publishing => {
            let _admission = admit_release_mutation(record, ReleaseMutationKind::Publish)?;
            let tag = record
                .mutation
                .tag_name
                .clone()
                .ok_or_else(|| RrcError::Invalid("release tag is missing".into()))?;
            if let Some(receipt) = executor.publication(repository, &tag)? {
                record.mutation.publication_run_id = Some(receipt.run_id);
                record.mutation.publication_verified = true;
                record.mutation.published_asset_names = receipt.assets;
                apply_controller_event(
                    record,
                    ReleaseControllerEvent::PublicationVerified {
                        version: receipt.version,
                        evidence_refs: vec![
                            format!("github:run:{}", receipt.run_id),
                            format!("github:release:{tag}"),
                        ],
                    },
                )?;
                ledger.save(record)?;
            }
        }
        ReleaseRecoveryState::Published => {
            // Publication is an immutable stop boundary. A later closeout/main
            // SHA is admitted only from independently observed main evidence.
        }
        _ => {}
    }
    let _ = workspace;
    Ok(())
}

pub fn spawn_release_worker(
    workspace: PathBuf,
    repository: String,
    repo_identity: String,
) -> Result<ReleaseWorkerRegistration, RrcError> {
    spawn_release_worker_with_factory(workspace, repository, repo_identity, None)
}

pub fn spawn_release_worker_with_factory(
    workspace: PathBuf,
    repository: String,
    repo_identity: String,
    repair_factory: Option<crate::WorkerFactory>,
) -> Result<ReleaseWorkerRegistration, RrcError> {
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    spawn_release_worker_at_root(workspace, repository, repo_identity, repair_factory, root)
}

pub(crate) fn spawn_release_worker_at_root(
    workspace: PathBuf,
    repository: String,
    repo_identity: String,
    repair_factory: Option<crate::WorkerFactory>,
    root: PathBuf,
) -> Result<ReleaseWorkerRegistration, RrcError> {
    let ledger = ReleaseLedger::open(root.clone(), &repo_identity)?;
    let mut record = ledger.load()?.ok_or_else(|| {
        RrcError::Invalid("release checkpoint disappeared before worker spawn".into())
    })?;
    record.refresh_progress();
    let epoch_id = record.epoch_id.clone();
    // RRC uses a controller-owned user-state cache, never the source
    // worktree's `target/`. The single scheduler root serializes expensive
    // compiler/linker gates across RRC epochs; the repository digest keeps
    // compiled artifacts isolated between workspaces.
    let resource_root = root.join("host-resources");
    let resource_governor = HostResourceGovernor::new(
        resource_policy_for_worker(),
        resource_root.join("scheduler"),
        resource_root
            .join("targets")
            .join(digest(repo_identity.as_bytes())),
    )
    .map_err(|error| RrcError::Invalid(format!("Host Resource Governor setup failed: {error}")))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let now = Instant::now();
    let activity = Arc::new(Mutex::new(ReleaseWorkerActivity {
        epoch_id: epoch_id.clone(),
        stage: "Release recovery".into(),
        detail: release_activity_detail(&record),
        current_gate: None,
        current_command: None,
        current_child: None,
        gate_started_at: None,
        last_activity_at: now,
        completed_gates: 0,
        total_gates: record.mutation.local_gates.len(),
        version_before: record.mutation.version_before.clone(),
        version_after: record.mutation.version_after.clone(),
        candidate_sha: None,
        retry_budget: retry_budget_label(&record),
        failure_fingerprint: record
            .failures
            .last()
            .map(|failure| failure.fingerprint.0.clone()),
        recent_output: VecDeque::new(),
        progress: record.progress.clone(),
        resource_telemetry: resource_governor.snapshot(None).ok(),
    }));
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel::<()>(0);
    let worker_identity = repo_identity.clone();
    let worker_cancelled = Arc::clone(&cancelled);
    let worker_activity = Arc::clone(&activity);
    let handle = std::thread::Builder::new()
        .name("vesper-release-controller".into())
        .spawn(move || {
            if start_rx.recv().is_err() {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_release_worker(
                    &workspace,
                    &repository,
                    &worker_identity,
                    repair_factory.as_ref(),
                    &root,
                    Arc::clone(&worker_cancelled),
                    &worker_activity,
                    resource_governor,
                )
            }))
            .unwrap_or_else(|_| {
                Err(RrcError::Invalid(
                    "release controller task panicked before settlement".into(),
                ))
            });
            if let Err(error) = result {
                persist_worker_failure(&root, &worker_identity, &error);
                if let Ok(mut activity) = worker_activity.lock() {
                    activity.detail = if matches!(error, RrcError::ResourceConstrained(_)) {
                        "Local verification paused for host resource recovery".into()
                    } else {
                        "Release recovery paused safely".into()
                    };
                    activity.last_activity_at = Instant::now();
                    push_telemetry_line(&mut activity, &error.to_string());
                }
            }
            if let Ok(mut guard) = active_workers().lock() {
                guard.remove(&worker_identity);
            }
        })?;
    {
        let mut guard = active_workers()
            .lock()
            .map_err(|_| RrcError::Invalid("release worker lock is poisoned".into()))?;
        if let Some(existing) = guard.get(&repo_identity) {
            return Ok(ReleaseWorkerRegistration {
                repo_identity,
                epoch_id: existing
                    .activity
                    .lock()
                    .map_err(|_| {
                        RrcError::Invalid("release worker activity lock is poisoned".into())
                    })?
                    .epoch_id
                    .clone(),
            });
        }
        guard.insert(
            repo_identity.clone(),
            ActiveReleaseWorker {
                cancelled,
                activity,
                _handle: handle,
            },
        );
    }
    start_tx.send(()).map_err(|_| {
        RrcError::Invalid("release worker exited before execution admission".into())
    })?;
    Ok(ReleaseWorkerRegistration {
        repo_identity,
        epoch_id,
    })
}

#[allow(clippy::too_many_arguments)] // background-worker composition owns distinct state, ports and cancellation.
fn run_release_worker(
    workspace: &Path,
    repository: &str,
    repo_identity: &str,
    repair_factory: Option<&crate::WorkerFactory>,
    root: &Path,
    cancelled: Arc<AtomicBool>,
    activity: &Arc<Mutex<ReleaseWorkerActivity>>,
    resource_governor: HostResourceGovernor,
) -> Result<(), RrcError> {
    let ledger = ReleaseLedger::open(root.to_path_buf(), repo_identity)?;
    let executor = NativeReleaseExecutor::with_activity_and_governor(
        workspace,
        Arc::clone(&cancelled),
        Arc::clone(activity),
        resource_governor,
    )?;
    for _ in 0..16 {
        let Some(mut record) = ledger.load()? else {
            return Ok(());
        };
        update_release_activity(activity, &record);
        let before = record.state;
        advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace,
                repository,
                ledger: &ledger,
                executor: &executor,
                github: &GhCliEvidenceAdapter,
                health: &CurlGitHubStatusAdapter,
                repair_factory,
                cancelled: Arc::clone(&cancelled),
            },
        )?;
        update_release_activity(activity, &record);
        if matches!(
            record.state,
            ReleaseRecoveryState::WaitingForMatrix
                | ReleaseRecoveryState::ClassifyingFailure
                | ReleaseRecoveryState::NeedMoreEvidence
                | ReleaseRecoveryState::PausedExternal
                | ReleaseRecoveryState::Published
                | ReleaseRecoveryState::PostReleaseMainDegraded
                | ReleaseRecoveryState::Escalated
                | ReleaseRecoveryState::Complete
                | ReleaseRecoveryState::Cancelled
        ) || before == ReleaseRecoveryState::RetryAdmissible
            || (record.state == before && record.state != ReleaseRecoveryState::LocalVerification)
        {
            break;
        }
    }
    Ok(())
}

fn retry_budget_label(record: &ReleaseRecoveryRecord) -> String {
    format!(
        "full {}/{} · infra {}/{} · diagnostic {}/{}",
        record.retry_budget.full_gate_used,
        record.retry_budget.full_gate_limit,
        record.retry_budget.infrastructure_used,
        record.retry_budget.infrastructure_limit,
        record.retry_budget.targeted_diagnostic_used,
        record.retry_budget.targeted_diagnostic_limit,
    )
}

fn child_name_from_output(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let candidate = trimmed
        .strip_prefix("Running ")
        .or_else(|| trimmed.strip_prefix("running "))?;
    let raw = candidate
        .rsplit_once('(')
        .map_or(candidate, |(_, path)| path.trim_end_matches(')').trim());
    let file = Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(raw);
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    let stem = stem
        .rsplit_once('-')
        .filter(|(_, suffix)| suffix.len() >= 8 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map_or(stem, |(name, _)| name);
    (!stem.is_empty()).then(|| stem.to_owned())
}

fn push_telemetry_line(activity: &mut ReleaseWorkerActivity, line: &str) {
    let clean = redact_secrets(&sanitize_terminal_text(line));
    let bounded = clean
        .chars()
        .take(MAX_TELEMETRY_LINE_CHARS)
        .collect::<String>();
    activity.recent_output.push_back(bounded);
    while activity.recent_output.len() > MAX_TELEMETRY_LINES {
        activity.recent_output.pop_front();
    }
}

fn release_activity_detail(record: &ReleaseRecoveryRecord) -> String {
    let mut view = record.clone();
    view.refresh_progress();
    view.progress.headline
}

fn update_release_activity(
    activity: &Arc<Mutex<ReleaseWorkerActivity>>,
    record: &ReleaseRecoveryRecord,
) {
    let mut progress_view = record.clone();
    progress_view.refresh_progress();
    if let Ok(mut activity) = activity.lock() {
        let detail = progress_view.progress.headline.clone();
        let completed = record
            .mutation
            .local_gates
            .iter()
            .filter(|gate| gate.state == SettlementState::Succeeded)
            .count();
        let active_gate = record
            .mutation
            .local_gates
            .iter()
            .find(|gate| gate.state == SettlementState::Running);
        let changed = activity.detail != detail
            || activity.completed_gates != completed
            || activity.current_gate.as_deref() != active_gate.map(|gate| gate.name.as_str());
        activity.epoch_id.clone_from(&record.epoch_id);
        activity.detail = detail;
        activity.completed_gates = completed;
        activity.total_gates = record.mutation.local_gates.len();
        activity
            .version_before
            .clone_from(&record.mutation.version_before);
        activity
            .version_after
            .clone_from(&record.mutation.version_after);
        activity.candidate_sha = record
            .mutation
            .candidate_committed
            .then(|| record.release_commit.clone())
            .flatten();
        activity.retry_budget = retry_budget_label(record);
        activity.failure_fingerprint = record
            .failures
            .last()
            .map(|failure| failure.fingerprint.0.clone());
        activity.progress = progress_view.progress.clone();
        if let Some(gate) = active_gate {
            if activity.current_gate.as_deref() != Some(gate.name.as_str()) {
                activity.gate_started_at = Some(Instant::now());
            }
            activity.current_gate = Some(gate.name.clone());
            activity.current_command = Some(gate.command.clone());
        } else if record.state != ReleaseRecoveryState::LocalVerification {
            activity.current_gate = None;
            activity.current_command = None;
            activity.current_child = None;
            activity.gate_started_at = None;
        }
        if changed {
            activity.last_activity_at = Instant::now();
        }
    }
}

fn persist_worker_failure(root: &Path, repo_identity: &str, error: &RrcError) {
    // A governor defer deliberately leaves the epoch in LocalVerification so
    // that a later `/release resume` re-samples the host. Do not transform a
    // workstation-safety condition into a source-repair diagnosis.
    if matches!(error, RrcError::ResourceConstrained(_)) {
        return;
    }
    let Ok(ledger) = ReleaseLedger::open(root.to_path_buf(), repo_identity) else {
        return;
    };
    let Ok(Some(mut record)) = ledger.load() else {
        return;
    };
    if record.state == ReleaseRecoveryState::LocalVerification {
        if let Some(gate) = record
            .mutation
            .local_gates
            .iter_mut()
            .find(|gate| gate.state == SettlementState::Running)
        {
            gate.state = SettlementState::Failed;
            gate.evidence_ref = Some("local:worker-failed".into());
        }
        if let Some(commit) = record.release_commit.clone() {
            let _ = record.transition(
                ReleaseRecoveryState::DiagnosingLocalFailure,
                &commit,
                format!("release controller task failed: {error}"),
                vec!["local:controller-task-failed".into()],
                None,
            );
        }
        let _ = ledger.save(&record);
    }
}

#[must_use]
pub fn active_release_worker(repo_identity: &str) -> Option<ReleaseWorkerSnapshot> {
    let guard = active_workers().lock().ok()?;
    let worker = guard.get(repo_identity)?;
    let activity = worker.activity.lock().ok()?;
    let now = Instant::now();
    Some(ReleaseWorkerSnapshot {
        repo_identity: repo_identity.to_owned(),
        epoch_id: activity.epoch_id.clone(),
        stage: activity.stage.clone(),
        detail: activity.detail.clone(),
        cancellable: !worker.cancelled.load(Ordering::Acquire),
        current_gate: activity.current_gate.clone(),
        current_command: activity.current_command.clone(),
        current_child: activity.current_child.clone(),
        gate_elapsed_secs: activity
            .gate_started_at
            .map(|started| now.saturating_duration_since(started).as_secs()),
        last_activity_ago_secs: now
            .saturating_duration_since(activity.last_activity_at)
            .as_secs(),
        completed_gates: activity.completed_gates,
        total_gates: activity.total_gates,
        version_before: activity.version_before.clone(),
        version_after: activity.version_after.clone(),
        candidate_sha: activity.candidate_sha.clone(),
        retry_budget: activity.retry_budget.clone(),
        failure_fingerprint: activity.failure_fingerprint.clone(),
        recent_output: activity.recent_output.iter().cloned().collect(),
        process_alive: activity.current_child.is_some(),
        progress: activity.progress.clone(),
        resource_telemetry: activity.resource_telemetry.clone(),
    })
}

#[cfg(test)]
pub(crate) fn active_release_worker_count_for_repo(repo_identity: &str) -> usize {
    active_workers().lock().map_or(0, |workers| {
        usize::from(workers.contains_key(repo_identity))
    })
}

#[must_use]
pub fn active_release_worker_for_workspace(workspace: &Path) -> Option<ReleaseWorkerSnapshot> {
    let identity = crate::release_recovery::repository_identity_for_workspace(workspace).ok()?;
    active_release_worker(&identity)
}

/// Renders the registered controller's live state for text-only hosts such as
/// ACP. The values come from the process owner; no host infers activity from
/// admission prose or a ledger timestamp.
#[must_use]
pub fn render_active_worker_status(worker: &ReleaseWorkerSnapshot) -> String {
    let mut lines = vec![format!("RUN                  {}", worker.detail)];
    lines.push(format!(
        "Progress             {} · {}/{} local · {}/{} remote jobs",
        worker.progress.phase.label(),
        worker.progress.completed_local_gates,
        worker.progress.total_local_gates,
        worker.progress.terminal_remote_jobs,
        worker.progress.total_remote_jobs,
    ));
    if let Some(gate) = worker.current_gate.as_deref() {
        lines.push(format!("Gate                 {gate}"));
    }
    if let Some(command) = worker.current_command.as_deref() {
        lines.push(format!("Command              {command}"));
    }
    if let Some(child) = worker.current_child.as_deref() {
        lines.push(format!("Current process      {child}"));
    }
    if let Some(elapsed) = worker.gate_elapsed_secs {
        lines.push(format!("Gate elapsed         {elapsed}s"));
    }
    lines.push(format!(
        "Activity             {}s ago{}",
        worker.last_activity_ago_secs,
        if worker.process_alive {
            " (process alive)"
        } else {
            ""
        }
    ));
    lines.push(format!(
        "Gates                {}/{}",
        worker.completed_gates, worker.total_gates
    ));
    if !worker.progress.tasks.is_empty() {
        lines.push("Tasks".into());
        for task in &worker.progress.tasks {
            render_progress_task(&mut lines, task, 0);
        }
    }
    for milestone in worker.progress.milestones.iter().rev().take(3).rev() {
        lines.push(format!(
            "Milestone #{}       {}",
            milestone.sequence, milestone.summary
        ));
    }
    if let Some(resources) = worker.resource_telemetry.as_ref() {
        lines.push(resources.render());
    }
    lines.join("\n")
}

fn render_progress_task(
    lines: &mut Vec<String>,
    task: &crate::release_recovery::ReleaseProgressTask,
    depth: usize,
) {
    let indent = "  ".repeat(depth.min(3));
    let units = task
        .units
        .render()
        .map(|units| format!(" · {units}"))
        .unwrap_or_default();
    lines.push(format!(
        "{indent}{} · {}{units}",
        task.state.label(),
        task.name
    ));
    for child in &task.children {
        render_progress_task(lines, child, depth.saturating_add(1));
    }
}

/// Debug-build-only end-to-end probe used by the native PTY regression.
/// It exercises the exact production capture and worker-snapshot path without
/// admitting a release or mutating release state.
#[cfg(debug_assertions)]
pub fn spawn_terminal_ownership_probe(
    workspace: &Path,
    program: &Path,
) -> Result<ReleaseWorkerRegistration, RrcError> {
    let repo_identity = crate::release_recovery::repository_identity_for_workspace(workspace)?;
    let epoch_id = format!("terminal-ownership-probe-{}", std::process::id());
    let cancelled = Arc::new(AtomicBool::new(false));
    let now = Instant::now();
    let activity = Arc::new(Mutex::new(ReleaseWorkerActivity {
        epoch_id: epoch_id.clone(),
        stage: "Release recovery".into(),
        detail: "Local verification · terminal-ownership-probe".into(),
        current_gate: Some("terminal-ownership-probe".into()),
        current_command: Some(program.display().to_string()),
        current_child: Some(
            program
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("probe")
                .to_owned(),
        ),
        gate_started_at: Some(now),
        last_activity_at: now,
        completed_gates: 0,
        total_gates: 1,
        version_before: Some("0.24.4".into()),
        version_after: Some("0.24.5".into()),
        candidate_sha: None,
        retry_budget: "probe".into(),
        failure_fingerprint: None,
        recent_output: VecDeque::new(),
        progress: ReleaseProgress::default(),
        resource_telemetry: None,
    }));
    let workspace = workspace.canonicalize()?;
    let program = program.to_path_buf();
    let worker_identity = repo_identity.clone();
    let worker_cancelled = Arc::clone(&cancelled);
    let worker_activity = Arc::clone(&activity);
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel::<()>(0);
    let handle = thread::Builder::new()
        .name("vesper-release-terminal-probe".into())
        .spawn(move || {
            if start_rx.recv().is_err() {
                return;
            }
            let result = NativeReleaseExecutor::with_activity(
                &workspace,
                Arc::clone(&worker_cancelled),
                Arc::clone(&worker_activity),
            )
            .and_then(|executor| {
                executor.command(
                    program.to_str().ok_or_else(|| {
                        RrcError::Invalid("terminal probe path is not UTF-8".into())
                    })?,
                    &[],
                )
            });
            if let Ok(mut activity) = worker_activity.lock() {
                activity.current_child = None;
                activity.completed_gates = usize::from(result.is_ok());
                activity.detail = if result.is_ok() {
                    "Local verification settled".into()
                } else {
                    "Local verification failed".into()
                };
                activity.last_activity_at = Instant::now();
            }
            thread::sleep(Duration::from_millis(250));
            if let Ok(mut workers) = active_workers().lock() {
                workers.remove(&worker_identity);
            }
        })?;
    {
        let mut workers = active_workers()
            .lock()
            .map_err(|_| RrcError::Invalid("release worker lock is poisoned".into()))?;
        if workers.contains_key(&repo_identity) {
            return Err(RrcError::Invalid(
                "terminal probe found an existing release worker".into(),
            ));
        }
        workers.insert(
            repo_identity.clone(),
            ActiveReleaseWorker {
                cancelled,
                activity,
                _handle: handle,
            },
        );
    }
    start_tx
        .send(())
        .map_err(|_| RrcError::Invalid("terminal probe worker exited before start".into()))?;
    Ok(ReleaseWorkerRegistration {
        repo_identity,
        epoch_id,
    })
}

/// Signals the user-owned local worker, if present. Remote GitHub workflows are
/// intentionally not cancelled or described as cancelled by this operation.
pub fn cancel_release_worker(repo_identity: &str) {
    if let Ok(guard) = active_workers().lock()
        && let Some(worker) = guard.get(repo_identity)
    {
        worker.cancelled.store(true, Ordering::Release);
    }
}

pub fn production_local_gates() -> Vec<LocalGateRecord> {
    [
        ("workspace-verify", "cargo xtask verify"),
        ("acceptance", "cargo xtask acceptance"),
        ("architecture", "cargo xtask architecture"),
        ("msrv", "cargo xtask msrv"),
        ("supply-chain-policy", "cargo deny --all-features check"),
        ("advisories", "cargo audit"),
        ("release-build", "cargo build --locked --release --package agent-vesper-acp --package agent-vesper-tui --package vesper-web-fetch --package vesper-sandbox --features agent-vesper-acp/docker,agent-vesper-tui/docker,agent-vesper-acp/swarm,agent-vesper-tui/swarm,agent-vesper-acp/bridge,agent-vesper-tui/bridge,agent-vesper-tui/voice-kokoro,agent-vesper-tui/voice-flm"),
    ].into_iter().map(|(name, command)| LocalGateRecord {
        name: name.into(), state: SettlementState::NotStarted,
        command: command.into(), evidence_ref: None,
    }).collect()
}

fn local_gate_cost(name: &str) -> GateCost {
    match name {
        // These commands build/test substantial portions of the workspace,
        // can recursively launch Cargo, or drive a release feature build.
        "workspace-verify" | "acceptance" | "architecture" | "msrv" | "release-build" => {
            GateCost::Expensive
        }
        // Metadata, advisory, and policy inspection stay controller-owned but
        // do not take the expensive compiler/linker slot.
        "supply-chain-policy" | "advisories" => GateCost::Cheap,
        // Unknown names are rejected by local_gate_argv before execution; keep
        // their classification conservative for future callers.
        _ => GateCost::Expensive,
    }
}

fn local_gate_argv(name: &str) -> Option<(&'static str, &'static [&'static str])> {
    match name {
        "workspace-verify" => Some(("cargo", &["xtask", "verify"])),
        "acceptance" => Some(("cargo", &["xtask", "acceptance"])),
        "architecture" => Some(("cargo", &["xtask", "architecture"])),
        "msrv" => Some(("cargo", &["xtask", "msrv"])),
        "supply-chain-policy" => Some(("cargo", &["deny", "--all-features", "check"])),
        "advisories" => Some(("cargo", &["audit"])),
        "release-build" => Some((
            "cargo",
            &[
                "build",
                "--locked",
                "--release",
                "--package",
                "agent-vesper-acp",
                "--package",
                "agent-vesper-tui",
                "--package",
                "vesper-web-fetch",
                "--package",
                "vesper-sandbox",
                "--features",
                "agent-vesper-acp/docker,agent-vesper-tui/docker,agent-vesper-acp/swarm,agent-vesper-tui/swarm,agent-vesper-acp/bridge,agent-vesper-tui/bridge,agent-vesper-tui/voice-kokoro,agent-vesper-tui/voice-flm",
            ],
        )),
        _ => None,
    }
}

fn require_kind(
    admission: ReleaseMutationAdmission,
    expected: ReleaseMutationKind,
) -> Result<(), RrcError> {
    if admission.kind() == expected {
        Ok(())
    } else {
        Err(RrcError::MutationBlocked(format!(
            "expected {expected:?} admission"
        )))
    }
}

fn workspace_version(manifest: &str) -> Result<String, RrcError> {
    let mut workspace_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            workspace_package = trimmed == "[workspace.package]";
            continue;
        }
        if workspace_package && trimmed.starts_with("version = ") {
            return trimmed
                .split('"')
                .nth(1)
                .map(str::to_owned)
                .ok_or_else(|| RrcError::Invalid("workspace version is malformed".into()));
        }
    }
    Err(RrcError::Invalid(
        "workspace package version is missing".into(),
    ))
}

fn bump_semver(version: &str, bump: &str) -> Result<String, RrcError> {
    let parts = version
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RrcError::Invalid("workspace version is not stable semver".into()))?;
    if parts.len() != 3 {
        return Err(RrcError::Invalid("workspace version is not x.y.z".into()));
    }
    let (major, minor, patch) = (parts[0], parts[1], parts[2]);
    Ok(match bump {
        "patch" => format!("{major}.{minor}.{}", patch + 1),
        "minor" => format!("{major}.{}.0", minor + 1),
        "major" => format!("{}.0.0", major + 1),
        _ => {
            return Err(RrcError::Invalid(
                "release bump must be patch, minor, or major".into(),
            ));
        }
    })
}

fn build_version_mutation_plan(
    workspace: &Path,
    bump: &str,
) -> Result<VersionMutationPlan, RrcError> {
    let root_path = workspace.join("Cargo.toml");
    let root = fs::read_to_string(&root_path)?;
    let before = workspace_version(&root)?;
    let after = bump_semver(&before, bump)?;
    let manifests = workspace_member_manifests(workspace, &root)?;
    let member_dirs = manifests
        .iter()
        .map(|path| {
            path.parent()
                .ok_or_else(|| RrcError::Invalid("workspace member has no parent".into()))?
                .canonicalize()
                .map_err(RrcError::Io)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut files = Vec::new();
    for path in manifests {
        let input = fs::read_to_string(&path)?;
        let is_root = path == root_path;
        let updated =
            update_workspace_manifest(&input, &before, &after, is_root, &path, &member_dirs)?;
        if updated != input {
            files.push(VersionFileMutation {
                relative_path: relative_version_path(workspace, &path)?,
                before: input.into_bytes(),
                after: updated.into_bytes(),
            });
        }
    }
    let registry_path = workspace.join("registry/agent.json");
    let registry = fs::read_to_string(&registry_path)?;
    let updated_registry = update_registry_manifest(&registry, &before, &after)?;
    files.push(VersionFileMutation {
        relative_path: "registry/agent.json".into(),
        before: registry.into_bytes(),
        after: updated_registry.into_bytes(),
    });
    Ok(VersionMutationPlan {
        before,
        after,
        files,
        generated_lockfile: "Cargo.lock".into(),
    })
}

fn workspace_member_manifests(workspace: &Path, root: &str) -> Result<Vec<PathBuf>, RrcError> {
    let mut members = vec![workspace.join("Cargo.toml")];
    let mut in_members = false;
    let mut closed = false;
    for line in root.lines() {
        let trimmed = line.trim();
        if !in_members {
            if trimmed.starts_with("members") && trimmed.contains('[') {
                in_members = true;
            } else {
                continue;
            }
        }
        for value in quoted_values(trimmed) {
            let path = workspace.join(value).join("Cargo.toml");
            if !path.is_file() {
                return Err(RrcError::Invalid(format!(
                    "workspace member manifest is missing: {}",
                    relative_version_path(workspace, &path)?
                )));
            }
            members.push(path);
        }
        if trimmed.contains(']') {
            closed = true;
            break;
        }
    }
    if !closed {
        return Err(RrcError::Invalid(
            "workspace members inventory is malformed".into(),
        ));
    }
    members.sort();
    members.dedup();
    Ok(members)
}

fn quoted_values(line: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else { break };
        values.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    values
}

fn relative_version_path(workspace: &Path, path: &Path) -> Result<String, RrcError> {
    path.strip_prefix(workspace)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|_| RrcError::Invalid("version path escaped release workspace".into()))
}

fn update_workspace_manifest(
    manifest: &str,
    before: &str,
    after: &str,
    is_root: bool,
    manifest_path: &Path,
    member_dirs: &BTreeSet<PathBuf>,
) -> Result<String, RrcError> {
    let lines = manifest.lines().collect::<Vec<_>>();
    let (multiline_path_lines, multiline_version_lines) =
        multiline_internal_pins(&lines, before, manifest_path, member_dirs)?;
    let mut output = String::with_capacity(manifest.len());
    let mut workspace_package = false;
    let mut workspace_version_seen = false;
    let mut package_inherits_workspace_version = is_root;
    for (line_index, line) in lines.into_iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            workspace_package = trimmed == "[workspace.package]";
        }
        if trimmed == "version.workspace = true" {
            package_inherits_workspace_version = true;
        }
        let mut updated = line.to_owned();
        if multiline_version_lines.contains(&line_index) {
            updated = line.replacen(
                &format!("version = \"={before}\""),
                &format!("version = \"={after}\""),
                1,
            );
        }
        if workspace_package && trimmed.starts_with("version = ") {
            let version = quoted_values(trimmed)
                .first()
                .copied()
                .ok_or_else(|| RrcError::Invalid("workspace version is malformed".into()))?;
            if !is_root || version != before {
                return Err(RrcError::Invalid(format!(
                    "workspace version preflight mismatch in {}: expected {before}, found {version}",
                    manifest_path.display()
                )));
            }
            workspace_version_seen = true;
            updated = line.replacen(before, after, 1);
        }
        if let Some(path_value) = inline_key_value(trimmed, "path") {
            let parent = manifest_path
                .parent()
                .ok_or_else(|| RrcError::Invalid("manifest has no parent".into()))?;
            let dependency_dir = parent.join(path_value).canonicalize()?;
            if member_dirs.contains(&dependency_dir) && !multiline_path_lines.contains(&line_index)
            {
                let version = inline_key_value(trimmed, "version").ok_or_else(|| {
                    RrcError::Invalid(format!(
                        "workspace dependency in {} lacks an exact version pin: {trimmed}",
                        manifest_path.display()
                    ))
                })?;
                let expected = format!("={before}");
                if version != expected {
                    return Err(RrcError::Invalid(format!(
                        "workspace dependency preflight mismatch in {}: expected {expected}, found {version}",
                        manifest_path.display()
                    )));
                }
                updated = updated.replacen(
                    &format!("version = \"{expected}\""),
                    &format!("version = \"={after}\""),
                    1,
                );
            }
        }
        output.push_str(&updated);
        output.push('\n');
    }
    if is_root && !workspace_version_seen {
        return Err(RrcError::Invalid(
            "workspace version did not match expected source".into(),
        ));
    }
    if !package_inherits_workspace_version {
        return Err(RrcError::Invalid(format!(
            "workspace member does not inherit workspace.package.version: {}",
            manifest_path.display()
        )));
    }
    Ok(output)
}

fn multiline_internal_pins(
    lines: &[&str],
    before: &str,
    manifest_path: &Path,
    member_dirs: &BTreeSet<PathBuf>,
) -> Result<(BTreeSet<usize>, BTreeSet<usize>), RrcError> {
    let mut path_lines = BTreeSet::new();
    let mut version_lines = BTreeSet::new();
    let mut start = 0;
    while start < lines.len() {
        let header = lines[start].trim();
        if !is_dependency_detail_header(header) {
            start += 1;
            continue;
        }
        let end = (start + 1..lines.len())
            .find(|index| lines[*index].trim().starts_with('['))
            .unwrap_or(lines.len());
        let path_row = (start + 1..end).find_map(|index| {
            inline_key_value(lines[index].trim(), "path").map(|path| (index, path))
        });
        if let Some((path_index, path_value)) = path_row {
            let parent = manifest_path
                .parent()
                .ok_or_else(|| RrcError::Invalid("manifest has no parent".into()))?;
            let dependency_dir = parent.join(path_value).canonicalize()?;
            if member_dirs.contains(&dependency_dir) {
                let (version_index, version) = (start + 1..end)
                    .find_map(|index| {
                        inline_key_value(lines[index].trim(), "version")
                            .map(|version| (index, version))
                    })
                    .ok_or_else(|| {
                        RrcError::Invalid(format!(
                            "workspace dependency table in {} lacks an exact version pin",
                            manifest_path.display()
                        ))
                    })?;
                let expected = format!("={before}");
                if version != expected {
                    return Err(RrcError::Invalid(format!(
                        "workspace dependency preflight mismatch in {}: expected {expected}, found {version}",
                        manifest_path.display()
                    )));
                }
                path_lines.insert(path_index);
                version_lines.insert(version_index);
            }
        }
        start = end;
    }
    Ok((path_lines, version_lines))
}

fn is_dependency_detail_header(line: &str) -> bool {
    let Some(section) = line
        .strip_prefix('[')
        .and_then(|line| line.strip_suffix(']'))
    else {
        return false;
    };
    section.starts_with("dependencies.")
        || section.starts_with("dev-dependencies.")
        || section.starts_with("build-dependencies.")
        || section.contains(".dependencies.")
}

fn inline_key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("{key} = \"");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    Some(&rest[..rest.find('"')?])
}

fn apply_version_mutation_plan(
    workspace: &Path,
    plan: &VersionMutationPlan,
    fail_after: Option<usize>,
) -> Result<(), RrcError> {
    let mut written: Vec<&VersionFileMutation> = Vec::new();
    for (index, file) in plan.files.iter().enumerate() {
        if fail_after == Some(index) {
            for prior in written.iter().rev() {
                atomic_write(&workspace.join(&prior.relative_path), &prior.before)?;
            }
            return Err(RrcError::Invalid(
                "injected version mutation write failure".into(),
            ));
        }
        if let Err(error) = atomic_write(&workspace.join(&file.relative_path), &file.after) {
            for prior in written.iter().rev() {
                atomic_write(&workspace.join(&prior.relative_path), &prior.before)?;
            }
            return Err(error);
        }
        written.push(file);
    }
    Ok(())
}

fn restore_version_mutation(
    workspace: &Path,
    plan: &VersionMutationPlan,
    lock_path: &Path,
    lock_before: &[u8],
) -> Result<(), RrcError> {
    for file in &plan.files {
        atomic_write(&workspace.join(&file.relative_path), &file.before)?;
    }
    atomic_write(lock_path, lock_before)
}

fn validate_version_mutation(
    workspace: &Path,
    plan: &VersionMutationPlan,
    cargo: &CargoResourcePolicy,
) -> Result<(), RrcError> {
    let root_path = workspace.join("Cargo.toml");
    let root = fs::read_to_string(&root_path)?;
    if workspace_version(&root)? != plan.after {
        return Err(RrcError::Invalid(
            "post-mutation workspace version is inconsistent".into(),
        ));
    }
    let manifests = workspace_member_manifests(workspace, &root)?;
    let member_dirs = manifests
        .iter()
        .map(|path| path.parent().unwrap().canonicalize().map_err(RrcError::Io))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let owned_manifests = manifests
        .iter()
        .map(|path| path.canonicalize().map_err(RrcError::Io))
        .collect::<Result<BTreeSet<_>, _>>()?;
    for path in manifests {
        let input = fs::read_to_string(&path)?;
        update_workspace_manifest(
            &input,
            &plan.after,
            &plan.after,
            path == root_path,
            &path,
            &member_dirs,
        )?;
    }
    let registry = fs::read_to_string(workspace.join("registry/agent.json"))?;
    update_registry_manifest(&registry, &plan.after, &plan.after)?;
    let metadata = Command::new("cargo")
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .envs(cargo.environment())
        .output()?;
    if !metadata.status.success() {
        return Err(RrcError::Invalid(format!(
            "post-mutation cargo metadata failed: {}",
            bounded_output(&metadata.stderr)
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    for package in value["packages"].as_array().into_iter().flatten() {
        let manifest = package["manifest_path"].as_str().unwrap_or_default();
        let owned = Path::new(manifest)
            .canonicalize()
            .is_ok_and(|path| owned_manifests.contains(&path));
        if owned && package["version"].as_str() != Some(plan.after.as_str()) {
            return Err(RrcError::Invalid(format!(
                "workspace package metadata retained a stale version: {}",
                package["name"].as_str().unwrap_or("unknown")
            )));
        }
    }
    Ok(())
}

fn update_registry_manifest(input: &str, before: &str, after: &str) -> Result<String, RrcError> {
    let mut value: serde_json::Value = serde_json::from_str(input)?;
    if value.get("version").and_then(serde_json::Value::as_str) != Some(before) {
        return Err(RrcError::Invalid(
            "registry version does not match workspace".into(),
        ));
    }
    validate_registry_release_urls(&value, before)?;
    value["version"] = serde_json::Value::String(after.into());
    rewrite_version_urls(&mut value, before, after);
    let mut rendered = serde_json::to_string_pretty(&value)?;
    rendered.push('\n');
    Ok(rendered)
}

fn validate_registry_release_urls(
    value: &serde_json::Value,
    expected: &str,
) -> Result<(), RrcError> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "archive" {
                    let archive = child.as_str().ok_or_else(|| {
                        RrcError::Invalid("registry archive URL is not a string".into())
                    })?;
                    if !archive.contains(&format!("/v{expected}/")) {
                        return Err(RrcError::Invalid(format!(
                            "registry archive URL does not match workspace version {expected}"
                        )));
                    }
                }
                validate_registry_release_urls(child, expected)?;
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                validate_registry_release_urls(child, expected)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn rewrite_version_urls(value: &mut serde_json::Value, before: &str, after: &str) {
    match value {
        serde_json::Value::String(text) => {
            *text = text.replace(&format!("/v{before}/"), &format!("/v{after}/"))
        }
        serde_json::Value::Array(rows) => rows
            .iter_mut()
            .for_each(|row| rewrite_version_urls(row, before, after)),
        serde_json::Value::Object(map) => map
            .values_mut()
            .for_each(|row| rewrite_version_urls(row, before, after)),
        _ => {}
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RrcError> {
    let parent = path
        .parent()
        .ok_or_else(|| RrcError::Invalid("version path has no parent".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write as _;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| RrcError::Io(error.error))?;
    Ok(())
}

fn gh_json(workspace: &Path, args: &[&str]) -> Result<serde_json::Value, RrcError> {
    let output = Command::new("gh")
        .args(args)
        .current_dir(workspace)
        .env_remove("GH_DEBUG")
        .output()?;
    if !output.status.success() {
        return Err(RrcError::Invalid(format!(
            "GitHub command failed: {}",
            bounded_output(&output.stderr)
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(RrcError::Json)
}

fn exact_sha(value: &str) -> Result<String, RrcError> {
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(RrcError::Invalid(
            "expected exact 40-character commit identity".into(),
        ))
    }
}

fn format_command(program: &str, args: &[&str]) -> String {
    let mut text = program.to_owned();
    for arg in args {
        text.push(' ');
        text.push_str(arg);
    }
    redact_secrets(&text)
}

fn bounded_output(bytes: &[u8]) -> String {
    let clean = redact_secrets(&String::from_utf8_lossy(bytes));
    clean.chars().take(MAX_COMMAND_OUTPUT).collect()
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_telemetry_retains_only_the_newest_bounded_output_lines() {
        let now = Instant::now();
        let mut activity = ReleaseWorkerActivity {
            epoch_id: "epoch".into(),
            stage: "Release recovery".into(),
            detail: "Local verification".into(),
            current_gate: None,
            current_command: None,
            current_child: None,
            gate_started_at: None,
            last_activity_at: now,
            completed_gates: 0,
            total_gates: 7,
            version_before: None,
            version_after: None,
            candidate_sha: None,
            retry_budget: String::new(),
            failure_fingerprint: None,
            recent_output: VecDeque::new(),
            progress: ReleaseProgress::default(),
            resource_telemetry: None,
        };
        for index in 0..32 {
            push_telemetry_line(&mut activity, &format!("line-{index}"));
        }
        assert_eq!(activity.recent_output.len(), MAX_TELEMETRY_LINES);
        assert_eq!(
            activity.recent_output.front().map(String::as_str),
            Some("line-24")
        );
        assert_eq!(
            activity.recent_output.back().map(String::as_str),
            Some("line-31")
        );
        push_telemetry_line(&mut activity, &"x".repeat(MAX_TELEMETRY_LINE_CHARS + 20));
        assert_eq!(
            activity.recent_output.back().unwrap().chars().count(),
            MAX_TELEMETRY_LINE_CHARS
        );
        push_telemetry_line(
            &mut activity,
            "\u{1b}[2Junsafe\r\u{1b}]0;title\u{7}visible\u{1}text",
        );
        assert_eq!(
            activity.recent_output.back().map(String::as_str),
            Some("unsafevisibletext")
        );
    }

    #[test]
    fn trusted_acceptance_output_updates_only_the_active_acceptance_subtask() {
        let now = Instant::now();
        let activity = Arc::new(Mutex::new(ReleaseWorkerActivity {
            epoch_id: "epoch".into(),
            stage: "Release recovery".into(),
            detail: "Local verification".into(),
            current_gate: Some("acceptance".into()),
            current_command: Some("cargo xtask acceptance".into()),
            current_child: Some("cargo".into()),
            gate_started_at: Some(now),
            last_activity_at: now,
            completed_gates: 0,
            total_gates: 2,
            version_before: None,
            version_after: None,
            candidate_sha: None,
            retry_budget: String::new(),
            failure_fingerprint: None,
            recent_output: VecDeque::new(),
            progress: ReleaseProgress {
                tasks: vec![crate::release_recovery::ReleaseProgressTask {
                    name: "Local verification".into(),
                    state: crate::release_recovery::ReleaseProgressState::Running,
                    units: crate::release_recovery::ReleaseProgressUnits::counted(0, 2),
                    children: vec![crate::release_recovery::ReleaseProgressTask {
                        name: "acceptance".into(),
                        state: crate::release_recovery::ReleaseProgressState::Running,
                        units: crate::release_recovery::ReleaseProgressUnits::counted(0, 1),
                        children: vec![crate::release_recovery::ReleaseProgressTask {
                            name: "Exact acceptance cases".into(),
                            state: crate::release_recovery::ReleaseProgressState::Running,
                            units: crate::release_recovery::ReleaseProgressUnits::default(),
                            children: Vec::new(),
                        }],
                    }],
                }],
                ..ReleaseProgress::default()
            },
            resource_telemetry: None,
        }));

        record_captured_line(
            Some(&activity),
            b"acceptance progress: 18/41 \xe2\x80\x94 real named case",
        );
        record_captured_line(Some(&activity), b"acceptance progress: 42/41 bad marker");
        let guard = activity.lock().expect("activity");
        let exact_cases = &guard.progress.tasks[0].children[0].children[0];
        assert_eq!(exact_cases.units.render().as_deref(), Some("18/41"));
        assert_eq!(
            exact_cases.state,
            crate::release_recovery::ReleaseProgressState::Running
        );
    }

    fn version_fixture(member_version: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("crates/a")).unwrap();
        fs::create_dir_all(root.path().join("crates/b")).unwrap();
        fs::create_dir_all(root.path().join("registry")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n[workspace.package]\nversion = \"0.24.4\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("crates/a/Cargo.toml"),
            format!(
                "[package]\nname = \"a\"\nversion.workspace = true\n[dependencies]\nb = {{ path = \"../b\", version = \"={member_version}\" }}\n[dev-dependencies]\nb-dev = {{ package = \"b\", path = \"../b\", version = \"={member_version}\" }}\n[build-dependencies]\nb-build = {{ package = \"b\", path = \"../b\", version = \"={member_version}\" }}\n[target.'cfg(unix)'.dependencies]\nb-target = {{ package = \"b\", path = \"../b\", version = \"={member_version}\" }}\n[target.'cfg(windows)'.dependencies.b-table]\npath = \"../b\"\nversion = \"={member_version}\"\n[dependencies]\nexternal = {{ version = \"=0.24.4\" }}\n"
            ),
        )
        .unwrap();
        fs::write(
            root.path().join("crates/b/Cargo.toml"),
            "[package]\nname = \"b\"\nversion.workspace = true\n",
        )
        .unwrap();
        fs::write(
            root.path().join("registry/agent.json"),
            "{\"version\":\"0.24.4\",\"archive\":\"https://example/v0.24.4/a.tgz\"}",
        )
        .unwrap();
        root
    }

    #[test]
    fn version_plan_updates_every_internal_dependency_section_and_not_external_pins() {
        let root = version_fixture("0.24.4");
        let plan = build_version_mutation_plan(root.path(), "patch").unwrap();
        let member = plan
            .files
            .iter()
            .find(|file| file.relative_path == "crates/a/Cargo.toml")
            .unwrap();
        let updated = String::from_utf8(member.after.clone()).unwrap();
        assert_eq!(updated.matches("version = \"=0.24.5\"").count(), 5);
        assert!(updated.contains("external = { version = \"=0.24.4\" }"));
        assert_eq!(plan.files.len(), 3);
    }

    #[test]
    fn version_plan_rejects_one_mismatched_internal_pin_before_writes() {
        let root = version_fixture("0.24.3");
        let before = fs::read(root.path().join("Cargo.toml")).unwrap();
        let error = build_version_mutation_plan(root.path(), "patch").unwrap_err();
        assert!(error.to_string().contains("preflight mismatch"));
        assert_eq!(fs::read(root.path().join("Cargo.toml")).unwrap(), before);
    }

    #[test]
    fn repository_version_plan_covers_every_owned_manifest_and_lockfile() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let plan = build_version_mutation_plan(workspace, "patch").unwrap();
        let root = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
        let manifests = workspace_member_manifests(workspace, &root).unwrap();
        for path in manifests {
            let source = fs::read_to_string(&path).unwrap();
            if source.contains("path =")
                && source.contains(&format!("version = \"={}\"", plan.before))
            {
                let relative = relative_version_path(workspace, &path).unwrap();
                assert!(
                    plan.files.iter().any(|file| file.relative_path == relative),
                    "missing manifest mutation for {relative}"
                );
            }
        }
        assert_eq!(plan.generated_lockfile, "Cargo.lock");
        assert!(
            plan.files
                .iter()
                .any(|file| file.relative_path == "registry/agent.json")
        );
    }

    #[test]
    fn version_plan_write_failure_rolls_back_every_written_file() {
        let root = version_fixture("0.24.4");
        let plan = build_version_mutation_plan(root.path(), "patch").unwrap();
        let originals = plan
            .files
            .iter()
            .map(|file| (file.relative_path.clone(), file.before.clone()))
            .collect::<Vec<_>>();
        let error = apply_version_mutation_plan(root.path(), &plan, Some(1)).unwrap_err();
        assert!(error.to_string().contains("injected"));
        for (path, bytes) in originals {
            assert_eq!(fs::read(root.path().join(path)).unwrap(), bytes);
        }
    }

    #[test]
    fn registry_bump_updates_version_and_archive_urls() {
        let source = r#"{"version":"0.24.4","archive":"https://example/v0.24.4/a.tgz"}"#;
        let updated = update_registry_manifest(source, "0.24.4", "0.24.5").unwrap();
        assert!(updated.contains("\"version\": \"0.24.5\""));
        assert!(updated.contains("/v0.24.5/a.tgz"));
    }

    #[test]
    fn registry_bump_rejects_stale_archive_version() {
        let source = r#"{"version":"0.24.4","archive":"https://example/v0.24.3/a.tgz"}"#;
        let error = update_registry_manifest(source, "0.24.4", "0.24.5").unwrap_err();
        assert!(error.to_string().contains("archive URL"));
    }

    #[test]
    fn community_input_is_never_requested_by_production_health_builder() {
        struct Healthy;
        impl ExternalHealthPort for Healthy {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                Ok(OfficialStatusSnapshot {
                    degraded: false,
                    summary: "none: operational".into(),
                    evidence_ref: "official".into(),
                })
            }
        }
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        record.state = ReleaseRecoveryState::ClassifyingFailure;
        let evidence = external_health_evidence(&record, &Healthy).unwrap();
        assert!(!evidence.community_reports);
        assert!(evidence.community_evidence.is_empty());
        assert!(
            !evidence.repository_checks_green,
            "missing local-gate evidence must not satisfy outage admission"
        );
    }

    struct FakeRelease;
    impl ReleaseExecutionPort for FakeRelease {
        fn prepare_version_bump(
            &self,
            _bump: &str,
            admission: ReleaseMutationAdmission,
        ) -> Result<VersionBumpReceipt, RrcError> {
            require_kind(admission, ReleaseMutationKind::VersionBump)?;
            Ok(VersionBumpReceipt {
                before: "0.24.4".into(),
                after: "0.24.5".into(),
                files: vec!["Cargo.toml".into(), "registry/agent.json".into()],
            })
        }
        fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError> {
            Ok(format!("{} passed", gate.name))
        }
        fn commit_candidate(
            &self,
            _version: &str,
            admission: ReleaseMutationAdmission,
        ) -> Result<String, RrcError> {
            require_kind(admission, ReleaseMutationKind::CommitCandidate)?;
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }
        fn push_candidate(&self, admission: ReleaseMutationAdmission) -> Result<String, RrcError> {
            require_kind(admission, ReleaseMutationKind::PushCandidate)?;
            Ok("origin/main@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }
        fn create_and_push_tag(
            &self,
            version: &str,
            _commit: &str,
            admission: ReleaseMutationAdmission,
        ) -> Result<(String, String), RrcError> {
            require_kind(admission, ReleaseMutationKind::CreateTag)?;
            Ok((
                format!("v{version}"),
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ))
        }
        fn publication(
            &self,
            _repository: &str,
            _tag: &str,
        ) -> Result<Option<PublicationReceipt>, RrcError> {
            Ok(Some(PublicationReceipt {
                run_id: 77,
                version: "0.24.5".into(),
                assets: vec!["agent-vesper-acp-linux-x86_64.tar.gz".into()],
            }))
        }
    }

    struct GreenGithub;
    impl GitHubEvidencePort for GreenGithub {
        fn matrix_for_sha(
            &self,
            _repository: &str,
            head_sha: &str,
        ) -> Result<Vec<crate::release_recovery::GateRecord>, RrcError> {
            Ok(crate::release_recovery::default_pre_release_gates()
                .into_iter()
                .enumerate()
                .map(|(index, name)| crate::release_recovery::GateRecord {
                    name: name.clone(),
                    head_sha: head_sha.into(),
                    run_id: Some(index as u64 + 1),
                    run_attempt: Some(1),
                    jobs: vec![crate::release_recovery::JobSnapshot {
                        workflow_id: index as u64 + 10,
                        run_id: index as u64 + 1,
                        attempt: 1,
                        job_id: index as u64 + 100,
                        workflow_name: name,
                        job_name: "fixture".into(),
                        platform: Some("linux".into()),
                        state: crate::release_recovery::JobState::Success,
                        failed_step: None,
                        url: "https://example.invalid/job".into(),
                    }],
                    url: None,
                })
                .collect())
        }
        fn job_log(&self, _repository: &str, _job_id: u64) -> Result<String, RrcError> {
            Ok(String::new())
        }
        fn rerun_job(&self, _repository: &str, _job_id: u64) -> Result<(), RrcError> {
            Ok(())
        }
        fn rerun_failed(&self, _repository: &str, _run_id: u64) -> Result<(), RrcError> {
            Ok(())
        }
        fn rerun_workflow(&self, _repository: &str, _run_id: u64) -> Result<(), RrcError> {
            Ok(())
        }
    }

    const LIFECYCLE_TEST_NAME: &str =
        "release_executor::tests::release_worker_cancel_restart_process_acceptance";

    fn spawn_lifecycle_fixture(mode: &str, root: &Path) -> std::process::Child {
        Command::new(std::env::current_exe().expect("current test executable"))
            .arg(LIFECYCLE_TEST_NAME)
            .arg("--exact")
            .arg("--nocapture")
            .env("VESPER_RRC_LIFECYCLE_MODE", mode)
            .env("VESPER_RRC_LIFECYCLE_ROOT", root)
            .spawn()
            .expect("spawn lifecycle fixture")
    }

    fn run_lifecycle_child(mode: &str, root: &Path) {
        match mode {
            "leader" => {
                let mut descendant = spawn_lifecycle_fixture("descendant", root);
                descendant.wait().expect("wait for descendant fixture");
            }
            "descendant" => {
                fs::write(root.join("descendant-started"), b"started")
                    .expect("write descendant start receipt");
                thread::sleep(Duration::from_secs(4));
                fs::write(root.join("descendant-survived"), b"survived")
                    .expect("write descendant survival receipt");
            }
            "restart" => {
                let ledger = ReleaseLedger::open(root.to_path_buf(), "lifecycle-repository")
                    .expect("open persisted release ledger");
                let record = ledger
                    .load()
                    .expect("load persisted release ledger")
                    .expect("persisted release record");
                let job = record.required_gates[0]
                    .jobs
                    .first()
                    .expect("persisted job");
                assert_eq!(record.state, ReleaseRecoveryState::WaitingForMatrix);
                assert_eq!(record.required_gates[0].run_id, Some(7001));
                assert_eq!(job.run_id, 7001);
                assert_eq!(job.job_id, 8001);
                fs::write(
                    root.join("restart-receipt"),
                    format!(
                        "epoch={};run={};job={}",
                        record.epoch_id, job.run_id, job.job_id
                    ),
                )
                .expect("write restart receipt");
            }
            other => panic!("unknown lifecycle fixture mode {other}"),
        }
    }

    #[test]
    fn release_worker_cancel_restart_process_acceptance() {
        if let Ok(mode) = std::env::var("VESPER_RRC_LIFECYCLE_MODE") {
            let root = PathBuf::from(
                std::env::var_os("VESPER_RRC_LIFECYCLE_ROOT").expect("lifecycle fixture root"),
            );
            run_lifecycle_child(&mode, &root);
            return;
        }

        let temp = tempfile::tempdir().expect("temporary lifecycle root");
        let cancelled = Arc::new(AtomicBool::new(false));
        let executor = NativeReleaseExecutor::new(temp.path(), Arc::clone(&cancelled))
            .expect("native release executor");
        let executable = std::env::current_exe()
            .expect("current test executable")
            .to_string_lossy()
            .into_owned();
        let root = temp.path().to_path_buf();
        let command_thread = thread::spawn(move || {
            let root_text = root.to_string_lossy().into_owned();
            let args = [LIFECYCLE_TEST_NAME, "--exact", "--nocapture"];
            // The fixture receives these two variables and creates a real
            // descendant process inside the executor-owned process group/job.
            executor.command_with_env(
                &executable,
                &args,
                &[
                    ("VESPER_RRC_LIFECYCLE_MODE", "leader"),
                    ("VESPER_RRC_LIFECYCLE_ROOT", &root_text),
                ],
            )
        });
        let start_deadline = Instant::now() + Duration::from_secs(15);
        while !temp.path().join("descendant-started").exists() {
            assert!(
                Instant::now() < start_deadline,
                "descendant fixture did not start within the acceptance bound"
            );
            thread::sleep(Duration::from_millis(25));
        }
        cancelled.store(true, Ordering::Release);
        let error = command_thread
            .join()
            .expect("release command thread")
            .expect_err("cancelled release process must fail truthfully");
        assert!(error.to_string().contains("cancelled by the user"));
        thread::sleep(Duration::from_millis(4_250));
        assert!(
            !temp.path().join("descendant-survived").exists(),
            "release cancellation leaked a descendant process"
        );

        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "lifecycle-repository")
            .expect("open lifecycle ledger");
        let mut record = crate::release_recovery::start_release(
            "lifecycle-repository",
            "patch",
            "main",
            "1111111111111111111111111111111111111111",
        )
        .expect("start lifecycle record");
        record.state = ReleaseRecoveryState::WaitingForMatrix;
        record.required_gates = vec![crate::release_recovery::GateRecord {
            name: "five-target-foundation".into(),
            head_sha: "1111111111111111111111111111111111111111".into(),
            run_id: Some(7001),
            run_attempt: Some(1),
            jobs: vec![crate::release_recovery::JobSnapshot {
                workflow_id: 6001,
                run_id: 7001,
                attempt: 1,
                job_id: 8001,
                workflow_name: "five-target-foundation".into(),
                job_name: "native-host".into(),
                platform: Some(std::env::consts::OS.into()),
                state: crate::release_recovery::JobState::InProgress,
                failed_step: None,
                url: "https://example.invalid/jobs/8001".into(),
            }],
            url: Some("https://example.invalid/runs/7001".into()),
        }];
        let epoch = record.epoch_id.clone();
        ledger.save(&record).expect("persist lifecycle record");
        let status = spawn_lifecycle_fixture("restart", temp.path())
            .wait()
            .expect("wait for restarted host fixture");
        assert!(status.success(), "restarted host fixture failed: {status}");
        assert_eq!(
            fs::read_to_string(temp.path().join("restart-receipt")).expect("restart receipt"),
            format!("epoch={epoch};run=7001;job=8001")
        );
    }

    #[test]
    fn production_orchestrator_reaches_publication_only_through_settled_gates() {
        struct Healthy;
        impl ExternalHealthPort for Healthy {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                Ok(OfficialStatusSnapshot {
                    degraded: false,
                    summary: "operational".into(),
                    evidence_ref: "official".into(),
                })
            }
        }
        fn run(provider_fixture: &str) -> ReleaseRecoveryRecord {
            let temp = tempfile::tempdir().unwrap();
            let ledger = ReleaseLedger::open(temp.path().to_path_buf(), provider_fixture).unwrap();
            let mut record = crate::release_recovery::start_release(
                provider_fixture,
                "patch",
                "main",
                "1111111111111111111111111111111111111111",
            )
            .unwrap();
            ledger.save(&record).unwrap();
            for _ in 0..16 {
                advance_release(
                    &mut record,
                    ReleaseAdvanceContext {
                        workspace: temp.path(),
                        repository: "owner/repo",
                        ledger: &ledger,
                        executor: &FakeRelease,
                        github: &GreenGithub,
                        health: &Healthy,
                        repair_factory: None,
                        cancelled: Arc::new(AtomicBool::new(false)),
                    },
                )
                .unwrap();
                if record.state == ReleaseRecoveryState::Published {
                    break;
                }
            }
            record
        }
        let first = run("fixture-provider-a");
        let second = run("fixture-provider-b");
        assert_eq!(first.state, ReleaseRecoveryState::Published);
        assert_eq!(second.state, ReleaseRecoveryState::Published);
        assert_eq!(first.release_version, second.release_version);
        assert_eq!(first.retry_budget, second.retry_budget);
        assert!(first.mutation.candidate_pushed);
        assert!(first.mutation.tag_pushed);
        assert!(first.mutation.publication_verified);
    }

    #[test]
    fn resource_governor_defer_keeps_local_epoch_resumable_without_source_failure() {
        struct ResourceDeferred;
        impl ReleaseExecutionPort for ResourceDeferred {
            fn prepare_version_bump(
                &self,
                bump: &str,
                admission: ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                FakeRelease.prepare_version_bump(bump, admission)
            }
            fn run_local_gate(&self, _gate: &LocalGateRecord) -> Result<String, RrcError> {
                Err(RrcError::ResourceConstrained(
                    "critical memory pressure in constrained acceptance fixture".into(),
                ))
            }
            fn commit_candidate(
                &self,
                version: &str,
                admission: ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.commit_candidate(version, admission)
            }
            fn push_candidate(
                &self,
                admission: ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.push_candidate(admission)
            }
            fn create_and_push_tag(
                &self,
                version: &str,
                commit: &str,
                admission: ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                FakeRelease.create_and_push_tag(version, commit, admission)
            }
            fn publication(
                &self,
                repository: &str,
                tag: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                FakeRelease.publication(repository, tag)
            }
        }
        struct Healthy;
        impl ExternalHealthPort for Healthy {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                Ok(OfficialStatusSnapshot {
                    degraded: false,
                    summary: "operational".into(),
                    evidence_ref: "fixture".into(),
                })
            }
        }

        let temp = tempfile::tempdir().expect("temporary release root");
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "resource-fixture")
            .expect("open ledger");
        let mut record = crate::release_recovery::start_release(
            "resource-fixture",
            "patch",
            "main",
            "1111111111111111111111111111111111111111",
        )
        .expect("start record");
        ledger.save(&record).expect("save initial record");

        // Version preparation is a separate controller step. The following
        // local gate must defer, not classify a source failure.
        advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &ResourceDeferred,
                github: &GreenGithub,
                health: &Healthy,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect("version preparation");
        let error = advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &ResourceDeferred,
                github: &GreenGithub,
                health: &Healthy,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect_err("constrained host must defer the expensive gate");

        assert!(matches!(error, RrcError::ResourceConstrained(_)));
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        let gate = record
            .mutation
            .local_gates
            .first()
            .expect("first local gate");
        assert_eq!(gate.state, SettlementState::NotStarted);
        assert_eq!(
            gate.evidence_ref.as_deref(),
            Some("local:resource-deferred")
        );
        assert!(
            record.failures.is_empty(),
            "RAM pressure is not source evidence"
        );
        assert_eq!(
            ledger
                .load()
                .expect("load deferred record")
                .expect("record")
                .state,
            ReleaseRecoveryState::LocalVerification
        );
    }
}
