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
    EXTERNAL_HEALTH_BUDGET, ExternalHealthEvidence, GhCliEvidenceAdapter, GitHubEvidencePort,
    LocalGateRecord, ReleaseControllerEvent, ReleaseLedger, ReleaseMutationAdmission,
    ReleaseMutationKind, ReleaseProgress, ReleaseRecoveryRecord, ReleaseRecoveryState,
    ReleaseVersionSelector, RelevantStateChange, RelevantStateChangeKind, ResourceDeferredRecord,
    RrcError, SettlementState, admit_release_mutation, apply_controller_event,
    default_release_root, poll_interval, redact_secrets, refresh_remote_evidence,
    stable_semver_components,
};

const MAX_COMMAND_OUTPUT: usize = 4096;

const MAX_TELEMETRY_LINES: usize = 8;

const MAX_TELEMETRY_LINE_CHARS: usize = 240;

const OFFICIAL_STATUS_URL: &str = "https://www.githubstatus.com/api/v2/summary.json";

const RESOURCE_WATCH_NORMAL_CONFIRMATIONS: u8 = 3;

const RESOURCE_WATCH_INITIAL_INTERVAL: Duration = Duration::from_secs(5);

const RESOURCE_WATCH_BACKOFF_INTERVAL: Duration = Duration::from_secs(15);

const RESOURCE_WATCH_MAX_INTERVAL: Duration = Duration::from_secs(30);

const WATCHDOG_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommandWatchdogPolicy {
    pub(crate) operation: &'static str,
    pub(crate) inactivity_timeout: Duration,
    /// Per-operation protocol/safety boundary. Local work has no absolute
    /// deadline: continuing meaningful progress keeps it alive.
    pub(crate) hard_deadline: Option<Duration>,
}

pub(crate) const LOCAL_GATE_WATCHDOG: CommandWatchdogPolicy = CommandWatchdogPolicy {
    operation: "local verification command",
    inactivity_timeout: Duration::from_secs(30 * 60),
    hard_deadline: None,
};

pub(crate) const MUTATION_WATCHDOG: CommandWatchdogPolicy = CommandWatchdogPolicy {
    operation: "release mutation command",
    inactivity_timeout: Duration::from_secs(5 * 60),
    hard_deadline: None,
};

const REMOTE_MUTATION_WATCHDOG: CommandWatchdogPolicy = CommandWatchdogPolicy {
    operation: "remote Git mutation command",
    inactivity_timeout: Duration::from_secs(5 * 60),
    hard_deadline: Some(Duration::from_secs(30 * 60)),
};

pub(crate) const REMOTE_COMMAND_WATCHDOG: CommandWatchdogPolicy = CommandWatchdogPolicy {
    operation: "GitHub evidence request",
    inactivity_timeout: Duration::from_secs(2 * 60),
    hard_deadline: Some(Duration::from_secs(5 * 60)),
};

const PUBLICATION_WATCH_LIMIT_MILLIS: u64 = 2 * 60 * 60 * 1000;

const PUBLICATION_WATCHDOG: CommandWatchdogPolicy = CommandWatchdogPolicy {
    operation: "publication evidence request",
    inactivity_timeout: Duration::from_secs(2 * 60),
    hard_deadline: Some(Duration::from_secs(10 * 60)),
};

const EXTERNAL_HEALTH_WATCHDOG: CommandWatchdogPolicy = CommandWatchdogPolicy {
    operation: "official GitHub health request",
    inactivity_timeout: EXTERNAL_HEALTH_BUDGET,
    hard_deadline: Some(EXTERNAL_HEALTH_BUDGET),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandLivenessState {
    Active,
    QuietButAlive,
    Stagnant,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessTreeHeartbeatObservation {
    process_count: u32,
    rustc_count: u32,
}

fn process_tree_heartbeat_observation(
    telemetry: &ResourceTelemetry,
) -> ProcessTreeHeartbeatObservation {
    // RSS remains diagnostic telemetry only: allocator and resident-set movement does not prove
    // that the supervised operation advanced. Child/process and rustc-count transitions do.
    ProcessTreeHeartbeatObservation {
        process_count: telemetry.process_count,
        rustc_count: telemetry.rustc_count,
    }
}

fn command_liveness_state(
    cancelled: bool,
    elapsed: Duration,
    output_inactive_for: Duration,
    process_inactive_for: Option<Duration>,
    policy: CommandWatchdogPolicy,
) -> CommandLivenessState {
    if cancelled {
        return CommandLivenessState::Cancelled;
    }
    if policy
        .hard_deadline
        .is_some_and(|deadline| elapsed >= deadline)
    {
        return CommandLivenessState::TimedOut;
    }
    if output_inactive_for < policy.inactivity_timeout {
        return CommandLivenessState::Active;
    }
    if process_inactive_for.is_some_and(|quiet| quiet < policy.inactivity_timeout) {
        return CommandLivenessState::QuietButAlive;
    }
    CommandLivenessState::Stagnant
}

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
// It removes host-pressure admission thresholds so the test can hold a fake
// Cargo child deterministically on constrained CI machines. The test-support
// governor uses a synthetic snapshot so shared-runner procfs/cgroup/disk timing
// cannot control process-lifecycle acceptance. Production binaries neither
// compile this setter nor invoke it.
#[cfg(feature = "test-support")]
static PROCESS_TEST_PERMISSIVE_RESOURCE_POLICY: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "test-support")]
pub fn enable_permissive_resource_governor_for_process_tests() {
    PROCESS_TEST_PERMISSIVE_RESOURCE_POLICY.store(true, Ordering::Release);
}

#[cfg(any(test, feature = "test-support"))]
fn permissive_resource_policy() -> ResourcePolicy {
    ResourcePolicy {
        fixed_reserve_bytes: 0,
        reserve_fraction_numerator: 0,
        reserve_fraction_denominator: 1,
        normal_headroom_min_bytes: 0,
        normal_headroom_fraction_numerator: 0,
        normal_headroom_fraction_denominator: 1,
        estimated_rustc_bytes: 1,
        max_cargo_jobs: 1,
        pressure_swap_growth_bytes_per_minute: u64::MAX,
        critical_swap_growth_bytes_per_minute: u64::MAX,
        pressure_psi_some_avg10_bps: u32::MAX,
        pressure_psi_full_avg10_bps: u32::MAX,
        critical_psi_full_avg10_bps: u32::MAX,
        disk_reserve_min_bytes: 0,
        disk_reserve_fraction_numerator: 0,
        disk_reserve_fraction_denominator: 1,
        expected_gate_growth_bytes: 0,
        admission_pressure_enabled: false,
    }
}

fn resource_policy_for_worker() -> ResourcePolicy {
    #[cfg(feature = "test-support")]
    if PROCESS_TEST_PERMISSIVE_RESOURCE_POLICY.load(Ordering::Acquire) {
        return permissive_resource_policy();
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
    resource_deferred: bool,
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
    /// The persisted controller state is resource-deferred. This remains true
    /// while a passive watch is waiting for pressure to clear.
    pub resource_deferred: bool,
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
    heartbeat: Arc<Mutex<Instant>>,
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
        if let Ok(mut observed) = heartbeat.lock() {
            *observed = Instant::now();
        }
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

/// Runs a non-local controller command with bounded output capture,
/// cancellation polling, an inactivity watchdog, and an absolute deadline.
/// The child is spawned as an owned process group so timeout/cancellation does
/// not leave a CLI descendant running after the controller returns.
pub(crate) fn run_bounded_external_command(
    command: &mut Command,
    cancelled: &AtomicBool,
    policy: CommandWatchdogPolicy,
) -> Result<Output, RrcError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(RrcError::Cancelled);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    let mut child = command.group().kill_on_drop(true).spawn()?;
    #[cfg(not(windows))]
    let mut child = command.group_spawn()?;
    let stdout = child
        .inner()
        .stdout
        .take()
        .ok_or_else(|| RrcError::Invalid("external child stdout pipe is missing".into()))?;
    let stderr = child
        .inner()
        .stderr
        .take()
        .ok_or_else(|| RrcError::Invalid("external child stderr pipe is missing".into()))?;
    let started_at = Instant::now();
    let heartbeat = Arc::new(Mutex::new(started_at));
    let stdout_heartbeat = Arc::clone(&heartbeat);
    let stderr_heartbeat = Arc::clone(&heartbeat);
    let stdout_reader =
        thread::spawn(move || capture_release_stream(stdout, None, stdout_heartbeat));
    let stderr_reader =
        thread::spawn(move || capture_release_stream(stderr, None, stderr_heartbeat));
    let stop = loop {
        let last_output = heartbeat
            .lock()
            .map(|observed| *observed)
            .unwrap_or(started_at);
        let reason = match command_liveness_state(
            cancelled.load(Ordering::Acquire),
            started_at.elapsed(),
            last_output.elapsed(),
            None,
            policy,
        ) {
            CommandLivenessState::Active | CommandLivenessState::QuietButAlive => None,
            CommandLivenessState::Stagnant => Some(RrcError::WatchdogStalled {
                operation: policy.operation.into(),
                limit_seconds: policy.inactivity_timeout.as_secs(),
            }),
            CommandLivenessState::TimedOut => Some(RrcError::WatchdogDeadline {
                operation: policy.operation.into(),
                limit_seconds: policy
                    .hard_deadline
                    .expect("TimedOut requires a hard deadline")
                    .as_secs(),
            }),
            CommandLivenessState::Cancelled => Some(RrcError::Cancelled),
        };
        if let Some(reason) = reason {
            let _ = child.kill();
            break Some(reason);
        }
        if child.inner().try_wait()?.is_some() {
            break None;
        }
        thread::sleep(WATCHDOG_POLL_INTERVAL);
    };
    let status = child.inner().wait()?;
    let stdout = join_capture_reader(stdout_reader, "stdout")?;
    let stderr = join_capture_reader(stderr_reader, "stderr")?;
    if let Some(reason) = stop {
        return Err(reason);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
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
    fn latest_resource_telemetry(&self) -> Option<ResourceTelemetry> {
        None
    }

    fn plan_closeout(
        &self,
        _record: &ReleaseRecoveryRecord,
        _repository: &str,
    ) -> Result<Option<crate::release_closeout::RegistryTarget>, RrcError> {
        Ok(None)
    }
    fn finish_closeout(
        &self,
        _record: &ReleaseRecoveryRecord,
        _allow_mutation: bool,
        _admission: &ReleaseMutationAdmission,
    ) -> Result<crate::release_closeout::CloseoutReceipt, RrcError> {
        Err(RrcError::Invalid(
            "native closeout/report port unavailable".into(),
        ))
    }
    fn preview_release_version(&self, _target: &str) -> Result<(String, String), RrcError> {
        Err(RrcError::Invalid(
            "release version preview is unavailable for this executor".into(),
        ))
    }

    fn prepare_version_bump(
        &self,
        bump: &str,
        admission: &ReleaseMutationAdmission,
    ) -> Result<VersionBumpReceipt, RrcError>;

    fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError>;

    fn commit_candidate(
        &self,
        version: &str,
        admission: &ReleaseMutationAdmission,
    ) -> Result<String, RrcError>;

    fn push_candidate(&self, admission: &ReleaseMutationAdmission) -> Result<String, RrcError>;

    fn create_and_push_tag(
        &self,
        version: &str,
        commit: &str,
        admission: &ReleaseMutationAdmission,
    ) -> Result<(String, String), RrcError>;

    fn publication(
        &self,
        repository: &str,
        tag: &str,
        expected_commit: &str,
    ) -> Result<Option<PublicationReceipt>, RrcError>;

    fn last_green_release_commit(&self, _repository: &str) -> Result<Option<String>, RrcError> {
        Ok(None)
    }

    fn source_changed_since(&self, _green: &str, _candidate: &str) -> Result<bool, RrcError> {
        Err(RrcError::Invalid(
            "last-green source comparison unavailable".into(),
        ))
    }
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
    firewall: Option<Arc<vesper_policy::firewall::CommandFirewall>>,
    activity: Option<Arc<Mutex<ReleaseWorkerActivity>>>,
    /// Production RRC workers always carry this governor. The public
    /// constructor leaves it absent only for narrow executor tests that do
    /// not represent a controller-admitted local gate.
    resource_governor: Option<HostResourceGovernor>,
    repair_heartbeat: Option<Arc<Mutex<Instant>>>,
}

impl NativeReleaseExecutor {
    pub fn new(workspace: &Path, cancelled: Arc<AtomicBool>) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
            firewall: None,
            activity: None,
            resource_governor: None,
            repair_heartbeat: None,
        })
    }

    #[cfg(debug_assertions)]
    fn with_activity(
        workspace: &Path,
        cancelled: Arc<AtomicBool>,
        activity: Arc<Mutex<ReleaseWorkerActivity>>,
    ) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
            firewall: None,
            activity: Some(activity),
            resource_governor: None,
            repair_heartbeat: None,
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
            cancelled: Arc::clone(&cancelled),
            firewall: None,
            activity: Some(activity),
            resource_governor: Some(resource_governor.with_cancellation(Arc::clone(&cancelled))),
            repair_heartbeat: None,
        })
    }

    fn command(&self, program: &str, args: &[&str]) -> Result<Output, RrcError> {
        self.command_with_env_and_policy(program, args, &[], MUTATION_WATCHDOG)
    }

    fn command_with_policy(
        &self,
        program: &str,
        args: &[&str],
        policy: CommandWatchdogPolicy,
    ) -> Result<Output, RrcError> {
        self.command_with_env_and_policy(program, args, &[], policy)
    }

    #[cfg(test)]
    fn command_with_env(
        &self,
        program: &str,
        args: &[&str],
        environment: &[(&str, &str)],
    ) -> Result<Output, RrcError> {
        self.command_with_env_and_policy(program, args, environment, MUTATION_WATCHDOG)
    }

    fn command_with_env_and_policy(
        &self,
        program: &str,
        args: &[&str],
        environment: &[(&str, &str)],
        policy: CommandWatchdogPolicy,
    ) -> Result<Output, RrcError> {
        if let Some(firewall) = &self.firewall
            && firewall.scan(&format_command(program, args)).decision
                == vesper_policy::firewall::RuleDecision::Deny
        {
            return Err(RrcError::MutationBlocked(
                "command firewall denied release subprocess".into(),
            ));
        }

        if self.cancelled.load(Ordering::Acquire) {
            return Err(RrcError::Cancelled);
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
        let started_at = Instant::now();
        let heartbeat = self
            .repair_heartbeat
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(started_at)));
        if let Ok(mut observed) = heartbeat.lock() {
            *observed = started_at;
        }
        let stdout_heartbeat = Arc::clone(&heartbeat);
        let stderr_heartbeat = Arc::clone(&heartbeat);
        let stdout_reader = thread::spawn(move || {
            capture_release_stream(stdout, stdout_activity, stdout_heartbeat)
        });
        let stderr_reader = thread::spawn(move || {
            capture_release_stream(stderr, stderr_activity, stderr_heartbeat)
        });
        let mut cancelled = false;
        let mut watchdog_stop = None;
        let mut resource_stop = None;
        let mut process_observation = None;
        let mut process_heartbeat = None;
        let status = loop {
            // Reap before sampling. A fast controlled Cargo exits in milliseconds;
            // walking procfs/cgroup on a busy runner must not delay the next gate.
            if let Some(status) = child.inner().try_wait()? {
                break status;
            }
            if let Some(governor) = self.resource_governor.as_ref() {
                match governor.snapshot(Some(root_pid)) {
                    Ok(telemetry) => {
                        let observation = process_tree_heartbeat_observation(&telemetry);
                        match process_observation.replace(observation) {
                            Some(previous) if previous != observation => {
                                let observed_at = Instant::now();
                                process_heartbeat = Some(observed_at);
                                if let Some(shared) = &self.repair_heartbeat
                                    && let Ok(mut observed) = shared.lock()
                                {
                                    *observed = observed_at;
                                }
                                if let Some(activity) = self.activity.as_ref()
                                    && let Ok(mut activity) = activity.lock()
                                {
                                    activity.last_activity_at = observed_at;
                                }
                            }
                            None => process_heartbeat = Some(started_at),
                            _ => {}
                        }
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
            let last_output = heartbeat
                .lock()
                .map(|observed| *observed)
                .unwrap_or(started_at);
            match command_liveness_state(
                self.cancelled.load(Ordering::Acquire),
                started_at.elapsed(),
                last_output.elapsed(),
                process_heartbeat.map(|observed: Instant| observed.elapsed()),
                policy,
            ) {
                CommandLivenessState::Active | CommandLivenessState::QuietButAlive => {}
                CommandLivenessState::Stagnant => {
                    let _ = child.kill();
                    watchdog_stop = Some(RrcError::WatchdogStalled {
                        operation: policy.operation.into(),
                        limit_seconds: policy.inactivity_timeout.as_secs(),
                    });
                }
                CommandLivenessState::TimedOut => {
                    let _ = child.kill();
                    watchdog_stop = Some(RrcError::WatchdogDeadline {
                        operation: policy.operation.into(),
                        limit_seconds: policy
                            .hard_deadline
                            .expect("TimedOut requires a hard deadline")
                            .as_secs(),
                    });
                }
                CommandLivenessState::Cancelled => {
                    let _ = child.kill();
                    cancelled = true;
                }
            }
            thread::sleep(WATCHDOG_POLL_INTERVAL);
        };
        self.note_process_activity(None, Some("process exited"));
        let stdout_bytes = join_capture_reader(stdout_reader, "stdout")?;
        let stderr_bytes = join_capture_reader(stderr_reader, "stderr")?;
        if cancelled {
            return Err(RrcError::Cancelled);
        }
        if let Some(error) = watchdog_stop {
            return Err(error);
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
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(RrcError::Cancelled);
                }
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

    fn reevaluate_expensive_resources(&self) -> Result<(bool, ResourceTelemetry), RrcError> {
        let governor = self.resource_governor.as_ref().ok_or_else(|| {
            RrcError::Invalid("resource watch requires the Host Resource Governor".into())
        })?;
        match governor.preflight(GateCost::Expensive) {
            Ok(admission) => {
                let telemetry = admission.telemetry.clone();
                self.note_resource_telemetry(telemetry.clone());
                // The passive watch never retains the expensive lease. The
                // real gate must win a fresh admission after recovery.
                drop(admission);
                Ok((true, telemetry))
            }
            Err(error) => {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(RrcError::Cancelled);
                }
                let telemetry = error.telemetry().cloned().ok_or_else(|| {
                    RrcError::ResourceConstrained(format!(
                        "resource watch could not obtain telemetry: {error}"
                    ))
                })?;
                self.note_resource_telemetry(telemetry.clone());
                Ok((false, telemetry))
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
        let output =
            self.command_with_env_and_policy(program, args, &environment, LOCAL_GATE_WATCHDOG)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "{} failed with status {}: {}",
                format_command(program, args),
                output.status,
                command_failure_evidence(&output.stdout, &output.stderr)
            )));
        }
        Ok(bounded_output(&output.stdout))
    }

    fn checked(&self, program: &str, args: &[&str]) -> Result<String, RrcError> {
        self.checked_with_policy(program, args, MUTATION_WATCHDOG)
    }

    fn checked_with_policy(
        &self,
        program: &str,
        args: &[&str],
        policy: CommandWatchdogPolicy,
    ) -> Result<String, RrcError> {
        let output = self.command_with_policy(program, args, policy)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "{} failed with status {}: {}",
                format_command(program, args),
                output.status,
                command_failure_evidence(&output.stdout, &output.stderr)
            )));
        }
        Ok(bounded_output(&output.stdout))
    }

    fn git_bytes(&self, args: &[&str]) -> Result<Vec<u8>, RrcError> {
        let output = self.command("git", args)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "Git evidence command failed: {}",
                command_failure_evidence(&output.stdout, &output.stderr)
            )));
        }
        // The subprocess capture has this hard bound. At the bound, completeness
        // is uncertain: a truncated inventory or patch must never grant admission.
        if output.stdout.len() >= 4 * 1024 * 1024 {
            return Err(RrcError::MutationBlocked(
                "Git evidence exceeds the complete capture bound".into(),
            ));
        }
        Ok(output.stdout)
    }

    fn git_status(&self) -> Result<GitWorkspaceStatus, RrcError> {
        let bytes = self.git_bytes(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let mut fields = literal_git_paths(&bytes)?.into_iter();
        let mut paths = Vec::new();
        while let Some(entry) = fields.next() {
            let raw = entry.as_bytes();
            if raw.len() < 4 || raw[2] != b' ' {
                return Err(RrcError::MutationBlocked(
                    "malformed Git status entry".into(),
                ));
            }
            paths.push(entry[3..].to_owned());
            // In -z porcelain, rename/copy destinations precede their source.
            if raw[..2].iter().any(|byte| matches!(byte, b'R' | b'C')) {
                paths.push(fields.next().ok_or_else(|| {
                    RrcError::MutationBlocked("missing Git rename source".into())
                })?);
            }
        }
        Ok(GitWorkspaceStatus { bytes, paths })
    }

    fn stage_admitted_paths(&self, allowed: &[String]) -> Result<(), RrcError> {
        let indexed = literal_git_paths(&self.git_bytes(&["ls-files", "-z"])?)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut args = vec!["add", "--all", "--"];
        args.extend(
            allowed
                .iter()
                .filter(|path| {
                    fs::symlink_metadata(self.workspace.join(path)).is_ok()
                        || indexed.contains(*path)
                })
                .map(String::as_str),
        );
        // Already-staged deletions/rename sources are absent from both the
        // worktree and index. They are admitted above but need no second add.
        if args.len() > 3 {
            self.checked("git", &args)?;
        }
        Ok(())
    }

    fn require_repair_mutations_indexed(
        &self,
        history: &[vesper_domain::ConversationMessage],
    ) -> Result<(), RrcError> {
        let indexed = literal_git_paths(&self.git_bytes(&["ls-files", "-z"])?)?
            .into_iter()
            .map(|path| self.workspace.join(path))
            .collect::<BTreeSet<_>>();
        let mut calls = HashMap::new();
        for message in history {
            for part in &message.content {
                match part {
                    ContentPart::ToolCall(call)
                        if matches!(
                            call.tool_id.as_str(),
                            "write_file" | "edit_file" | "apply_patch" | "apply_patch_set"
                        ) =>
                    {
                        calls.insert(call.id.as_str(), &call.arguments);
                    }
                    ContentPart::ToolResult(result)
                        if result.status == ToolResultStatus::Succeeded =>
                    {
                        if let Some(arguments) = calls.get(result.call_id.as_str()) {
                            let paths = arguments
                                .get("path")
                                .and_then(serde_json::Value::as_str)
                                .into_iter()
                                .chain(
                                    arguments
                                        .get("patches")
                                        .and_then(serde_json::Value::as_array)
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|patch| {
                                            patch.get("path").and_then(serde_json::Value::as_str)
                                        }),
                                );
                            for path in paths {
                                let written = self.workspace.join(path);
                                // Removed files need not remain in the index. Every
                                // surviving file written by the role must be in it.
                                if fs::symlink_metadata(&written).is_ok()
                                    && !indexed.contains(&fs::canonicalize(&written)?)
                                {
                                    return Err(RrcError::Invalid(format!(
                                        "repair wrote a file not represented in the promoted Git tree: {path}; include it in the tracked patch before rerunning proof"
                                    )));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn repair_snapshot(&self) -> Result<RepairVerificationSnapshot, RrcError> {
        Ok(RepairVerificationSnapshot {
            status: self.git_status()?,
            patch: self.git_bytes(&["diff", "--binary", "HEAD"])?,
            index: self.git_bytes(&["write-tree"])?,
        })
    }

    fn require_repair_snapshot(
        &self,
        expected: &RepairVerificationSnapshot,
    ) -> Result<(), RrcError> {
        if &self.repair_snapshot()? != expected {
            return Err(RrcError::Invalid("verification changed the repair source or index; focused proof must be rerun for the final patch".into()));
        }
        Ok(())
    }

    fn gh_json(&self, args: &[&str]) -> Result<serde_json::Value, RrcError> {
        let output = self.command_with_policy("gh", args, PUBLICATION_WATCHDOG)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "GitHub command failed: {}",
                bounded_output(&output.stderr)
            )));
        }
        serde_json::from_slice(&output.stdout).map_err(RrcError::Json)
    }

    /// Attaches the controller-owned governor required by native Cargo gates.
    pub fn with_resource_governor(mut self, governor: HostResourceGovernor) -> Self {
        self.resource_governor = Some(governor.with_cancellation(Arc::clone(&self.cancelled)));
        self
    }

    fn with_firewall(
        mut self,
        firewall: Option<Arc<vesper_policy::firewall::CommandFirewall>>,
    ) -> Self {
        self.firewall = firewall;
        self
    }
}

impl ReleaseExecutionPort for NativeReleaseExecutor {
    fn latest_resource_telemetry(&self) -> Option<ResourceTelemetry> {
        self.activity
            .as_ref()
            .and_then(|activity| activity.lock().ok())
            .and_then(|activity| activity.resource_telemetry.clone())
    }

    fn plan_closeout(
        &self,
        record: &ReleaseRecoveryRecord,
        repository: &str,
    ) -> Result<Option<crate::release_closeout::RegistryTarget>, RrcError> {
        if !self.workspace.join("registry/agent.json").exists() {
            return Ok(None);
        }
        let commit = record
            .release_commit
            .as_deref()
            .ok_or_else(|| RrcError::Invalid("closeout release identity missing".into()))?;
        let output = self.command("git", &["show", &format!("{commit}:registry/agent.json")])?;
        if !output.status.success() {
            return Err(RrcError::Invalid(
                "committed registry manifest unavailable".into(),
            ));
        }
        let manifest = serde_json::from_slice(&output.stdout)?;
        let version = record
            .release_version
            .as_deref()
            .ok_or_else(|| RrcError::Invalid("published version missing".into()))?;
        let pr = std::env::var("AGENT_VESPER_REGISTRY_PR")
            .ok()
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|_| RrcError::Invalid("invalid existing registry PR number".into()))?
            .unwrap_or(539);
        crate::release_closeout::prepare_registry(manifest, version, repository, pr, |endpoint| {
            self.gh_json(&["api", endpoint])
        })
        .map(Some)
    }
    fn finish_closeout(
        &self,
        record: &ReleaseRecoveryRecord,
        allow_mutation: bool,
        admission: &ReleaseMutationAdmission,
    ) -> Result<crate::release_closeout::CloseoutReceipt, RrcError> {
        require_kind(admission, ReleaseMutationKind::Closeout)?;
        if let Some(target) = record.mutation.closeout_registry.as_ref() {
            let remote = self.checked("git", &["config", "--get", "remote.origin.url"])?;
            let repository = crate::release_recovery::github_repository_slug(&remote)?;
            let current = self.plan_closeout(record, &repository)?.ok_or_else(|| {
                RrcError::Invalid("closeout registry manifest disappeared".into())
            })?;
            if current.repository != target.repository
                || current.branch != target.branch
                || current.path != target.path
                || current.manifest != target.manifest
                || current.pull_request != target.pull_request
            {
                return Err(RrcError::MutationBlocked(
                    "registry target changed after admission; no write performed".into(),
                ));
            }
        }
        let blob = record
            .mutation
            .closeout_registry
            .as_ref()
            .map(|target| {
                crate::release_closeout::update_registry(target, allow_mutation, |args| {
                    self.gh_json(args)
                })
            })
            .transpose()?;
        let root = record
            .mutation
            .source_workspace
            .as_deref()
            .map(Path::new)
            .unwrap_or(&self.workspace)
            .canonicalize()?;
        if crate::release_recovery::repository_identity_for_workspace(&root)?
            != record.repo_identity
        {
            return Err(RrcError::Invalid(
                "closeout report workspace identity mismatch".into(),
            ));
        }
        crate::release_closeout::write_report(&root, record, blob)
    }
    fn preview_release_version(&self, target: &str) -> Result<(String, String), RrcError> {
        let plan = build_version_mutation_plan(&self.workspace, target)?;
        Ok((plan.before, plan.after))
    }

    fn prepare_version_bump(
        &self,
        target: &str,
        admission: &ReleaseMutationAdmission,
    ) -> Result<VersionBumpReceipt, RrcError> {
        require_kind(admission, ReleaseMutationKind::VersionBump)?;
        if let Some(activity) = &self.activity
            && let Ok(mut activity) = activity.lock()
        {
            activity.current_gate = Some("version-preparation".into());
            activity.current_command = Some("cargo check --workspace --all-targets".into());
            activity.current_child = Some("cargo".into());
            activity.detail = "Preparing release target and validating the version graph".into();
            activity.gate_started_at = Some(Instant::now());
            activity.last_activity_at = Instant::now();
        }
        let original_status = self.git_status()?;
        let original_patch = self.command("git", &["diff", "--binary", "HEAD"])?;
        if !original_patch.status.success() {
            return Err(RrcError::MutationBlocked(
                "preparation baseline diff is unavailable".into(),
            ));
        }
        if !original_status.paths.is_empty() {
            let owned = original_status.paths.iter().all(|path| {
                admission
                    .candidate_paths()
                    .iter()
                    .any(|allowed| allowed == path)
            });
            if !owned
                || admission.preparation_baseline_digest()
                    != Some(digest(&original_patch.stdout).as_str())
            {
                return Err(RrcError::MutationBlocked(
                    "release preparation requires a clean tree or the exact admitted local repair patch".into(),
                ));
            }
        }
        let plan = build_version_mutation_plan(&self.workspace, target)?;
        let version_changes = plan.before != plan.after;
        let lock_path = self.workspace.join(&plan.generated_lockfile);
        let lock_before = fs::read(&lock_path)?;
        if version_changes {
            apply_version_mutation_plan(&self.workspace, &plan, None)?;
        }
        let resources = match self.admit_local_resources(GateCost::Expensive) {
            Ok(resources) => resources,
            Err(error) => {
                if version_changes {
                    restore_version_mutation(&self.workspace, &plan, &lock_path, &lock_before)?;
                }
                return Err(error);
            }
        };
        let check_args = if version_changes {
            ["check", "--workspace", "--all-targets"].as_slice()
        } else {
            ["check", "--locked", "--workspace", "--all-targets"].as_slice()
        };
        if let Err(error) = self
            .checked_with_admitted_resources("cargo", check_args, &resources)
            .and_then(|_| {
                validate_version_mutation(
                    &self.workspace,
                    &plan,
                    &resources.cargo,
                    self.cancelled.as_ref(),
                )
            })
        {
            if version_changes {
                restore_version_mutation(&self.workspace, &plan, &lock_path, &lock_before)?;
            }
            if matches!(error, RrcError::Invalid(_)) {
                let restored = self.command("git", &["diff", "--binary", "HEAD"])?;
                if !restored.status.success()
                    || restored.stdout != original_patch.stdout
                    || self.git_status()? != original_status
                {
                    return Err(RrcError::MutationBlocked(
                        "failed version preparation did not restore its exact approved baseline"
                            .into(),
                    ));
                }
                return Err(RrcError::VersionPreparationFailed(error.to_string()));
            }
            return Err(error);
        }
        if !version_changes && fs::read(&lock_path)? != lock_before {
            atomic_write(&lock_path, &lock_before)?;
            return Err(RrcError::Invalid(
                "equal release target unexpectedly changed Cargo.lock".into(),
            ));
        }
        let mut files = plan
            .files
            .into_iter()
            .map(|file| file.relative_path)
            .collect::<Vec<_>>();
        if version_changes {
            files.push(plan.generated_lockfile);
        }
        Ok(VersionBumpReceipt {
            before: plan.before,
            after: plan.after,
            files,
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
        admission: &ReleaseMutationAdmission,
    ) -> Result<String, RrcError> {
        require_kind(admission, ReleaseMutationKind::CommitCandidate)?;
        let allowed = admission.candidate_paths();
        let status = self.git_status()?;
        for path in &status.paths {
            if !allowed.iter().any(|allowed| allowed == path) {
                return Err(RrcError::Invalid(format!(
                    "candidate commit contains unadmitted path {path}"
                )));
            }
        }
        if status.paths.is_empty() {
            let plan = build_version_mutation_plan(&self.workspace, version)?;
            if plan.before != version || plan.after != version || !plan.files.is_empty() {
                return Err(RrcError::MutationBlocked(
                    "clean candidate does not match the admitted release version".into(),
                ));
            }
            return exact_sha(self.checked("git", &["rev-parse", "HEAD"])?.trim());
        }
        self.stage_admitted_paths(allowed)?;
        self.checked("git", &["commit", "-m", &format!("Release v{version}")])?;
        let commit = self.checked("git", &["rev-parse", "HEAD"])?;
        exact_sha(commit.trim())
    }

    fn push_candidate(&self, admission: &ReleaseMutationAdmission) -> Result<String, RrcError> {
        require_kind(admission, ReleaseMutationKind::PushCandidate)?;
        let expected = admission
            .candidate_commit()
            .ok_or_else(|| RrcError::MutationBlocked("push candidate SHA missing".into()))?
            .to_owned();
        let commit = self.checked("git", &["rev-parse", "HEAD"])?;
        if commit.trim() != expected {
            return Err(RrcError::MutationBlocked(
                "HEAD changed after candidate admission".into(),
            ));
        }
        self.checked(
            "git",
            &["push", "origin", &format!("{expected}:refs/heads/main")],
        )?;
        Ok(format!("origin/main@{}", exact_sha(commit.trim())?))
    }

    fn create_and_push_tag(
        &self,
        version: &str,
        commit: &str,
        admission: &ReleaseMutationAdmission,
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
        self.checked_with_policy("git", &["push", "origin", &tag], REMOTE_MUTATION_WATCHDOG)?;
        Ok((tag, object))
    }

    fn publication(
        &self,
        repository: &str,
        tag: &str,
        expected_commit: &str,
    ) -> Result<Option<PublicationReceipt>, RrcError> {
        let runs = self.gh_json(&[
            "run",
            "list",
            "--repo",
            repository,
            "--workflow",
            "release.yml",
            "--branch",
            tag,
            "--commit",
            expected_commit,
            "--limit",
            "20",
            "--json",
            "databaseId,status,conclusion,headBranch,headSha",
        ])?;
        let rows = runs
            .as_array()
            .ok_or_else(|| RrcError::Invalid("release run response is not an array".into()))?;
        let Some(run) = rows
            .iter()
            .find(|row| row.get("headBranch").and_then(serde_json::Value::as_str) == Some(tag))
        else {
            return Ok(None);
        };
        if run.get("headSha").and_then(serde_json::Value::as_str) != Some(expected_commit) {
            return Err(RrcError::MutationBlocked(
                "publication workflow SHA does not match verified release candidate".into(),
            ));
        }
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
        let release = self.gh_json(&["api", &format!("repos/{repository}/releases/tags/{tag}")])?;
        if release.get("tag_name").and_then(serde_json::Value::as_str) != Some(tag)
            || release.get("draft").and_then(serde_json::Value::as_bool) != Some(false)
            || release
                .get("prerelease")
                .and_then(serde_json::Value::as_bool)
                != Some(false)
        {
            return Err(RrcError::Invalid(
                "published release identity is not settled".into(),
            ));
        }
        let assets = verified_publication_assets(&release)?;
        verify_publication_checksums(&release, |asset_id| {
            let mut command = Command::new("gh");
            command
                .current_dir(&self.workspace)
                .env_remove("GH_DEBUG")
                .args([
                    "api",
                    &format!("repos/{repository}/releases/assets/{asset_id}"),
                    "-H",
                    "Accept: application/octet-stream",
                ]);
            let output = run_bounded_external_command(
                &mut command,
                &self.cancelled,
                CommandWatchdogPolicy {
                    operation: "publication evidence request",
                    inactivity_timeout: Duration::from_secs(30),
                    hard_deadline: Some(Duration::from_secs(30)),
                },
            )?;
            if !output.status.success() || output.stdout.len() > 1024 {
                return Err(RrcError::Invalid(
                    "checksum asset read failed or exceeded bound".into(),
                ));
            }
            Ok(output.stdout)
        })?;
        let reference =
            self.gh_json(&["api", &format!("repos/{repository}/git/ref/tags/{tag}")])?;
        if reference
            .pointer("/object/type")
            .and_then(serde_json::Value::as_str)
            != Some("tag")
        {
            return Err(RrcError::MutationBlocked(
                "release reference must be an annotated tag".into(),
            ));
        }
        let tag_object = reference
            .pointer("/object/sha")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| RrcError::Invalid("remote tag object missing".into()))?;
        let object =
            self.gh_json(&["api", &format!("repos/{repository}/git/tags/{tag_object}")])?;
        if object
            .pointer("/object/type")
            .and_then(serde_json::Value::as_str)
            != Some("commit")
            || object
                .pointer("/object/sha")
                .and_then(serde_json::Value::as_str)
                != Some(expected_commit)
        {
            return Err(RrcError::MutationBlocked(
                "published tag does not point to verified candidate".into(),
            ));
        }
        Ok(Some(PublicationReceipt {
            run_id,
            version: tag.trim_start_matches('v').to_owned(),
            assets,
        }))
    }

    fn last_green_release_commit(&self, repository: &str) -> Result<Option<String>, RrcError> {
        let value = self.gh_json(&["api", &format!("repos/{repository}/releases/latest")])?;
        let tag = value
            .get("tag_name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| RrcError::Invalid("last release tag missing".into()))?;
        if !tag.starts_with('v')
            || !tag[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Err(RrcError::Invalid(
                "last release tag is not stable semver".into(),
            ));
        }
        let sha = self.checked(
            "git",
            &[
                "rev-parse",
                "--verify",
                &format!("refs/tags/{tag}^{{commit}}"),
            ],
        )?;
        Ok(Some(exact_sha(sha.trim())?))
    }

    fn source_changed_since(&self, green: &str, candidate: &str) -> Result<bool, RrcError> {
        let green = exact_sha(green)?;
        let candidate = exact_sha(candidate)?;
        let output = self.command("git", &["diff", "--name-only", &green, &candidate, "--"])?;
        if !output.status.success() {
            return Err(RrcError::Invalid("source comparison failed".into()));
        }
        // Inventory must not use the UI excerpt bound: a relevant path may be
        // after thousands of documentation paths.
        let paths = String::from_utf8_lossy(&output.stdout);
        for path in paths.lines() {
            if [
                "crates/",
                "apps/",
                ".github/",
                "scripts/",
                "xtask/",
                "fixtures/",
                ".cargo/",
            ]
            .iter()
            .any(|prefix| path.starts_with(prefix))
                && !path.ends_with("/Cargo.toml")
            {
                return Ok(true);
            }
            if matches!(
                path,
                "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "build.rs" | ".gitattributes"
            ) || path.ends_with("/Cargo.toml")
            {
                if !matches!(path, "Cargo.toml" | "Cargo.lock") && !path.ends_with("/Cargo.toml") {
                    return Ok(true);
                }
                let read = |sha: &str, name: &str| -> Result<String, RrcError> {
                    let output = self.command("git", &["show", &format!("{sha}:{name}")])?;
                    if !output.status.success() {
                        return Err(RrcError::Invalid(
                            "baseline build metadata unavailable".into(),
                        ));
                    }
                    String::from_utf8(output.stdout)
                        .map_err(|_| RrcError::Invalid("build metadata is not UTF-8".into()))
                };
                let old_manifest = read(&green, "Cargo.toml")?;
                let new_manifest = read(&candidate, "Cargo.toml")?;
                let before = workspace_version(&old_manifest)?;
                let after = workspace_version(&new_manifest)?;
                let old = read(&green, path)?;
                let new = read(&candidate, path)?;
                let expected = if path == "Cargo.toml" {
                    update_workspace_manifest(
                        &old,
                        &before,
                        &after,
                        true,
                        &self.workspace.join("Cargo.toml"),
                        &workspace_member_manifests(&self.workspace, &new_manifest)?
                            .into_iter()
                            .filter_map(|p| p.parent().map(Path::to_path_buf))
                            .collect(),
                    )?
                } else if path == "Cargo.lock" {
                    version_only_lockfile(&old, &before, &after)
                } else {
                    update_internal_dependency_versions(&old, &before, &after)
                };
                if expected != new {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CurlGitHubStatusAdapter;

impl CurlGitHubStatusAdapter {
    pub fn with_cancellation(&self, cancelled: Arc<AtomicBool>) -> CancellableGitHubStatusAdapter {
        CancellableGitHubStatusAdapter { cancelled }
    }
}

impl ExternalHealthPort for CurlGitHubStatusAdapter {
    fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
        self.with_cancellation(Arc::new(AtomicBool::new(false)))
            .official_status()
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
    let current = record
        .failures
        .iter()
        .filter(|failure| Some(failure.source_commit.as_str()) == record.active_commit())
        .collect::<Vec<_>>();
    let infrastructure = current
        .iter()
        .filter(|failure| failure.class.infrastructure_like())
        .count();
    let local_gates_green =
        !record.mutation.local_gates.is_empty()
            && record.mutation.local_gates.iter().all(|gate| {
                gate.state == SettlementState::Succeeded && gate.evidence_ref.is_some()
            });
    let repository_checks_green = local_gates_green
        && current.iter().all(|failure| {
            failure.class.infrastructure_like() && failure.exists_on_last_green == Some(false)
        });
    let source_explanation_absent = !current.is_empty()
        && current
            .iter()
            .all(|failure| failure.related_source_touched == Some(false));
    let official = port.official_status()?;
    Ok(ExternalHealthEvidence {
        repository_checks_green,
        source_explanation_absent,
        infrastructure_failures: infrastructure,
        official_degraded: official.degraded,
        direct_api_failure: current.iter().any(|failure| {
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

fn repairable_failures(
    record: &ReleaseRecoveryRecord,
) -> Vec<crate::release_recovery::FailureRecord> {
    let active_commit = record.active_commit();
    let mut failures = record
        .failures
        .iter()
        .filter(|failure| {
            Some(failure.source_commit.as_str()) == active_commit
                && !failure.class.infrastructure_like()
                && matches!(
                    failure.confidence,
                    crate::release_recovery::EvidenceConfidence::Proven
                        | crate::release_recovery::EvidenceConfidence::StronglySupported
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    failures.sort_by(|left, right| {
        left.fingerprint
            .0
            .cmp(&right.fingerprint.0)
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
    failures.dedup_by(|left, right| left.fingerprint == right.fingerprint);
    failures
}

fn repair_worktree_leaf(epoch_id: &str, repair_count: usize, timestamp_micros: i64) -> String {
    format!("{epoch_id}-repair-{repair_count}-{timestamp_micros}")
}

const REPAIR_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

const REPAIR_CANCEL_GRACE: Duration = Duration::from_secs(10);

fn controller_stop_is_not_source_failure(error: &RrcError) -> bool {
    matches!(
        error,
        RrcError::WatchdogStalled { .. }
            | RrcError::WatchdogDeadline { .. }
            | RrcError::Cancelled
            | RrcError::AuthorizationBlocked(_)
    )
}

fn repair_watchdog_error(
    cancelled: bool,
    inactive_for: Duration,
    heartbeat_timeout: Duration,
) -> Option<RrcError> {
    if cancelled {
        Some(RrcError::Cancelled)
    } else if inactive_for >= heartbeat_timeout {
        Some(RrcError::WatchdogStalled {
            operation: "focused repair agent".into(),
            limit_seconds: heartbeat_timeout.as_secs(),
        })
    } else {
        None
    }
}

/// Provider activity is liveness, not proof of recovery progress. Only a new
/// successful tool observation resets the repair's semantic stagnation window.
struct RepairEvidenceProgress {
    last_evidence: Instant,
    seen: BTreeSet<String>,
    stagnant_actions: u8,
    verification_active: usize,
}

impl RepairEvidenceProgress {
    fn new() -> Self {
        Self {
            last_evidence: Instant::now(),
            seen: BTreeSet::new(),
            stagnant_actions: 0,
            verification_active: 0,
        }
    }

    fn observe(
        &mut self,
        call: &vesper_domain::ToolCall,
        result: &Result<vesper_agent::ToolResult, vesper_agent::ToolError>,
    ) {
        // Never retain tool contents or credentials; identity excludes call IDs.
        let fresh = result.as_ref().ok().is_some_and(|result| {
            let text = if call.tool_id.as_str() == "run_command" {
                static TIMING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
                    regex::Regex::new(r"\b\d+(?:\.\d+)?(?:ms|s| seconds)\b")
                        .expect("static duration pattern")
                });
                TIMING
                    .replace_all(result.text.as_str(), "<elapsed>")
                    .into_owned()
            } else {
                result.text.as_str().to_owned()
            };
            let observation = digest(
                format!(
                    "{}\n{}\n{}\n{:?}",
                    call.tool_id.as_str(),
                    call.arguments,
                    text,
                    result.change,
                )
                .as_bytes(),
            );
            self.seen.len() < 4096 && self.seen.insert(observation)
        });
        if fresh {
            self.last_evidence = Instant::now();
            self.stagnant_actions = 0;
        } else {
            self.stagnant_actions = self.stagnant_actions.saturating_add(1);
        }
    }

    fn watchdog_error(&self) -> Option<RrcError> {
        self.watchdog_error_with_inactivity(self.last_evidence.elapsed())
    }

    fn watchdog_error_with_inactivity(&self, no_evidence_for: Duration) -> Option<RrcError> {
        if self.stagnant_actions >= crate::release_recovery::STAGNATION_ACTION_LIMIT {
            return Some(RrcError::WatchdogStalled {
                operation: "focused repair repeated six actions without new evidence".into(),
                limit_seconds: crate::release_recovery::STAGNATION_TIME_LIMIT.as_secs(),
            });
        }
        // Governed focused verification has its own native subprocess inactivity
        // watchdog. A progressing compiler is not a model reasoning loop.
        if self.verification_active == 0
            && no_evidence_for >= crate::release_recovery::STAGNATION_TIME_LIMIT
        {
            return Some(RrcError::WatchdogStalled {
                operation: "focused repair produced no new evidence".into(),
                limit_seconds: crate::release_recovery::STAGNATION_TIME_LIMIT.as_secs(),
            });
        }
        None
    }
}

struct RepairVerificationActivity(Option<Arc<Mutex<RepairEvidenceProgress>>>);

impl Drop for RepairVerificationActivity {
    fn drop(&mut self) {
        if let Some(progress) = &self.0
            && let Ok(mut observed) = progress.lock()
        {
            observed.verification_active = observed.verification_active.saturating_sub(1);
        }
    }
}

struct RepairHeartbeatProgress {
    heartbeat: Arc<Mutex<Instant>>,
    downstream: Option<Arc<dyn vesper_agent::AgentProgressPort>>,
}

impl vesper_agent::AgentProgressPort for RepairHeartbeatProgress {
    fn emit(&self, event: vesper_agent::AgentProgressEvent) {
        if let Ok(mut heartbeat) = self.heartbeat.lock() {
            *heartbeat = Instant::now();
        }
        if let Some(downstream) = self.downstream.as_ref() {
            downstream.emit(event);
        }
    }
}

fn run_bounded_repair_agent(
    workspace: &Path,
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    factory: &crate::WorkerFactory,
    cancelled: Arc<AtomicBool>,
) -> Result<(), RrcError> {
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    let resources = root.join("host-resources");
    let governor = HostResourceGovernor::new(
        resource_policy_for_worker(),
        resources.join("scheduler"),
        resources
            .join("targets")
            .join(digest(record.repo_identity.as_bytes())),
    )
    .map_err(|error| RrcError::Invalid(format!("repair resource governor: {error}")))?;
    run_bounded_repair_agent_with_verification(
        workspace,
        record,
        ledger,
        factory,
        cancelled,
        RepairVerification {
            root: &root,
            governor: Some(governor),
            verify: &|executor| {
                for gate in production_local_gates() {
                    executor.run_local_gate(&gate)?;
                }
                Ok(())
            },
        },
    )
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
                            && credible_focused_command(command)
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

fn defer_local_resources(
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    executor: &dyn ReleaseExecutionPort,
    index: usize,
    detail: &str,
) -> Result<(), RrcError> {
    let gate = record.mutation.local_gates[index].clone();
    // A safety defer is a typed active state, never source
    // evidence. Keep completed gates settled, preserve the
    // pending gate, and spend no repair/retry budget.
    record.mutation.local_gates[index].state = SettlementState::NotStarted;
    record.mutation.local_gates[index].evidence_ref = Some("local:resource-deferred".into());
    let mut telemetry = executor.latest_resource_telemetry().unwrap_or_default();
    if telemetry.action == "resource telemetry not sampled" {
        telemetry.pressure = ResourcePressure::Pressure;
        telemetry.pressure_reason = "resource governor deferred admission".into();
        telemetry.action = detail.into();
    }
    let now = Utc::now();
    record.resource_deferred = Some(ResourceDeferredRecord {
        gate_name: gate.name.clone(),
        deferred_at: now,
        last_observed_at: now,
        telemetry,
        consecutive_normal_observations: 0,
        unchanged_observations: 0,
        next_check_seconds: RESOURCE_WATCH_INITIAL_INTERVAL.as_secs(),
    });
    let commit = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("resource defer has no active commit".into()))?;
    let completed = record
        .mutation
        .local_gates
        .iter()
        .take(index)
        .rev()
        .find(|row| row.state == SettlementState::Succeeded)
        .map(|row| format!("{} passed; ", row.name))
        .unwrap_or_default();
    record.transition(
        ReleaseRecoveryState::ResourceDeferred,
        &commit,
        format!(
            "Release paused safely — host resource pressure. {completed}{} is waiting.",
            gate.name
        ),
        vec!["local:resource-deferred".into()],
        None,
    )?;
    ledger.save(record)?;
    Ok(())
}

fn record_local_gate_failure(
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    index: usize,
    error: &RrcError,
) -> Result<(), RrcError> {
    let gate = record.mutation.local_gates[index].clone();
    let excerpt = crate::release_recovery::first_causal_excerpt(&error.to_string());
    let (class, confidence) = crate::release_recovery::classify_failure(&excerpt, None);
    let source_commit = record.active_commit().unwrap_or("unknown").to_owned();
    record
        .failures
        .push(crate::release_recovery::FailureRecord {
            workflow_id: 0,
            run_id: 0,
            attempt: record.repair_attempts.len() as u32 + 1,
            job_id: index as u64,
            workflow_name: "local-verification".into(),
            job_name: gate.name.clone(),
            platform: Some(std::env::consts::OS.into()),
            step_name: Some(gate.command.clone()),
            fingerprint: crate::release_recovery::failure_fingerprint(
                "local-verification",
                &gate.name,
                Some(&gate.command),
                Some(std::env::consts::OS),
                None,
                &excerpt,
            ),
            class,
            confidence,
            causal_excerpt: excerpt,
            source_commit,
            observed_at: Utc::now(),
            other_platforms_passed: false,
            exists_on_last_green: None,
            related_source_touched: None,
        });
    record.mutation.local_gates[index].state = SettlementState::Failed;
    record.mutation.local_gates[index].evidence_ref = Some("local:failed".into());
    record.note_progress_milestone(format!(
        "Local gate failed: {}; preserving failure evidence",
        gate.name
    ));
    apply_controller_event(
        record,
        ReleaseControllerEvent::LocalVerificationFailed(vec![format!("local:{}", gate.name)]),
    )?;
    ledger.save(record)?;
    Ok(())
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
    if cancelled.load(Ordering::Acquire) || record.state == ReleaseRecoveryState::Cancelled {
        return Ok(());
    }
    if record.state == ReleaseRecoveryState::ClassifyingFailure
        && matches!(
            crate::release_recovery::next_directive(record, 0),
            crate::release_recovery::ReleaseDirective::Escalate
        )
    {
        let commit = record.active_commit().unwrap_or("unknown").to_owned();
        record.transition(
            ReleaseRecoveryState::Escalated,
            &commit,
            "GitHub account execution restriction requires owner action before progression",
            vec!["rrc:account-execution-restriction".into()],
            None,
        )?;
        ledger.save(record)?;
        return Ok(());
    }
    if matches!(
        record.state,
        ReleaseRecoveryState::ClassifyingFailure | ReleaseRecoveryState::DiagnosingLocalFailure
    ) && record.failures.last().is_some_and(|failure| {
        matches!(
            failure.confidence,
            crate::release_recovery::EvidenceConfidence::Tentative
                | crate::release_recovery::EvidenceConfidence::Unknown
        )
    }) {
        let commit = record.active_commit().unwrap_or("unknown").to_owned();
        record.transition(
            ReleaseRecoveryState::NeedMoreEvidence,
            &commit,
            "tentative/unknown cause requires new evidence before repair",
            vec!["rrc:classification-uncertain".into()],
            None,
        )?;
        ledger.save(record)?;
        return Ok(());
    }
    if record.state == ReleaseRecoveryState::ClassifyingFailure
        && record
            .failures
            .last()
            .is_some_and(|failure| failure.exists_on_last_green.is_none())
        && let Some(green) = executor.last_green_release_commit(repository)?
    {
        let candidate = record
            .active_commit()
            .ok_or_else(|| RrcError::Invalid("comparison candidate missing".into()))?
            .to_owned();
        let changed = executor.source_changed_since(&green, &candidate)?;
        crate::release_recovery::compare_with_last_green(
            record, repository, &green, changed, github,
        )?;
        ledger.save(record)?;
    }
    match record.state {
        ReleaseRecoveryState::LocalVerification => {
            if record.mutation.version_after.is_none() {
                if record.mutation.local_gates.is_empty()
                    && let Ok((before, after)) =
                        executor.preview_release_version(record.objective.version.as_str())
                {
                    record.release_version = Some(after);
                    record.mutation.version_before = Some(before);
                    let mut gates = vec![LocalGateRecord {
                        name: "version-preparation".into(),
                        state: SettlementState::Running,
                        command: "cargo check --workspace --all-targets".into(),
                        evidence_ref: None,
                    }];
                    gates.extend(production_local_gates());
                    record.mutation.local_gates = gates;
                    record.note_liveness_active("preparing release target");
                    record.note_progress_milestone(
                        "Preparing release target and validating the version graph",
                    );
                    ledger.save_for_scheduling(record)?;
                }
                if let Some(index) = record
                    .mutation
                    .local_gates
                    .iter()
                    .position(|gate| gate.name == "version-preparation")
                    && record.mutation.local_gates[index].state != SettlementState::Running
                {
                    record.mutation.local_gates[index].state = SettlementState::Running;
                    record.mutation.local_gates[index].evidence_ref = None;
                    record.note_liveness_active("preparing repaired release target");
                    record.note_progress_milestone(
                        "Preparing repaired release target and validating the version graph",
                    );
                    ledger.save_for_scheduling(record)?;
                }
                let receipt = match executor.prepare_version_bump(
                    record.objective.version.as_str(),
                    &admit_release_mutation(record, ReleaseMutationKind::VersionBump)?,
                ) {
                    Ok(receipt) => receipt,
                    Err(RrcError::ResourceConstrained(detail)) => {
                        let index = match record
                            .mutation
                            .local_gates
                            .iter()
                            .position(|gate| gate.name == "version-preparation")
                        {
                            Some(index) => index,
                            None => {
                                record.mutation.local_gates.insert(
                                    0,
                                    LocalGateRecord {
                                        name: "version-preparation".into(),
                                        state: SettlementState::Running,
                                        command: "cargo check --workspace --all-targets".into(),
                                        evidence_ref: None,
                                    },
                                );
                                0
                            }
                        };
                        // The executor has restored the version transaction before
                        // returning this error. Settle its journal in the same
                        // checkpoint as deferral so restart cannot see an uncertain
                        // mutation for a safely rolled-back preparation.
                        record.mutation.in_flight_operation = None;
                        defer_local_resources(record, ledger, executor, index, &detail)?;
                        return Ok(());
                    }
                    Err(error @ RrcError::VersionPreparationFailed(_)) => {
                        let index = record
                            .mutation
                            .local_gates
                            .iter()
                            .position(|gate| gate.name == "version-preparation")
                            .ok_or_else(|| {
                                RrcError::Invalid(
                                    "version-preparation gate receipt is missing".into(),
                                )
                            })?;
                        // The typed executor error proves the exact transaction rollback.
                        // Settle the journal in the same checkpoint as causal evidence.
                        record.mutation.in_flight_operation = None;
                        record_local_gate_failure(record, ledger, index, &error)?;
                        return Err(error);
                    }
                    Err(error) => return Err(error),
                };
                record.mutation.source_commit = record.release_commit.clone();
                record.mutation.version_before = Some(receipt.before.clone());
                record.mutation.version_after = Some(receipt.after.clone());
                record.release_version = Some(receipt.after);
                record.mutation.version_files = receipt.files;
                if let Some(gate) = record
                    .mutation
                    .local_gates
                    .iter_mut()
                    .find(|gate| gate.name == "version-preparation")
                {
                    gate.state = SettlementState::Succeeded;
                    gate.evidence_ref = Some("local:version-preparation".into());
                } else {
                    record.mutation.local_gates = production_local_gates();
                }
                record.note_progress_milestone(format!(
                    "Version preparation completed; {} local gates are ready",
                    record.mutation.local_gates.len()
                ));
                ledger.save_for_scheduling(record)?;
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
                ledger.save_for_scheduling(record)?;
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
                        defer_local_resources(record, ledger, executor, index, &detail)?;
                        return Ok(());
                    }
                    Err(error) if controller_stop_is_not_source_failure(&error) => {
                        return Err(error);
                    }
                    Err(error) => {
                        record_local_gate_failure(record, ledger, index, &error)?;
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
                    &admit_release_mutation(record, ReleaseMutationKind::CommitCandidate)?,
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
            let pushed = executor.push_candidate(&admit_release_mutation(
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
            let admission = &admit_release_mutation(record, ReleaseMutationKind::PushCandidate)?;
            record.consume_retry(crate::release_recovery::RetryKind::FullGate)?;
            ledger.save(record)?;
            let pushed = executor.push_candidate(admission)?;
            record.mutation.candidate_pushed = true;
            record.mutation.candidate_push_ref = Some(pushed.clone());
            record.required_gates.clear();
            let commit = record
                .active_commit()
                .map(str::to_owned)
                .ok_or_else(|| RrcError::Invalid("repaired candidate commit is missing".into()))?;
            record.transition(
                ReleaseRecoveryState::RemoteGateRunning,
                &commit,
                "verified repair candidate pushed for a fresh exact-SHA gate set",
                vec![pushed],
                None,
            )?;
            record.note_progress(true);
            ledger.save(record)?;
        }
        ReleaseRecoveryState::RemoteGateRunning
        | ReleaseRecoveryState::WaitingForMatrix
        | ReleaseRecoveryState::PostReleaseMainDegraded => {
            refresh_remote_evidence(record, repository, github)?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::DiagnosingLocalFailure | ReleaseRecoveryState::DiagnosingRepair => {
            let factory = repair_factory.ok_or_else(|| {
                RrcError::Invalid(
                    "focused repair requires an active host permission/provider port".into(),
                )
            })?;
            run_bounded_repair_agent(workspace, record, ledger, factory, Arc::clone(&cancelled))?;
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
                .iter()
                .filter(|failure| Some(failure.source_commit.as_str()) == record.active_commit())
                .any(|failure| failure.class.infrastructure_like()) =>
        {
            if record
                .retry_admission(crate::release_recovery::RetryKind::Infrastructure)
                .admitted
            {
                let token = crate::release_recovery::admit_retry(
                    record,
                    crate::release_recovery::RetryKind::Infrastructure,
                )?;
                record.retry_run_floors.clear();
                for gate in record
                    .required_gates
                    .iter_mut()
                    .filter(|gate| gate.has_failure())
                {
                    let id = gate
                        .run_id
                        .ok_or_else(|| RrcError::Invalid("retry run id missing".into()))?;
                    let attempt = gate
                        .run_attempt
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or_else(|| RrcError::Invalid("run attempt overflow".into()))?;
                    record.retry_run_floors.push((id, attempt));
                    gate.jobs.clear();
                    gate.run_state = None;
                    gate.run_attempt = Some(attempt);
                }
                let commit = record.active_commit().unwrap_or("unknown").to_owned();
                record.transition(
                    ReleaseRecoveryState::RemoteGateRunning,
                    &commit,
                    "one infrastructure retry reserved after confirmed service recovery",
                    vec!["rrc:infrastructure-retry".into()],
                    None,
                )?;
                ledger.save(record)?;
                github.rerun_admitted(repository, token)?;
                return Ok(());
            }
            let evidence = external_health_evidence(record, health)?;
            apply_controller_event(record, ReleaseControllerEvent::ExternalHealth(evidence))?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::PausedExternal => {
            let official = health.official_status()?;
            if official.degraded {
                record.updated_at = Utc::now();
            } else {
                record.external_block = None;
                record.state_changes.push(RelevantStateChange {
                    kind: RelevantStateChangeKind::ExternalServiceRecovered,
                    description: "official GitHub Actions status is operational".into(),
                    evidence_refs: vec![official.evidence_ref],
                    observed_at: Utc::now(),
                });
                let commit = record
                    .active_commit()
                    .ok_or_else(|| RrcError::Invalid("controller commit missing".into()))?
                    .to_owned();
                record.transition(
                    ReleaseRecoveryState::RemoteGateRunning,
                    &commit,
                    "confirmed service recovery requires fresh exact-SHA evidence",
                    vec!["github:official-status-recheck".into()],
                    None,
                )?;
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
                &admit_release_mutation(record, ReleaseMutationKind::CreateTag)?,
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
        ReleaseRecoveryState::Tagging => {
            apply_controller_event(
                record,
                ReleaseControllerEvent::PublicationStarted(vec!["rrc:settled-tag-receipt".into()]),
            )?;
            ledger.save(record)?;
        }
        ReleaseRecoveryState::PostReleaseCloseout => {
            let main = github.current_main_commit(repository)?.ok_or_else(|| {
                RrcError::Invalid("current main identity unavailable for closeout".into())
            })?;
            if record.release_commit.as_deref() != Some(main.as_str()) {
                *record = crate::release_recovery::post_release_main_epoch(record, &main)?;
            } else {
                record.current_main = Some(main.clone());
                record.objective.post_release_main_epoch = true;
                record.required_gates.clear();
                record.transition(
                    ReleaseRecoveryState::WaitingForMatrix,
                    &main,
                    "post-release closeout refreshes current main gates",
                    vec![],
                    None,
                )?;
            }
            ledger.save(record)?;
        }
        ReleaseRecoveryState::Publishing => {
            if record.mutation.publication_watch_millis >= PUBLICATION_WATCH_LIMIT_MILLIS {
                return Err(RrcError::WatchdogDeadline {
                    operation: "publication verification".into(),
                    limit_seconds: PUBLICATION_WATCH_LIMIT_MILLIS / 1000,
                });
            }
            let _admission = &admit_release_mutation(record, ReleaseMutationKind::Publish)?;
            let tag = record
                .mutation
                .tag_name
                .clone()
                .ok_or_else(|| RrcError::Invalid("release tag is missing".into()))?;
            let started = Instant::now();
            let observation = executor.publication(
                repository,
                &tag,
                record
                    .release_commit
                    .as_deref()
                    .ok_or_else(|| RrcError::Invalid("publication candidate missing".into()))?,
            );
            charge_publication_watch(record, started.elapsed());
            if matches!(observation, Err(RrcError::Cancelled)) {
                return Err(RrcError::Cancelled);
            }
            ledger.save(record)?;
            if let Some(receipt) = publication_read_observation(record, ledger, observation)? {
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
            if !record.mutation.publication_verified {
                return Err(RrcError::Invalid(
                    "unverified publication cannot enter closeout".into(),
                ));
            }
            record.mutation.closeout_registry = executor.plan_closeout(record, repository)?;
            apply_controller_event(
                record,
                ReleaseControllerEvent::PostReleaseCloseoutStarted(vec![
                    "rrc:verified-publication-closeout".into(),
                ]),
            )?;
            ledger.save(record)?;
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
    spawn_release_worker_at_root_with_policy(
        workspace,
        repository,
        repo_identity,
        repair_factory,
        root,
        resource_policy_for_worker(),
    )
}

// The lifecycle fixture proves worker ownership and real child progression,
// not live resource admission. Dedicated governor tests keep the production
// thresholds covered without making this test depend on workstation pressure.
#[cfg(test)]
pub(crate) fn spawn_release_worker_at_root_with_permissive_policy(
    workspace: PathBuf,
    repository: String,
    repo_identity: String,
    repair_factory: Option<crate::WorkerFactory>,
    root: PathBuf,
) -> Result<ReleaseWorkerRegistration, RrcError> {
    spawn_release_worker_at_root_with_policy(
        workspace,
        repository,
        repo_identity,
        repair_factory,
        root,
        permissive_resource_policy(),
    )
}

fn spawn_release_worker_at_root_with_policy(
    workspace: PathBuf,
    repository: String,
    repo_identity: String,
    repair_factory: Option<crate::WorkerFactory>,
    root: PathBuf,
    resource_policy: ResourcePolicy,
) -> Result<ReleaseWorkerRegistration, RrcError> {
    {
        let guard = active_workers()
            .lock()
            .map_err(|_| RrcError::Invalid("release worker lock is poisoned".into()))?;
        if let Some(existing) = guard.get(&repo_identity) {
            let epoch_id = existing
                .activity
                .lock()
                .map_err(|_| RrcError::Invalid("release worker activity lock is poisoned".into()))?
                .epoch_id
                .clone();
            return Ok(ReleaseWorkerRegistration {
                repo_identity,
                epoch_id,
            });
        }
    }
    let ledger = ReleaseLedger::open(root.clone(), &repo_identity)?;
    let owner = ledger.acquire_owner()?;
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
        resource_policy,
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
        resource_deferred: record.state == ReleaseRecoveryState::ResourceDeferred,
        resource_telemetry: record
            .resource_deferred
            .as_ref()
            .map(|deferred| deferred.telemetry.clone()),
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
            let _owner = owner;
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

fn resource_watch_interval(unchanged_observations: u8) -> Duration {
    if unchanged_observations >= 10 {
        RESOURCE_WATCH_MAX_INTERVAL
    } else if unchanged_observations >= 3 {
        RESOURCE_WATCH_BACKOFF_INTERVAL
    } else {
        RESOURCE_WATCH_INITIAL_INTERVAL
    }
}

fn apply_resource_watch_observation(
    record: &mut ReleaseRecoveryRecord,
    telemetry: ResourceTelemetry,
    safely_admissible: bool,
) -> Result<bool, RrcError> {
    if record.state != ReleaseRecoveryState::ResourceDeferred {
        return Err(RrcError::Invalid(
            "resource watch observation requires ResourceDeferred state".into(),
        ));
    }
    let prior_pressure = record
        .resource_deferred
        .as_ref()
        .map(|deferred| deferred.telemetry.pressure)
        .ok_or_else(|| RrcError::Invalid("ResourceDeferred detail is missing".into()))?;
    let recovered = {
        let deferred = record
            .resource_deferred
            .as_mut()
            .expect("checked ResourceDeferred detail");
        deferred.last_observed_at = Utc::now();
        deferred.unchanged_observations = if telemetry.pressure == prior_pressure {
            deferred.unchanged_observations.saturating_add(1)
        } else {
            0
        };
        deferred.consecutive_normal_observations =
            if safely_admissible && telemetry.pressure == ResourcePressure::Normal {
                deferred.consecutive_normal_observations.saturating_add(1)
            } else {
                0
            };
        deferred.next_check_seconds =
            resource_watch_interval(deferred.unchanged_observations).as_secs();
        deferred.telemetry = telemetry;
        deferred.consecutive_normal_observations >= RESOURCE_WATCH_NORMAL_CONFIRMATIONS
    };
    if !recovered {
        record.refresh_progress();
        return Ok(false);
    }

    let gate = record
        .resource_deferred
        .as_ref()
        .map(|deferred| deferred.gate_name.clone())
        .unwrap_or_else(|| "next local gate".into());
    let commit = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("resource recovery has no active commit".into()))?;
    record.transition(
        ReleaseRecoveryState::LocalVerification,
        &commit,
        format!("Host capacity recovered — continuing {gate}."),
        vec!["local:resource-recovered".into()],
        None,
    )?;
    record.resource_deferred = None;
    Ok(true)
}

fn watch_resource_deferred(
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    executor: &NativeReleaseExecutor,
    cancelled: &Arc<AtomicBool>,
    activity: &Arc<Mutex<ReleaseWorkerActivity>>,
) -> Result<(), RrcError> {
    loop {
        let delay = record
            .resource_deferred
            .as_ref()
            .map(|deferred| Duration::from_secs(deferred.next_check_seconds.max(1)))
            .unwrap_or(RESOURCE_WATCH_INITIAL_INTERVAL);
        let deadline = Instant::now() + delay;
        while Instant::now() < deadline {
            if cancelled.load(Ordering::Acquire) {
                return Err(RrcError::Cancelled);
            }
            thread::sleep(Duration::from_millis(100));
        }
        let (safe, telemetry) = executor.reevaluate_expensive_resources()?;
        let recovered = apply_resource_watch_observation(record, telemetry, safe)?;
        ledger.save(record)?;
        update_release_activity(activity, record);
        if recovered {
            return Ok(());
        }
    }
}

fn charge_publication_watch(record: &mut ReleaseRecoveryRecord, elapsed: Duration) {
    record.mutation.publication_watch_millis = record
        .mutation
        .publication_watch_millis
        .saturating_add(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
}

fn publication_read_observation(
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    outcome: Result<Option<PublicationReceipt>, RrcError>,
) -> Result<Option<PublicationReceipt>, RrcError> {
    match outcome {
        Err(error @ (RrcError::WatchdogStalled { .. } | RrcError::WatchdogDeadline { .. }))
            if matches!(&error,
                RrcError::WatchdogStalled { operation, .. } | RrcError::WatchdogDeadline { operation, .. }
                if operation == "publication evidence request")
                && record.state == ReleaseRecoveryState::Publishing
                && record.mutation.tag_pushed
                && record.mutation.in_flight_operation.is_none()
                && record.mutation.publication_read_retries < 2 =>
        {
            // This stage only observes workflow/assets/tag receipts. Persist its
            // bounded read recovery before another observation; never replay a write
            // or consume/reset causal source/CI retry admissions.
            record.mutation.publication_read_retries += 1;
            record.note_progress_milestone(format!(
                "Publication observation timed out — read-only recovery {}/2 scheduled; published state remains unverified",
                record.mutation.publication_read_retries,
            ));
            record.note_liveness_active("publication observation recovery");
            ledger.save(record)?;
            Ok(None)
        }
        other => other,
    }
}

fn wait_for_remote_poll(delay: Duration, cancelled: &AtomicBool) -> Result<(), RrcError> {
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if cancelled.load(Ordering::Acquire) {
            return Err(RrcError::Cancelled);
        }
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }
    Ok(())
}

fn release_worker_terminal(state: ReleaseRecoveryState) -> bool {
    matches!(
        state,
        ReleaseRecoveryState::NeedMoreEvidence
            | ReleaseRecoveryState::PausedExternal
            | ReleaseRecoveryState::PostReleaseMainDegraded
            | ReleaseRecoveryState::Escalated
            | ReleaseRecoveryState::Complete
            | ReleaseRecoveryState::Cancelled
    )
}

fn release_liveness_operation(state: ReleaseRecoveryState) -> &'static str {
    match state {
        ReleaseRecoveryState::LocalVerification => "local verification",
        ReleaseRecoveryState::RemoteGateRunning | ReleaseRecoveryState::WaitingForMatrix => {
            "remote matrix watch"
        }
        ReleaseRecoveryState::ClassifyingFailure | ReleaseRecoveryState::FocusedRepair => {
            "focused repair agent"
        }
        ReleaseRecoveryState::Tagging | ReleaseRecoveryState::Publishing => {
            "publication verification"
        }
        ReleaseRecoveryState::ResourceDeferred => "resource recovery watch",
        _ => "release controller step",
    }
}

fn finish_release_closeout(
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    executor: &dyn ReleaseExecutionPort,
    factory: Option<&crate::WorkerFactory>,
    repository: &str,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), RrcError> {
    let allow_mutation = match record.mutation.in_flight_operation.as_deref() {
        None => true,
        Some("Closeout") => false,
        Some(_) => {
            return Err(RrcError::MutationBlocked(
                "unsettled non-closeout operation cannot be replayed".into(),
            ));
        }
    };
    if record.mutation.closeout_registry.is_none() {
        record.mutation.closeout_registry = executor.plan_closeout(record, repository)?;
        ledger.save(record)?;
    }
    if allow_mutation {
        authorize_controller_step(factory, record, repository, cancelled)?;
        record.mutation.in_flight_operation = Some("Closeout".into());
        ledger.save(record)?;
    }
    let receipt = executor.finish_closeout(
        record,
        allow_mutation,
        &admit_release_mutation(record, ReleaseMutationKind::Closeout)?,
    )?;
    record.mutation.closeout_receipt = Some(receipt);
    record.mutation.in_flight_operation = None;
    record.note_progress_milestone(
        "Release completed — registry and execution report verified; final summary ready",
    );
    ledger.save(record)?;
    Ok(())
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
    let _watcher = CheckpointCancellationWatcher::start(ledger.clone(), Arc::clone(&cancelled))?;
    let executor = NativeReleaseExecutor::with_activity_and_governor(
        workspace,
        Arc::clone(&cancelled),
        Arc::clone(activity),
        resource_governor,
    )?
    .with_firewall(repair_factory.and_then(|factory| factory.config.firewall.clone()));
    let github = GhCliEvidenceAdapter.with_cancellation(Arc::clone(&cancelled));
    let health = CurlGitHubStatusAdapter.with_cancellation(Arc::clone(&cancelled));
    loop {
        let Some(mut record) = ledger.load()? else {
            return Ok(());
        };
        record.note_liveness_active(release_liveness_operation(record.state));
        ledger.save_for_scheduling(&record)?;
        update_release_activity(activity, &record);
        if record.state == ReleaseRecoveryState::ResourceDeferred {
            watch_resource_deferred(&mut record, &ledger, &executor, &cancelled, activity)?;
            update_release_activity(activity, &record);
        }
        let before = record.state;
        if record.state == ReleaseRecoveryState::Complete
            && record.mutation.publication_verified
            && record.mutation.closeout_receipt.is_none()
        {
            finish_release_closeout(
                &mut record,
                &ledger,
                &executor,
                repair_factory,
                repository,
                &cancelled,
            )?;
            update_release_activity(activity, &record);
        }
        if record.mutation.in_flight_operation.is_some() {
            return Err(RrcError::MutationBlocked(
                "previous release operation did not settle; reconcile receipts before retry".into(),
            ));
        }
        let has_side_effects = release_stage_has_side_effects(&record);
        if has_side_effects {
            authorize_controller_step(repair_factory, &record, repository, &cancelled)?;
        }
        let journals_mutation = has_side_effects
            && (record.state != ReleaseRecoveryState::LocalVerification
                || record.mutation.version_after.is_none()
                || record
                    .mutation
                    .local_gates
                    .iter()
                    .all(|gate| gate.state == SettlementState::Succeeded));
        if journals_mutation {
            record.mutation.in_flight_operation = Some(format!("{:?}", record.state));
            record.updated_at = Utc::now();
            ledger.save(&record)?;
        }
        let outcome = advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace,
                repository,
                ledger: &ledger,
                executor: &executor,
                github: &github,
                health: &health,
                repair_factory,
                cancelled: Arc::clone(&cancelled),
            },
        );
        settle_release_step_outcome(before, record.state, outcome)?;
        if journals_mutation && record.state != ReleaseRecoveryState::Cancelled {
            record.mutation.in_flight_operation = None;
            record.updated_at = Utc::now();
            ledger.save(&record)?;
        }
        update_release_activity(activity, &record);
        if matches!(
            record.state,
            ReleaseRecoveryState::WaitingForMatrix | ReleaseRecoveryState::Publishing
        ) {
            // The first exact-SHA lookup happens immediately. Subsequent
            // unchanged observations use bounded 20/40/80/120-second backoff,
            // while this controller-owned worker remains registered.
            let unchanged = record.consecutive_stagnant_actions.saturating_sub(1);
            let wait_started = Instant::now();
            wait_for_remote_poll(poll_interval(unchanged), &cancelled)?;
            record.metrics.ci_wait_millis = record.metrics.ci_wait_millis.saturating_add(
                u64::try_from(wait_started.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
            if record.state == ReleaseRecoveryState::Publishing {
                charge_publication_watch(&mut record, wait_started.elapsed());
            }
            if let Some(latest) = ledger.load()?
                && latest.state == ReleaseRecoveryState::Cancelled
            {
                return Ok(());
            }
            record.updated_at = Utc::now();
            ledger.save(&record)?;
            continue;
        }
        if record.state == ReleaseRecoveryState::Complete
            && record.mutation.publication_verified
            && record.mutation.closeout_receipt.is_none()
        {
            finish_release_closeout(
                &mut record,
                &ledger,
                &executor,
                repair_factory,
                repository,
                &cancelled,
            )?;
        }
        if release_worker_terminal(record.state) {
            record.liveness = Default::default();
            record.refresh_progress();
            ledger.save(&record)?;
            update_release_activity(activity, &record);
            break;
        }
        if record.state == before
            && !matches!(
                record.state,
                ReleaseRecoveryState::LocalVerification
                    | ReleaseRecoveryState::RemoteGateRunning
                    | ReleaseRecoveryState::RetryAdmissible
            )
        {
            record.liveness = Default::default();
            record.refresh_progress();
            ledger.save(&record)?;
            update_release_activity(activity, &record);
            break;
        }
    }
    Ok(())
}

fn settle_release_step_outcome(
    before: ReleaseRecoveryState,
    after: ReleaseRecoveryState,
    outcome: Result<(), RrcError>,
) -> Result<(), RrcError> {
    match outcome {
        Err(error)
            if before == ReleaseRecoveryState::LocalVerification
                && after == ReleaseRecoveryState::DiagnosingLocalFailure
                && !controller_stop_is_not_source_failure(&error) =>
        {
            Ok(())
        }
        result => result,
    }
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
            .find(|gate| gate.state == SettlementState::Running)
            .or_else(|| {
                (record.state == ReleaseRecoveryState::ResourceDeferred)
                    .then(|| {
                        record
                            .mutation
                            .local_gates
                            .iter()
                            .find(|gate| gate.state == SettlementState::NotStarted)
                    })
                    .flatten()
            });
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
        activity.resource_deferred = record.state == ReleaseRecoveryState::ResourceDeferred;
        if let Some(deferred) = record.resource_deferred.as_ref() {
            activity.resource_telemetry = Some(deferred.telemetry.clone());
        }
        if let Some(gate) = active_gate {
            if record.state == ReleaseRecoveryState::ResourceDeferred {
                activity.gate_started_at = None;
                activity.current_child = None;
            } else if activity.current_gate.as_deref() != Some(gate.name.as_str()) {
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
    // Resource deferral is persisted and normally watched inside the active
    // worker. If a governor-side discovery error escapes that loop, never
    // transform the workstation-safety condition into source-failure evidence.
    let Ok(ledger) = ReleaseLedger::open(root.to_path_buf(), repo_identity) else {
        return;
    };
    let Ok(Some(mut record)) = ledger.load() else {
        return;
    };
    if matches!(error, RrcError::ResourceConstrained(_)) {
        record.liveness.state = crate::release_recovery::ReleaseLivenessState::OwnerExited;
        record.liveness.detail = redact_secrets(&error.to_string());
        record.liveness.observed_at = Some(Utc::now());
        record.note_progress_milestone(format!("Resource recovery worker stopped: {error}"));
        record.refresh_progress();
        let _ = ledger.save(&record);
        return;
    }
    if matches!(error, RrcError::Cancelled) {
        return;
    }
    if controller_stop_is_not_source_failure(error) {
        record.note_liveness_failure(error);
        let _ = ledger.save(&record);
        return;
    }
    if record.state == ReleaseRecoveryState::LocalVerification {
        record.liveness = Default::default();
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
    } else {
        record.note_liveness_failure(error);
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
        resource_deferred: activity.resource_deferred,
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

/// Live in-process worker, or a non-running recoverable epoch. A dead owner
/// is persisted as `OwnerExited` and is never rendered as Active or Ready.
#[must_use]
pub fn release_run_snapshot_for_workspace(workspace: &Path) -> Option<ReleaseWorkerSnapshot> {
    if let Some(mut live) = active_release_worker_for_workspace(workspace) {
        // Commands update the durable ledger before their blocking execution.
        // Never wait for a whole gate to finish before projecting its milestone.
        if let Ok(Some(record)) = crate::release_recovery::reconcile_unowned_release(workspace)
            && let Some(progress) =
                current_release_progress(&live.epoch_id, &live.progress, &record)
        {
            live.detail.clone_from(&progress.headline);
            live.current_gate.clone_from(&progress.current_gate);
            live.completed_gates = progress.completed_local_gates;
            live.total_gates = progress.total_local_gates;
            live.progress = progress;
        }
        return Some(live);
    }
    let record = crate::release_recovery::reconcile_unowned_release(workspace).ok()??;
    if matches!(
        record.state,
        ReleaseRecoveryState::Idle | ReleaseRecoveryState::Cancelled
    ) || (record.state == ReleaseRecoveryState::Complete
        && record.mutation.closeout_receipt.is_some())
    {
        return None;
    }
    let recoverable =
        record.liveness.state != crate::release_recovery::ReleaseLivenessState::Active;
    Some(ReleaseWorkerSnapshot {
        repo_identity: record.repo_identity,
        epoch_id: record.epoch_id,
        stage: if recoverable {
            "Release recoverable".into()
        } else {
            "Release controller".into()
        },
        detail: if recoverable {
            record.progress.headline.clone()
        } else {
            format!(
                "Release controller process {} is still running",
                record.liveness.owner_pid.unwrap_or(0)
            )
        },
        cancellable: false,
        current_gate: record
            .mutation
            .local_gates
            .iter()
            .find(|gate| gate.state == SettlementState::Running)
            .map(|gate| gate.name.clone()),
        current_command: None,
        current_child: None,
        gate_elapsed_secs: None,
        last_activity_ago_secs: 0,
        completed_gates: record
            .mutation
            .local_gates
            .iter()
            .filter(|gate| gate.state == SettlementState::Succeeded)
            .count(),
        total_gates: record.mutation.local_gates.len(),
        version_before: record.mutation.version_before,
        version_after: record.mutation.version_after.clone(),
        candidate_sha: record.release_commit,
        retry_budget: String::new(),
        failure_fingerprint: None,
        recent_output: Vec::new(),
        process_alive: !recoverable,
        progress: record.progress,
        resource_deferred: record.state == ReleaseRecoveryState::ResourceDeferred,
        resource_telemetry: record.resource_deferred.map(|deferred| deferred.telemetry),
    })
}

fn current_release_progress(
    epoch: &str,
    live: &ReleaseProgress,
    record: &ReleaseRecoveryRecord,
) -> Option<ReleaseProgress> {
    if record.epoch_id != epoch {
        return None;
    }
    let mut view = record.clone();
    view.refresh_progress();
    fn preserve_case_units(
        durable: &mut [crate::release_recovery::ReleaseProgressTask],
        live: &[crate::release_recovery::ReleaseProgressTask],
    ) {
        for task in durable {
            if let Some(observed) = live.iter().find(|item| item.name == task.name) {
                if task.name == "Exact acceptance cases"
                    && task.state == crate::release_recovery::ReleaseProgressState::Running
                    && matches!(
                        observed.state,
                        crate::release_recovery::ReleaseProgressState::Running
                            | crate::release_recovery::ReleaseProgressState::Passed
                    )
                    && observed.units.total.is_some()
                {
                    task.units = observed.units.clone();
                    task.state = observed.state;
                }
                preserve_case_units(&mut task.children, &observed.children);
            }
        }
    }
    preserve_case_units(&mut view.progress.tasks, &live.tasks);
    Some(view.progress)
}

/// Durable final delivery is separate from an active RUN snapshot. Hosts must
/// consume this receipt after the worker disappears; publication alone is insufficient.
pub fn release_completion_for_workspace(workspace: &Path) -> Option<(String, u64, String)> {
    let record = crate::release_recovery::reconcile_unowned_release(workspace).ok()??;
    if record.state != ReleaseRecoveryState::Complete {
        return None;
    }
    let summary = crate::release_closeout::summary(&record)?;
    Some((record.epoch_id, record.progress.next_sequence, summary))
}

pub fn relinquish_release_ownership() {
    crate::release_recovery::relinquish_release_owner_writes();
    let Some(root) = default_release_root() else {
        return;
    };
    let Ok(guard) = active_workers().lock() else {
        return;
    };
    for identity in guard.keys() {
        let Ok(ledger) = ReleaseLedger::open(root.clone(), identity) else {
            continue;
        };
        let Ok(Some(record)) = ledger.load() else {
            continue;
        };
        let _ = ledger.save(&record);
    }
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
        resource_deferred: false,
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
    admission: &ReleaseMutationAdmission,
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

fn selected_release_version(version: &str, target: &str) -> Result<String, RrcError> {
    let [major, minor, patch] = stable_semver_components(version)
        .ok_or_else(|| RrcError::Invalid("workspace version is not stable x.y.z semver".into()))?;
    let selector = ReleaseVersionSelector::parse(target).map_err(RrcError::Invalid)?;
    match selector {
        ReleaseVersionSelector::Patch => patch
            .checked_add(1)
            .map(|patch| format!("{major}.{minor}.{patch}"))
            .ok_or_else(|| RrcError::Invalid("patch version overflowed".into())),
        ReleaseVersionSelector::Minor => minor
            .checked_add(1)
            .map(|minor| format!("{major}.{minor}.0"))
            .ok_or_else(|| RrcError::Invalid("minor version overflowed".into())),
        ReleaseVersionSelector::Major => major
            .checked_add(1)
            .map(|major| format!("{major}.0.0"))
            .ok_or_else(|| RrcError::Invalid("major version overflowed".into())),
        ReleaseVersionSelector::Exact(after) => {
            let components = stable_semver_components(&after)
                .expect("exact release selectors are normalized stable semver");
            if components < [major, minor, patch] {
                return Err(RrcError::Invalid(format!(
                    "exact release version {after} is lower than workspace version {version}"
                )));
            }
            Ok(after)
        }
    }
}

fn build_version_mutation_plan(
    workspace: &Path,
    target: &str,
) -> Result<VersionMutationPlan, RrcError> {
    let root_path = workspace.join("Cargo.toml");
    let root = fs::read_to_string(&root_path)?;
    let before = workspace_version(&root)?;
    let after = selected_release_version(&before, target)?;
    let version_changes = before != after;
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
        if version_changes && updated != input {
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
    if version_changes {
        files.push(VersionFileMutation {
            relative_path: "registry/agent.json".into(),
            before: registry.into_bytes(),
            after: updated_registry.into_bytes(),
        });
    }
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
    cancelled: &AtomicBool,
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
    let mut metadata_command = Command::new("cargo");
    metadata_command
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .envs(cargo.environment());
    let metadata =
        run_bounded_external_command(&mut metadata_command, cancelled, LOCAL_GATE_WATCHDOG)?;
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

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RrcError> {
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

fn command_failure_evidence(stdout: &[u8], stderr: &[u8]) -> String {
    // Rust test assertions are written to stdout; compiler diagnostics usually
    // arrive on stderr. Select the cause before limiting the persisted receipt.
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    crate::release_recovery::first_causal_excerpt(&combined)
        .chars()
        .take(MAX_COMMAND_OUTPUT)
        .collect()
}

fn bounded_output(bytes: &[u8]) -> String {
    let clean = redact_secrets(&String::from_utf8_lossy(bytes));
    clean.chars().take(MAX_COMMAND_OUTPUT).collect()
}

#[derive(PartialEq, Eq)]
struct GitWorkspaceStatus {
    bytes: Vec<u8>,
    paths: Vec<String>,
}

#[derive(PartialEq, Eq)]
struct RepairVerificationSnapshot {
    status: GitWorkspaceStatus,
    patch: Vec<u8>,
    index: Vec<u8>,
}

fn literal_git_paths(bytes: &[u8]) -> Result<Vec<String>, RrcError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let body = bytes.strip_suffix(&[0]).ok_or_else(|| {
        RrcError::MutationBlocked("incomplete NUL-delimited Git inventory".into())
    })?;
    body.split(|byte| *byte == 0)
        .map(|path| {
            if path.is_empty() {
                return Err(RrcError::MutationBlocked("empty Git inventory path".into()));
            }
            String::from_utf8(path.to_vec())
                .map_err(|_| RrcError::MutationBlocked("Git inventory path is not UTF-8".into()))
        })
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn local_failure_receipt_preserves_late_stdout_cause_and_redacts_secrets() {
        let stdout = format!(
            "{}\nthread 'late_regression' panicked at tests/case.rs:42:3:\nassertion failed: expected current controller state\nauthorization: Bearer github_pat_LOCALFAILURESECRET000000000000000000\n",
            "test successful_case ... ok\n".repeat(500)
        );
        let stderr = format!(
            "{}\nerror: test failed, to rerun pass --test case\n",
            "Compiling successful_dependency\n".repeat(500)
        );
        let evidence = command_failure_evidence(stdout.as_bytes(), stderr.as_bytes());
        assert!(
            evidence.contains("late_regression"),
            "missing late stdout cause: {evidence}"
        );
        assert!(evidence.contains("assertion failed: expected current controller state"));
        assert!(!evidence.contains("LOCALFAILURESECRET"));
        assert!(evidence.chars().count() <= MAX_COMMAND_OUTPUT);
    }

    #[test]
    fn repair_worktree_is_a_restart_safe_sibling_of_the_epoch_workspace() {
        let first = repair_worktree_leaf("epoch", 0, 100);
        let restarted = repair_worktree_leaf("epoch", 0, 101);

        assert_ne!(first, "epoch");
        assert_ne!(first, restarted);
        assert_eq!(first, "epoch-repair-0-100");
    }

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
            resource_deferred: false,
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
            resource_deferred: false,
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

    #[test]
    fn live_progress_uses_durable_milestones_without_losing_case_counts() {
        use crate::release_recovery::{
            LocalGateRecord, ReleaseProgressState, ReleaseProgressUnits,
        };
        let mut record =
            crate::release_recovery::start_release("repo", "0.24.7", "main", &"a".repeat(40))
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.local_gates = vec![
            LocalGateRecord {
                name: "workspace-verify".into(),
                state: SettlementState::Succeeded,
                ..Default::default()
            },
            LocalGateRecord {
                name: "acceptance".into(),
                state: SettlementState::Running,
                ..Default::default()
            },
        ];
        record.note_progress_milestone("Running local gate 2/2: acceptance");
        record.refresh_progress();
        let mut live = record.progress.clone();
        live.milestones.clear();
        live.headline = "stale prior gate".into();
        let cases = &mut live.tasks[0].children[1].children[0];
        cases.state = ReleaseProgressState::Running;
        cases.units = ReleaseProgressUnits::counted(18, 118);
        let current = current_release_progress(&record.epoch_id, &live, &record).unwrap();
        assert_eq!(current.milestones, record.progress.milestones);
        assert_eq!(current.current_gate.as_deref(), Some("acceptance"));
        assert_eq!(current.completed_local_gates, 1);
        assert_eq!(
            current.tasks[0].children[1].children[0]
                .units
                .render()
                .as_deref(),
            Some("18/118")
        );
        assert_ne!(current.headline, live.headline);
        let cases = &mut live.tasks[0].children[1].children[0];
        cases.state = ReleaseProgressState::Passed;
        cases.units = ReleaseProgressUnits::counted(118, 118);
        let current = current_release_progress(&record.epoch_id, &live, &record).unwrap();
        assert_eq!(
            current.tasks[0].children[1].state,
            ReleaseProgressState::Running
        );
        assert_eq!(
            current.tasks[0].children[1].children[0].state,
            ReleaseProgressState::Passed
        );
        assert_eq!(
            current.tasks[0].children[1].children[0]
                .units
                .render()
                .as_deref(),
            Some("118/118")
        );
        assert!(current_release_progress("different-epoch", &live, &record).is_none());
    }

    fn version_fixture(member_version: &str) -> tempfile::TempDir {
        version_fixture_at("0.24.4", member_version)
    }

    fn version_fixture_at(workspace_version: &str, member_version: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("crates/a")).unwrap();
        fs::create_dir_all(root.path().join("crates/b")).unwrap();
        fs::create_dir_all(root.path().join("registry")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n[workspace.package]\nversion = \"{workspace_version}\"\n"
            ),
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
            format!(
                "{{\"version\":\"{workspace_version}\",\"archive\":\"https://example/v{workspace_version}/a.tgz\"}}"
            ),
        )
        .unwrap();
        fs::write(root.path().join("Cargo.lock"), "# fixture lockfile\n").unwrap();
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
    fn version_plan_honors_an_exact_stable_release_target() {
        let root = version_fixture("0.24.4");
        let plan = build_version_mutation_plan(root.path(), "0.25.0").unwrap();
        assert_eq!(plan.before, "0.24.4");
        assert_eq!(plan.after, "0.25.0");
        let root_manifest = plan
            .files
            .iter()
            .find(|file| file.relative_path == "Cargo.toml")
            .unwrap();
        assert!(
            String::from_utf8(root_manifest.after.clone())
                .unwrap()
                .contains("version = \"0.25.0\"")
        );
    }

    #[test]
    fn version_plan_rejects_decreasing_or_unstable_exact_targets() {
        let root = version_fixture("0.24.4");
        for target in ["0.24.3", "0.25.0-beta.1", "01.25.0"] {
            assert!(
                build_version_mutation_plan(root.path(), target).is_err(),
                "invalid exact target unexpectedly admitted: {target}"
            );
        }
    }

    #[test]
    fn explicit_unpublished_version_does_not_bump_again() {
        let root = version_fixture_at("0.24.5", "0.24.5");
        let paths = [
            "Cargo.toml",
            "crates/a/Cargo.toml",
            "crates/b/Cargo.toml",
            "registry/agent.json",
            "Cargo.lock",
        ];
        let before = paths
            .iter()
            .map(|path| (*path, fs::read(root.path().join(path)).unwrap()))
            .collect::<std::collections::BTreeMap<_, _>>();

        let plan = build_version_mutation_plan(root.path(), "v0.24.5").unwrap();
        assert_eq!(plan.before, "0.24.5");
        assert_eq!(plan.after, "0.24.5");
        assert!(plan.files.is_empty());
        apply_version_mutation_plan(root.path(), &plan, None).unwrap();
        for (path, expected) in &before {
            assert_eq!(&fs::read(root.path().join(path)).unwrap(), expected);
        }

        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "rrc@example.invalid"]);
        git(&["config", "user.name", "RRC Test"]);
        git(&["add", "."]);
        git(&["commit", "-m", "already versioned"]);
        let canonical_head = git(&["rev-parse", "HEAD"]);

        let mut record =
            crate::release_recovery::start_release("repo", "0.24.5", "main", &canonical_head)
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.version_after = Some("0.24.5".into());
        record.mutation.local_gates = vec![LocalGateRecord {
            name: "focused".into(),
            state: SettlementState::Succeeded,
            command: "focused".into(),
            evidence_ref: Some("local:focused".into()),
        }];
        let executor =
            NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
        let candidate = executor
            .commit_candidate(
                "0.24.5",
                &admit_release_mutation(&record, ReleaseMutationKind::CommitCandidate).unwrap(),
            )
            .unwrap();
        assert_eq!(candidate, canonical_head);
        assert_eq!(git(&["rev-list", "--count", "HEAD"]), "1");
        assert!(git(&["status", "--porcelain"]).is_empty());
        for (path, expected) in &before {
            assert_eq!(&fs::read(root.path().join(path)).unwrap(), expected);
        }
    }

    #[test]
    fn candidate_commit_preserves_literal_repair_paths() {
        assert_literal_candidate_paths(false);
    }

    #[test]
    fn candidate_commit_preserves_staged_literal_repair_paths() {
        assert_literal_candidate_paths(true);
    }

    fn assert_literal_candidate_paths(staged: bool) {
        let root = tempfile::tempdir().unwrap();
        let native =
            NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
        native.checked("git", &["init", "-b", "main"]).unwrap();
        native
            .checked("git", &["config", "user.name", "fixture"])
            .unwrap();
        native
            .checked("git", &["config", "user.email", "fixture@example.invalid"])
            .unwrap();
        fs::write(root.path().join("Cargo.toml"), "fixture baseline").unwrap();
        fs::write(root.path().join("Cargo.lock"), "fixture lock").unwrap();
        fs::create_dir(root.path().join("registry")).unwrap();
        fs::write(root.path().join("registry/agent.json"), "{}").unwrap();
        native.checked("git", &["add", "--all"]).unwrap();
        native
            .checked("git", &["commit", "-m", "baseline"])
            .unwrap();
        let base = native.checked("git", &["rev-parse", "HEAD"]).unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        let mut paths = vec!["docs/repair evidence.md", "docs/évidence.md"];
        if cfg!(unix) {
            paths.extend([
                "docs/line\nbreak.md",
                "docs/tab\tname.md",
                "docs/quoted\"name.md",
            ]);
        }
        for path in &paths {
            fs::write(root.path().join(path), "verified repair evidence").unwrap();
        }
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", base.trim())
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.version_after = Some("0.24.5".into());
        record.mutation.repair_files = paths.iter().map(|path| (*path).to_owned()).collect();
        record.mutation.local_gates = vec![LocalGateRecord {
            name: "focused".into(),
            state: SettlementState::Succeeded,
            command: "focused".into(),
            evidence_ref: Some("fixture:focused".into()),
        }];
        let admission =
            admit_release_mutation(&record, ReleaseMutationKind::CommitCandidate).unwrap();
        fs::write(root.path().join("unowned file.md"), "unadmitted").unwrap();
        assert!(native.commit_candidate("0.24.5", &admission).is_err());
        assert_eq!(native.checked("git", &["rev-parse", "HEAD"]).unwrap(), base);
        fs::remove_file(root.path().join("unowned file.md")).unwrap();
        if staged {
            native.checked("git", &["add", "--all"]).unwrap();
        }
        let committed = native.commit_candidate("0.24.5", &admission).unwrap();
        assert_ne!(committed, base.trim());
        assert!(
            native
                .checked("git", &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
        for path in paths {
            assert_eq!(
                native
                    .checked("git", &["show", &format!("HEAD:{path}")])
                    .unwrap(),
                "verified repair evidence"
            );
        }
        let old_path = "docs/repair evidence.md";
        let new_path = "docs/renamed evidence.md";
        native.checked("git", &["mv", old_path, new_path]).unwrap();
        for admitted in [vec![old_path], vec![new_path]] {
            record.mutation.repair_files = admitted.into_iter().map(str::to_owned).collect();
            let admission =
                admit_release_mutation(&record, ReleaseMutationKind::CommitCandidate).unwrap();
            assert!(
                native.commit_candidate("0.24.5", &admission).is_err(),
                "both rename paths must be admitted"
            );
        }
        record.mutation.repair_files = vec![old_path.into(), new_path.into()];
        let admission =
            admit_release_mutation(&record, ReleaseMutationKind::CommitCandidate).unwrap();
        native.commit_candidate("0.24.5", &admission).unwrap();
        assert!(native.git_status().unwrap().paths.is_empty());
    }

    #[test]
    fn candidate_commit_checks_paths_beyond_display_receipt_bound() {
        let root = tempfile::tempdir().unwrap();
        let native =
            NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
        native.checked("git", &["init", "-b", "main"]).unwrap();
        native
            .checked("git", &["config", "user.name", "fixture"])
            .unwrap();
        native
            .checked("git", &["config", "user.email", "fixture@example.invalid"])
            .unwrap();
        fs::create_dir(root.path().join("registry")).unwrap();
        for path in ["Cargo.toml", "Cargo.lock", "registry/agent.json"] {
            fs::write(root.path().join(path), "baseline").unwrap();
        }
        native.checked("git", &["add", "--all"]).unwrap();
        native
            .checked("git", &["commit", "-m", "baseline"])
            .unwrap();
        let base = native.checked("git", &["rev-parse", "HEAD"]).unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", base.trim())
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.version_after = Some("0.24.5".into());
        record.mutation.local_gates = vec![LocalGateRecord {
            name: "focused".into(),
            state: SettlementState::Succeeded,
            command: "focused".into(),
            evidence_ref: Some("fixture:focused".into()),
        }];
        // Each porcelain line occupies exactly sixteen bytes. The old display
        // receipt ended on a complete line, silently omitting the unowned tail.
        for index in 0..(MAX_COMMAND_OUTPUT / 16) {
            let path = format!("a{index:07}.txt");
            fs::write(root.path().join(&path), "admitted").unwrap();
            record.mutation.repair_files.push(path);
        }
        fs::write(
            root.path().join("zzz-unadmitted.txt"),
            "unowned staged change",
        )
        .unwrap();
        native.checked("git", &["add", "--all"]).unwrap();
        let status = native.command("git", &["status", "--porcelain"]).unwrap();
        assert!(status.stdout.len() > MAX_COMMAND_OUTPUT);
        let admission =
            admit_release_mutation(&record, ReleaseMutationKind::CommitCandidate).unwrap();
        assert!(
            native.commit_candidate("0.24.5", &admission).is_err(),
            "display truncation hid an unadmitted staged path"
        );
        assert_eq!(native.checked("git", &["rev-parse", "HEAD"]).unwrap(), base);
    }

    #[test]
    fn literal_git_inventory_refuses_incomplete_or_non_utf8_paths() {
        for bytes in [b"missing terminator".as_slice(), b"a\0\0", b"\xff\0"] {
            assert!(literal_git_paths(bytes).is_err());
        }
        assert_eq!(
            literal_git_paths(b"a\nb\0c\td\0").unwrap(),
            vec!["a\nb", "c\td"]
        );
    }

    #[test]
    fn next_patch_still_bumps_normally() {
        let root = version_fixture_at("0.24.5", "0.24.5");
        let plan = build_version_mutation_plan(root.path(), "patch").unwrap();
        assert_eq!(plan.before, "0.24.5");
        assert_eq!(plan.after, "0.24.6");
        assert!(!plan.files.is_empty());
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
    fn worker_keeps_matrix_and_classification_states_controller_owned() {
        assert!(!release_worker_terminal(
            ReleaseRecoveryState::RemoteGateRunning
        ));
        assert!(!release_worker_terminal(
            ReleaseRecoveryState::WaitingForMatrix
        ));
        assert!(!release_worker_terminal(
            ReleaseRecoveryState::ClassifyingFailure
        ));
        assert!(release_worker_terminal(ReleaseRecoveryState::Complete));
    }

    #[test]
    fn remote_poll_wait_is_responsive_to_cancellation() {
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();
        let error = wait_for_remote_poll(Duration::from_secs(30), &cancelled).unwrap_err();
        assert!(matches!(error, RrcError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn complete_matrix_repair_selection_retains_distinct_causal_families() {
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        record.release_commit = Some("abcdef123456".into());
        record.state = ReleaseRecoveryState::ClassifyingFailure;
        for (job_id, workflow, job, fingerprint) in [
            (1, "five-target-foundation", "macos", "darwin-openpty"),
            (2, "web-driver", "linux-x86_64", "debian-package-pin"),
        ] {
            record
                .failures
                .push(crate::release_recovery::FailureRecord {
                    workflow_id: job_id,
                    run_id: job_id,
                    attempt: 1,
                    job_id,
                    workflow_name: workflow.into(),
                    job_name: job.into(),
                    platform: Some(job.into()),
                    step_name: Some("test".into()),
                    fingerprint: crate::release_recovery::FailureFingerprint(fingerprint.into()),
                    class: crate::release_recovery::ReleaseFailureClass::CompileFailure,
                    confidence: crate::release_recovery::EvidenceConfidence::Proven,
                    causal_excerpt: fingerprint.into(),
                    source_commit: "abcdef123456".into(),
                    observed_at: Utc::now(),
                    other_platforms_passed: true,
                    exists_on_last_green: None,
                    related_source_touched: None,
                });
        }
        let selected = repairable_failures(&record);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].fingerprint.0, "darwin-openpty");
        assert_eq!(selected[1].fingerprint.0, "debian-package-pin");
    }

    #[test]
    fn published_release_keeps_worker_until_post_release_closeout() {
        assert!(
            !release_worker_terminal(ReleaseRecoveryState::Published),
            "publication must not strand the pending closeout directive"
        );
    }

    #[test]
    fn published_closeout_finishes_report_without_replaying_release_mutations() {
        struct MainGreen;
        impl GitHubEvidencePort for MainGreen {
            fn current_main_commit(&self, _: &str) -> Result<Option<String>, RrcError> {
                Ok(Some("a".repeat(40)))
            }
            fn matrix_for_sha(
                &self,
                repo: &str,
                sha: &str,
            ) -> Result<Vec<crate::release_recovery::GateRecord>, RrcError> {
                GreenGithub.matrix_for_sha(repo, sha)
            }
            fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("no rerun expected")
            }
            fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("no rerun expected")
            }
            fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("no rerun expected")
            }
            fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
                panic!("no failure log expected")
            }
        }
        let root = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(root.path().join("state"), "fixture").unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", &"a".repeat(40))
                .unwrap();
        record.state = ReleaseRecoveryState::Published;
        record.release_commit = Some("a".repeat(40));
        record.release_version = Some("0.24.6".into());
        record.mutation.source_workspace = Some(root.path().to_string_lossy().into_owned());
        record.mutation.publication_verified = true;
        record.mutation.tag_object = Some("immutable-tag".into());
        record.mutation.local_gates = production_local_gates();
        for gate in &mut record.mutation.local_gates {
            gate.state = SettlementState::Succeeded;
        }
        let publication = record.mutation.tag_object.clone();
        ledger.save(&record).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        for expected in [
            ReleaseRecoveryState::PostReleaseCloseout,
            ReleaseRecoveryState::WaitingForMatrix,
            ReleaseRecoveryState::Complete,
        ] {
            advance_release(
                &mut record,
                ReleaseAdvanceContext {
                    workspace: root.path(),
                    repository: "fixture/repo",
                    ledger: &ledger,
                    executor: &FakeRelease,
                    github: &MainGreen,
                    health: &HealthyStatus,
                    repair_factory: None,
                    cancelled: cancelled.clone(),
                },
            )
            .unwrap();
            assert_eq!(record.state, expected);
        }
        assert!(crate::release_closeout::summary(&record).is_none());
        let (factory, session, _runtime) =
            repair_test_factory_with_commands("closeout-fixture", "", &[]);
        finish_release_closeout(
            &mut record,
            &ledger,
            &FakeRelease,
            Some(&factory),
            "fixture/repo",
            &cancelled,
        )
        .unwrap();
        let receipt = record.mutation.closeout_receipt.as_ref().unwrap();
        assert!(Path::new(&receipt.report).is_file());
        assert_eq!(record.mutation.tag_object, publication);
        assert!(record.mutation.in_flight_operation.is_none());
        assert!(
            crate::release_closeout::summary(&record)
                .unwrap()
                .contains("Release v0.24.6 completed")
        );
        assert!(
            session.requests().is_empty(),
            "closeout must not dispatch a model"
        );
        assert!(
            ledger
                .load()
                .unwrap()
                .unwrap()
                .mutation
                .closeout_receipt
                .is_some()
        );
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

    #[test]
    fn version_preparation_is_persisted_before_the_expensive_check() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(root.path(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "0.24.5", "main", "abcdef123456")
                .unwrap();
        ledger.save(&record).unwrap();
        struct Exec {
            root: std::path::PathBuf,
        }
        impl ReleaseExecutionPort for Exec {
            fn preview_release_version(&self, _: &str) -> Result<(String, String), RrcError> {
                Ok(("0.24.4".into(), "0.24.5".into()))
            }
            fn prepare_version_bump(
                &self,
                _: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                require_kind(admission, ReleaseMutationKind::VersionBump)?;
                let saved = ReleaseLedger::open(self.root.clone(), "repo")
                    .unwrap()
                    .load()
                    .unwrap()
                    .unwrap();
                assert_eq!(saved.release_version.as_deref(), Some("0.24.5"));
                assert!(saved.mutation.local_gates.iter().any(|gate| {
                    gate.name == "version-preparation" && gate.state == SettlementState::Running
                }));
                assert_eq!(
                    saved.liveness.state,
                    crate::release_recovery::ReleaseLivenessState::Active
                );
                Ok(VersionBumpReceipt {
                    before: "0.24.4".into(),
                    after: "0.24.5".into(),
                    files: Vec::new(),
                })
            }
            fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError> {
                Ok(gate.name.clone())
            }
            fn commit_candidate(
                &self,
                _: &str,
                _: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                unreachable!("commit is outside this test")
            }
            fn push_candidate(&self, _: &ReleaseMutationAdmission) -> Result<String, RrcError> {
                unreachable!("push is outside this test")
            }
            fn create_and_push_tag(
                &self,
                _: &str,
                _: &str,
                _: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                unreachable!("tag is outside this test")
            }
            fn publication(
                &self,
                _: &str,
                _: &str,
                _expected_commit: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                Ok(None)
            }
        }
        struct Quiet;
        impl GitHubEvidencePort for Quiet {
            fn matrix_for_sha(
                &self,
                _: &str,
                _: &str,
            ) -> Result<Vec<crate::release_recovery::GateRecord>, RrcError> {
                Ok(Vec::new())
            }
            fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
                Ok(String::new())
            }
            fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
                Ok(())
            }
            fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
                Ok(())
            }
            fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
                Ok(())
            }
        }
        struct Unused;
        impl ExternalHealthPort for Unused {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                Ok(OfficialStatusSnapshot {
                    degraded: false,
                    summary: "unused".into(),
                    evidence_ref: "unused".into(),
                })
            }
        }
        advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: workspace.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &Exec {
                    root: root.path().to_path_buf(),
                },
                github: &Quiet,
                health: &Unused,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .unwrap();
        assert_eq!(record.release_version.as_deref(), Some("0.24.5"));
        assert!(record.mutation.local_gates.iter().any(|gate| {
            gate.name == "version-preparation" && gate.state == SettlementState::Succeeded
        }));
        assert!(
            record
                .mutation
                .local_gates
                .iter()
                .any(|gate| gate.name == "workspace-verify")
        );
    }

    struct FakeRelease;

    impl ReleaseExecutionPort for FakeRelease {
        fn finish_closeout(
            &self,
            record: &ReleaseRecoveryRecord,
            _allow_mutation: bool,
            admission: &ReleaseMutationAdmission,
        ) -> Result<crate::release_closeout::CloseoutReceipt, RrcError> {
            require_kind(admission, ReleaseMutationKind::Closeout)?;
            let root = Path::new(record.mutation.source_workspace.as_deref().unwrap());
            crate::release_closeout::write_report(root, record, None)
        }
        fn prepare_version_bump(
            &self,
            _bump: &str,
            admission: &ReleaseMutationAdmission,
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
            admission: &ReleaseMutationAdmission,
        ) -> Result<String, RrcError> {
            require_kind(admission, ReleaseMutationKind::CommitCandidate)?;
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }
        fn push_candidate(&self, admission: &ReleaseMutationAdmission) -> Result<String, RrcError> {
            require_kind(admission, ReleaseMutationKind::PushCandidate)?;
            Ok("origin/main@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }
        fn create_and_push_tag(
            &self,
            version: &str,
            _commit: &str,
            admission: &ReleaseMutationAdmission,
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
            _expected_commit: &str,
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
                    run_state: Some(crate::release_recovery::JobState::Success),
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
            run_state: Some(crate::release_recovery::JobState::Success),
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
    fn publication_read_timeout_continues_same_candidate_without_another_tag() {
        struct TimeoutOnce(AtomicBool);
        impl ReleaseExecutionPort for TimeoutOnce {
            fn prepare_version_bump(
                &self,
                bump: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                FakeRelease.prepare_version_bump(bump, admission)
            }
            fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError> {
                FakeRelease.run_local_gate(gate)
            }
            fn commit_candidate(
                &self,
                version: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.commit_candidate(version, admission)
            }
            fn push_candidate(
                &self,
                admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.push_candidate(admission)
            }
            fn create_and_push_tag(
                &self,
                version: &str,
                commit: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                FakeRelease.create_and_push_tag(version, commit, admission)
            }
            fn publication(
                &self,
                repository: &str,
                tag: &str,
                commit: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                if !self.0.swap(true, Ordering::AcqRel) {
                    return Err(RrcError::WatchdogStalled {
                        operation: "publication evidence request".into(),
                        limit_seconds: 120,
                    });
                }
                FakeRelease.publication(repository, tag, commit)
            }
        }
        for provider in ["fixture-provider-a", "fixture-provider-b"] {
            let temp = tempfile::tempdir().unwrap();
            let ledger = ReleaseLedger::open(temp.path().to_path_buf(), provider).unwrap();
            let mut record =
                crate::release_recovery::start_release(provider, "patch", "main", &"1".repeat(40))
                    .unwrap();
            ledger.save(&record).unwrap();
            let executor = TimeoutOnce(AtomicBool::new(false));
            for _ in 0..16 {
                advance_release(
                    &mut record,
                    ReleaseAdvanceContext {
                        workspace: temp.path(),
                        repository: "fixture/repo",
                        ledger: &ledger,
                        executor: &executor,
                        github: &GreenGithub,
                        health: &HealthyStatus,
                        repair_factory: None,
                        cancelled: Arc::new(AtomicBool::new(false)),
                    },
                )
                .unwrap();
                record = ledger.load().unwrap().unwrap();
                if record.state == ReleaseRecoveryState::Published {
                    break;
                }
            }
            assert_eq!(record.state, ReleaseRecoveryState::Published);
            assert_eq!(record.mutation.publication_read_retries, 1);
            assert!(record.mutation.publication_verified);
            assert_eq!(
                record.release_commit.as_deref(),
                Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            );
            assert_eq!(
                record
                    .transitions
                    .iter()
                    .filter(|t| t.to == ReleaseRecoveryState::Tagging)
                    .count(),
                1
            );
            assert!(record.failures.is_empty());
            assert_eq!(record.retry_budget.full_gate_used, 0);
            assert_eq!(record.retry_budget.infrastructure_used, 0);
        }
    }

    #[test]
    fn publication_watch_budget_survives_reload_and_read_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "fixture").unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", &"1".repeat(40))
                .unwrap();
        ledger.save(&record).unwrap();
        for _ in 0..16 {
            if record.state == ReleaseRecoveryState::Publishing {
                break;
            }
            advance_release(
                &mut record,
                ReleaseAdvanceContext {
                    workspace: temp.path(),
                    repository: "fixture/repo",
                    ledger: &ledger,
                    executor: &FakeRelease,
                    github: &GreenGithub,
                    health: &HealthyStatus,
                    repair_factory: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
            )
            .unwrap();
        }
        record.mutation.publication_watch_millis = 60 * 60 * 1000;
        charge_publication_watch(&mut record, Duration::from_secs(60 * 60));
        let error = RrcError::WatchdogStalled {
            operation: "publication evidence request".into(),
            limit_seconds: 120,
        };
        publication_read_observation(&mut record, &ledger, Err(error)).unwrap();
        record = ledger.load().unwrap().unwrap();
        assert_eq!(record.mutation.publication_watch_millis, 2 * 60 * 60 * 1000);
        let outcome = advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "fixture/repo",
                ledger: &ledger,
                executor: &FakeRelease,
                github: &GreenGithub,
                health: &HealthyStatus,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        );
        assert!(
            matches!(outcome, Err(RrcError::WatchdogDeadline { operation, limit_seconds: 7200 }) if operation == "publication verification")
        );
        assert!(!record.mutation.publication_verified);
        assert!(record.failures.is_empty());
        assert_eq!(record.retry_budget.full_gate_used, 0);
    }

    #[test]
    fn publication_read_watchdog_recovery_is_persisted_and_bounded() {
        for provider in ["fixture-provider-a", "fixture-provider-b"] {
            let temp = tempfile::tempdir().unwrap();
            let ledger = ReleaseLedger::open(temp.path().to_path_buf(), provider).unwrap();
            let mut record =
                crate::release_recovery::start_release(provider, "patch", "main", &"a".repeat(40))
                    .unwrap();
            record.state = ReleaseRecoveryState::Publishing;
            record.mutation.tag_pushed = true;
            record.mutation.tag_object = Some("immutable-tag".into());
            let original = record.retry_budget.clone();
            ledger.save(&record).unwrap();
            for attempt in 1..=2 {
                let error = RrcError::WatchdogStalled {
                    operation: "publication evidence request".into(),
                    limit_seconds: 120,
                };
                assert!(
                    publication_read_observation(&mut record, &ledger, Err(error))
                        .unwrap()
                        .is_none()
                );
                record = ledger.load().unwrap().unwrap();
                assert_eq!(record.mutation.publication_read_retries, attempt);
                assert_eq!(record.retry_budget, original);
                assert_eq!(record.state, ReleaseRecoveryState::Publishing);
                assert!(record.failures.is_empty());
                assert_eq!(record.mutation.tag_object.as_deref(), Some("immutable-tag"));
            }
            let error = RrcError::WatchdogDeadline {
                operation: "publication evidence request".into(),
                limit_seconds: 600,
            };
            assert!(publication_read_observation(&mut record, &ledger, Err(error)).is_err());
            assert_eq!(record.mutation.publication_read_retries, 2);
            let receipt = PublicationReceipt {
                run_id: 77,
                version: "0.24.5".into(),
                assets: vec![],
            };
            assert_eq!(
                publication_read_observation(&mut record, &ledger, Ok(Some(receipt.clone())))
                    .unwrap(),
                Some(receipt)
            );
        }
    }

    #[test]
    fn publication_read_recovery_never_retries_cancellation_denials_or_invalid_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "fixture").unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", &"a".repeat(40))
                .unwrap();
        record.state = ReleaseRecoveryState::Publishing;
        for error in [
            RrcError::Cancelled,
            RrcError::AuthorizationBlocked("denied".into()),
            RrcError::Invalid("bad digest".into()),
            RrcError::WatchdogStalled {
                operation: "release mutation command".into(),
                limit_seconds: 300,
            },
        ] {
            assert!(publication_read_observation(&mut record, &ledger, Err(error)).is_err());
            assert_eq!(record.mutation.publication_read_retries, 0);
        }
        record.mutation.tag_pushed = true;
        for journal in [None, Some("Closeout".into())] {
            record.state = if journal.is_none() {
                ReleaseRecoveryState::RemoteGatesGreen
            } else {
                ReleaseRecoveryState::Publishing
            };
            record.mutation.in_flight_operation = journal;
            let error = RrcError::WatchdogStalled {
                operation: "publication evidence request".into(),
                limit_seconds: 120,
            };
            assert!(publication_read_observation(&mut record, &ledger, Err(error)).is_err());
            assert_eq!(record.mutation.publication_read_retries, 0);
        }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn native_version_resource_rejection_restores_all_bytes_before_deferral() {
        let root = version_fixture("0.24.4");
        let resources = tempfile::tempdir().unwrap();
        let plan = build_version_mutation_plan(root.path(), "patch").unwrap();
        let lock_before = fs::read(root.path().join("Cargo.lock")).unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy {
                disk_reserve_min_bytes: u64::MAX,
                ..ResourcePolicy::default()
            },
            resources.path().join("scheduler"),
            resources.path().join("target"),
        )
        .unwrap();
        let executor = NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false)))
            .unwrap()
            .with_resource_governor(governor);
        executor.checked("git", &["init"]).unwrap();
        executor.checked("git", &["add", "."]).unwrap();
        executor
            .checked(
                "git",
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "-m",
                    "fixture",
                ],
            )
            .unwrap();
        let record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        let error = executor
            .prepare_version_bump(
                "patch",
                &admit_release_mutation(&record, ReleaseMutationKind::VersionBump).unwrap(),
            )
            .unwrap_err();
        assert!(matches!(error, RrcError::ResourceConstrained(_)));
        assert!(error.to_string().contains("filesystem headroom"));
        for file in &plan.files {
            assert_eq!(
                fs::read(root.path().join(&file.relative_path)).unwrap(),
                file.before
            );
        }
        assert_eq!(
            fs::read(root.path().join("Cargo.lock")).unwrap(),
            lock_before
        );
        assert!(
            executor
                .checked("git", &["status", "--porcelain"])
                .unwrap()
                .trim()
                .is_empty()
        );
        assert!(!resources.path().join("target/debug").exists());
    }

    #[test]
    fn escaped_resource_error_preserves_reason_without_source_failure_or_journal_reset() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path(), "resource-error-fixture").unwrap();
        let mut record = crate::release_recovery::start_release(
            "resource-error-fixture",
            "patch",
            "main",
            "abcdef123456",
        )
        .unwrap();
        record.note_liveness_active("resource recovery watch");
        record.mutation.in_flight_operation = Some("LocalVerification".into());
        ledger.save(&record).unwrap();
        let budget = record.retry_budget.clone();
        persist_worker_failure(
            temp.path(),
            "resource-error-fixture",
            &RrcError::ResourceConstrained("fixture filesystem discovery failed".into()),
        );
        let saved = ledger.load().unwrap().unwrap();
        assert_eq!(
            saved.liveness.state,
            crate::release_recovery::ReleaseLivenessState::OwnerExited
        );
        assert!(
            saved
                .liveness
                .detail
                .contains("fixture filesystem discovery failed")
        );
        assert!(
            saved
                .progress
                .milestones
                .last()
                .unwrap()
                .summary
                .contains("fixture filesystem discovery failed")
        );
        assert!(saved.failures.is_empty());
        assert_eq!(saved.retry_budget, budget);
        assert_eq!(
            saved.mutation.in_flight_operation,
            record.mutation.in_flight_operation
        );
    }

    #[test]
    fn version_preparation_resource_defer_recovers_without_replaying_uncertain_mutation() {
        struct VersionDeferred<'a>(&'a std::sync::atomic::AtomicUsize);
        impl ReleaseExecutionPort for VersionDeferred<'_> {
            fn preview_release_version(&self, _: &str) -> Result<(String, String), RrcError> {
                Ok(("0.24.4".into(), "0.24.5".into()))
            }
            fn prepare_version_bump(
                &self,
                bump: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
                    Err(RrcError::ResourceConstrained(
                        "fixture version admission pressure".into(),
                    ))
                } else {
                    FakeRelease.prepare_version_bump(bump, admission)
                }
            }
            fn run_local_gate(&self, _gate: &LocalGateRecord) -> Result<String, RrcError> {
                Ok("gate passed".into())
            }
            fn commit_candidate(
                &self,
                version: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.commit_candidate(version, admission)
            }
            fn push_candidate(
                &self,
                admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.push_candidate(admission)
            }
            fn create_and_push_tag(
                &self,
                version: &str,
                commit: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                FakeRelease.create_and_push_tag(version, commit, admission)
            }
            fn publication(
                &self,
                repository: &str,
                tag: &str,
                _expected_commit: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                FakeRelease.publication(repository, tag, _expected_commit)
            }
        }
        struct UnusedHealth;
        impl ExternalHealthPort for UnusedHealth {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                unreachable!("resource deferral does not query external health")
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path(), "version-resource-fixture").unwrap();
        let mut record = crate::release_recovery::start_release(
            "version-resource-fixture",
            "0.24.5",
            "main",
            "abcdef123456",
        )
        .unwrap();
        record.mutation.in_flight_operation = Some("LocalVerification".into());
        ledger.save(&record).unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let executor = VersionDeferred(&calls);
        let budget = record.retry_budget.clone();
        let advance = |record: &mut ReleaseRecoveryRecord| {
            advance_release(
                record,
                ReleaseAdvanceContext {
                    workspace: temp.path(),
                    repository: "owner/repo",
                    ledger: &ledger,
                    executor: &executor,
                    github: &GreenGithub,
                    health: &UnusedHealth,
                    repair_factory: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
            )
        };
        advance(&mut record).expect("version admission pressure must remain controller-owned");
        assert_eq!(record.state, ReleaseRecoveryState::ResourceDeferred);
        assert_eq!(
            record.resource_deferred.as_ref().unwrap().gate_name,
            "version-preparation"
        );
        assert!(
            record
                .resource_deferred
                .as_ref()
                .unwrap()
                .telemetry
                .action
                .contains("fixture version admission pressure")
        );
        assert_eq!(
            record.mutation.local_gates[0].state,
            SettlementState::NotStarted
        );
        assert!(record.mutation.in_flight_operation.is_none());
        assert!(record.mutation.version_after.is_none());
        assert!(record.failures.is_empty());
        assert_eq!(record.retry_budget, budget);
        let mut record = ledger.load().unwrap().unwrap();
        let pressure = ResourceTelemetry {
            pressure: ResourcePressure::Pressure,
            ..ResourceTelemetry::default()
        };
        assert!(!apply_resource_watch_observation(&mut record, pressure, false).unwrap());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let normal = ResourceTelemetry {
            pressure: ResourcePressure::Normal,
            ..ResourceTelemetry::default()
        };
        for _ in 0..RESOURCE_WATCH_NORMAL_CONFIRMATIONS {
            apply_resource_watch_observation(&mut record, normal.clone(), true).unwrap();
        }
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        advance(&mut record).unwrap();
        assert_eq!(record.mutation.version_after.as_deref(), Some("0.24.5"));
        assert_eq!(
            record.mutation.local_gates[0].state,
            SettlementState::Succeeded
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert!(record.failures.is_empty());
        assert_eq!(record.retry_budget, budget);
    }

    #[test]
    fn resource_governor_defer_keeps_local_epoch_resumable_without_source_failure() {
        struct ResourceDeferred<'a>(&'a std::sync::atomic::AtomicUsize);
        impl ReleaseExecutionPort for ResourceDeferred<'_> {
            fn prepare_version_bump(
                &self,
                bump: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                FakeRelease.prepare_version_bump(bump, admission)
            }
            fn run_local_gate(&self, _gate: &LocalGateRecord) -> Result<String, RrcError> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Err(RrcError::ResourceConstrained(
                    "critical memory pressure in constrained acceptance fixture".into(),
                ))
            }
            fn commit_candidate(
                &self,
                version: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.commit_candidate(version, admission)
            }
            fn push_candidate(
                &self,
                admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                FakeRelease.push_candidate(admission)
            }
            fn create_and_push_tag(
                &self,
                version: &str,
                commit: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                FakeRelease.create_and_push_tag(version, commit, admission)
            }
            fn publication(
                &self,
                repository: &str,
                tag: &str,
                _expected_commit: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                FakeRelease.publication(repository, tag, _expected_commit)
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
        let gate_spawns = std::sync::atomic::AtomicUsize::new(0);
        let executor = ResourceDeferred(&gate_spawns);

        // Version preparation is a separate controller step. The following
        // local gate must defer, not classify a source failure.
        advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &executor,
                github: &GreenGithub,
                health: &Healthy,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect("version preparation");
        let budget_before = record.retry_budget.clone();
        let milestones_before_defer = record.progress.milestones.len();
        advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &executor,
                github: &GreenGithub,
                health: &Healthy,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect("constrained host must enter a typed deferred state");

        assert_eq!(record.state, ReleaseRecoveryState::ResourceDeferred);
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
        assert_eq!(record.retry_budget, budget_before);
        assert_eq!(
            record.progress.milestones.len(),
            milestones_before_defer + 2,
            "the gate-start event and one defer event must be recorded"
        );
        assert_eq!(
            record
                .progress
                .milestones
                .iter()
                .filter(|milestone| milestone.summary.contains("Release paused safely"))
                .count(),
            1,
            "resource admission must emit exactly one defer milestone"
        );
        assert!(record.resource_deferred.is_some());
        assert!(matches!(
            crate::release_recovery::next_directive(&record, 0),
            crate::release_recovery::ReleaseDirective::WatchResourcePressure { after }
                if after == RESOURCE_WATCH_INITIAL_INTERVAL
        ));
        assert_eq!(
            ledger
                .load()
                .expect("load deferred record")
                .expect("record")
                .state,
            ReleaseRecoveryState::ResourceDeferred
        );

        let defer_milestones = record.progress.milestones.len();
        let mut pressure = ResourceTelemetry {
            pressure: ResourcePressure::Pressure,
            pressure_reason: "fixture pressure remains".into(),
            ..ResourceTelemetry::default()
        };
        for _ in 0..4 {
            assert!(
                !apply_resource_watch_observation(&mut record, pressure.clone(), false)
                    .expect("unchanged pressure observation"),
                "unchanged pressure must stay deferred"
            );
        }
        assert_eq!(record.state, ReleaseRecoveryState::ResourceDeferred);
        assert_eq!(record.retry_budget, budget_before);
        assert_eq!(
            gate_spawns.load(Ordering::Relaxed),
            1,
            "unchanged pressure observations must not respawn the expensive gate"
        );
        assert_eq!(
            record.progress.milestones.len(),
            defer_milestones,
            "passive polling must not emit milestone spam"
        );
        assert_eq!(
            record
                .resource_deferred
                .as_ref()
                .expect("deferred detail")
                .next_check_seconds,
            RESOURCE_WATCH_BACKOFF_INTERVAL.as_secs()
        );

        pressure.pressure = ResourcePressure::Normal;
        pressure.pressure_reason = "fixture pressure cleared".into();
        for confirmation in 1..=RESOURCE_WATCH_NORMAL_CONFIRMATIONS {
            let recovered = apply_resource_watch_observation(&mut record, pressure.clone(), true)
                .expect("normal confirmation");
            assert_eq!(
                recovered,
                confirmation == RESOURCE_WATCH_NORMAL_CONFIRMATIONS,
                "only the final bounded normal confirmation may continue"
            );
        }
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        assert!(record.resource_deferred.is_none());
        assert_eq!(record.retry_budget, budget_before);
        assert_eq!(record.progress.milestones.len(), defer_milestones + 1);
        assert!(
            record
                .progress
                .milestones
                .last()
                .expect("recovery milestone")
                .summary
                .contains("Host capacity recovered")
        );
    }

    #[cfg(unix)]
    #[test]
    fn silent_local_command_is_stopped_by_state_specific_watchdog() {
        let temporary = tempfile::tempdir().unwrap();
        let executor =
            NativeReleaseExecutor::new(temporary.path(), Arc::new(AtomicBool::new(false))).unwrap();
        let error = executor
            .command_with_env_and_policy(
                "sh",
                &["-c", "sleep 5"],
                &[],
                CommandWatchdogPolicy {
                    operation: "local fixture",
                    inactivity_timeout: Duration::from_millis(150),
                    hard_deadline: None,
                },
            )
            .expect_err("silent local child must be stopped");

        assert!(matches!(error, RrcError::WatchdogStalled { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn silent_external_command_is_stopped_by_inactivity_watchdog() {
        let cancelled = AtomicBool::new(false);
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let error = run_bounded_external_command(
            &mut command,
            &cancelled,
            CommandWatchdogPolicy {
                operation: "silent fixture",
                inactivity_timeout: Duration::from_millis(150),
                hard_deadline: Some(Duration::from_secs(2)),
            },
        )
        .expect_err("silent child must not outlive its inactivity budget");

        assert!(matches!(error, RrcError::WatchdogStalled { .. }));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn output_progress_extends_inactivity_window() {
        let cancelled = AtomicBool::new(false);
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "i=0; while [ $i -lt 30 ]; do echo tick; i=$((i+1)); sleep 0.1; done",
        ]);
        // Total work exceeds one inactivity window; periodic output must extend it.
        // The scheduling margin belongs to this fixture, not production policy.
        let started = Instant::now();
        let output = run_bounded_external_command(
            &mut command,
            &cancelled,
            CommandWatchdogPolicy {
                operation: "progress fixture",
                inactivity_timeout: Duration::from_secs(2),
                hard_deadline: None,
            },
        )
        .expect("observable progress must keep the command alive");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 30);
        assert!(started.elapsed() > Duration::from_secs(2));
    }

    #[test]
    fn long_running_with_continuous_progress_has_no_absolute_deadline() {
        let state = command_liveness_state(
            false,
            Duration::from_secs(43 * 60 * 60),
            Duration::from_secs(1),
            None,
            LOCAL_GATE_WATCHDOG,
        );

        assert_eq!(state, CommandLivenessState::Active);
        assert!(LOCAL_GATE_WATCHDOG.hard_deadline.is_none());
    }

    #[test]
    fn quiet_but_alive_child_transition_resets_progress() {
        let state = command_liveness_state(
            false,
            Duration::from_secs(60),
            Duration::from_secs(60),
            Some(Duration::from_millis(1)),
            CommandWatchdogPolicy {
                operation: "quiet local fixture",
                inactivity_timeout: Duration::from_secs(30),
                hard_deadline: None,
            },
        );

        assert_eq!(state, CommandLivenessState::QuietButAlive);
    }

    #[test]
    fn tiny_rss_jitter_does_not_refresh_process_heartbeat() {
        let mut previous = ResourceTelemetry {
            process_count: 2,
            rustc_count: 1,
            process_tree_rss_bytes: 64 * 1024 * 1024,
            ..ResourceTelemetry::default()
        };
        let before = process_tree_heartbeat_observation(&previous);
        previous.process_tree_rss_bytes += 1;
        let after = process_tree_heartbeat_observation(&previous);
        assert_eq!(
            before, after,
            "one RSS byte must not be meaningful progress"
        );
        previous.process_tree_rss_bytes += 1024 * 1024 * 1024;
        assert_eq!(
            after,
            process_tree_heartbeat_observation(&previous),
            "RSS growth of any size remains telemetry, not progress"
        );

        let inactivity = Duration::from_secs(31 * 60);
        let process_inactive_for = if before != after {
            Duration::ZERO
        } else {
            inactivity
        };
        let state = command_liveness_state(
            false,
            inactivity,
            inactivity,
            Some(process_inactive_for),
            CommandWatchdogPolicy {
                operation: "RSS jitter fixture",
                inactivity_timeout: Duration::from_secs(30 * 60),
                hard_deadline: None,
            },
        );

        assert_eq!(state, CommandLivenessState::Stagnant);
    }

    #[test]
    fn material_process_tree_transition_supports_quiet_but_alive() {
        let previous = ResourceTelemetry {
            process_count: 2,
            rustc_count: 1,
            process_tree_rss_bytes: 64 * 1024 * 1024,
            ..ResourceTelemetry::default()
        };
        let mut child_started = previous.clone();
        child_started.process_count += 1;
        let mut rustc_started = previous.clone();
        rustc_started.rustc_count += 1;

        assert_ne!(
            process_tree_heartbeat_observation(&previous),
            process_tree_heartbeat_observation(&child_started)
        );
        assert_ne!(
            process_tree_heartbeat_observation(&previous),
            process_tree_heartbeat_observation(&rustc_started)
        );

        let inactivity = Duration::from_secs(31 * 60);
        let state = command_liveness_state(
            false,
            inactivity,
            inactivity,
            Some(Duration::ZERO),
            CommandWatchdogPolicy {
                operation: "process transition fixture",
                inactivity_timeout: Duration::from_secs(30 * 60),
                hard_deadline: None,
            },
        );

        assert_eq!(state, CommandLivenessState::QuietButAlive);
    }

    #[test]
    fn external_request_hard_deadline_is_operation_scoped() {
        let state = command_liveness_state(
            false,
            Duration::from_secs(6 * 60),
            Duration::ZERO,
            None,
            REMOTE_COMMAND_WATCHDOG,
        );

        assert_eq!(state, CommandLivenessState::TimedOut);
    }

    #[cfg(unix)]
    #[test]
    fn external_command_cancellation_preempts_watchdog_deadlines() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancelled);
        let trigger = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Release);
        });
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let error = run_bounded_external_command(
            &mut command,
            cancelled.as_ref(),
            CommandWatchdogPolicy {
                operation: "cancellation fixture",
                inactivity_timeout: Duration::from_secs(1),
                hard_deadline: Some(Duration::from_secs(2)),
            },
        )
        .expect_err("user cancellation must stop the owned process group");
        trigger.join().expect("cancellation trigger");

        assert!(matches!(error, RrcError::Cancelled));
    }

    #[test]
    fn repair_heartbeat_distinguishes_progress_stall_and_cancellation() {
        let heartbeat = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(5)));
        let progress = RepairHeartbeatProgress {
            heartbeat: Arc::clone(&heartbeat),
            downstream: None,
        };
        vesper_agent::AgentProgressPort::emit(
            &progress,
            vesper_agent::AgentProgressEvent::TurnStarted,
        );
        assert!(heartbeat.lock().unwrap().elapsed() < Duration::from_secs(1));

        assert!(repair_watchdog_error(false, Duration::ZERO, Duration::from_secs(10)).is_none());
        assert!(matches!(
            repair_watchdog_error(false, Duration::from_secs(10), Duration::from_secs(10)),
            Some(RrcError::WatchdogStalled { .. })
        ));
        assert!(matches!(
            repair_watchdog_error(true, Duration::ZERO, Duration::from_secs(10)),
            Some(RrcError::Cancelled)
        ));
    }

    #[test]
    fn repair_evidence_watchdog_ignores_streaming_and_repeated_tool_results() {
        let mut evidence = RepairEvidenceProgress::new();
        let mut call = repair_command_test_call("cargo test exact_regression -- --exact");
        evidence.observe(
            &call,
            &Ok(vesper_agent::ToolResult::new("1 passed; 0 failed; finished in 1.23s").unwrap()),
        );
        let last = evidence.last_evidence;
        for attempt in 0..6 {
            call.id = vesper_domain::ToolCallId::new(format!("different-call-{attempt}")).unwrap();
            evidence.observe(
                &call,
                &Ok(vesper_agent::ToolResult::new(format!(
                    "1 passed; 0 failed; finished in {attempt}.56s"
                ))
                .unwrap()),
            );
        }
        assert_eq!(evidence.last_evidence, last);
        assert!(matches!(
            evidence.watchdog_error(),
            Some(RrcError::WatchdogStalled { .. })
        ));
        let heartbeat = Arc::new(Mutex::new(Instant::now()));
        let progress = RepairHeartbeatProgress {
            heartbeat,
            downstream: None,
        };
        vesper_agent::AgentProgressPort::emit(
            &progress,
            vesper_agent::AgentProgressEvent::Status {
                text: "still thinking".into(),
            },
        );
        assert_eq!(evidence.last_evidence, last);
        evidence.observe(
            &call,
            &Ok(vesper_agent::ToolResult::new("2 passed; 0 failed").unwrap()),
        );
        assert_eq!(evidence.stagnant_actions, 0);
        assert!(evidence.watchdog_error().is_none());
    }

    #[test]
    fn repair_registry_stops_repeated_observations_for_each_provider_context() {
        for provider in ["fixture.alpha", "fixture.beta"] {
            let workspace = tempfile::tempdir().unwrap();
            fs::write(workspace.path().join("cause.rs"), "fn cause() {}\n").unwrap();
            let evidence = Arc::new(Mutex::new(RepairEvidenceProgress::new()));
            let registry = build_repair_tool_registry(
                None,
                true,
                Some(RepairCommandProgress {
                    heartbeat: Arc::new(Mutex::new(Instant::now())),
                    activity: None,
                    evidence: Some(Arc::clone(&evidence)),
                }),
            );
            let mut context = repair_command_test_context(workspace.path());
            context.provider_id = vesper_domain::ProviderId::new(provider).unwrap();
            let call = vesper_domain::ToolCall {
                id: vesper_domain::ToolCallId::new("read-cause").unwrap(),
                tool_id: vesper_domain::ToolId::new("read_file").unwrap(),
                arguments: serde_json::json!({"path": "cause.rs"}),
                extensions: Default::default(),
            };
            let runtime = tokio::runtime::Runtime::new().unwrap();
            for _ in 0..7 {
                runtime.block_on(registry.execute(&call, &context)).unwrap();
            }
            let error = runtime
                .block_on(registry.execute(&call, &context))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("six actions without new evidence")
            );
            assert_eq!(evidence.lock().unwrap().stagnant_actions, 6);
            assert_eq!(
                fs::read_to_string(workspace.path().join("cause.rs")).unwrap(),
                "fn cause() {}\n"
            );
        }
    }

    #[test]
    fn repair_dispatch_budget_survives_restart_without_a_focused_proof() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(root.path().to_path_buf(), "fixture").unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", &"a".repeat(40))
                .unwrap();
        let families = BTreeSet::from(["workflow:source-regression".into()]);
        reserve_repair_dispatch(&mut record, &families, &ledger).unwrap();
        let mut restored = ledger.load().unwrap().unwrap();
        assert!(restored.repair_attempts.is_empty());
        reserve_repair_dispatch(&mut restored, &families, &ledger).unwrap();
        let mut restored = ledger.load().unwrap().unwrap();
        let before = fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
            .map(|entry| fs::read(entry.path()).unwrap())
            .unwrap();
        assert!(matches!(
            reserve_repair_dispatch(&mut restored, &families, &ledger),
            Err(RrcError::RepairBudgetExhausted(_))
        ));
        assert!(restored.repair_attempts.is_empty());
        assert_eq!(restored.retry_budget.full_gate_used, 0);
        assert_eq!(
            restored
                .mutation
                .repair_admissions
                .values()
                .copied()
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            fs::read_dir(root.path())
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json"))
                .map(|entry| fs::read(entry.path()).unwrap())
                .unwrap(),
            before
        );
    }

    #[test]
    fn repair_evidence_deadline_and_native_verification_are_distinct() {
        let mut evidence = RepairEvidenceProgress::new();
        let inactive = crate::release_recovery::STAGNATION_TIME_LIMIT;
        assert!(matches!(
            evidence.watchdog_error_with_inactivity(inactive),
            Some(RrcError::WatchdogStalled { .. })
        ));
        evidence.verification_active = 1;
        assert!(evidence.watchdog_error_with_inactivity(inactive).is_none());
        evidence.stagnant_actions = crate::release_recovery::STAGNATION_ACTION_LIMIT;
        assert!(evidence.watchdog_error().is_some());
        let shared = Arc::new(Mutex::new(evidence));
        drop(RepairVerificationActivity(Some(Arc::clone(&shared))));
        assert_eq!(shared.lock().unwrap().verification_active, 0);
    }

    #[test]
    fn version_preparation_rollback_routes_to_repair_and_pins_promoted_patch() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir(root.path().join("registry")).unwrap();
        fs::write(root.path().join("Cargo.toml"), "[workspace]\nmembers=[\".\"]\n[workspace.package]\nversion = \"0.24.4\"\n[package]\nname=\"preparation-repair-fixture\"\nversion.workspace=true\nedition=\"2024\"\n").unwrap();
        fs::write(
            root.path().join("registry/agent.json"),
            "{\"version\":\"0.24.4\"}",
        )
        .unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn answer() -> u32 { undefined_answer }\n",
        )
        .unwrap();
        let resources = tempfile::tempdir().unwrap();
        let governor = HostResourceGovernor::new(
            ResourcePolicy {
                admission_pressure_enabled: false,
                ..ResourcePolicy::default()
            },
            resources.path().join("scheduler"),
            resources.path().join("target"),
        )
        .unwrap();
        let native = NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false)))
            .unwrap()
            .with_resource_governor(governor);
        native
            .checked("cargo", &["generate-lockfile", "--offline"])
            .unwrap();
        native.checked("git", &["init", "-b", "main"]).unwrap();
        native
            .checked("git", &["config", "user.name", "fixture"])
            .unwrap();
        native
            .checked("git", &["config", "user.email", "fixture@example.invalid"])
            .unwrap();
        native.checked("git", &["add", "--all"]).unwrap();
        native
            .checked("git", &["commit", "-m", "broken source fixture"])
            .unwrap();
        let base = native
            .checked("git", &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_owned();
        let manifest = fs::read(root.path().join("Cargo.toml")).unwrap();
        let lock = fs::read(root.path().join("Cargo.lock")).unwrap();
        let mut record =
            crate::release_recovery::start_release("preparation", "patch", "main", &base).unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.in_flight_operation = Some("LocalVerification".into());
        let ledger = ReleaseLedger::open(resources.path().join("ledger"), "preparation").unwrap();
        ledger.save(&record).unwrap();
        let error = advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: root.path(),
                repository: "fixture/repo",
                ledger: &ledger,
                executor: &native,
                github: &GreenGithub,
                health: &HealthyStatus,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .unwrap_err();
        assert!(
            matches!(error, RrcError::VersionPreparationFailed(_)),
            "{error:?}"
        );
        assert_eq!(record.state, ReleaseRecoveryState::DiagnosingLocalFailure);
        assert_eq!(record.failures.len(), 1);
        assert_eq!(
            record.failures[0].class,
            crate::release_recovery::ReleaseFailureClass::CompileFailure
        );
        assert!(
            record.failures[0]
                .causal_excerpt
                .contains("undefined_answer")
        );
        assert!(record.mutation.in_flight_operation.is_none());
        assert_eq!(fs::read(root.path().join("Cargo.toml")).unwrap(), manifest);
        assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lock);
        assert!(
            native
                .checked("git", &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );

        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();
        native.checked("git", &["add", "src/lib.rs"]).unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.repair_files = vec!["src/lib.rs".into()];
        let patch = native
            .command("git", &["diff", "--binary", "HEAD"])
            .unwrap();
        record
            .repair_attempts
            .push(crate::release_recovery::RepairAttempt {
                fingerprint: record.failures[0].fingerprint.clone(),
                causal_family: "fixture".into(),
                hypothesis: "replace undefined answer".into(),
                source_commit_before: base,
                source_commit_after: None,
                focused_proof: "cargo check --workspace --all-targets".into(),
                focused_status: crate::release_recovery::FocusedProofStatus::Passed,
                evidence_refs: vec![format!(
                    "repair:preparation-baseline:{}",
                    digest(&patch.stdout)
                )],
                disproven_or_insufficient: false,
            });
        let admission = admit_release_mutation(&record, ReleaseMutationKind::VersionBump).unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn answer() -> u32 { 43 }\n",
        )
        .unwrap();
        assert!(native.prepare_version_bump("patch", &admission).is_err());
        assert_eq!(fs::read(root.path().join("Cargo.toml")).unwrap(), manifest);
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();
        let receipt = native.prepare_version_bump("patch", &admission).unwrap();
        assert_eq!(receipt.after, "0.24.5");
        assert_eq!(
            fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
            "pub fn answer() -> u32 { 42 }\n"
        );
        assert!(record.release_commit.is_none() || !record.mutation.candidate_pushed);
    }

    #[test]
    fn only_newly_classified_local_gate_errors_continue_to_repair() {
        assert!(
            settle_release_step_outcome(
                ReleaseRecoveryState::LocalVerification,
                ReleaseRecoveryState::DiagnosingLocalFailure,
                Err(RrcError::Invalid("fixture gate has causal evidence".into())),
            )
            .is_ok()
        );
        assert!(
            settle_release_step_outcome(
                ReleaseRecoveryState::LocalVerification,
                ReleaseRecoveryState::DiagnosingLocalFailure,
                Err(RrcError::AuthorizationBlocked("fixture denial".into())),
            )
            .is_err()
        );
        for state in [
            ReleaseRecoveryState::DiagnosingLocalFailure,
            ReleaseRecoveryState::DiagnosingRepair,
        ] {
            assert!(
                settle_release_step_outcome(
                    state,
                    state,
                    Err(RrcError::Invalid("fixture repair failed".into())),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn local_repair_dispatch_errors_are_retained_as_liveness_failures() {
        let temporary = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temporary.path(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        record.state = ReleaseRecoveryState::DiagnosingLocalFailure;
        record
            .mutation
            .repair_admissions
            .insert("fixture-family".into(), 1);
        ledger.save(&record).unwrap();
        let error = settle_release_step_outcome(
            record.state,
            record.state,
            Err(RrcError::Invalid("fixture repair dispatch refused".into())),
        )
        .expect_err("a repair error must reach worker failure settlement");
        persist_worker_failure(temporary.path(), "repo", &error);
        let persisted = ledger.load().unwrap().unwrap();
        assert_eq!(persisted.state, record.state);
        assert_eq!(
            persisted.liveness.state,
            crate::release_recovery::ReleaseLivenessState::Failed
        );
        assert!(
            persisted
                .liveness
                .detail
                .contains("fixture repair dispatch refused")
        );
        assert_eq!(
            persisted.mutation.repair_admissions,
            record.mutation.repair_admissions
        );
        assert!(persisted.failures.is_empty());
        assert!(!persisted.mutation.candidate_pushed);
    }

    #[test]
    fn genuine_local_gate_failure_enters_diagnosing_local_failure() {
        struct FailingRelease;
        impl ReleaseExecutionPort for FailingRelease {
            fn prepare_version_bump(
                &self,
                _bump: &str,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                unreachable!("fixture starts after version preparation")
            }

            fn run_local_gate(&self, _gate: &LocalGateRecord) -> Result<String, RrcError> {
                Err(RrcError::Invalid(
                    "cargo xtask verify failed with status exit status: 1: compiler error".into(),
                ))
            }

            fn commit_candidate(
                &self,
                _version: &str,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                unreachable!("failed local gate must prevent candidate commit")
            }

            fn push_candidate(
                &self,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                unreachable!("failed local gate must prevent candidate push")
            }

            fn create_and_push_tag(
                &self,
                _version: &str,
                _commit: &str,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                unreachable!("failed local gate must prevent tagging")
            }

            fn publication(
                &self,
                _repository: &str,
                _tag: &str,
                _expected_commit: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                unreachable!("failed local gate must prevent publication")
            }
        }

        struct UnusedHealth;
        impl ExternalHealthPort for UnusedHealth {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                unreachable!("local verification must not query external health")
            }
        }

        let temporary = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temporary.path(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.version_after = Some("0.24.5".into());
        record.mutation.local_gates = vec![LocalGateRecord {
            name: "fixture".into(),
            state: SettlementState::NotStarted,
            command: "cargo xtask verify".into(),
            evidence_ref: None,
        }];

        let error = advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temporary.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &FailingRelease,
                github: &GreenGithub,
                health: &UnusedHealth,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect_err("a nonzero local gate must remain a source failure");

        assert!(matches!(error, RrcError::Invalid(_)));
        assert_eq!(record.state, ReleaseRecoveryState::DiagnosingLocalFailure);
        assert_eq!(
            record.mutation.local_gates[0].state,
            SettlementState::Failed
        );
        assert_eq!(
            record.mutation.local_gates[0].evidence_ref.as_deref(),
            Some("local:failed")
        );
        assert_eq!(
            record.transitions.last().unwrap().evidence_refs,
            vec!["local:fixture"]
        );

        let persisted = ledger.load().unwrap().expect("persisted source failure");
        assert_eq!(
            persisted.state,
            ReleaseRecoveryState::DiagnosingLocalFailure
        );
        assert_eq!(
            persisted.mutation.local_gates[0].evidence_ref.as_deref(),
            Some("local:failed")
        );
    }

    #[test]
    fn local_gate_watchdog_stop_is_not_persisted_as_source_failure() {
        struct WatchdogRelease;
        impl ReleaseExecutionPort for WatchdogRelease {
            fn prepare_version_bump(
                &self,
                _bump: &str,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                unreachable!("fixture starts after version preparation")
            }

            fn run_local_gate(&self, _gate: &LocalGateRecord) -> Result<String, RrcError> {
                Err(RrcError::WatchdogStalled {
                    operation: "local verification command".into(),
                    limit_seconds: 1,
                })
            }

            fn commit_candidate(
                &self,
                _version: &str,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                unreachable!("watchdog stop must prevent candidate commit")
            }

            fn push_candidate(
                &self,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                unreachable!("watchdog stop must prevent candidate push")
            }

            fn create_and_push_tag(
                &self,
                _version: &str,
                _commit: &str,
                _admission: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                unreachable!("watchdog stop must prevent tagging")
            }

            fn publication(
                &self,
                _repository: &str,
                _tag: &str,
                _expected_commit: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                unreachable!("watchdog stop must prevent publication")
            }
        }

        struct UnusedHealth;
        impl ExternalHealthPort for UnusedHealth {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                unreachable!("local verification must not query external health")
            }
        }

        let temporary = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temporary.path(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.version_after = Some("0.24.5".into());
        record.mutation.local_gates = vec![LocalGateRecord {
            name: "fixture".into(),
            state: SettlementState::NotStarted,
            command: "fixture".into(),
            evidence_ref: None,
        }];

        let error = advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace: temporary.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &WatchdogRelease,
                github: &GreenGithub,
                health: &UnusedHealth,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect_err("watchdog stop must leave the local gate unsettled");

        assert!(matches!(error, RrcError::WatchdogStalled { .. }));
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            record.mutation.local_gates[0].state,
            SettlementState::Running
        );
        assert!(record.mutation.local_gates[0].evidence_ref.is_none());
        assert!(record.failures.is_empty());

        persist_worker_failure(temporary.path(), "repo", &error);
        let persisted = ledger.load().unwrap().expect("persisted watchdog stop");
        assert_eq!(persisted.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            persisted.mutation.local_gates[0].state,
            SettlementState::Running
        );
        assert!(persisted.mutation.local_gates[0].evidence_ref.is_none());
        assert_eq!(
            persisted.liveness.state,
            crate::release_recovery::ReleaseLivenessState::Stalled
        );
    }

    #[test]
    fn host_authorization_preserves_denial_reason_for_two_provider_fixtures() {
        struct DenyPermission;
        impl vesper_agent::PermissionPort for DenyPermission {
            fn authorize<'a>(
                &'a self,
                _: &'a vesper_domain::ToolCall,
                _: &'a vesper_domain::ToolDefinition,
                _: &'a vesper_agent::ToolContext,
            ) -> vesper_agent::ToolFuture<'a, vesper_agent::PermissionDecision> {
                Box::pin(async {
                    vesper_agent::PermissionDecision::Deny("ACP client rejected permission".into())
                })
            }
        }
        for label in ["fixture.authorization-a", "fixture.authorization-b"] {
            let (factory, _, _runtime) = repair_test_factory(label, "unused");
            let factory = factory
                .with_permission_port(Arc::new(DenyPermission))
                .with_release_policy(
                    vesper_domain::SessionOperatingMode::Code,
                    vesper_domain::SessionPermissionMode::Ask,
                );
            let record =
                crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                    .unwrap();
            let error = authorize_controller_step(
                Some(&factory),
                &record,
                "owner/repo",
                &AtomicBool::new(false),
            )
            .unwrap_err();
            assert!(matches!(&error, RrcError::AuthorizationBlocked(reason)
                if reason == "ACP client rejected permission"));
            assert!(controller_stop_is_not_source_failure(&error));
        }
    }

    #[test]
    fn authorization_stop_preserves_checkpoint_without_source_failure() {
        for reason in [
            "approval timed out",
            "ACP client rejected permission",
            "command firewall denied release stage",
            "release mutation requires a host permission port",
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let ledger = ReleaseLedger::open(temporary.path(), "repo").unwrap();
            let mut record =
                crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                    .unwrap();
            record.state = ReleaseRecoveryState::LocalVerification;
            record.mutation.local_gates = vec![LocalGateRecord {
                name: "fixture".into(),
                state: SettlementState::NotStarted,
                command: "fixture".into(),
                evidence_ref: None,
            }];
            ledger.save(&record).unwrap();
            let error = RrcError::AuthorizationBlocked(reason.into());
            assert!(controller_stop_is_not_source_failure(&error));
            persist_worker_failure(temporary.path(), "repo", &error);
            let persisted = ledger.load().unwrap().unwrap();
            assert_eq!(persisted.state, record.state);
            assert_eq!(persisted.mutation, record.mutation);
            assert_eq!(persisted.retry_budget, record.retry_budget);
            assert!(persisted.failures.is_empty());
            assert!(persisted.repair_attempts.is_empty());
            assert_eq!(
                persisted.liveness.state,
                crate::release_recovery::ReleaseLivenessState::Failed
            );
            assert!(persisted.liveness.detail.contains(reason));
        }
    }

    #[test]
    fn cancellation_does_not_create_local_failure_evidence() {
        let temporary = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temporary.path(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", "abcdef123456")
                .unwrap();
        record.state = ReleaseRecoveryState::LocalVerification;
        record.mutation.local_gates = vec![LocalGateRecord {
            name: "fixture".into(),
            state: SettlementState::Running,
            command: "fixture".into(),
            evidence_ref: None,
        }];
        ledger.save(&record).unwrap();

        persist_worker_failure(temporary.path(), "repo", &RrcError::Cancelled);

        let persisted = ledger.load().unwrap().expect("preserved checkpoint");
        assert_eq!(persisted.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(
            persisted.mutation.local_gates[0].state,
            SettlementState::Running
        );
        assert!(persisted.mutation.local_gates[0].evidence_ref.is_none());
        assert!(persisted.failures.is_empty());
    }

    pub(crate) fn repair_test_factory(
        label: &str,
        repaired: &str,
    ) -> (
        crate::WorkerFactory,
        vesper_testkit::FakeProviderSession,
        tokio::runtime::Runtime,
    ) {
        repair_test_factory_with_command(
            label,
            repaired,
            "cargo test exact_regression --offline -- --exact",
        )
    }

    fn repair_test_factory_with_command(
        label: &str,
        repaired: &str,
        command: &str,
    ) -> (
        crate::WorkerFactory,
        vesper_testkit::FakeProviderSession,
        tokio::runtime::Runtime,
    ) {
        repair_test_factory_with_commands(label, repaired, &[command])
    }

    fn repair_test_factory_with_commands(
        label: &str,
        repaired: &str,
        commands: &[&str],
    ) -> (
        crate::WorkerFactory,
        vesper_testkit::FakeProviderSession,
        tokio::runtime::Runtime,
    ) {
        repair_test_factory_with_prefix(label, repaired, commands, Vec::new())
    }

    fn repair_test_factory_with_prefix(
        label: &str,
        repaired: &str,
        commands: &[&str],
        prefix: Vec<vesper_testkit::ScriptedProviderResponse>,
    ) -> (
        crate::WorkerFactory,
        vesper_testkit::FakeProviderSession,
        tokio::runtime::Runtime,
    ) {
        use vesper_domain::{
            BoundedString, ContentPart, ContentText, ExtensionMap, FinishOutcome, ProviderId,
            ToolCall, ToolCallId, ToolId,
        };
        use vesper_provider::{ProviderFactory, ProviderStreamEvent};
        #[derive(Clone)]
        struct Fixture {
            id: ProviderId,
            session: vesper_testkit::FakeProviderSession,
        }
        impl ProviderFactory for Fixture {
            type Session = vesper_testkit::FakeProviderSession;
            fn provider_id(&self) -> &ProviderId {
                &self.id
            }
            fn create_session<'a>(
                &'a self,
                _: &'a vesper_provider::ProviderConfiguration,
                _: Arc<dyn vesper_agent::CancellationSignal>,
            ) -> vesper_provider::ProviderFuture<
                'a,
                Result<Self::Session, vesper_provider::ProviderError>,
            > {
                Box::pin(async move { Ok(self.session.clone()) })
            }
        }
        let script = |name: &str, arguments, ordinal| {
            vec![
                Ok(ProviderStreamEvent::ToolCallCompleted(ToolCall {
                    id: ToolCallId::new(format!("fixture-{name}-{ordinal}")).unwrap(),
                    tool_id: ToolId::new(name).unwrap(),
                    arguments,
                    extensions: ExtensionMap::default(),
                })),
                Ok(ProviderStreamEvent::Completed {
                    finish: FinishOutcome::ToolCalls,
                    metadata: ExtensionMap::default(),
                }),
            ]
        };
        let mut scripts = vec![Ok(script(
            "write_file",
            serde_json::json!({"path":"src/lib.rs", "content":repaired}),
            0,
        ))];
        scripts.extend(commands.iter().enumerate().map(|(index, command)| {
            Ok(script(
                if command.starts_with("fixture-read:") {
                    "read_file"
                } else {
                    "run_command"
                },
                if let Some(path) = command.strip_prefix("fixture-read:") {
                    serde_json::json!({"path":path})
                } else {
                    serde_json::json!({"command":command})
                },
                index + 1,
            ))
        }));
        scripts.push(Ok(vec![
            Ok(ProviderStreamEvent::ContentDelta {
                stream_id: BoundedString::new("text").unwrap(),
                part: ContentPart::Text(
                    ContentText::new(
                        "Hypothesis: return the required answer; exact regression now passes.",
                    )
                    .unwrap(),
                ),
            }),
            Ok(ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                metadata: ExtensionMap::default(),
            }),
        ]));
        let session =
            vesper_testkit::FakeProviderSession::with_scripts(prefix.into_iter().chain(scripts));
        let id = ProviderId::new(label).unwrap();
        let registry = Arc::new(vesper_runtime::ProviderRegistry::new());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(registry.register(Fixture {
                id: id.clone(),
                session: session.clone(),
            }))
            .unwrap();
        let configuration = vesper_agent::AgentLoopConfig {
            provider_id: id.clone(),
            provider_configuration: vesper_provider::ProviderConfiguration {
                provider_id: id.clone(),
                values: vesper_domain::VersionedExtensionEnvelope {
                    namespace: vesper_domain::ExtensionNamespace::new("provider.fixture").unwrap(),
                    version: vesper_domain::SchemaVersion::new(1).unwrap(),
                    values: ExtensionMap::default(),
                },
            },
            model: vesper_domain::QualifiedModelId {
                provider_id: id,
                model_id: vesper_domain::ModelId::new("fixture").unwrap(),
            },
            context_window_tokens: 128000,
            native_compaction: vesper_agent::NativeCompactionPolicy::Disabled,
            hosted_tools: Vec::new(),
            system_instructions: Vec::new(),
            workspace_roots: Vec::new(),
            max_tool_iterations: 24,
            firewall: None,
            sandbox: None,
        };
        let factory = crate::WorkerFactory::new(registry, configuration).with_release_policy(
            vesper_domain::SessionOperatingMode::Code,
            vesper_domain::SessionPermissionMode::Bypass,
        );
        (factory, session, runtime)
    }

    #[test]
    fn repair_preserves_provider_owned_hosted_tool_selections() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("Cargo.toml"),
            "[package]\nname = \"release-hosted-selection-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
        let (mut factory, session, runtime) = repair_test_factory(
            "fixture.release-hosted",
            "#[test] fn exact_regression() {}\n",
        );
        let selected = vec![vesper_provider::HostedToolSelection {
            tool_id: vesper_domain::BoundedString::new("fixture.remote-search").unwrap(),
            configuration: None,
        }];
        factory.config.hosted_tools = selected.clone();
        let (outcome, _) = runtime
            .block_on(factory.run_coding_turn_in_workspace(
                root.path().to_path_buf(),
                "Repair with the already-selected provider tools".into(),
                Arc::new(vesper_runtime::RuntimeCancellation::new()),
            ))
            .unwrap();
        assert!(outcome.is_success());
        assert_eq!(session.requests().len(), 3);
        for request in session.requests() {
            assert_eq!(
                request.hosted_tools, selected,
                "provider-owned tool selections must not be confused with client registry gateways"
            );
        }
        assert_eq!(factory.config.hosted_tools, selected);
    }

    #[test]
    fn repair_iteration_budget_survives_disabled_host_cap() {
        for (host_cap, expected_cap) in [(0, 96), (5, 20), (100, 96)] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("src")).unwrap();
            fs::write(root.path().join("Cargo.toml"),
                "[package]\nname = \"release-iteration-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
            let commands = (0..100)
                .map(|index| {
                    let path = format!("part-{index}");
                    fs::write(
                        root.path().join(&path),
                        format!("distinct evidence {index}"),
                    )
                    .unwrap();
                    format!("fixture-read:{path}")
                })
                .collect::<Vec<_>>();
            let command_refs = commands.iter().map(String::as_str).collect::<Vec<_>>();
            let (mut factory, session, runtime) = repair_test_factory_with_commands(
                "fixture.release-iterations",
                "pub fn answer() {}\n",
                &command_refs,
            );
            factory.config.max_tool_iterations = host_cap;
            let (outcome, _) = runtime
                .block_on(factory.run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Execute bounded focused verification".into(),
                    Arc::new(vesper_runtime::RuntimeCancellation::new()),
                ))
                .unwrap();
            assert_eq!(
                session.requests().len(),
                expected_cap,
                "host cap {host_cap} must not weaken the repair budget"
            );
            assert!(
                !outcome.is_success(),
                "iteration exhaustion cannot become successful repair"
            );
            assert_eq!(
                factory.config.max_tool_iterations, host_cap,
                "ordinary host setting stays intact"
            );
        }
    }

    #[test]
    fn repair_continues_unplanned_segments_for_two_provider_fixtures() {
        for label in ["fixture.segment-a", "fixture.segment-b"] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("src")).unwrap();
            let commands = (0..30)
                .map(|index| {
                    let path = format!("part-{index}");
                    fs::write(
                        root.path().join(&path),
                        format!("distinct evidence {index}"),
                    )
                    .unwrap();
                    format!("fixture-read:{path}")
                })
                .collect::<Vec<_>>();
            let refs = commands.iter().map(String::as_str).collect::<Vec<_>>();
            let (factory, session, runtime) =
                repair_test_factory_with_commands(label, "pub fn answer() {}\n", &refs);
            let (outcome, history) = runtime
                .block_on(factory.run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Complete the isolated repair".into(),
                    Arc::new(vesper_runtime::RuntimeCancellation::new()),
                ))
                .unwrap();
            assert!(matches!(
                outcome,
                vesper_agent::AgentTurnOutcome::Completed { iterations: 32, .. }
            ));
            assert_eq!(session.requests().len(), 32);
            assert!(history.iter().any(|message| message.content.iter().any(|part|
                matches!(part, ContentPart::ToolCall(call) if call.id.as_str() == "fixture-read_file-30"))));
            assert_eq!(
                fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
                "pub fn answer() {}\n"
            );
        }
    }

    #[test]
    fn repair_initial_provider_retry_is_bounded_and_never_replays_tools() {
        fn failure(
            label: &str,
            retry: vesper_domain::Retryability,
            delay: Option<u64>,
        ) -> vesper_testkit::ScriptedProviderResponse {
            Err(Box::new(vesper_provider::ProviderError {
                provider_id: vesper_domain::ProviderId::new(label).unwrap(),
                provider_code: None,
                http_status: Some(503),
                continuation_possible: false,
                info: vesper_domain::ErrorInfo {
                    category: vesper_domain::ErrorCategory::Transport,
                    retryability: retry,
                    retry_after_ms: delay,
                    visible_output_emitted: false,
                    safe_message: vesper_domain::SafeMessage::new(
                        "service temporarily unavailable",
                    )
                    .unwrap(),
                    diagnostics: Default::default(),
                    provider_code: None,
                    causes: Vec::new(),
                },
                metadata: Default::default(),
            }))
        }
        for label in ["fixture.retry-a", "fixture.retry-b"] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("src")).unwrap();
            let (factory, session, runtime) = repair_test_factory_with_prefix(
                label,
                "pub fn answer() {}\n",
                &[],
                vec![failure(
                    label,
                    vesper_domain::Retryability::BeforeVisibleOutput,
                    None,
                )],
            );
            let (outcome, _) = runtime
                .block_on(factory.run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Complete repair".into(),
                    Arc::new(vesper_runtime::RuntimeCancellation::new()),
                ))
                .unwrap();
            assert!(outcome.is_success());
            assert_eq!(session.requests().len(), 3);
            assert_eq!(
                session.requests()[0].messages,
                session.requests()[1].messages
            );
            assert!(root.path().join("src/lib.rs").exists());
        }
        for (retry, delay, failures, expected) in [
            (vesper_domain::Retryability::Never, None, 1, 1),
            (
                vesper_domain::Retryability::BeforeVisibleOutput,
                Some(30_001),
                1,
                1,
            ),
            (vesper_domain::Retryability::BeforeVisibleOutput, None, 2, 2),
        ] {
            let root = tempfile::tempdir().unwrap();
            let label = "fixture.retry-refusal";
            let prefix = (0..failures)
                .map(|_| failure(label, retry, delay))
                .collect();
            let (factory, session, runtime) =
                repair_test_factory_with_prefix(label, "", &[], prefix);
            let error = runtime
                .block_on(factory.run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Complete repair".into(),
                    Arc::new(vesper_runtime::RuntimeCancellation::new()),
                ))
                .unwrap_err();
            assert_eq!(session.requests().len(), expected);
            assert!(error.contains("HTTP=503"));
            assert!(!root.path().join("src/lib.rs").exists());
        }
        let root = tempfile::tempdir().unwrap();
        let label = "fixture.retry-ambiguous";
        let error = match failure(
            label,
            vesper_domain::Retryability::BeforeVisibleOutput,
            None,
        ) {
            Err(error) => *error,
            Ok(_) => unreachable!(),
        };
        let prefix = vec![Ok(vec![
            Ok(vesper_provider::ProviderStreamEvent::ToolCallStarted {
                index: 0,
                call_id: None,
                name: None,
            }),
            Err(error),
        ])];
        let (factory, session, runtime) = repair_test_factory_with_prefix(label, "", &[], prefix);
        let (outcome, _) = runtime
            .block_on(factory.run_coding_turn_in_workspace(
                root.path().to_path_buf(),
                "Complete repair".into(),
                Arc::new(vesper_runtime::RuntimeCancellation::new()),
            ))
            .unwrap();
        assert!(matches!(
            outcome,
            vesper_agent::AgentTurnOutcome::Interrupted {
                tool_call_started: true,
                ..
            }
        ));
        assert_eq!(session.requests().len(), 1);

        let root = tempfile::tempdir().unwrap();
        let label = "fixture.retry-cancel";
        let (factory, session, runtime) = repair_test_factory_with_prefix(
            label,
            "",
            &[],
            vec![failure(
                label,
                vesper_domain::Retryability::BeforeVisibleOutput,
                None,
            )],
        );
        let cancellation = Arc::new(vesper_runtime::RuntimeCancellation::new());
        runtime.block_on(async {
            let signal = cancellation.clone();
            let canceller = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                signal.cancel();
            });
            let result = factory
                .run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Complete repair".into(),
                    cancellation,
                )
                .await;
            assert!(
                result
                    .unwrap_err()
                    .contains("cancelled before initial-request retry")
            );
            canceller.await.unwrap();
        });
        assert_eq!(session.requests().len(), 1);

        // A transient rejection after the successful write must not restart it.
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        let label = "fixture.retry-after-write";
        let initial = Ok(vec![
            Ok(vesper_provider::ProviderStreamEvent::ToolCallCompleted(
                vesper_domain::ToolCall {
                    id: vesper_domain::ToolCallId::new("write-once").unwrap(),
                    tool_id: vesper_domain::ToolId::new("write_file").unwrap(),
                    arguments: serde_json::json!({"path":"src/lib.rs","content":"preserved action"}),
                    extensions: Default::default(),
                },
            )),
            Ok(vesper_provider::ProviderStreamEvent::Completed {
                finish: vesper_domain::FinishOutcome::ToolCalls,
                metadata: Default::default(),
            }),
        ]);
        let (factory, session, runtime) = repair_test_factory_with_prefix(
            label,
            "overwritten",
            &[],
            vec![
                initial,
                failure(
                    label,
                    vesper_domain::Retryability::BeforeVisibleOutput,
                    None,
                ),
            ],
        );
        assert!(
            runtime
                .block_on(factory.run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Complete repair".into(),
                    Arc::new(vesper_runtime::RuntimeCancellation::new())
                ))
                .is_err()
        );
        assert_eq!(session.requests().len(), 2);
        assert_eq!(
            fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
            "preserved action"
        );
    }

    #[test]
    fn repair_terminal_diagnostics_identify_the_safety_stop() {
        let outcome = vesper_agent::AgentTurnOutcome::MaxIterationsReached {
            iterations: 96,
            plan: None,
        };
        let text = repair_terminal_diagnostic(&outcome);
        assert!(text.contains("96 turns"));
        assert!(text.contains("unfinished_plan=false"));
    }

    #[test]
    fn repair_worker_cannot_create_unadmitted_release_tags() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/lib.rs"), "pub fn answer() {}\n").unwrap();
        let native =
            NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
        native.checked("git", &["init", "--quiet"]).unwrap();
        native
            .checked("git", &["config", "user.name", "RRC Fixture"])
            .unwrap();
        native
            .checked(
                "git",
                &["config", "user.email", "rrc-fixture@example.invalid"],
            )
            .unwrap();
        native.checked("git", &["add", "src/lib.rs"]).unwrap();
        native
            .checked("git", &["commit", "--quiet", "-m", "fixture"])
            .unwrap();
        let (factory, _, runtime) = repair_test_factory_with_command(
            "fixture.release-authority",
            "pub fn answer() {}\n",
            "git tag rrc_unadmitted_fixture",
        );
        runtime
            .block_on(factory.run_coding_turn_in_workspace(
                root.path().to_path_buf(),
                "Repair source without publishing".into(),
                Arc::new(vesper_runtime::RuntimeCancellation::new()),
            ))
            .unwrap();
        assert!(
            !native
                .command(
                    "git",
                    &["rev-parse", "--verify", "refs/tags/rrc_unadmitted_fixture"]
                )
                .unwrap()
                .status
                .success(),
            "repair command bypassed native tag admission"
        );
        let registry = repair_tool_registry();
        let context = vesper_agent::ToolContext {
            workspace_roots: vec![vesper_domain::WorkspaceRoot {
                name: vesper_domain::BoundedString::new("repair-fixture").unwrap(),
                path: vesper_domain::BoundedString::new(root.path().display().to_string()).unwrap(),
                primary: true,
            }],
            firewall: None,
            sandbox: None,
            provider_id: factory.config.provider_id.clone(),
            operating_mode: vesper_domain::SessionOperatingMode::Code,
            permission_mode: vesper_domain::SessionPermissionMode::Bypass,
            conversation: Vec::new(),
            cancellation: Arc::new(vesper_runtime::RuntimeCancellation::new()),
        };
        for (tool, arguments) in [
            (
                "run_command",
                serde_json::json!({"command":"gh release create v0.1.0"}),
            ),
            (
                "run_command",
                serde_json::json!({"command":"cargo test exact; git tag bypass"}),
            ),
            (
                "write_file",
                serde_json::json!({"path":".git/refs/tags/bypass", "content":"unadmitted"}),
            ),
            (
                "write_file",
                serde_json::json!({"path":".GIT/refs/tags/bypass", "content":"unadmitted"}),
            ),
            (
                "edit_file",
                serde_json::json!({"path":".git/config", "old_text":"fixture", "new_text":"bypass"}),
            ),
            (
                "apply_patch",
                serde_json::json!({"path":".git/config", "patch":"unadmitted"}),
            ),
        ] {
            let call = vesper_domain::ToolCall {
                id: vesper_domain::ToolCallId::new("denial-fixture").unwrap(),
                tool_id: vesper_domain::ToolId::new(tool).unwrap(),
                arguments,
                extensions: vesper_domain::ExtensionMap::default(),
            };
            assert!(
                matches!(
                    runtime.block_on(registry.execute(&call, &context)),
                    Err(vesper_agent::ToolError::InvalidArguments { .. })
                ),
                "{tool}"
            );
        }
        // Source mentioning Git metadata remains a legitimate repair input.
        let source = vesper_domain::ToolCall {
            id: vesper_domain::ToolCallId::new("source-fixture").unwrap(),
            tool_id: vesper_domain::ToolId::new("write_file").unwrap(),
            arguments: serde_json::json!({"path":"src/lib.rs", "content":"// .git/config is controller-owned\n"}),
            extensions: vesper_domain::ExtensionMap::default(),
        };
        assert!(
            runtime
                .block_on(registry.execute(&source, &context))
                .is_ok()
        );
        assert!(!root.path().join(".git/refs/tags/bypass").exists());
    }

    #[test]
    fn repair_factory_executes_real_tools_for_two_provider_fixtures() {
        for label in ["fixture.release-a", "fixture.release-b"] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("src")).unwrap();
            fs::write(
                root.path().join("Cargo.toml"),
                "[package]\nname = \"release-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            let broken = "pub fn answer() -> u32 { 0 }\n#[test] fn exact_regression() { assert_eq!(answer(), 42); }\n";
            let repaired = broken.replace("{ 0 }", "{ 42 }");
            fs::write(root.path().join("src/lib.rs"), broken).unwrap();
            let native =
                NativeReleaseExecutor::new(root.path(), Arc::new(AtomicBool::new(false))).unwrap();
            assert!(
                !native
                    .command("cargo", &["test", "exact_regression", "--offline"])
                    .unwrap()
                    .status
                    .success()
            );
            let (factory, session, runtime) = repair_test_factory(label, &repaired);
            let (outcome, history) = runtime
                .block_on(factory.run_coding_turn_in_workspace(
                    root.path().to_path_buf(),
                    "Repair the exact answer regression".into(),
                    Arc::new(vesper_runtime::RuntimeCancellation::new()),
                ))
                .unwrap();
            assert!(outcome.is_success());
            assert_eq!(session.requests().len(), 3);
            assert_eq!(
                fs::read_to_string(root.path().join("src/lib.rs")).unwrap(),
                repaired
            );
            let (mutation, proof) = observed_repair_receipts(&history);
            assert!(mutation);
            assert_eq!(
                proof.as_deref(),
                Some("cargo test exact_regression --offline -- --exact"),
                "history: {history:?}"
            );
            let output = native
                .command(
                    "cargo",
                    &["test", "exact_regression", "--offline", "--", "--exact"],
                )
                .unwrap();
            assert!(output.status.success());
            assert!(cargo_test_executed(&String::from_utf8_lossy(
                &output.stdout
            )));
        }
    }

    #[test]
    fn isolated_agent_repair_verifies_and_promotes_one_patch_for_two_provider_fixtures() {
        assert_isolated_repair_promotion(None);
    }

    #[test]
    fn repair_verification_changes_cannot_be_promoted_as_verified() {
        for change in ["tracked", "index", "untracked"] {
            assert_isolated_repair_promotion(Some(change));
        }
    }

    #[test]
    fn isolated_repair_preserves_new_literal_paths_for_two_provider_fixtures() {
        assert_isolated_repair_promotion(Some("newfile"));
    }

    #[test]
    fn isolated_repair_rejects_ignored_source_inputs_for_two_provider_fixtures() {
        assert_isolated_repair_promotion(Some("ignored"));
    }

    #[test]
    fn repair_proof_checks_staged_whitespace() {
        assert_isolated_repair_promotion(Some("whitespace"));
    }

    fn assert_isolated_repair_promotion(verification_change: Option<&str>) {
        for label in ["fixture.release-a", "fixture.release-b"] {
            for preparing in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let workspace = temp.path().join("workspace");
                fs::create_dir_all(workspace.join("src")).unwrap();
                fs::create_dir(workspace.join("registry")).unwrap();
                fs::write(
                    workspace.join("registry/agent.json"),
                    "{\"version\":\"0.1.0\"}",
                )
                .unwrap();
                fs::write(
                    workspace.join("Cargo.toml"),
                    "[package]\nname=\"repair-composition\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
                )
                .unwrap();
                fs::write(workspace.join(".gitattributes"), "* text eol=lf\n").unwrap();
                fs::write(
                    workspace.join(".gitignore"),
                    if verification_change == Some("ignored") {
                        "/target/\n/src/generated.rs\n"
                    } else {
                        "/target/\n"
                    },
                )
                .unwrap();
                let broken = "pub fn answer() -> u32 { 0 }\n#[test] fn exact_regression() { assert_eq!(answer(), 42); }\n";
                let repaired = if verification_change == Some("ignored") {
                    "mod generated; pub use generated::answer;\n#[test] fn exact_regression() { assert_eq!(answer(), 42); }\n".into()
                } else {
                    broken.replace("{ 0 }", "{ 42 }")
                };
                fs::write(workspace.join("src/lib.rs"), broken).unwrap();
                let native =
                    NativeReleaseExecutor::new(&workspace, Arc::new(AtomicBool::new(false)))
                        .unwrap();
                native.checked("git", &["init", "-b", "main"]).unwrap();
                native
                    .checked("git", &["config", "user.name", "fixture"])
                    .unwrap();
                native
                    .checked("git", &["config", "user.email", "fixture@example.invalid"])
                    .unwrap();
                assert!(
                    !native
                        .command("cargo", &["test", "exact_regression", "--offline"])
                        .unwrap()
                        .status
                        .success()
                );
                native.checked("git", &["add", "--all"]).unwrap();
                native
                    .checked("git", &["commit", "-m", "fixture red regression"])
                    .unwrap();
                let base = native
                    .checked("git", &["rev-parse", "HEAD"])
                    .unwrap()
                    .trim()
                    .to_owned();
                let mut record =
                    crate::release_recovery::start_release("composition", "patch", "main", &base)
                        .unwrap();
                record.state = if preparing {
                    ReleaseRecoveryState::DiagnosingLocalFailure
                } else {
                    ReleaseRecoveryState::ClassifyingFailure
                };
                record.mutation.candidate_committed = !preparing;
                record.mutation.candidate_pushed = !preparing;
                record.mutation.candidate_push_ref = Some(format!("origin/main@{base}"));
                record.required_gates = GreenGithub.matrix_for_sha("fixture/repo", &base).unwrap();
                record.required_gates[0].jobs[0].state = crate::release_recovery::JobState::Failure;
                record.required_gates[0].run_state =
                    Some(crate::release_recovery::JobState::Failure);
                record
                    .failures
                    .push(crate::release_recovery::FailureRecord {
                        workflow_id: 1,
                        run_id: 1,
                        attempt: 1,
                        job_id: 1,
                        workflow_name: record.required_gates[0].name.clone(),
                        job_name: record.required_gates[0].jobs[0].job_name.clone(),
                        platform: Some(std::env::consts::OS.into()),
                        step_name: Some("cargo test exact_regression".into()),
                        fingerprint: crate::release_recovery::FailureFingerprint(
                            "fixture-regression".into(),
                        ),
                        class: crate::release_recovery::ReleaseFailureClass::TestRegression,
                        confidence: crate::release_recovery::EvidenceConfidence::Proven,
                        causal_excerpt:
                            "thread 'exact_regression' panicked at src/lib.rs:2: assertion failed"
                                .into(),
                        source_commit: base.clone(),
                        observed_at: Utc::now(),
                        other_platforms_passed: true,
                        exists_on_last_green: Some(false),
                        related_source_touched: Some(true),
                    });
                let ledger = ReleaseLedger::open(temp.path().join("state"), "composition").unwrap();
                ledger.save(&record).unwrap();
                let extra_file = matches!(
                    verification_change,
                    Some("newfile" | "ignored" | "whitespace")
                );
                let prefix = if extra_file {
                    vec![Ok(vec![
                        Ok(vesper_provider::ProviderStreamEvent::ToolCallCompleted(
                            vesper_domain::ToolCall {
                                id: vesper_domain::ToolCallId::new("extra-file").unwrap(),
                                tool_id: vesper_domain::ToolId::new("write_file").unwrap(),
                                arguments: if verification_change == Some("ignored") {
                                    serde_json::json!({"path":"src/generated.rs", "content":"pub fn answer() -> u32 { 42 }\n"})
                                } else {
                                    serde_json::json!({"path":"repair evidence.md", "content": if verification_change == Some("whitespace") { "verified evidence \n" } else { "verified evidence" }})
                                },
                                extensions: Default::default(),
                            },
                        )),
                        Ok(vesper_provider::ProviderStreamEvent::Completed {
                            finish: vesper_domain::FinishOutcome::ToolCalls,
                            metadata: Default::default(),
                        }),
                    ])]
                } else {
                    Vec::new()
                };
                let (factory, session, _runtime) = repair_test_factory_with_prefix(
                    label,
                    &repaired,
                    &["cargo test exact_regression --offline -- --exact"],
                    prefix,
                );
                let gates_observed = std::sync::atomic::AtomicUsize::new(0);
                let verify_fixture_gates = |executor: &NativeReleaseExecutor| {
                    gates_observed.fetch_add(1, Ordering::SeqCst);
                    assert_ne!(
                        executor.workspace, workspace,
                        "verification must use the isolated worktree"
                    );
                    executor.checked("cargo", &["test", "--offline"])?;
                    match verification_change {
                        Some("tracked" | "index") => {
                            fs::write(
                                executor.workspace.join("src/lib.rs"),
                                repaired.replace("{ 42 }", "{ 43 }"),
                            )?;
                            if verification_change == Some("index") {
                                executor.checked("git", &["add", "src/lib.rs"])?;
                            }
                        }
                        Some("untracked") => {
                            fs::write(executor.workspace.join("unverified source.rs"), broken)?;
                        }
                        _ => {}
                    }
                    Ok(())
                };
                run_bounded_repair_agent_with_verification(
                    &workspace,
                    &mut record,
                    &ledger,
                    &factory,
                    Arc::new(AtomicBool::new(false)),
                    RepairVerification {
                        root: &temp.path().join("state"),
                        governor: None,
                        verify: &verify_fixture_gates,
                    },
                )
                .unwrap();
                assert_eq!(
                    gates_observed.load(Ordering::SeqCst),
                    if verification_change == Some("ignored") {
                        0
                    } else {
                        1
                    }
                );
                assert_eq!(session.requests().len(), if extra_file { 4 } else { 3 });
                if let Some(change) = verification_change.filter(|change| *change != "newfile") {
                    assert_eq!(
                        fs::read_to_string(workspace.join("src/lib.rs")).unwrap(),
                        broken,
                        "verification changed {change}; no unverified patch may reach the controller"
                    );
                    assert_eq!(record.repair_attempts.len(), 1);
                    let receipt = &record.repair_attempts[0];
                    assert_eq!(
                        receipt.focused_status,
                        crate::release_recovery::FocusedProofStatus::Failed
                    );
                    assert!(receipt.disproven_or_insufficient);
                    assert!(receipt.source_commit_after.is_none());
                    assert!(
                        receipt
                            .evidence_refs
                            .iter()
                            .any(|evidence| evidence.contains(if change == "ignored" {
                                "not represented in the promoted Git tree"
                            } else if change == "whitespace" {
                                "whitespace"
                            } else {
                                "verification changed"
                            })),
                        "safe failure context must survive for the next bounded attempt"
                    );
                    assert_eq!(record.retry_budget.full_gate_used, 0);
                    assert_eq!(
                        record
                            .mutation
                            .repair_admissions
                            .values()
                            .copied()
                            .sum::<u8>(),
                        1
                    );
                    assert_eq!(ledger.load().unwrap().unwrap(), record);
                    continue;
                }
                assert_eq!(
                    record.state,
                    if preparing {
                        ReleaseRecoveryState::LocalVerification
                    } else {
                        ReleaseRecoveryState::RetryAdmissible
                    }
                );
                assert_eq!(record.repair_attempts.len(), 1);
                let receipt = &record.repair_attempts[0];
                assert_eq!(receipt.source_commit_before, base);
                if preparing {
                    assert!(receipt.source_commit_after.is_none());
                } else {
                    assert_ne!(receipt.source_commit_after.as_deref(), Some(base.as_str()));
                }
                assert_eq!(
                    receipt.focused_status,
                    crate::release_recovery::FocusedProofStatus::Passed
                );
                assert!(!receipt.hypothesis.is_empty());
                assert_eq!(
                    fs::read_to_string(workspace.join("src/lib.rs")).unwrap(),
                    repaired
                );
                if preparing {
                    assert_eq!(
                        native
                            .checked("git", &["rev-parse", "HEAD"])
                            .unwrap()
                            .trim(),
                        base
                    );
                    let patch = native
                        .command("git", &["diff", "--binary", "HEAD"])
                        .unwrap();
                    let admission =
                        admit_release_mutation(&record, ReleaseMutationKind::VersionBump).unwrap();
                    assert_eq!(
                        admission.preparation_baseline_digest(),
                        Some(digest(&patch.stdout).as_str())
                    );
                    assert_eq!(
                        record.mutation.repair_files,
                        if extra_file {
                            vec!["repair evidence.md", "src/lib.rs"]
                        } else {
                            vec!["src/lib.rs"]
                        }
                    );
                } else {
                    assert!(
                        native
                            .checked("git", &["status", "--porcelain"])
                            .unwrap()
                            .is_empty()
                    );
                    assert_eq!(
                        literal_git_paths(
                            &native
                                .git_bytes(&["diff", "--name-only", "-z", &base, "HEAD"])
                                .unwrap()
                        )
                        .unwrap(),
                        if extra_file {
                            vec!["repair evidence.md", "src/lib.rs"]
                        } else {
                            vec!["src/lib.rs"]
                        }
                    );
                }
                assert!(!record.mutation.candidate_pushed);
                assert_eq!(
                    record.retry_budget.full_gate_used, 0,
                    "promotion is not a remote retry"
                );
                assert!(record.metrics.model_active_millis > 0);
                assert_eq!(ledger.load().unwrap().unwrap(), record);
            }
        }
    }

    #[test]
    fn native_version_bump_keeps_all_workspace_dependency_pins_resolvable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let resources = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.join("registry")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"3\"\n[workspace.package]\nversion = \"0.24.4\"\nedition = \"2024\"\n").unwrap();
        fs::write(
            root.join("registry/agent.json"),
            "{\"version\":\"0.24.4\",\"archive\":\"https://example.invalid/v0.24.4/a.tar.gz\"}",
        )
        .unwrap();
        for name in ["a", "b"] {
            fs::create_dir_all(root.join(name).join("src")).unwrap();
            fs::write(root.join(name).join("src/lib.rs"), "").unwrap();
            let dependency = if name == "a" {
                "[dependencies]\nvesper-b = { path = \"../b\", version = \"=0.24.4\" }\n"
            } else {
                ""
            };
            fs::write(root.join(name).join("Cargo.toml"), format!("[package]\nname = \"vesper-{name}\"\nversion.workspace = true\nedition.workspace = true\n{dependency}")).unwrap();
        }
        let executor = NativeReleaseExecutor::new(root, Arc::new(AtomicBool::new(false)))
            .unwrap()
            .with_resource_governor(
                HostResourceGovernor::new(
                    permissive_resource_policy(),
                    resources.path().join("resource-scheduler"),
                    resources.path().join("resource-target"),
                )
                .unwrap(),
            );
        executor
            .checked("cargo", &["generate-lockfile", "--offline"])
            .unwrap();
        executor.checked("git", &["init"]).unwrap();
        executor.checked("git", &["add", "--all"]).unwrap();
        executor
            .checked(
                "git",
                &[
                    "-c",
                    "user.name=RRC Fixture",
                    "-c",
                    "user.email=rrc@example.invalid",
                    "commit",
                    "-m",
                    "base",
                ],
            )
            .unwrap();
        let base = executor.checked("git", &["rev-parse", "HEAD"]).unwrap();
        let record =
            crate::release_recovery::start_release("repo", "patch", "main", base.trim()).unwrap();
        let receipt = executor
            .prepare_version_bump(
                "patch",
                &admit_release_mutation(&record, ReleaseMutationKind::VersionBump).unwrap(),
            )
            .unwrap();
        assert!(receipt.files.contains(&"a/Cargo.toml".into()));
        executor
            .checked(
                "cargo",
                &[
                    "metadata",
                    "--format-version",
                    "1",
                    "--no-deps",
                    "--offline",
                ],
            )
            .unwrap();
        assert!(
            fs::read_to_string(root.join("a/Cargo.toml"))
                .unwrap()
                .contains("version = \"=0.24.5\"")
        );
        let mut candidate = record;
        candidate.mutation.version_after = Some(receipt.after);
        candidate.mutation.version_files = receipt.files;
        candidate.mutation.local_gates = production_local_gates();
        for gate in &mut candidate.mutation.local_gates {
            gate.state = SettlementState::Succeeded;
            gate.evidence_ref = Some("fixture:passed".into());
        }
        let token =
            &admit_release_mutation(&candidate, ReleaseMutationKind::CommitCandidate).unwrap();
        assert!(token.candidate_paths().contains(&"a/Cargo.toml".into()));
    }

    #[test]
    fn release_read_ports_share_worker_cancellation_before_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let github = GhCliEvidenceAdapter.with_cancellation(Arc::clone(&cancelled));
        let health = CurlGitHubStatusAdapter.with_cancellation(Arc::clone(&cancelled));
        let executor = NativeReleaseExecutor::new(root.path(), Arc::clone(&cancelled)).unwrap();
        // Flip after constructing the ports: they must retain the worker's token,
        // rather than copy its initial value or allocate an independent token.
        cancelled.store(true, Ordering::Release);
        let sha = "0123456789012345678901234567890123456789";
        for result in [
            github.current_main_commit("fixture/repository").map(|_| ()),
            github.matrix_for_sha("fixture/repository", sha).map(|_| ()),
            github.job_log("fixture/repository", 1).map(|_| ()),
            health.official_status().map(|_| ()),
            executor
                .last_green_release_commit("fixture/repository")
                .map(|_| ()),
            executor
                .publication("fixture/repository", "v0.24.4", sha)
                .map(|_| ()),
        ] {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("cancelled by the user")
            );
        }
    }

    struct HealthyStatus;

    impl ExternalHealthPort for HealthyStatus {
        fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
            Ok(OfficialStatusSnapshot {
                degraded: false,
                summary: "none: operational".into(),
                evidence_ref: "https://www.githubstatus.com/".into(),
            })
        }
    }

    #[test]
    fn native_worker_stops_owner_action_and_uncertain_causes_before_repair_permission() {
        struct FailedGithub(&'static str);
        impl GitHubEvidencePort for FailedGithub {
            fn matrix_for_sha(
                &self,
                repository: &str,
                sha: &str,
            ) -> Result<Vec<crate::release_recovery::GateRecord>, RrcError> {
                let mut gates = GreenGithub.matrix_for_sha(repository, sha)?;
                gates[0].run_state = Some(crate::release_recovery::JobState::Failure);
                gates[0].jobs[0].state = crate::release_recovery::JobState::Failure;
                gates[0].jobs[0].failed_step = Some("Execute job".into());
                Ok(gates)
            }
            fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
                Ok(self.0.into())
            }
            fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("no retry admission")
            }
            fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("no retry admission")
            }
            fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("no retry admission")
            }
        }
        struct NoHealth;
        impl ExternalHealthPort for NoHealth {
            fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
                panic!("no outage claim")
            }
        }
        for (cause, expected) in [
            (
                "The job was not started because recent account payments have failed. Please check your account billing settings.",
                ReleaseRecoveryState::Escalated,
            ),
            (
                "Unrecognized terminal diagnostic",
                ReleaseRecoveryState::NeedMoreEvidence,
            ),
        ] {
            for continuous in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
                let mut record = crate::release_recovery::start_release(
                    "repo",
                    "patch",
                    "main",
                    &"a".repeat(40),
                )
                .unwrap();
                record.state = ReleaseRecoveryState::DiagnosingLocalFailure;
                assert!(release_stage_has_side_effects(&record));
                record.state = ReleaseRecoveryState::RemoteGateRunning;
                let github = FailedGithub(cause);
                refresh_remote_evidence(&mut record, "owner/repo", &github).unwrap();
                assert_eq!(record.state, ReleaseRecoveryState::ClassifyingFailure);
                let budget = record.retry_budget.clone();
                ledger.save(&record).unwrap();
                let context = ReleaseAdvanceContext {
                    workspace: temp.path(),
                    repository: "owner/repo",
                    ledger: &ledger,
                    executor: &FakeRelease,
                    github: &github,
                    health: &NoHealth,
                    repair_factory: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                };
                if continuous {
                    drive_release_worker(
                        context,
                        |_| true,
                        |_| panic!("read-only routing must precede repair permission"),
                    )
                    .unwrap();
                } else {
                    advance_release(&mut record, context).unwrap();
                }
                let settled = ledger.load().unwrap().unwrap();
                assert_eq!(settled.state, expected);
                assert_eq!(settled.retry_budget, budget);
                assert!(settled.mutation.in_flight_operation.is_none());
                assert!(settled.repair_attempts.is_empty());
                assert_eq!(settled.metrics.model_active_millis, 0);
            }
        }
    }

    #[test]
    fn read_only_health_checks_do_not_request_source_repair_permissions() {
        struct RunnerFailure;
        impl GitHubEvidencePort for RunnerFailure {
            fn matrix_for_sha(
                &self,
                repository: &str,
                sha: &str,
            ) -> Result<Vec<crate::release_recovery::GateRecord>, RrcError> {
                let mut gates = GreenGithub.matrix_for_sha(repository, sha)?;
                gates[0].run_state = Some(crate::release_recovery::JobState::Failure);
                gates[0].jobs[0].state = crate::release_recovery::JobState::Failure;
                gates[0].jobs[0].failed_step = Some("Run fixture".into());
                Ok(gates)
            }
            fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
                Ok("Error: runner connection lost".into())
            }
            fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("rerun requires owner permission")
            }
            fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("rerun requires owner permission")
            }
            fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
                panic!("rerun requires owner permission")
            }
        }
        let github = RunnerFailure;
        for recovered in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
            let mut record =
                crate::release_recovery::start_release("repo", "patch", "main", &"a".repeat(40))
                    .unwrap();
            record.state = ReleaseRecoveryState::RemoteGateRunning;
            refresh_remote_evidence(&mut record, "owner/repo", &github).unwrap();
            assert!(record.failures.last().unwrap().class.infrastructure_like());
            if recovered {
                record.state_changes.push(RelevantStateChange {
                    kind: RelevantStateChangeKind::ExternalServiceRecovered,
                    description:
                        "fixture official recovery observed after the immutable failed attempt"
                            .into(),
                    evidence_refs: vec!["fixture:official-recovery".into()],
                    observed_at: Utc::now(),
                });
                assert!(
                    record
                        .retry_admission(crate::release_recovery::RetryKind::Infrastructure)
                        .admitted
                );
                assert_eq!(
                    release_stage_commands(&record, "owner/repo"),
                    vec!["gh api --method POST repos/owner/repo/actions/runs/1/rerun"]
                );
                let (mut factory, _session, _runtime) =
                    repair_test_factory("health-permission-fixture", "unused");
                factory.config.firewall = Some(Arc::new(
                    vesper_policy::firewall::CommandFirewall::compile(&[(
                        "git",
                        vesper_policy::firewall::RuleDecision::Deny,
                        "source edits denied",
                    )])
                    .unwrap(),
                ));
                assert!(
                    authorize_controller_step(
                        Some(&factory),
                        &record,
                        "owner/repo",
                        &AtomicBool::new(false)
                    )
                    .is_ok()
                );
                factory.config.firewall = Some(Arc::new(
                    vesper_policy::firewall::CommandFirewall::compile(&[(
                        "gh",
                        vesper_policy::firewall::RuleDecision::Deny,
                        "GitHub writes denied",
                    )])
                    .unwrap(),
                ));
                assert!(matches!(
                    authorize_controller_step(
                        Some(&factory),
                        &record,
                        "owner/repo",
                        &AtomicBool::new(false)
                    ),
                    Err(RrcError::AuthorizationBlocked(reason))
                        if reason == "command firewall denied release stage"
                ));
            } else {
                assert!(release_stage_commands(&record, "owner/repo").is_empty());
            }
            ledger.save(&record).unwrap();
            let mut permissions = 0;
            let outcome = drive_release_worker(
                ReleaseAdvanceContext {
                    workspace: temp.path(),
                    repository: "owner/repo",
                    ledger: &ledger,
                    executor: &FakeRelease,
                    github: &github,
                    health: &HealthyStatus,
                    repair_factory: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
                |_| true,
                |_| {
                    permissions += 1;
                    assert!(
                        recovered,
                        "read-only health must not ask for source mutation permission"
                    );
                    Err(RrcError::MutationBlocked(
                        "fixture owner refused rerun".into(),
                    ))
                },
            );
            if recovered {
                assert!(matches!(outcome, Err(RrcError::MutationBlocked(_))));
                assert_eq!(permissions, 1);
            } else {
                outcome.unwrap();
                assert_eq!(permissions, 0);
                assert_eq!(
                    ledger.load().unwrap().unwrap().state,
                    ReleaseRecoveryState::NeedMoreEvidence
                );
            }
            let settled = ledger.load().unwrap().unwrap();
            assert_eq!(settled.retry_budget.infrastructure_used, 0);
            assert!(settled.mutation.in_flight_operation.is_none());
            assert!(settled.repair_attempts.is_empty());
        }
    }

    struct EventualGreen(std::sync::atomic::AtomicUsize);

    impl GitHubEvidencePort for EventualGreen {
        fn matrix_for_sha(
            &self,
            repository: &str,
            sha: &str,
        ) -> Result<Vec<crate::release_recovery::GateRecord>, RrcError> {
            let observed = self.0.fetch_add(1, Ordering::SeqCst);
            let mut gates = GreenGithub.matrix_for_sha(repository, sha)?;
            if observed < 2 {
                for gate in &mut gates {
                    gate.run_state = Some(crate::release_recovery::JobState::InProgress);
                    gate.jobs[0].state = crate::release_recovery::JobState::InProgress;
                }
                gates[0].jobs[0].state = crate::release_recovery::JobState::Failure;
            }
            Ok(gates)
        }
        fn job_log(&self, _: &str, _: u64) -> Result<String, RrcError> {
            panic!("logs must wait for complete matrix")
        }
        fn rerun_job(&self, _: &str, _: u64) -> Result<(), RrcError> {
            panic!("no rerun admission")
        }
        fn rerun_failed(&self, _: &str, _: u64) -> Result<(), RrcError> {
            panic!("no rerun admission")
        }
        fn rerun_workflow(&self, _: &str, _: u64) -> Result<(), RrcError> {
            panic!("no rerun admission")
        }
    }

    #[test]
    fn background_controller_waits_then_publishes_without_continue_prompts() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let mut record = crate::release_recovery::start_release(
            "repo",
            "patch",
            "main",
            "a".repeat(40).as_str(),
        )
        .unwrap();
        record.state = ReleaseRecoveryState::RemoteGateRunning;
        record.release_version = Some("0.24.5".into());
        record.mutation.candidate_pushed = true;
        ledger.save(&record).unwrap();
        let github = EventualGreen(std::sync::atomic::AtomicUsize::new(0));
        let mut waits = 0;
        let mut mutations = 0;
        drive_release_worker(
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &FakeRelease,
                github: &github,
                health: &HealthyStatus,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
            |delay| {
                assert!(
                    [
                        poll_interval(0),
                        poll_interval(1),
                        poll_interval(2),
                        poll_interval(3)
                    ]
                    .contains(&delay),
                    "worker must use the bounded current polling policy"
                );
                waits += 1;
                thread::sleep(Duration::from_millis(3));
                true
            },
            |_| {
                assert!(github.0.load(Ordering::SeqCst) >= 3);
                mutations += 1;
                Ok(())
            },
        )
        .unwrap();
        let settled = ledger.load().unwrap().unwrap();
        assert_eq!(waits, 2);
        assert!(settled.metrics.ci_wait_millis >= 6);
        assert_eq!(settled.metrics.model_active_millis, 0);
        let legacy = {
            let mut value = serde_json::to_value(&settled).unwrap();
            value.as_object_mut().unwrap().remove("metrics");
            serde_json::from_value::<ReleaseRecoveryRecord>(value).unwrap()
        };
        assert_eq!(
            legacy.metrics,
            crate::release_recovery::ReleaseMetrics::default()
        );
        assert_eq!(mutations, 1);
        assert_eq!(settled.state, ReleaseRecoveryState::Published);
        assert!(settled.mutation.publication_verified);
    }

    #[test]
    fn denied_host_permission_prevents_every_release_side_effect() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let record =
            crate::release_recovery::start_release("repo", "patch", "main", &"a".repeat(40))
                .unwrap();
        ledger.save(&record).unwrap();
        assert!(
            drive_release_worker(
                ReleaseAdvanceContext {
                    workspace: temp.path(),
                    repository: "owner/repo",
                    ledger: &ledger,
                    executor: &FakeRelease,
                    github: &GreenGithub,
                    health: &HealthyStatus,
                    repair_factory: None,
                    cancelled: Arc::new(AtomicBool::new(false))
                },
                |_| panic!("denied work cannot poll"),
                |_| Err(RrcError::MutationBlocked("denied".into()))
            )
            .is_err()
        );
        assert_eq!(ledger.load().unwrap().unwrap(), record);
    }

    #[test]
    fn an_unsettled_mutation_is_never_replayed_on_restart() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", &"a".repeat(40))
                .unwrap();
        record.mutation.in_flight_operation = Some("VersionBump".into());
        ledger.save(&record).unwrap();
        assert!(
            drive_release_worker(
                ReleaseAdvanceContext {
                    workspace: temp.path(),
                    repository: "owner/repo",
                    ledger: &ledger,
                    executor: &FakeRelease,
                    github: &GreenGithub,
                    health: &HealthyStatus,
                    repair_factory: None,
                    cancelled: Arc::new(AtomicBool::new(false))
                },
                |_| panic!("uncertain work cannot poll"),
                |_| panic!("uncertain work cannot mutate")
            )
            .is_err()
        );
        assert_eq!(ledger.load().unwrap().unwrap(), record);
    }

    #[test]
    fn arbitrary_successful_shell_commands_are_not_focused_proof() {
        for command in [
            "cargo test --no-run",
            "cargo test -- --list",
            "cargo test --help",
            "echo pass",
            "true",
            "git status",
            "cargo test exact || true",
            "cargo test exact; echo pass",
            "cargo test exact > /tmp/log",
            "cargo test exact && true",
            "git tag v0.1.0",
            "gh release create v0.1.0",
            "cargo test exact\ngit tag v0.1.0",
            "cargo test $(git tag v0.1.0)",
            "cargo + test exact",
        ] {
            assert!(!credible_focused_command(command), "{command}");
        }
        assert!(credible_focused_command(
            "cargo test -p vesper-harness exact_regression -- --exact"
        ));
        assert!(credible_focused_command(
            "cargo +1.88.0 test exact_regression -- --exact"
        ));
        assert!(credible_focused_command("cargo xtask msrv"));
        assert_eq!(
            cargo_command(&["cargo", "+1.88.0", "test", "exact"]),
            Some("test")
        );
    }

    #[test]
    fn repair_cargo_requires_governor_and_rejects_policy_overrides() {
        let workspace = tempfile::tempdir().unwrap();
        let resources = tempfile::tempdir().unwrap();
        let context = repair_command_test_context(workspace.path());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let command = repair_command_test_call("cargo test actual_regression -- --exact");
        let missing =
            runtime.block_on(repair_tool_registry_with_governor(None).execute(&command, &context));
        assert!(
            matches!(missing, Err(vesper_agent::ToolError::Failed(message)) if message.contains("controller-owned resource governor"))
        );
        let governor = HostResourceGovernor::new(
            permissive_resource_policy(),
            resources.path().join("scheduler"),
            resources.path().join("target"),
        )
        .unwrap();
        let registry = repair_tool_registry_with_governor(Some(governor));
        for flag in [
            "--jobs=99",
            "-j99",
            "--target-dir=elsewhere",
            "--config=elsewhere",
            "--manifest-path=elsewhere",
            "--test-threads=99",
        ] {
            let call = repair_command_test_call(&format!("cargo test actual_regression {flag}"));
            assert!(
                matches!(runtime.block_on(registry.execute(&call, &context)), Err(vesper_agent::ToolError::InvalidArguments { reason, .. }) if reason.contains("resource policy")),
                "{flag}"
            );
        }
        assert!(!resources.path().join("target/debug").exists());
        assert!(fs::read_dir(workspace.path()).unwrap().next().is_none());

        use vesper_agent::sandbox_route::*;
        struct NoDispatch;
        impl SandboxBackendPort for NoDispatch {
            fn capabilities(&self) -> SandboxCapabilities {
                SandboxCapabilities {
                    backend: "fixture-full".into(),
                    process_tree: CapabilityStatus::Available,
                    filesystem: CapabilityStatus::Available,
                    network: CapabilityStatus::Available,
                    strength: SecurityStrength::Full,
                }
            }
            fn run_command(
                &self,
                _: &str,
                _: &Path,
                _: u64,
                _: &Arc<dyn vesper_provider::CancellationSignal>,
            ) -> Result<SandboxOutcome, SandboxRunError> {
                panic!("unsupported governed route must refuse before dispatch")
            }
        }
        let mut sandbox_context = context;
        sandbox_context.sandbox = Some(Arc::new(SandboxRoute::new(
            SandboxDemand {
                requirement: IsolationRequirement::Full,
                ..SandboxDemand::none()
            },
            SandboxBackendChoice::Default,
            Arc::new(NoDispatch),
        )));
        assert!(
            matches!(runtime.block_on(registry.execute(&command, &sandbox_context)), Err(vesper_agent::ToolError::Failed(message)) if message.contains("refusing host execution"))
        );
        assert!(fs::read_dir(workspace.path()).unwrap().next().is_none());
    }

    fn repair_command_test_context(workspace: &Path) -> vesper_agent::ToolContext {
        vesper_agent::ToolContext {
            workspace_roots: vec![vesper_domain::WorkspaceRoot {
                name: vesper_domain::BoundedString::new("repair-fixture").unwrap(),
                path: vesper_domain::BoundedString::new(workspace.display().to_string()).unwrap(),
                primary: true,
            }],
            firewall: None,
            sandbox: None,
            provider_id: vesper_domain::ProviderId::new("fixture.repair").unwrap(),
            operating_mode: vesper_domain::SessionOperatingMode::Code,
            permission_mode: vesper_domain::SessionPermissionMode::Bypass,
            conversation: Vec::new(),
            cancellation: Arc::new(vesper_runtime::RuntimeCancellation::new()),
        }
    }

    fn repair_command_test_call(command: &str) -> vesper_domain::ToolCall {
        vesper_domain::ToolCall {
            id: vesper_domain::ToolCallId::new("repair-cargo").unwrap(),
            tool_id: vesper_domain::ToolId::new("run_command").unwrap(),
            arguments: serde_json::json!({"command": command}),
            extensions: Default::default(),
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn repair_cargo_uses_managed_cache_and_bounded_environment() {
        let workspace = tempfile::tempdir().unwrap();
        let resources = tempfile::tempdir().unwrap();
        fs::create_dir(workspace.path().join("src")).unwrap();
        fs::write(workspace.path().join("Cargo.toml"), "[package]\nname = \"governed-repair-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
        fs::write(workspace.path().join("src/lib.rs"), r#"
#[test]
fn actual_regression() {
    assert_eq!(std::env::var("CARGO_BUILD_JOBS").unwrap(), "1");
    assert_eq!(std::env::var("RUST_TEST_THREADS").unwrap(), "1");
    assert!(std::path::Path::new(&std::env::var("CARGO_TARGET_DIR").unwrap()).ends_with("managed-cache"));
}
"#).unwrap();
        let cache = resources.path().join("managed-cache");
        let governor = HostResourceGovernor::new(
            permissive_resource_policy(),
            resources.path().join("scheduler"),
            &cache,
        )
        .unwrap();
        let started = Instant::now();
        let heartbeat = Arc::new(Mutex::new(started - Duration::from_secs(600)));
        let activity = Arc::new(Mutex::new(ReleaseWorkerActivity {
            epoch_id: "repair-fixture".into(),
            stage: "Release recovery".into(),
            detail: "Focused repair".into(),
            current_gate: None,
            current_command: None,
            current_child: None,
            gate_started_at: None,
            last_activity_at: started,
            completed_gates: 0,
            total_gates: 0,
            version_before: None,
            version_after: None,
            candidate_sha: None,
            retry_budget: String::new(),
            failure_fingerprint: None,
            recent_output: VecDeque::new(),
            progress: ReleaseProgress::default(),
            resource_deferred: false,
            resource_telemetry: None,
        }));
        let registry = build_repair_tool_registry(
            Some(governor.clone()),
            false,
            Some(RepairCommandProgress {
                heartbeat: Arc::clone(&heartbeat),
                activity: Some(Arc::clone(&activity)),
                evidence: None,
            }),
        );
        let call = repair_command_test_call("cargo test --offline actual_regression -- --exact");
        let context = repair_command_test_context(workspace.path());
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(registry.execute(&call, &context))
            .unwrap();
        assert!(output.text.as_str().contains("1 passed; 0 failed"));
        assert!(*heartbeat.lock().unwrap() >= started);
        let observed = activity.lock().unwrap();
        assert!(observed.current_child.is_none());
        assert_eq!(
            observed.current_command.as_deref(),
            Some("cargo test --offline actual_regression -- --exact")
        );
        assert!(observed.current_gate.is_none());
        assert!(observed.resource_telemetry.is_some());
        assert!(
            observed
                .recent_output
                .iter()
                .any(|line| line.contains("1 passed; 0 failed"))
        );
        assert!(cache.join("debug/deps").exists());
        assert!(!workspace.path().join("target").exists());
        // The native expensive-resource lease is released after owned-child settlement.
        assert!(governor.preflight(GateCost::Expensive).is_ok());
    }

    #[test]
    fn repair_command_signal_watcher_settles_on_unwind() {
        let cancelled = Arc::new(AtomicBool::new(false));
        drop(RepairCommandCancellation {
            cancelled: Arc::clone(&cancelled),
            settled: false,
        });
        assert!(cancelled.load(Ordering::Acquire));
        cancelled.store(false, Ordering::Release);
        drop(RepairCommandCancellation {
            cancelled: Arc::clone(&cancelled),
            settled: true,
        });
        assert!(!cancelled.load(Ordering::Acquire));
        let settled = Arc::new(AtomicBool::new(false));
        let observed_settled = Arc::clone(&settled);
        let result = std::panic::catch_unwind(move || {
            let done = Arc::new(AtomicBool::new(false));
            let observed_done = Arc::clone(&done);
            let worker = thread::spawn(move || {
                while !observed_done.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(1));
                }
                observed_settled.store(true, Ordering::Release);
            });
            let _watcher = RepairCommandSignalWatcher {
                done,
                worker: Some(worker),
            };
            panic!("controlled repair task unwind");
        });
        assert!(result.is_err());
        assert!(settled.load(Ordering::Acquire));
    }

    #[test]
    fn focused_proof_identifies_rust_panic_with_numeric_thread_id() {
        for line in [
            "thread 'actual_regression' panicked at tests/case.rs:272:9:",
            "2026-10-05T21:39:18Z thread 'actual_regression' (14427) panicked at tests/case.rs:272:9:",
        ] {
            assert_eq!(failed_rust_test(line).as_deref(), Some("actual_regression"));
        }
        for line in [
            "thread 'main' (1234) panicked at src/main.rs:1:1:",
            "thread '<unnamed>' (1234) panicked at src/lib.rs:1:1:",
            "thread 'actual_regression' (invalid) panicked at src/lib.rs:1:1:",
        ] {
            assert_eq!(failed_rust_test(line), None);
        }
    }

    #[test]
    fn focused_test_proof_requires_a_nonzero_executed_pass() {
        assert!(!cargo_test_executed(
            "test result: ok. 0 passed; 0 failed; 5 filtered out"
        ));
        assert!(!cargo_test_executed("my_test: test\n1 test, 0 benchmarks"));
        assert!(cargo_test_executed(
            "test result: ok. 1 passed; 0 failed; 0 filtered out"
        ));
        let input = "[[package]]\nname = \"vesper-harness\"\nversion = \"0.24.4\"\n[[package]]\nname = \"other\"\nversion = \"0.24.4\"\n";
        let normalized = version_only_lockfile(input, "0.24.4", "0.24.5");
        assert!(normalized.contains("name = \"vesper-harness\"\nversion = \"0.24.5\""));
        assert!(normalized.contains("name = \"other\"\nversion = \"0.24.4\""));
    }

    #[test]
    fn publication_requires_all_fourteen_nonempty_uploaded_assets() {
        let archives = [
            "agent-vesper-acp-linux-x86_64.tar.gz",
            "agent-vesper-acp-linux-aarch64.tar.gz",
            "agent-vesper-acp-darwin-x86_64.tar.gz",
            "agent-vesper-acp-darwin-aarch64.tar.gz",
            "agent-vesper-acp-windows-x86_64.zip",
            "vesper-web-driver-linux-x86_64.tar.gz",
            "vesper-web-driver-linux-aarch64.tar.gz",
        ];
        let assets = archives
            .iter()
            .flat_map(|archive| [archive.to_string(), format!("{archive}.sha256")])
            .map(|name| serde_json::json!({"name": name, "size": 42, "state": "uploaded"}))
            .collect::<Vec<_>>();
        let mut release = serde_json::json!({"assets": assets});
        assert_eq!(verified_publication_assets(&release).unwrap().len(), 14);
        release["assets"][0]["size"] = serde_json::json!(0);
        assert!(verified_publication_assets(&release).is_err());
        release["assets"][0]["size"] = serde_json::json!(42);
        release["assets"].as_array_mut().unwrap().pop();
        assert!(verified_publication_assets(&release).is_err());
    }

    #[test]
    fn publication_checksum_content_must_match_both_server_digests() {
        let names = [
            "agent-vesper-acp-linux-x86_64.tar.gz",
            "agent-vesper-acp-linux-aarch64.tar.gz",
            "agent-vesper-acp-darwin-x86_64.tar.gz",
            "agent-vesper-acp-darwin-aarch64.tar.gz",
            "agent-vesper-acp-windows-x86_64.zip",
            "vesper-web-driver-linux-x86_64.tar.gz",
            "vesper-web-driver-linux-aarch64.tar.gz",
        ];
        let hash = "a".repeat(64);
        let mut contents = std::collections::HashMap::new();
        let mut assets = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let content = format!("{hash}  {name}\n").into_bytes();
            let id = index as u64 + 1;
            contents.insert(id, content.clone());
            assets.push(serde_json::json!({"name": name, "state":"uploaded", "size":42, "digest":format!("sha256:{hash}")}));
            assets.push(serde_json::json!({"name":format!("{name}.sha256"), "state":"uploaded", "size":content.len(), "id":id, "digest":format!("sha256:{}",digest(&content))}));
        }
        let release = serde_json::json!({"assets":assets});
        verify_publication_checksums(&release, |id| Ok(contents[&id].clone())).unwrap();
        assert!(verify_publication_checksums(&release, |_| Ok(b"tampered".to_vec())).is_err());
        let mut wrong = release.clone();
        wrong["assets"][0]["digest"] = serde_json::json!(format!("sha256:{}", "b".repeat(64)));
        assert!(verify_publication_checksums(&wrong, |id| Ok(contents[&id].clone())).is_err());
        wrong["assets"][0]["digest"] = serde_json::Value::Null;
        assert!(verify_publication_checksums(&wrong, |id| Ok(contents[&id].clone())).is_err());
    }

    #[test]
    fn persisted_cancellation_reaches_a_worker_in_another_host() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let mut record =
            crate::release_recovery::start_release("repo", "patch", "main", &"a".repeat(40))
                .unwrap();
        ledger.save(&record).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let _watcher =
            CheckpointCancellationWatcher::start(ledger.clone(), Arc::clone(&cancelled)).unwrap();
        record
            .transition(
                ReleaseRecoveryState::Cancelled,
                &"a".repeat(40),
                "user cancel",
                vec![],
                None,
            )
            .unwrap();
        ledger.save(&record).unwrap();
        // This fixture shares the native runner with Cargo/process tests; the
        // scheduling bound is separate from the production 100-ms poll interval.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cancelled.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn native_firewall_denial_cannot_be_bypassed_by_a_release_admission() {
        let temp = tempfile::tempdir().unwrap();
        let firewall = vesper_policy::firewall::CommandFirewall::compile(&[(
            "git",
            vesper_policy::firewall::RuleDecision::Deny,
            "test deny",
        )])
        .unwrap();
        let executor = NativeReleaseExecutor::new(temp.path(), Arc::new(AtomicBool::new(false)))
            .unwrap()
            .with_firewall(Some(Arc::new(firewall)));
        assert!(matches!(
            executor.command("git", &["status"]),
            Err(RrcError::MutationBlocked(_))
        ));
    }

    #[test]
    fn local_failure_retains_the_causal_error_for_bounded_diagnosis() {
        struct RedLocal;
        impl ReleaseExecutionPort for RedLocal {
            fn prepare_version_bump(
                &self,
                bump: &str,
                admission: &ReleaseMutationAdmission,
            ) -> Result<VersionBumpReceipt, RrcError> {
                FakeRelease.prepare_version_bump(bump, admission)
            }
            fn run_local_gate(&self, _: &LocalGateRecord) -> Result<String, RrcError> {
                Err(RrcError::Invalid(
                    "error[E0308]: exact local mismatch".into(),
                ))
            }
            fn commit_candidate(
                &self,
                _: &str,
                _: &ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                panic!("red local gate cannot commit")
            }
            fn push_candidate(&self, _: &ReleaseMutationAdmission) -> Result<String, RrcError> {
                panic!("red local gate cannot push")
            }
            fn create_and_push_tag(
                &self,
                _: &str,
                _: &str,
                _: &ReleaseMutationAdmission,
            ) -> Result<(String, String), RrcError> {
                panic!("red local gate cannot tag")
            }
            fn publication(
                &self,
                _: &str,
                _: &str,
                _: &str,
            ) -> Result<Option<PublicationReceipt>, RrcError> {
                panic!("red local gate cannot publish")
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let ledger = ReleaseLedger::open(temp.path().to_path_buf(), "repo").unwrap();
        let mut r =
            crate::release_recovery::start_release("repo", "patch", "main", &"a".repeat(40))
                .unwrap();
        r.mutation.version_after = Some("0.24.5".into());
        r.mutation.local_gates = production_local_gates();
        ledger.save(&r).unwrap();
        advance_release(
            &mut r,
            ReleaseAdvanceContext {
                workspace: temp.path(),
                repository: "owner/repo",
                ledger: &ledger,
                executor: &RedLocal,
                github: &GreenGithub,
                health: &HealthyStatus,
                repair_factory: None,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect_err("the captured nonzero local gate remains a failure result");
        assert_eq!(r.state, ReleaseRecoveryState::DiagnosingLocalFailure);
        assert_eq!(r.failures.len(), 1);
        assert!(
            r.failures[0]
                .causal_excerpt
                .contains("exact local mismatch")
        );
        assert_eq!(
            r.failures[0].class,
            crate::release_recovery::ReleaseFailureClass::CompileFailure
        );
        assert!(!r.mutation.candidate_committed);
    }

    #[test]
    fn native_repair_patch_keeps_version_seed_and_includes_new_regressions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("controller");
        let repair = temp.path().join("repair");
        fs::create_dir_all(&root).unwrap();
        let executor = NativeReleaseExecutor::new(&root, Arc::new(AtomicBool::new(false))).unwrap();
        executor.checked("git", &["init"]).unwrap();
        fs::write(root.join(".gitattributes"), "* text eol=lf\n").unwrap();
        fs::write(root.join("Cargo.toml"), "before\n").unwrap();
        fs::write(root.join("source.rs"), "broken\n").unwrap();
        executor.checked("git", &["add", "--all"]).unwrap();
        executor
            .checked(
                "git",
                &[
                    "-c",
                    "user.name=RRC Fixture",
                    "-c",
                    "user.email=rrc@example.invalid",
                    "commit",
                    "-m",
                    "base",
                ],
            )
            .unwrap();
        executor
            .checked(
                "git",
                &["worktree", "add", "--detach", repair.to_str().unwrap()],
            )
            .unwrap();
        fs::write(root.join("Cargo.toml"), "version seed\n").unwrap();
        let seed = executor
            .command("git", &["diff", "--binary", "HEAD"])
            .unwrap()
            .stdout;
        let worker = NativeReleaseExecutor::new(&repair, Arc::new(AtomicBool::new(false))).unwrap();
        apply_binary_patch(&worker, &seed).unwrap();
        let baseline = worker.checked("git", &["write-tree"]).unwrap();
        fs::write(repair.join("source.rs"), "repaired\n").unwrap();
        fs::write(repair.join("new_test.rs"), "regression\n").unwrap();
        worker.checked("git", &["add", "--all"]).unwrap();
        let delta = worker
            .command("git", &["diff", "--binary", baseline.trim()])
            .unwrap()
            .stdout;
        assert!(String::from_utf8_lossy(&delta).contains("new_test.rs"));
        assert!(!String::from_utf8_lossy(&delta).contains("version seed"));
        executor.checked("git", &["add", "Cargo.toml"]).unwrap();
        apply_binary_patch(&executor, &delta).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).unwrap(),
            "version seed\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("source.rs")).unwrap(),
            "repaired\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("new_test.rs")).unwrap(),
            "regression\n"
        );
    }
}

fn verified_publication_assets(release: &serde_json::Value) -> Result<Vec<String>, RrcError> {
    let assets = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| RrcError::Invalid("release assets missing".into()))?;
    let archives = [
        "agent-vesper-acp-linux-x86_64.tar.gz",
        "agent-vesper-acp-linux-aarch64.tar.gz",
        "agent-vesper-acp-darwin-x86_64.tar.gz",
        "agent-vesper-acp-darwin-aarch64.tar.gz",
        "agent-vesper-acp-windows-x86_64.zip",
        "vesper-web-driver-linux-x86_64.tar.gz",
        "vesper-web-driver-linux-aarch64.tar.gz",
    ];
    for archive in archives {
        for name in [archive.to_owned(), format!("{archive}.sha256")] {
            let matching = assets
                .iter()
                .filter(|asset| {
                    asset.get("name").and_then(serde_json::Value::as_str) == Some(&name)
                })
                .collect::<Vec<_>>();
            if matching.len() != 1
                || matching[0]
                    .get("size")
                    .and_then(serde_json::Value::as_u64)
                    .is_none_or(|size| size == 0)
                || matching[0].get("state").and_then(serde_json::Value::as_str) != Some("uploaded")
            {
                return Err(RrcError::MutationBlocked(format!(
                    "required release asset {name} is missing, duplicated, empty or unfinished"
                )));
            }
        }
    }
    Ok(assets
        .iter()
        .filter_map(|asset| asset.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect())
}

fn verify_publication_checksums(
    release: &serde_json::Value,
    mut read: impl FnMut(u64) -> Result<Vec<u8>, RrcError>,
) -> Result<(), RrcError> {
    let names = verified_publication_assets(release)?;
    let assets = release["assets"].as_array().expect("verified assets array");
    for name in names.iter().filter(|name| name.ends_with(".sha256")) {
        let archive_name = name.trim_end_matches(".sha256");
        let archive = assets
            .iter()
            .find(|asset| asset["name"].as_str() == Some(archive_name))
            .ok_or_else(|| RrcError::Invalid("checksum has no corresponding archive".into()))?;
        let checksum = assets
            .iter()
            .find(|asset| asset["name"].as_str() == Some(name))
            .expect("verified checksum entry");
        let server_digest = |asset: &serde_json::Value| -> Result<String, RrcError> {
            let value = asset["digest"]
                .as_str()
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .filter(|digest| {
                    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or_else(|| {
                    RrcError::Invalid("GitHub asset SHA-256 digest unavailable".into())
                })?;
            Ok(value.to_ascii_lowercase())
        };
        let expected = server_digest(archive)?;
        let id = checksum["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| RrcError::Invalid("checksum asset identity missing".into()))?;
        let bytes = read(id)?;
        if bytes.len() > 1024 || digest(&bytes) != server_digest(checksum)? {
            return Err(RrcError::MutationBlocked(
                "downloaded checksum does not match GitHub asset digest".into(),
            ));
        }
        let content = std::str::from_utf8(&bytes)
            .map_err(|_| RrcError::Invalid("checksum is not UTF-8".into()))?;
        let words = content
            .trim_start_matches('\u{feff}')
            .split_whitespace()
            .collect::<Vec<_>>();
        if words.len() != 2
            || words[0].to_ascii_lowercase() != expected
            || words[1].trim_start_matches('*') != archive_name
        {
            return Err(RrcError::MutationBlocked(
                "published archive checksum or filename mismatch".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CancellableGitHubStatusAdapter {
    cancelled: Arc<AtomicBool>,
}

impl ExternalHealthPort for CancellableGitHubStatusAdapter {
    fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
        let started = Instant::now();
        let mut command = Command::new("curl");
        command
            .args([
                "--disable",
                "--fail",
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
            .env_remove("GITHUB_TOKEN");
        let output =
            run_bounded_external_command(&mut command, &self.cancelled, EXTERNAL_HEALTH_WATCHDOG)?;
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
            .ok_or_else(|| RrcError::Invalid("official status indicator missing".into()))?;
        if !matches!(indicator, "none" | "minor" | "major" | "critical") {
            return Err(RrcError::Invalid(
                "official status indicator is unrecognized".into(),
            ));
        }
        if indicator != "none"
            && value
                .get("components")
                .and_then(serde_json::Value::as_array)
                .is_none_or(|components| {
                    !components.iter().any(|component| {
                        component.get("name").and_then(serde_json::Value::as_str) == Some("Actions")
                    })
                })
        {
            return Err(RrcError::Invalid(
                "official Actions component status is unconfirmed".into(),
            ));
        }
        let description = value
            .pointer("/status/description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("status description unavailable");
        Ok(OfficialStatusSnapshot {
            degraded: indicator != "none"
                && value
                    .get("components")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|components| {
                        components.iter().any(|component| {
                            component
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|name| name == "Actions" || name == "API Requests")
                                && component
                                    .get("status")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(|status| {
                                        matches!(
                                            status,
                                            "degraded_performance"
                                                | "partial_outage"
                                                | "major_outage"
                                        )
                                    })
                        })
                    }),
            summary: redact_secrets(&format!("{indicator}: {description}")),
            evidence_ref: OFFICIAL_STATUS_URL.into(),
        })
    }
}

// Internal composition seam: production always supplies the full native gate set.
struct RepairVerification<'a> {
    root: &'a Path,
    governor: Option<HostResourceGovernor>,
    verify: &'a dyn Fn(&NativeReleaseExecutor) -> Result<(), RrcError>,
}

fn repair_terminal_diagnostic(outcome: &vesper_agent::AgentTurnOutcome) -> String {
    match outcome {
        vesper_agent::AgentTurnOutcome::MaxIterationsReached { iterations, plan } => format!(
            "iteration safety ceiling after {iterations} turns; unfinished_plan={}",
            plan.is_some()
        ),
        vesper_agent::AgentTurnOutcome::Interrupted {
            cause,
            tool_call_started,
            iterations,
            ..
        } => redact_secrets(&format!(
            "provider interruption after {iterations} turns; cause={cause:?}; tool_call_started={tool_call_started}; ambiguous calls are never replayed"
        )),
        vesper_agent::AgentTurnOutcome::Acceptance {
            report, iterations, ..
        } => format!(
            "acceptance outcome after {iterations} turns; verified={}; coding repair proof remains required",
            report.is_verified()
        ),
        vesper_agent::AgentTurnOutcome::Completed { iterations, .. } => {
            format!("normal stop after {iterations} turns")
        }
    }
}

fn repair_admission_counts(
    record: &ReleaseRecoveryRecord,
    families: &BTreeSet<String>,
) -> Result<Vec<(String, u8)>, RrcError> {
    families
        .iter()
        .map(|family| {
            let key = digest(family.as_bytes());
            let observed = u8::try_from(
                record
                    .repair_attempts
                    .iter()
                    .filter(|attempt| &attempt.causal_family == family)
                    .count(),
            )
            .unwrap_or(u8::MAX);
            let used = record
                .mutation
                .repair_admissions
                .get(&key)
                .copied()
                .unwrap_or(0)
                .max(observed);
            if used >= record.retry_budget.repair_attempt_limit_per_family {
                return Err(RrcError::RepairBudgetExhausted(family.clone()));
            }
            Ok((key, used.saturating_add(1)))
        })
        .collect()
}

fn reserve_repair_dispatch(
    record: &mut ReleaseRecoveryRecord,
    families: &BTreeSet<String>,
    ledger: &ReleaseLedger,
) -> Result<(), RrcError> {
    let reservations = repair_admission_counts(record, families)?;
    let previous = record.mutation.repair_admissions.clone();
    record.mutation.repair_admissions.extend(reservations);
    record.updated_at = Utc::now();
    if let Err(error) = ledger.save(record) {
        record.mutation.repair_admissions = previous;
        return Err(error);
    }
    Ok(())
}

fn run_bounded_repair_agent_with_verification(
    workspace: &Path,
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    factory: &crate::WorkerFactory,
    cancelled: Arc<AtomicBool>,
    verification: RepairVerification<'_>,
) -> Result<(), RrcError> {
    let local = record.state == ReleaseRecoveryState::DiagnosingLocalFailure;
    let failures = if local {
        record
            .failures
            .last()
            .cloned()
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        repairable_failures(record)
    };
    let failure = failures
        .last()
        .cloned()
        .ok_or_else(|| RrcError::Invalid("focused repair has no admitted causal failure".into()))?;
    if (!local
        && !matches!(
            record.state,
            ReleaseRecoveryState::ClassifyingFailure | ReleaseRecoveryState::DiagnosingRepair
        ))
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
    if !local && record.retry_budget.full_gate_used >= record.retry_budget.full_gate_limit {
        return Err(RrcError::RetryBlocked(
            "full-gate budget exhausted; further repair promotion forbidden".into(),
        ));
    }
    let families = failures
        .iter()
        .map(|failure| format!("{}:{}", failure.workflow_name, failure.job_name))
        .collect::<BTreeSet<_>>();
    repair_admission_counts(record, &families)?;
    let base = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("repair candidate commit is missing".into()))?;
    let repair_root = verification
        .root
        .join("worktrees")
        .join(digest(record.repo_identity.as_bytes()))
        .join(repair_worktree_leaf(
            &record.epoch_id,
            record.repair_attempts.len(),
            Utc::now().timestamp_micros(),
        ));
    if repair_root.exists() {
        return Err(RrcError::Invalid(format!(
            "release repair worktree already exists: {}",
            repair_root.display()
        )));
    }
    let controller = NativeReleaseExecutor::new(workspace, Arc::clone(&cancelled))?
        .with_firewall(factory.config.firewall.clone());
    let original_status = controller.git_status()?;
    let original_patch = controller.command("git", &["diff", "--binary", "HEAD"])?;
    if !original_patch.status.success() {
        return Err(RrcError::Invalid("repair baseline diff failed".into()));
    }
    if local {
        for path in &original_status.paths {
            if !record
                .mutation
                .version_files
                .iter()
                .chain(record.mutation.repair_files.iter())
                .any(|allowed| allowed == path)
                && path != "Cargo.lock"
            {
                return Err(RrcError::MutationBlocked(
                    "local repair baseline contains unowned workspace changes".into(),
                ));
            }
        }
    } else if !original_status.paths.is_empty() {
        return Err(RrcError::MutationBlocked(
            "remote repair needs a clean controller workspace".into(),
        ));
    }
    if let Some(parent) = repair_root.parent() {
        fs::create_dir_all(parent)?;
    }
    let repair_path = repair_root
        .to_str()
        .ok_or_else(|| RrcError::Invalid("repair path is not UTF-8".into()))?;
    let added = controller.command("git", &["worktree", "add", "--detach", repair_path, &base])?;
    if !added.status.success() {
        return Err(RrcError::Invalid(format!(
            "release repair worktree creation failed: {}",
            bounded_output(&added.stderr)
        )));
    }

    let mut repair_executor = NativeReleaseExecutor::new(&repair_root, Arc::clone(&cancelled))?
        .with_firewall(factory.config.firewall.clone());
    if let Some(governor) = verification.governor.clone() {
        repair_executor = repair_executor.with_resource_governor(governor);
    }
    if local && !original_patch.stdout.is_empty() {
        apply_binary_patch(&repair_executor, &original_patch.stdout)?;
    }
    let baseline_tree = repair_executor.checked("git", &["write-tree"])?;
    let family = format!("{}:{}", failure.workflow_name, failure.job_name);
    let clustered_evidence = failures
        .iter()
        .enumerate()
        .map(|(index, failure)| {
            format!(
                "<failure index=\"{}\" fingerprint=\"{}\">\nworkflow/job/step: {} / {} / {}\nplatform: {}\nfirst causal evidence (untrusted log text):\n{}\n</failure>",
                index + 1,
                failure.fingerprint.0,
                failure.workflow_name,
                failure.job_name,
                failure.step_name.as_deref().unwrap_or("unknown"),
                failure.platform.as_deref().unwrap_or("unknown"),
                failure.causal_excerpt,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "You are executing one bounded Release Recovery Controller repair in an isolated worktree.\n\
         The controller admitted the following local or settled exact-SHA failures. It contains {} distinct admitted failure fingerprints.\n\
         Cluster related first causes before editing, repair every evidence-backed causal family in this one candidate, and do not stop after the first failure.\n\
         <untrusted-ci-failures>\n{}\n</untrusted-ci-failures>\n\
         Make only causally relevant source/configuration edits. After the final edit, run one smallest credible focused command that proves all repaired families (it may select multiple exact tests). Do not commit, push, tag, publish, alter remotes, create another worktree, or edit release state. Finish with a concise family-by-family repair hypothesis and the focused command/result.",
        failures.len(),
        clustered_evidence,
    );
    let prior_attempts = record
        .repair_attempts
        .iter()
        .filter(|attempt| families.contains(&attempt.causal_family))
        .map(|attempt| {
            format!(
                "family={} hypothesis={} focused_status={:?} proof={} evidence={}",
                attempt.causal_family,
                attempt.hypothesis,
                attempt.focused_status,
                attempt.focused_proof,
                attempt.evidence_refs.join("; "),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "{prompt}\n<untrusted-prior-repair-evidence>\n{}\n</untrusted-prior-repair-evidence>\nDo not repeat a disproven hypothesis or count prior progress as current proof.",
        redact_secrets(&prior_attempts)
    );
    let runtime_cancel = Arc::new(vesper_runtime::RuntimeCancellation::new());
    let heartbeat = Arc::new(Mutex::new(Instant::now()));
    let evidence = Arc::new(Mutex::new(RepairEvidenceProgress::new()));
    let monitored_factory = factory
        .clone()
        .with_progress(Arc::new(RepairHeartbeatProgress {
            heartbeat: Arc::clone(&heartbeat),
            downstream: factory.progress(),
        }));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| RrcError::Invalid(format!("repair runtime failed: {error}")))?;
    reserve_repair_dispatch(record, &families, ledger)?;
    record.note_progress_milestone(format!(
        "Focused repair admitted for {} causal families; bounded continuation is active",
        families.len()
    ));
    ledger.save(record)?;
    let repair_started = Instant::now();
    let turn = runtime.block_on(async {
        let tools = match verification.governor.clone() {
            Some(governor) => build_repair_tool_registry(
                Some(governor),
                false,
                Some(RepairCommandProgress {
                    heartbeat: Arc::clone(&heartbeat),
                    evidence: Some(Arc::clone(&evidence)),
                    activity: active_workers().lock().ok().and_then(|workers| {
                        workers
                            .get(&record.repo_identity)
                            .map(|worker| Arc::clone(&worker.activity))
                    }),
                }),
            ),
            None => {
                // The substituted fixture-wide gate port is test-only.
                #[cfg(test)]
                {
                    build_repair_tool_registry(
                        None,
                        true,
                        Some(RepairCommandProgress {
                            heartbeat: Arc::clone(&heartbeat),
                            activity: None,
                            evidence: Some(Arc::clone(&evidence)),
                        }),
                    )
                }
                #[cfg(not(test))]
                {
                    repair_tool_registry_with_governor(None)
                }
            }
        };
        let turn = monitored_factory.run_coding_turn_in_workspace_with_registry(
            repair_root.clone(),
            prompt,
            Arc::clone(&runtime_cancel),
            tools,
        );
        tokio::pin!(turn);
        loop {
            tokio::select! {
                result = &mut turn => break result.map_err(RrcError::Invalid),
                () = tokio::time::sleep(WATCHDOG_POLL_INTERVAL) => {
                    let last_progress = heartbeat
                        .lock()
                        .map(|observed| *observed)
                        .unwrap_or(repair_started);
                    if let Some(error) = repair_watchdog_error(
                        cancelled.load(Ordering::Acquire),
                        last_progress.elapsed(),
                        REPAIR_HEARTBEAT_TIMEOUT,
                    ).or_else(|| evidence.lock().ok().and_then(|p| p.watchdog_error())) {
                        runtime_cancel.cancel();
                        let _ = tokio::time::timeout(REPAIR_CANCEL_GRACE, &mut turn).await;
                        break Err(error);
                    }
                }
            }
        }
    });
    record.metrics.model_active_millis = record
        .metrics
        .model_active_millis
        .saturating_add(u64::try_from(repair_started.elapsed().as_millis()).unwrap_or(u64::MAX));
    record.updated_at = Utc::now();
    ledger.save(record)?;
    let (outcome, history) = turn?;
    // A provider can finish between watchdog ticks. Completion prose cannot
    // bypass the evidence limit reached by its last repeated tool action.
    if let Some(error) = evidence.lock().ok().and_then(|p| p.watchdog_error()) {
        return Err(error);
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(RrcError::Cancelled);
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
            return Err(RrcError::Invalid(format!(
                "repair agent stopped before verified completion: {}",
                repair_terminal_diagnostic(&outcome)
            )));
        }
    };
    if assistant_summary.trim().is_empty() {
        return Err(RrcError::Invalid(
            "repair has no recorded diagnosis/hypothesis".into(),
        ));
    }
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
    repair_executor.checked("git", &["add", "--all"])?;
    let verified_baseline = repair_executor.repair_snapshot()?;
    // Freeze the exact delta before proof. Later command/process side effects
    // cannot change which bytes are selected for promotion.
    let diff = repair_executor.git_bytes(&["diff", "--binary", baseline_tree.trim()])?;
    if diff.is_empty() {
        return Err(RrcError::Invalid(
            "repair worktree produced no promotable diff".into(),
        ));
    }
    let focused_words = focused_command.split_whitespace().collect::<Vec<_>>();
    let verification = (|| {
        repair_executor.require_repair_mutations_indexed(&history)?;
        for admitted in &failures {
            run_focused_verification(&repair_executor, &focused_words, admitted)?;
            repair_executor.require_repair_snapshot(&verified_baseline)?;
        }
        (verification.verify)(&repair_executor)?;
        repair_executor.require_repair_snapshot(&verified_baseline)?;
        repair_executor.checked(
            "git",
            &["diff", "--cached", "--check", baseline_tree.trim()],
        )?;
        Ok::<_, RrcError>(())
    })();
    if let Err(error) = verification {
        let repair = crate::release_recovery::RepairAttempt {
            fingerprint: failure.fingerprint.clone(),
            causal_family: family,
            hypothesis: redact_secrets(&assistant_summary),
            source_commit_before: base,
            source_commit_after: None,
            focused_proof: focused_command,
            focused_status: crate::release_recovery::FocusedProofStatus::Failed,
            evidence_refs: vec![
                format!("repair:failed:{}", digest(error.to_string().as_bytes())),
                format!(
                    "repair:verification-detail:{}",
                    redact_secrets(&error.to_string())
                        .chars()
                        .take(MAX_COMMAND_OUTPUT)
                        .collect::<String>()
                ),
            ],
            disproven_or_insufficient: true,
        };
        if local {
            record.record_repair(repair)?;
        } else {
            apply_controller_event(record, ReleaseControllerEvent::FailedRepair(repair))?;
        }
        ledger.save(record)?;
        return Ok(());
    }
    let root_status = controller.git_status()?;
    if (!local && !root_status.paths.is_empty()) || (local && root_status != original_status) {
        return Err(RrcError::Invalid(
            "repair promotion requires the controller workspace to remain clean".into(),
        ));
    }
    let head = controller.checked("git", &["rev-parse", "HEAD"])?;
    if head.trim() != base {
        return Err(RrcError::MutationBlocked(
            "controller HEAD changed since repair base".into(),
        ));
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(RrcError::Invalid(
            "release repair cancelled before promotion".into(),
        ));
    }
    if local {
        let current_patch = controller.command("git", &["diff", "--binary", "HEAD"])?;
        if !current_patch.status.success() || current_patch.stdout != original_patch.stdout {
            return Err(RrcError::MutationBlocked(
                "local repair baseline changed during focused verification".into(),
            ));
        }
        let mut paths = vec![
            "Cargo.toml".into(),
            "Cargo.lock".into(),
            "registry/agent.json".into(),
        ];
        paths.extend(record.mutation.version_files.iter().cloned());
        paths.extend(record.mutation.repair_files.iter().cloned());
        controller.stage_admitted_paths(&paths)?;
    }
    apply_binary_patch(&controller, &diff)?;
    if local {
        let paths =
            controller.git_bytes(&["diff", "--cached", "--name-only", "--no-renames", "-z"])?;
        record.mutation.repair_files = literal_git_paths(&paths)?;
        let mut evidence_refs = vec![
            format!("repair:patch:{}", digest(&diff)),
            "repair:local-gates".into(),
        ];
        if record.mutation.version_after.is_none() {
            let promoted = controller.command("git", &["diff", "--binary", "HEAD"])?;
            if !promoted.status.success() {
                return Err(RrcError::MutationBlocked(
                    "promoted preparation baseline is unavailable".into(),
                ));
            }
            evidence_refs.push(format!(
                "repair:preparation-baseline:{}",
                digest(&promoted.stdout)
            ));
        }
        record.record_repair(crate::release_recovery::RepairAttempt {
            fingerprint: failure.fingerprint,
            causal_family: family,
            hypothesis: redact_secrets(&assistant_summary),
            source_commit_before: base,
            source_commit_after: None,
            focused_proof: focused_command,
            focused_status: crate::release_recovery::FocusedProofStatus::Passed,
            evidence_refs,
            disproven_or_insufficient: false,
        })?;
        for gate in &mut record.mutation.local_gates {
            gate.state = SettlementState::NotStarted;
            gate.evidence_ref = None;
        }
        let commit = record.active_commit().unwrap_or("unknown").to_owned();
        record.transition(
            ReleaseRecoveryState::LocalVerification,
            &commit,
            "focused local repair promoted; candidate gates must rerun before commit",
            vec!["repair:local-gates".into()],
            None,
        )?;
        ledger.save(record)?;
        return Ok(());
    }
    let fingerprint_short = failure.fingerprint.0.chars().take(12).collect::<String>();
    controller.checked(
        "git",
        &[
            "commit",
            "-m",
            &format!("fix(release): repair {fingerprint_short}"),
        ],
    )?;
    let commit = exact_sha(controller.checked("git", &["rev-parse", "HEAD"])?.trim())?;
    let patch_digest = digest(&diff);
    let repairs = failures
        .into_iter()
        .map(|failure| crate::release_recovery::RepairAttempt {
            causal_family: format!("{}:{}", failure.workflow_name, failure.job_name),
            fingerprint: failure.fingerprint,
            hypothesis: redact_secrets(&assistant_summary)
                .chars()
                .take(1024)
                .collect(),
            source_commit_before: base.clone(),
            source_commit_after: Some(commit.clone()),
            focused_proof: focused_command.clone(),
            focused_status: crate::release_recovery::FocusedProofStatus::Passed,
            evidence_refs: vec![
                format!("repair:patch:{patch_digest}"),
                format!("github:job:{}", failure.job_id),
                "repair:agent-tool-history".into(),
            ],
            disproven_or_insufficient: false,
        })
        .collect();
    apply_controller_event(record, ReleaseControllerEvent::VerifiedRepairBatch(repairs))?;
    record.mutation.final_candidate_commit = record.active_commit().map(str::to_owned);
    ledger.save(record)?;
    let _ = controller.command("git", &["worktree", "remove", "--force", repair_path]);
    Ok(())
}

fn apply_binary_patch(executor: &NativeReleaseExecutor, bytes: &[u8]) -> Result<(), RrcError> {
    use std::io::Write as _;
    let mut patch = tempfile::NamedTempFile::new()?;
    patch.write_all(bytes)?;
    let path = patch
        .path()
        .to_str()
        .ok_or_else(|| RrcError::Invalid("repair patch path is not UTF-8".into()))?;
    executor.checked("git", &["apply", "--binary", "--index", path])?;
    Ok(())
}

/// The repair role can edit source and execute supported verification, but cannot
/// directly perform controller-owned Git/GitHub lifecycle operations.
#[cfg(test)]
pub(crate) fn repair_tool_registry() -> vesper_agent::ToolRegistry {
    build_repair_tool_registry(None, true, None)
}

fn repair_tool_registry_with_governor(
    governor: Option<HostResourceGovernor>,
) -> vesper_agent::ToolRegistry {
    build_repair_tool_registry(governor, false, None)
}

#[derive(Clone)]
struct RepairCommandProgress {
    heartbeat: Arc<Mutex<Instant>>,
    activity: Option<Arc<Mutex<ReleaseWorkerActivity>>>,
    evidence: Option<Arc<Mutex<RepairEvidenceProgress>>>,
}

fn build_repair_tool_registry(
    governor: Option<HostResourceGovernor>,
    fixture_commands: bool,
    progress: Option<RepairCommandProgress>,
) -> vesper_agent::ToolRegistry {
    struct RepairTools {
        inner: vesper_agent::ToolRegistry,
        governor: Option<HostResourceGovernor>,
        fixture_commands: bool,
        progress: Option<RepairCommandProgress>,
    }
    impl vesper_agent::ToolService for RepairTools {
        fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
            self.inner
                .definitions_for(vesper_domain::SessionOperatingMode::Code)
        }
        fn execute<'a>(
            &'a self,
            call: &'a vesper_domain::ToolCall,
            context: &'a vesper_agent::ToolContext,
        ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
        {
            Box::pin(async move {
                let evidence = self.progress.as_ref().and_then(|p| p.evidence.clone());
                if let Some(evidence) = &evidence
                    && let Some(error) = evidence.lock().ok().and_then(|p| p.watchdog_error())
                {
                    return Err(vesper_agent::ToolError::Failed(error.to_string()));
                }
                let governed_verification =
                    call.tool_id.as_str() == "run_command" && !self.fixture_commands;
                if governed_verification
                    && let Some(evidence) = &evidence
                    && let Ok(mut observed) = evidence.lock()
                {
                    observed.verification_active = observed.verification_active.saturating_add(1);
                }
                let _activity = RepairVerificationActivity(
                    governed_verification.then(|| evidence.clone()).flatten(),
                );
                let result = self.execute_checked(call, context).await;
                if let Some(evidence) = &evidence
                    && let Ok(mut observed) = evidence.lock()
                {
                    observed.observe(call, &result);
                }
                result
            })
        }
    }
    impl RepairTools {
        fn execute_checked<'a>(
            &'a self,
            call: &'a vesper_domain::ToolCall,
            context: &'a vesper_agent::ToolContext,
        ) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>>
        {
            if matches!(
                call.tool_id.as_str(),
                "write_file" | "edit_file" | "apply_patch"
            ) && call
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| {
                    path.split(['/', '\\'])
                        .any(|component| component.eq_ignore_ascii_case(".git"))
                })
            {
                return Box::pin(async move {
                    Err(vesper_agent::ToolError::InvalidArguments {
                        tool: call.tool_id.as_str().into(),
                        reason: "RRC repair tools cannot write Git metadata".into(),
                    })
                });
            }
            if call.tool_id.as_str() == "run_command"
                && !call
                    .arguments
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(credible_focused_command)
            {
                return Box::pin(async move {
                    Err(vesper_agent::ToolError::InvalidArguments {
                        tool: "run_command".into(),
                        reason: "RRC repair commands are restricted to supported focused verification; Git/GitHub lifecycle operations require controller admission".into(),
                    })
                });
            }
            if call.tool_id.as_str() == "run_command"
                && call
                    .arguments
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|command| command.split_whitespace().next() == Some("cargo"))
                && !self.fixture_commands
            {
                return run_governed_repair_command(
                    call,
                    context,
                    self.governor.clone(),
                    self.progress.clone(),
                );
            }
            self.inner.execute(call, context)
        }
    }
    vesper_agent::ToolRegistry::empty().with_service(Arc::new(RepairTools {
        inner: vesper_agent::ToolRegistry::parity_default(),
        governor,
        fixture_commands,
        progress,
    }))
}

struct RepairCommandCancellation {
    cancelled: Arc<AtomicBool>,
    settled: bool,
}

impl Drop for RepairCommandCancellation {
    fn drop(&mut self) {
        if !self.settled {
            self.cancelled.store(true, Ordering::Release);
        }
    }
}

struct RepairCommandSignalWatcher {
    done: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for RepairCommandSignalWatcher {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_governed_repair_command<'a>(
    call: &'a vesper_domain::ToolCall,
    context: &'a vesper_agent::ToolContext,
    governor: Option<HostResourceGovernor>,
    progress: Option<RepairCommandProgress>,
) -> vesper_agent::ToolFuture<'a, Result<vesper_agent::ToolResult, vesper_agent::ToolError>> {
    Box::pin(async move {
        use vesper_agent::ToolError;
        let governor = governor.ok_or_else(|| {
            ToolError::Failed(
                "RRC repair Cargo requires the controller-owned resource governor".into(),
            )
        })?;
        let command = call
            .arguments
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "run_command".into(),
                reason: "missing command".into(),
            })?;
        let words = command.split_whitespace().collect::<Vec<_>>();
        if words.iter().any(|word| {
            word.starts_with("--target-dir")
                || word.starts_with("--jobs")
                || word.starts_with("-j")
                || word.starts_with("--test-threads")
                || word.starts_with("--config")
                || word.starts_with("--manifest-path")
        }) {
            return Err(ToolError::InvalidArguments {
                tool: "run_command".into(),
                reason:
                    "repair Cargo cannot override the controller's resource policy or workspace"
                        .into(),
            });
        }
        // Native host verification cannot silently bypass a requested OS sandbox.
        // The existing sandbox port has no managed-cache/environment binding.
        if let Some(route) = context.sandbox.as_deref() {
            return Err(ToolError::Failed(if route.satisfies_demand() {
                "selected sandbox has no governed RRC Cargo route; refusing host execution".into()
            } else {
                route.refusal_text()
            }));
        }
        let workspace = context
            .workspace_roots
            .iter()
            .find(|root| root.primary)
            .or_else(|| context.workspace_roots.first())
            .ok_or_else(|| ToolError::Failed("repair workspace is missing".into()))?;
        let workspace = PathBuf::from(workspace.path.as_str());
        let signal = context.cancellation.clone();
        let cancelled = Arc::new(AtomicBool::new(signal.is_cancelled()));
        let mut guard = RepairCommandCancellation {
            cancelled: Arc::clone(&cancelled),
            settled: false,
        };
        let firewall = context.firewall.clone();
        let words = words.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let result = tokio::task::spawn_blocking(move || {
            let done = Arc::new(AtomicBool::new(false));
            let observed_done = Arc::clone(&done);
            let observed_cancelled = Arc::clone(&cancelled);
            let watcher = thread::spawn(move || {
                while !observed_done.load(Ordering::Acquire) {
                    if signal.is_cancelled() {
                        observed_cancelled.store(true, Ordering::Release);
                        break;
                    }
                    thread::sleep(WATCHDOG_POLL_INTERVAL);
                }
            });
            let _watcher = RepairCommandSignalWatcher {
                done,
                worker: Some(watcher),
            };
            // Hold the exclusive lease through native owned-child settlement.
            (|| {
                let mut executor = NativeReleaseExecutor::new(&workspace, cancelled)?
                    .with_firewall(firewall)
                    .with_resource_governor(governor);
                if let Some(progress) = progress {
                    executor.repair_heartbeat = Some(progress.heartbeat);
                    executor.activity = progress.activity;
                }
                let admission = executor.admit_local_resources(GateCost::Expensive)?;
                let args = words[1..].iter().map(String::as_str).collect::<Vec<_>>();
                if let Some(activity) = &executor.activity
                    && let Ok(mut activity) = activity.lock()
                {
                    activity.current_gate = None;
                    activity.current_command = Some(format_command(&words[0], &args));
                    activity.gate_started_at = Some(Instant::now());
                }
                executor.checked_with_admitted_resources(&words[0], &args, &admission)
            })()
        })
        .await
        .map_err(|error| ToolError::Failed(format!("governed repair task failed: {error}")))?;
        guard.settled = true;
        let output =
            result.map_err(|error| ToolError::Failed(redact_secrets(&error.to_string())))?;
        vesper_agent::ToolResult::new(output)
    })
}

fn cargo_command<'a>(words: &[&'a str]) -> Option<&'a str> {
    match words {
        ["cargo", toolchain, command, ..] if toolchain.starts_with('+') => Some(command),
        ["cargo", command, ..] => Some(command),
        _ => None,
    }
}

fn run_focused_verification(
    executor: &NativeReleaseExecutor,
    words: &[&str],
    failure: &crate::release_recovery::FailureRecord,
) -> Result<(), RrcError> {
    if matches!(
        failure.class,
        crate::release_recovery::ReleaseFailureClass::TestRegression
            | crate::release_recovery::ReleaseFailureClass::FlakyOrTimingSensitiveTest
    ) && !(cargo_command(words) == Some("test")
        || matches!(words, ["python" | "python3" | "node", ..]))
    {
        return Err(RrcError::Invalid(
            "test failure requires an executed regression, not compilation alone".into(),
        ));
    }
    let output = if executor.resource_governor.is_some() && words[0] == "cargo" {
        let admission = executor.admit_local_resources(GateCost::Expensive)?;
        let environment = admission.cargo.environment();
        let environment = environment
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        executor.command_with_env_and_policy(
            words[0],
            &words[1..],
            &environment,
            LOCAL_GATE_WATCHDOG,
        )?
    } else {
        executor.command_with_policy(words[0], &words[1..], LOCAL_GATE_WATCHDOG)?
    };
    if !output.status.success() {
        return Err(RrcError::Invalid(format!(
            "focused verification failed: {}",
            crate::release_recovery::first_causal_excerpt(&format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
        )));
    }
    if cargo_command(words) == Some("test") {
        let output = String::from_utf8_lossy(&output.stdout);
        if let Some(test) = failed_rust_test(&failure.causal_excerpt)
            && !output
                .lines()
                .any(|line| line.trim() == format!("test {test} ... ok"))
        {
            return Err(RrcError::Invalid(format!(
                "focused proof did not execute failing test {test}"
            )));
        }
        if !cargo_test_executed(&output) {
            return Err(RrcError::Invalid(
                "focused cargo test matched zero passing tests".into(),
            ));
        }
    }
    Ok(())
}

fn failed_rust_test(excerpt: &str) -> Option<String> {
    regex::Regex::new(r"thread '([^']+)'(?: \([0-9]+\))? panicked")
        .expect("static Rust panic regex")
        .captures(excerpt)
        .map(|captures| captures[1].to_owned())
        .filter(|name| name != "main" && name != "<unnamed>")
}

fn cargo_test_executed(output: &str) -> bool {
    regex::Regex::new(r"test result: ok\.\s+([1-9][0-9]*) passed")
        .expect("static test result regex")
        .is_match(output)
}

fn credible_focused_command(command: &str) -> bool {
    // Conservative admission. Compound shell commands may hide failed proof;
    // unsupported tools require intervention rather than a made-up pass.
    if command.contains([
        ';', '|', '&', '\n', '\r', '`', '$', '>', '<', '(', ')', '\\', '\'', '"',
    ]) {
        return false;
    }
    let words = command.split_whitespace().collect::<Vec<_>>();
    if words.iter().any(|word| {
        matches!(
            *word,
            "--list" | "--no-run" | "--help" | "-h" | "--version" | "-V"
        )
    }) {
        return false;
    }
    let normalized = if words.get(1).is_some_and(|word| word.starts_with('+')) {
        if words[1].len() == 1
            || !words[1][1..].chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
            })
        {
            return false;
        }
        words
            .iter()
            .enumerate()
            .filter_map(|(index, word)| (index != 1).then_some(*word))
            .collect::<Vec<_>>()
    } else {
        words.clone()
    };
    matches!(
        normalized.as_slice(),
        ["cargo", "test" | "check" | "clippy", ..]
            | [
                "cargo",
                "xtask",
                "acceptance" | "architecture" | "verify" | "fixtures" | "msrv",
                ..
            ]
    ) || matches!(words.as_slice(), ["python" | "python3" | "node", script, ..] if script.contains("test") && !script.starts_with('-'))
}

fn release_stage_commands(record: &ReleaseRecoveryRecord, repository: &str) -> Vec<String> {
    if record.state == ReleaseRecoveryState::Complete
        && record.mutation.publication_verified
        && record.mutation.closeout_receipt.is_none()
    {
        let mut commands = vec!["write local release execution report and evidence links".into()];
        if let Some(target) = &record.mutation.closeout_registry {
            commands.push(format!(
                "gh api --method PUT {} branch={}",
                target.endpoint(),
                target.branch
            ));
        }
        return commands;
    }
    match record.state {
        ReleaseRecoveryState::ClassifyingFailure
            if record
                .failures
                .last()
                .is_some_and(|failure| failure.class.infrastructure_like()) =>
        {
            if !record
                .retry_admission(crate::release_recovery::RetryKind::Infrastructure)
                .admitted
            {
                return Vec::new();
            }
            record
                .required_gates
                .iter()
                .filter(|gate| gate.has_failure())
                .filter_map(|gate| gate.run_id)
                .map(|run| {
                    format!("gh api --method POST repos/{repository}/actions/runs/{run}/rerun")
                })
                .collect()
        }
        ReleaseRecoveryState::CandidateReady | ReleaseRecoveryState::RetryAdmissible => {
            vec![format!(
                "git push origin {}:refs/heads/main",
                record.active_commit().unwrap_or("unknown")
            )]
        }
        ReleaseRecoveryState::RemoteGatesGreen => vec![
            format!(
                "git tag -a v{} {}",
                record.release_version.as_deref().unwrap_or("unknown"),
                record.active_commit().unwrap_or("unknown")
            ),
            format!(
                "git push origin v{}",
                record.release_version.as_deref().unwrap_or("unknown")
            ),
        ],
        ReleaseRecoveryState::LocalVerification => production_local_gates()
            .into_iter()
            .map(|gate| gate.command)
            .chain([format!(
                "git commit -m Release v{}",
                record
                    .mutation
                    .version_after
                    .as_deref()
                    .unwrap_or("pending")
            )])
            .collect(),
        _ => vec![
            "git apply --binary --index".into(),
            "git commit -m fix(release)".into(),
        ],
    }
}

fn authorize_controller_step(
    factory: Option<&crate::WorkerFactory>,
    record: &ReleaseRecoveryRecord,
    repository: &str,
    cancelled: &AtomicBool,
) -> Result<(), RrcError> {
    let factory = factory.ok_or_else(|| {
        RrcError::AuthorizationBlocked("release mutation requires a host permission port".into())
    })?;
    let tool_id = vesper_domain::ToolId::new("release_controller").expect("static tool id");
    let call = vesper_domain::ToolCall {
        id: vesper_domain::ToolCallId::new("rrc-approval").expect("static call id"),
        tool_id: tool_id.clone(),
        arguments: serde_json::json!({"stage": format!("{:?}", record.state),
        "candidate": record.active_commit(), "version": record.release_version,
        "operation": match record.state {
            ReleaseRecoveryState::LocalVerification => "version preparation, verification or candidate commit",
            ReleaseRecoveryState::CandidateReady | ReleaseRecoveryState::RetryAdmissible => "push the exact admitted candidate to origin/main",
            ReleaseRecoveryState::RemoteGatesGreen => "create and push immutable release tag; this starts publication",
            ReleaseRecoveryState::Complete => "update the existing registry PR branch and write local execution report/evidence links",
            ReleaseRecoveryState::ClassifyingFailure if record.failures.last().is_some_and(|failure| failure.class.infrastructure_like()) => "rerun the exact admitted failed infrastructure gates on GitHub",
            _ => "bounded focused repair, local checks and verified commit promotion",
        }}),
        extensions: Default::default(),
    };
    let definition = vesper_domain::ToolDefinition {
        id: tool_id,
        harness_name: vesper_domain::HarnessToolName::new("release_controller")
            .expect("static tool name"),
        provider_name: None,
        description: "Release controller side effect".into(),
        input_schema: serde_json::json!({"type":"object"}),
        execution_class: vesper_domain::ToolExecutionClass::Mutating,
        provider_scope: Default::default(),
        extensions: Default::default(),
        defer_loading: false,
    };
    let context = vesper_agent::ToolContext {
        workspace_roots: factory.config.workspace_roots.clone(),
        firewall: factory.config.firewall.clone(),
        sandbox: factory.config.sandbox.clone(),
        provider_id: factory.config.provider_id.clone(),
        operating_mode: factory.release_mode,
        permission_mode: factory.release_permission,
        conversation: Vec::new(),
        cancellation: Arc::new(vesper_runtime::RuntimeCancellation::new()),
    };
    let stage_commands = release_stage_commands(record, repository);
    let mut firewall_approval = false;
    if let Some(firewall) = &factory.config.firewall {
        for command in stage_commands {
            match firewall.scan(&command).decision {
                vesper_policy::firewall::RuleDecision::Deny => {
                    return Err(RrcError::AuthorizationBlocked(
                        "command firewall denied release stage".into(),
                    ));
                }
                vesper_policy::firewall::RuleDecision::RequireApproval => firewall_approval = true,
                vesper_policy::firewall::RuleDecision::Allow => {}
            }
        }
    }
    match vesper_agent::check_tool_permission(
        context.operating_mode,
        context.permission_mode,
        definition.execution_class,
    ) {
        vesper_agent::PermissionDecision::Allow if !firewall_approval => return Ok(()),
        vesper_agent::PermissionDecision::Allow => {}
        vesper_agent::PermissionDecision::Deny(reason) => {
            return Err(RrcError::AuthorizationBlocked(redact_secrets(&reason)));
        }
        vesper_agent::PermissionDecision::Ask(_) => {}
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let decision = runtime.block_on(async {
        let request = factory.permission.authorize(&call, &definition, &context);
        tokio::pin!(request);
        let deadline = tokio::time::sleep(Duration::from_secs(5 * 60));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                result = &mut request => return result,
                _ = &mut deadline => return vesper_agent::PermissionDecision::Deny("approval timed out".into()),
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if cancelled.load(Ordering::Acquire) { return vesper_agent::PermissionDecision::Deny("release cancelled".into()); }
                }
            }
        }
    });
    match decision {
        vesper_agent::PermissionDecision::Allow => Ok(()),
        _ if cancelled.load(Ordering::Acquire) => Err(RrcError::Cancelled),
        vesper_agent::PermissionDecision::Deny(reason)
        | vesper_agent::PermissionDecision::Ask(reason) => {
            Err(RrcError::AuthorizationBlocked(redact_secrets(&reason)))
        }
    }
}

struct CheckpointCancellationWatcher {
    done: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl CheckpointCancellationWatcher {
    fn start(ledger: ReleaseLedger, cancelled: Arc<AtomicBool>) -> Result<Self, RrcError> {
        let done = Arc::new(AtomicBool::new(false));
        let watcher_done = Arc::clone(&done);
        let thread = thread::Builder::new()
            .name("vesper-release-cancel".into())
            .spawn(move || {
                while !watcher_done.load(Ordering::Acquire) {
                    if let Ok(Some(record)) = ledger.load()
                        && record.state == ReleaseRecoveryState::Cancelled
                    {
                        cancelled.store(true, Ordering::Release);
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            })?;
        Ok(Self {
            done,
            thread: Some(thread),
        })
    }
}

impl Drop for CheckpointCancellationWatcher {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn release_stage_has_side_effects(record: &ReleaseRecoveryRecord) -> bool {
    use crate::release_recovery::{EvidenceConfidence, ReleaseDirective};
    use ReleaseRecoveryState as S;
    if record.state == ReleaseRecoveryState::Complete
        && record.mutation.publication_verified
        && record.mutation.closeout_receipt.is_none()
    {
        return true;
    }
    match record.state {
        S::ClassifyingFailure => {
            (match crate::release_recovery::next_directive(record, 0) {
                ReleaseDirective::RequestFocusedRepair { .. } => true,
                ReleaseDirective::RunExternalHealthCheck => {
                    record
                        .retry_admission(crate::release_recovery::RetryKind::Infrastructure)
                        .admitted
                }
                _ => false,
            }) && record.failures.last().is_some_and(|failure| {
                matches!(
                    failure.confidence,
                    EvidenceConfidence::Proven | EvidenceConfidence::StronglySupported
                )
            })
        }
        S::DiagnosingLocalFailure => !record.failures.last().is_some_and(|failure| {
            matches!(
                failure.confidence,
                EvidenceConfidence::Tentative | EvidenceConfidence::Unknown
            )
        }),
        S::LocalVerification
        | S::CandidateReady
        | S::RetryAdmissible
        | S::RemoteGatesGreen
        | S::DiagnosingRepair => true,
        _ => false,
    }
}

#[cfg(test)]
fn drive_release_worker(
    context: ReleaseAdvanceContext<'_>,
    mut wait: impl FnMut(Duration) -> bool,
    mut authorize: impl FnMut(&ReleaseRecoveryRecord) -> Result<(), RrcError>,
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
    let mut refreshed_on_resume = false;
    let mut unchanged_polls = 0_u8;
    let mut active_steps = 0_u8;
    let started = Instant::now();
    loop {
        let Some(mut record) = ledger.load()? else {
            return Ok(());
        };
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if matches!(
            record.state,
            ReleaseRecoveryState::Cancelled
                | ReleaseRecoveryState::Complete
                | ReleaseRecoveryState::Published
                | ReleaseRecoveryState::Escalated
                | ReleaseRecoveryState::PausedExternal
                | ReleaseRecoveryState::NeedMoreEvidence
        ) {
            // Paused external is checked once only on explicit resume.
            if record.state == ReleaseRecoveryState::PausedExternal && active_steps == 0 {
                advance_release(
                    &mut record,
                    ReleaseAdvanceContext {
                        workspace,
                        repository,
                        ledger,
                        executor,
                        github,
                        health,
                        repair_factory,
                        cancelled: Arc::clone(&cancelled),
                    },
                )?;
                active_steps += 1;
                if record.state != ReleaseRecoveryState::PausedExternal {
                    continue;
                }
            }
            return Ok(());
        }
        if active_steps >= 32 || started.elapsed() > Duration::from_secs(2 * 60 * 60) {
            let commit = record.active_commit().unwrap_or("unknown").to_owned();
            record.transition(
                ReleaseRecoveryState::Escalated,
                &commit,
                "controller work/time safety ceiling reached; unfinished work retained",
                vec![],
                None,
            )?;
            ledger.save(&record)?;
            return Ok(());
        }
        if record.mutation.in_flight_operation.is_some() {
            return Err(RrcError::MutationBlocked("previous release operation did not settle; reconcile external/local receipts before retry".into()));
        }
        if !refreshed_on_resume {
            refreshed_on_resume = true;
            if matches!(
                record.state,
                ReleaseRecoveryState::ClassifyingFailure
                    | ReleaseRecoveryState::DiagnosingRepair
                    | ReleaseRecoveryState::CollectingFailureEvidence
                    | ReleaseRecoveryState::RemoteGatesGreen
            ) {
                let commit = record
                    .active_commit()
                    .ok_or_else(|| RrcError::Invalid("resume SHA missing".into()))?
                    .to_owned();
                record.transition(
                    ReleaseRecoveryState::RemoteGateRunning,
                    &commit,
                    "resume refreshes exact-SHA evidence before progression",
                    vec!["rrc:resume-refresh".into()],
                    None,
                )?;
                refresh_remote_evidence(&mut record, repository, github)?;
                ledger.save(&record)?;
                continue;
            }
        }
        let before = record.clone();
        // Side effects require the same host permission port as ordinary tools.
        let has_side_effects = release_stage_has_side_effects(&record);
        if has_side_effects {
            authorize(&record)?;
        }
        let journals_mutation = has_side_effects;
        let journals_mutation = journals_mutation
            && (record.state != ReleaseRecoveryState::LocalVerification
                || record.mutation.version_after.is_none()
                || record
                    .mutation
                    .local_gates
                    .iter()
                    .all(|gate| gate.state == SettlementState::Succeeded));
        if journals_mutation {
            record.mutation.in_flight_operation = Some(format!("{:?}", record.state));
            record.updated_at = Utc::now();
            ledger.save(&record)?;
        }
        advance_release(
            &mut record,
            ReleaseAdvanceContext {
                workspace,
                repository,
                ledger,
                executor,
                github,
                health,
                repair_factory,
                cancelled: Arc::clone(&cancelled),
            },
        )?;
        if journals_mutation && record.state != ReleaseRecoveryState::Cancelled {
            record.mutation.in_flight_operation = None;
            record.updated_at = Utc::now();
            ledger.save(&record)?;
        }
        if matches!(
            record.state,
            ReleaseRecoveryState::WaitingForMatrix
                | ReleaseRecoveryState::RemoteGateRunning
                | ReleaseRecoveryState::Publishing
        ) {
            if before.required_gates == record.required_gates && before.state == record.state {
                unchanged_polls = unchanged_polls.saturating_add(1);
            } else {
                unchanged_polls = 0;
            }
            // Measure actual native waiting only; never impute idle time after restart.
            if matches!(
                before.state,
                ReleaseRecoveryState::RemoteGateRunning
                    | ReleaseRecoveryState::WaitingForMatrix
                    | ReleaseRecoveryState::Publishing
            ) {
                let wait_started = Instant::now();
                let keep_running = wait(crate::release_recovery::poll_interval(unchanged_polls));
                record.metrics.ci_wait_millis = record.metrics.ci_wait_millis.saturating_add(
                    u64::try_from(wait_started.elapsed().as_millis()).unwrap_or(u64::MAX),
                );
                if record.state == ReleaseRecoveryState::Publishing {
                    charge_publication_watch(&mut record, wait_started.elapsed());
                }
                // A different host may have cancelled while this wait was active.
                if let Some(latest) = ledger.load()?
                    && latest.state == ReleaseRecoveryState::Cancelled
                {
                    return Ok(());
                }
                record.updated_at = Utc::now();
                ledger.save(&record)?;
                if !keep_running {
                    return Ok(());
                }
            }
        } else {
            active_steps = active_steps.saturating_add(1);
            let progressed = before.state != record.state
                || before.required_gates != record.required_gates
                || before.mutation != record.mutation
                || before.failures.len() != record.failures.len()
                || before.repair_attempts.len() != record.repair_attempts.len()
                || before.state_changes.len() != record.state_changes.len();
            record.note_progress(progressed);
            if record.watchdog_triggered(Utc::now()) {
                let commit = record.active_commit().unwrap_or("unknown").to_owned();
                record.transition(ReleaseRecoveryState::Escalated, &commit,
                    "active recovery made no evidence/state progress; stagnation watchdog stopped work", vec!["rrc:stagnation-watchdog".into()], None)?;
            }
            ledger.save(&record)?;
        }
    }
}

fn update_internal_dependency_versions(input: &str, before: &str, after: &str) -> String {
    input
        .lines()
        .map(|line| {
            if line.contains("path =") {
                line.replace(
                    &format!("version = \"={before}\""),
                    &format!("version = \"={after}\""),
                )
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn version_only_lockfile(input: &str, before: &str, after: &str) -> String {
    let mut internal_package = false;
    let mut output = String::with_capacity(input.len());
    for line in input.lines() {
        if line == "[[package]]" {
            internal_package = false;
        } else if let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|value| value.strip_suffix('"'))
        {
            internal_package =
                name.starts_with("vesper-") || name.starts_with("agent-vesper-") || name == "xtask";
        }
        if internal_package && line == format!("version = \"{before}\"") {
            output.push_str(&format!("version = \"{after}\""));
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    output
}
