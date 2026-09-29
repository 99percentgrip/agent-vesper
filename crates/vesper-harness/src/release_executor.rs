#![forbid(unsafe_code)]
//! Native, controller-admitted release side effects and bounded external-health
//! checks. The executor follows the existing repository release contract; it
//! does not contain provider logic or a parallel release policy.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use command_group::CommandGroup;
use vesper_domain::{ContentPart, ToolResultStatus};

use crate::release_recovery::{
    EXTERNAL_HEALTH_BUDGET, ExternalHealthEvidence, ExternalHealthVerdict, GhCliEvidenceAdapter,
    GitHubEvidencePort, LocalGateRecord, ReleaseControllerEvent, ReleaseLedger,
    ReleaseMutationAdmission, ReleaseMutationKind, ReleaseRecoveryRecord, ReleaseRecoveryState,
    RelevantStateChange, RelevantStateChangeKind, RrcError, SettlementState,
    admit_release_mutation, apply_controller_event, classify_external_health, default_release_root,
    redact_secrets, refresh_remote_evidence,
};

const MAX_COMMAND_OUTPUT: usize = 4096;
const OFFICIAL_STATUS_URL: &str = "https://www.githubstatus.com/api/v2/summary.json";

static ACTIVE_RELEASE_WORKERS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionBumpReceipt {
    pub before: String,
    pub after: String,
    pub files: Vec<String>,
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
}

impl NativeReleaseExecutor {
    pub fn new(workspace: &Path, cancelled: Arc<AtomicBool>) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
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
        let mut stdout = tempfile::tempfile()?;
        let mut stderr = tempfile::tempfile()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&self.workspace)
            .env_remove("GH_DEBUG")
            .envs(environment.iter().copied())
            .stdout(Stdio::from(stdout.try_clone()?))
            .stderr(Stdio::from(stderr.try_clone()?));
        #[cfg(windows)]
        let mut child = command.group().kill_on_drop(true).spawn()?;
        #[cfg(not(windows))]
        let mut child = command.group_spawn()?;
        let status = loop {
            if self.cancelled.load(Ordering::Acquire) {
                let _ = child.kill();
                let _ = child.inner().wait();
                return Err(RrcError::Invalid(
                    "release recovery was cancelled by the user".into(),
                ));
            }
            if let Some(status) = child.inner().try_wait()? {
                break status;
            }
            thread::sleep(Duration::from_millis(100));
        };
        stdout.seek(SeekFrom::Start(0))?;
        stderr.seek(SeekFrom::Start(0))?;
        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();
        stdout
            .take(4 * 1024 * 1024)
            .read_to_end(&mut stdout_bytes)?;
        stderr
            .take(4 * 1024 * 1024)
            .read_to_end(&mut stderr_bytes)?;
        Ok(Output {
            status,
            stdout: stdout_bytes,
            stderr: stderr_bytes,
        })
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
        let cargo_path = self.workspace.join("Cargo.toml");
        let registry_path = self.workspace.join("registry/agent.json");
        let cargo = fs::read_to_string(&cargo_path)?;
        let before = workspace_version(&cargo)?;
        let after = bump_semver(&before, bump)?;
        let updated_cargo = update_workspace_manifest(&cargo, &before, &after)?;
        let registry = fs::read_to_string(&registry_path)?;
        let updated_registry = update_registry_manifest(&registry, &before, &after)?;
        atomic_write(&cargo_path, updated_cargo.as_bytes())?;
        atomic_write(&registry_path, updated_registry.as_bytes())?;
        Ok(VersionBumpReceipt {
            before,
            after,
            files: vec!["Cargo.toml".into(), "registry/agent.json".into()],
        })
    }

    fn run_local_gate(&self, gate: &LocalGateRecord) -> Result<String, RrcError> {
        let (program, args) = local_gate_argv(&gate.name).ok_or_else(|| {
            RrcError::Invalid(format!("unknown controller-owned local gate {}", gate.name))
        })?;
        self.checked(program, args)
    }

    fn commit_candidate(
        &self,
        version: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<String, RrcError> {
        require_kind(admission, ReleaseMutationKind::CommitCandidate)?;
        self.checked(
            "git",
            &["add", "Cargo.toml", "Cargo.lock", "registry/agent.json"],
        )?;
        let allowed = ["Cargo.lock", "Cargo.toml", "registry/agent.json"];
        let status = self.checked("git", &["status", "--porcelain"])?;
        for line in status.lines() {
            let path = line.get(3..).unwrap_or_default().trim();
            if !allowed.contains(&path) {
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
        .spawn()?;
    if let Some(stdin) = apply.stdin.as_mut() {
        use std::io::Write as _;
        stdin.write_all(&diff.stdout)?;
    }
    if !apply.wait()?.success() {
        return Err(RrcError::Invalid(
            "verified repair diff could not be promoted".into(),
        ));
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
                ledger.save(record)?;
                let gate = record.mutation.local_gates[index].clone();
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
                        ledger.save(record)?;
                    }
                    Err(error) => {
                        record.mutation.local_gates[index].state = SettlementState::Failed;
                        record.mutation.local_gates[index].evidence_ref =
                            Some("local:failed".into());
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
) -> Result<(), RrcError> {
    spawn_release_worker_with_factory(workspace, repository, repo_identity, None)
}

pub fn spawn_release_worker_with_factory(
    workspace: PathBuf,
    repository: String,
    repo_identity: String,
    repair_factory: Option<crate::WorkerFactory>,
) -> Result<(), RrcError> {
    let active = ACTIVE_RELEASE_WORKERS.get_or_init(|| Mutex::new(HashMap::new()));
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut guard = active
            .lock()
            .map_err(|_| RrcError::Invalid("release worker lock is poisoned".into()))?;
        if guard.contains_key(&repo_identity) {
            return Ok(());
        }
        guard.insert(repo_identity.clone(), Arc::clone(&cancelled));
    }
    std::thread::Builder::new()
        .name("vesper-release-controller".into())
        .spawn(move || {
            let result = (|| -> Result<(), RrcError> {
                let root = default_release_root().ok_or_else(|| {
                    RrcError::Invalid("no user-owned release state root is available".into())
                })?;
                let ledger = ReleaseLedger::open(root, &repo_identity)?;
                let executor = NativeReleaseExecutor::new(&workspace, Arc::clone(&cancelled))?;
                for _ in 0..16 {
                    let Some(mut record) = ledger.load()? else {
                        return Ok(());
                    };
                    let before = record.state;
                    advance_release(
                        &mut record,
                        ReleaseAdvanceContext {
                            workspace: &workspace,
                            repository: &repository,
                            ledger: &ledger,
                            executor: &executor,
                            github: &GhCliEvidenceAdapter,
                            health: &CurlGitHubStatusAdapter,
                            repair_factory: repair_factory.as_ref(),
                            cancelled: Arc::clone(&cancelled),
                        },
                    )?;
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
                        || (record.state == before
                            && record.state != ReleaseRecoveryState::LocalVerification)
                    {
                        break;
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("release controller paused safely: {error}");
            }
            if let Ok(mut guard) = ACTIVE_RELEASE_WORKERS.get().expect("initialized").lock() {
                guard.remove(&repo_identity);
            }
        })?;
    Ok(())
}

/// Signals the user-owned local worker, if present. Remote GitHub workflows are
/// intentionally not cancelled or described as cancelled by this operation.
pub fn cancel_release_worker(repo_identity: &str) {
    if let Some(active) = ACTIVE_RELEASE_WORKERS.get()
        && let Ok(guard) = active.lock()
        && let Some(cancelled) = guard.get(repo_identity)
    {
        cancelled.store(true, Ordering::Release);
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

fn update_workspace_manifest(
    manifest: &str,
    before: &str,
    after: &str,
) -> Result<String, RrcError> {
    let mut output = String::with_capacity(manifest.len());
    let mut workspace_package = false;
    let mut changed = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            workspace_package = trimmed == "[workspace.package]";
        }
        let updated = if workspace_package && trimmed == format!("version = \"{before}\"") {
            changed = true;
            line.replacen(before, after, 1)
        } else if line.contains("path =") && line.contains(&format!("version = \"={before}\"")) {
            line.replace(
                &format!("version = \"={before}\""),
                &format!("version = \"={after}\""),
            )
        } else {
            line.to_owned()
        };
        output.push_str(&updated);
        output.push('\n');
    }
    if !changed {
        return Err(RrcError::Invalid(
            "workspace version did not match expected source".into(),
        ));
    }
    Ok(output)
}

fn update_registry_manifest(input: &str, before: &str, after: &str) -> Result<String, RrcError> {
    let mut value: serde_json::Value = serde_json::from_str(input)?;
    if value.get("version").and_then(serde_json::Value::as_str) != Some(before) {
        return Err(RrcError::Invalid(
            "registry version does not match workspace".into(),
        ));
    }
    value["version"] = serde_json::Value::String(after.into());
    rewrite_version_urls(&mut value, before, after);
    let mut rendered = serde_json::to_string_pretty(&value)?;
    rendered.push('\n');
    Ok(rendered)
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
    fn version_bump_updates_only_workspace_and_internal_path_versions() {
        let source = "[workspace.package]\nversion = \"0.24.4\"\n[workspace.dependencies]\na = { path = \"a\", version = \"=0.24.4\" }\nb = { version = \"=0.24.4\" }\n";
        let updated = update_workspace_manifest(source, "0.24.4", "0.24.5").unwrap();
        assert!(updated.contains("version = \"0.24.5\""));
        assert!(updated.contains("path = \"a\", version = \"=0.24.5\""));
        assert!(updated.contains("b = { version = \"=0.24.4\" }"));
    }

    #[test]
    fn registry_bump_updates_version_and_archive_urls() {
        let source = r#"{"version":"0.24.4","archive":"https://example/v0.24.4/a.tgz"}"#;
        let updated = update_registry_manifest(source, "0.24.4", "0.24.5").unwrap();
        assert!(updated.contains("\"version\": \"0.24.5\""));
        assert!(updated.contains("/v0.24.5/a.tgz"));
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
}
