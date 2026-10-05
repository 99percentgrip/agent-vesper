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
    EXTERNAL_HEALTH_BUDGET, ExternalHealthEvidence, GhCliEvidenceAdapter, GitHubEvidencePort,
    LocalGateRecord, ReleaseControllerEvent, ReleaseLedger, ReleaseMutationAdmission,
    ReleaseMutationKind, ReleaseRecoveryRecord, ReleaseRecoveryState, RelevantStateChange,
    RelevantStateChangeKind, RrcError, SettlementState, admit_release_mutation,
    apply_controller_event, default_release_root, redact_secrets, refresh_remote_evidence,
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
    fn last_green_release_commit(&self, _repository: &str) -> Result<Option<String>, RrcError> {
        Ok(None)
    }
    fn source_changed_since(&self, _green: &str, _candidate: &str) -> Result<bool, RrcError> {
        Err(RrcError::Invalid(
            "last-green source comparison unavailable".into(),
        ))
    }
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
        expected_commit: &str,
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
    firewall: Option<Arc<vesper_policy::firewall::CommandFirewall>>,
}

impl NativeReleaseExecutor {
    pub fn new(workspace: &Path, cancelled: Arc<AtomicBool>) -> Result<Self, RrcError> {
        Ok(Self {
            workspace: workspace.canonicalize()?,
            cancelled,
            firewall: None,
        })
    }

    fn with_firewall(
        mut self,
        firewall: Option<Arc<vesper_policy::firewall::CommandFirewall>>,
    ) -> Self {
        self.firewall = firewall;
        self
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
        if let Some(firewall) = &self.firewall
            && firewall.scan(&format_command(program, args)).decision
                == vesper_policy::firewall::RuleDecision::Deny
        {
            return Err(RrcError::MutationBlocked(
                "command firewall denied release subprocess".into(),
            ));
        }
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
            .envs(environment.iter().copied());
        run_bounded_command(&mut command, &self.cancelled, Duration::from_secs(60 * 60))
    }

    fn checked(&self, program: &str, args: &[&str]) -> Result<String, RrcError> {
        let output = self.command(program, args)?;
        if !output.status.success() {
            return Err(RrcError::Invalid(format!(
                "{} failed with status {}: {}",
                format_command(program, args),
                output.status,
                crate::release_recovery::first_causal_excerpt(&format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
            )));
        }
        Ok(bounded_output(&output.stdout))
    }
}

pub(crate) fn run_bounded_command(
    command: &mut Command,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> Result<Output, RrcError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(RrcError::Invalid(
            "release recovery was cancelled by the user".into(),
        ));
    }
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?));
    #[cfg(windows)]
    let mut child = command.group().kill_on_drop(true).spawn()?;
    #[cfg(not(windows))]
    let mut child = command.group_spawn()?;
    let started = Instant::now();
    let status = loop {
        let cancelled_now = cancelled.load(Ordering::Acquire);
        if cancelled_now
            || started.elapsed() >= timeout
            || stdout.metadata()?.len() > 64 * 1024 * 1024
            || stderr.metadata()?.len() > 64 * 1024 * 1024
        {
            let _ = child.kill();
            let _ = child.inner().wait();
            return Err(RrcError::Invalid(if cancelled_now {
                "release recovery was cancelled by the user".into()
            } else {
                "release subprocess exceeded its time or output bound".into()
            }));
        }
        if let Some(status) = child.inner().try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(100));
    };
    // A successful leader must not leave owned descendants running.
    let _ = child.kill();
    stdout.seek(SeekFrom::Start(0))?;
    stderr.seek(SeekFrom::Start(0))?;
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    stdout
        .take(64 * 1024 * 1024)
        .read_to_end(&mut stdout_bytes)?;
    stderr
        .take(64 * 1024 * 1024)
        .read_to_end(&mut stderr_bytes)?;
    Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    })
}

impl ReleaseExecutionPort for NativeReleaseExecutor {
    fn last_green_release_commit(&self, repository: &str) -> Result<Option<String>, RrcError> {
        let value = gh_json(
            &self.workspace,
            &self.cancelled,
            &["api", &format!("repos/{repository}/releases/latest")],
        )?;
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
                    update_workspace_manifest(&old, &before, &after)?
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

    fn prepare_version_bump(
        &self,
        bump: &str,
        admission: ReleaseMutationAdmission,
    ) -> Result<VersionBumpReceipt, RrcError> {
        require_kind(&admission, ReleaseMutationKind::VersionBump)?;
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
        let inventory = self.command(
            "cargo",
            &[
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
                "--offline",
            ],
        )?;
        if !inventory.status.success() {
            return Err(RrcError::Invalid(
                "workspace inventory must resolve before version mutation".into(),
            ));
        }
        let inventory: serde_json::Value = serde_json::from_slice(&inventory.stdout)?;
        let members = inventory
            .get("workspace_members")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| RrcError::Invalid("workspace member inventory missing".into()))?;
        let packages = inventory
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| RrcError::Invalid("workspace package inventory missing".into()))?;
        let mut writes = vec![
            (cargo_path, updated_cargo),
            (registry_path, updated_registry),
        ];
        for package in packages
            .iter()
            .filter(|package| members.contains(&package["id"]))
        {
            if package["version"].as_str() != Some(before.as_str()) {
                return Err(RrcError::Invalid(
                    "workspace member version differs from release version".into(),
                ));
            }
            let path = PathBuf::from(
                package["manifest_path"]
                    .as_str()
                    .ok_or_else(|| RrcError::Invalid("workspace manifest path missing".into()))?,
            );
            if fs::symlink_metadata(&path)?.file_type().is_symlink()
                || !path.canonicalize()?.starts_with(&self.workspace)
            {
                return Err(RrcError::MutationBlocked(
                    "workspace manifest is not a confined regular file".into(),
                ));
            }
            let path = path.canonicalize()?;
            let input = fs::read_to_string(&path)?;
            let updated = update_internal_dependency_versions(&input, &before, &after);
            if updated != input {
                writes.push((path, updated));
            }
        }
        let files = writes
            .iter()
            .map(|(path, _)| {
                path.strip_prefix(&self.workspace)
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                    .map_err(|_| {
                        RrcError::MutationBlocked("version output escaped workspace".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (path, updated) in writes {
            atomic_write(&path, updated.as_bytes())?;
        }
        Ok(VersionBumpReceipt {
            before,
            after,
            files,
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
        require_kind(&admission, ReleaseMutationKind::CommitCandidate)?;
        let allowed = admission.candidate_paths();
        let status = self.checked("git", &["status", "--porcelain"])?;
        for line in status.lines() {
            let path = line.get(3..).unwrap_or_default().trim();
            if !allowed.iter().any(|allowed| allowed == path) {
                return Err(RrcError::Invalid(format!(
                    "candidate commit contains unadmitted path {path}"
                )));
            }
        }
        let mut args = vec!["add", "--"];
        args.extend(allowed.iter().map(String::as_str));
        self.checked("git", &args)?;
        self.checked("git", &["commit", "-m", &format!("Release v{version}")])?;
        let commit = self.checked("git", &["rev-parse", "HEAD"])?;
        exact_sha(commit.trim())
    }

    fn push_candidate(&self, admission: ReleaseMutationAdmission) -> Result<String, RrcError> {
        require_kind(&admission, ReleaseMutationKind::PushCandidate)?;
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
        admission: ReleaseMutationAdmission,
    ) -> Result<(String, String), RrcError> {
        require_kind(&admission, ReleaseMutationKind::CreateTag)?;
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
        expected_commit: &str,
    ) -> Result<Option<PublicationReceipt>, RrcError> {
        let runs = gh_json(
            &self.workspace,
            &self.cancelled,
            &[
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
        let release = gh_json(
            &self.workspace,
            &self.cancelled,
            &["api", &format!("repos/{repository}/releases/tags/{tag}")],
        )?;
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
            let output =
                run_bounded_command(&mut command, &self.cancelled, Duration::from_secs(30))?;
            if !output.status.success() || output.stdout.len() > 1024 {
                return Err(RrcError::Invalid(
                    "checksum asset read failed or exceeded bound".into(),
                ));
            }
            Ok(output.stdout)
        })?;
        let reference = gh_json(
            &self.workspace,
            &self.cancelled,
            &["api", &format!("repos/{repository}/git/ref/tags/{tag}")],
        )?;
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
        let object = gh_json(
            &self.workspace,
            &self.cancelled,
            &["api", &format!("repos/{repository}/git/tags/{tag_object}")],
        )?;
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

#[derive(Debug, Clone, Default)]
pub struct CurlGitHubStatusAdapter;

impl CurlGitHubStatusAdapter {
    pub fn with_cancellation(&self, cancelled: Arc<AtomicBool>) -> CancellableGitHubStatusAdapter {
        CancellableGitHubStatusAdapter { cancelled }
    }
}

#[derive(Debug, Clone)]
pub struct CancellableGitHubStatusAdapter {
    cancelled: Arc<AtomicBool>,
}

impl ExternalHealthPort for CurlGitHubStatusAdapter {
    fn official_status(&self) -> Result<OfficialStatusSnapshot, RrcError> {
        self.with_cancellation(Arc::new(AtomicBool::new(false)))
            .official_status()
    }
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
        let output = run_bounded_command(&mut command, &self.cancelled, EXTERNAL_HEALTH_BUDGET)?;
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

fn run_bounded_repair_agent(
    workspace: &Path,
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    factory: &crate::WorkerFactory,
    cancelled: Arc<AtomicBool>,
) -> Result<(), RrcError> {
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    run_bounded_repair_agent_with_verification(
        workspace,
        record,
        ledger,
        factory,
        cancelled,
        RepairVerification {
            root: &root,
            verify: &|executor| {
                for gate in production_local_gates() {
                    executor.run_local_gate(&gate)?;
                }
                Ok(())
            },
        },
    )
}

// Internal composition seam: production always supplies the full native gate set.
struct RepairVerification<'a> {
    root: &'a Path,
    verify: &'a dyn Fn(&NativeReleaseExecutor) -> Result<(), RrcError>,
}

fn run_bounded_repair_agent_with_verification(
    workspace: &Path,
    record: &mut ReleaseRecoveryRecord,
    ledger: &ReleaseLedger,
    factory: &crate::WorkerFactory,
    cancelled: Arc<AtomicBool>,
    verification: RepairVerification<'_>,
) -> Result<(), RrcError> {
    let failure = record
        .failures
        .last()
        .cloned()
        .ok_or_else(|| RrcError::Invalid("focused repair has no captured failure".into()))?;
    let local = record.state == ReleaseRecoveryState::DiagnosingLocalFailure;
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
    let base = record
        .active_commit()
        .map(str::to_owned)
        .ok_or_else(|| RrcError::Invalid("repair candidate commit is missing".into()))?;
    let repair_root = verification
        .root
        .join("worktrees")
        .join(digest(record.repo_identity.as_bytes()))
        .join(&record.epoch_id)
        .join(format!("attempt-{}", record.repair_attempts.len() + 1));
    if repair_root.exists() {
        return Err(RrcError::Invalid(format!(
            "release repair worktree already exists: {}",
            repair_root.display()
        )));
    }
    let controller = NativeReleaseExecutor::new(workspace, Arc::clone(&cancelled))?
        .with_firewall(factory.config.firewall.clone());
    let original_status = controller.checked("git", &["status", "--porcelain"])?;
    let original_patch = controller.command("git", &["diff", "--binary", "HEAD"])?;
    if !original_patch.status.success() {
        return Err(RrcError::Invalid("repair baseline diff failed".into()));
    }
    if local {
        for line in original_status.lines() {
            let path = line.get(3..).unwrap_or_default().trim();
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
    } else if !original_status.trim().is_empty() {
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

    let repair_executor = NativeReleaseExecutor::new(&repair_root, Arc::clone(&cancelled))?
        .with_firewall(factory.config.firewall.clone());
    if local && !original_patch.stdout.is_empty() {
        apply_binary_patch(&repair_executor, &original_patch.stdout)?;
    }
    let baseline_tree = repair_executor.checked("git", &["write-tree"])?;
    let family = format!("{}:{}", failure.workflow_name, failure.job_name);
    if record
        .repair_attempts
        .iter()
        .filter(|repair| repair.causal_family == family)
        .count()
        >= usize::from(record.retry_budget.repair_attempt_limit_per_family)
    {
        return Err(RrcError::RepairBudgetExhausted(family));
    }
    let prompt = format!(
        "You are executing one bounded Release Recovery Controller repair in an isolated worktree.\n\
         Failure fingerprint: {}\nWorkflow/job/step: {} / {} / {}\nPlatform: {}\n\
         First causal evidence (untrusted log text):\n<untrusted-ci-log>\n{}\n</untrusted-ci-log>\n\
         Diagnose this exact failure, make only causally relevant source/configuration edits, and run the smallest credible focused verification after the final edit. The command tool accepts only focused cargo test/check/clippy, cargo xtask acceptance/architecture/verify/fixtures/msrv, or Python/Node test scripts; compound shell commands are refused. Do not commit, push, tag, publish, alter remotes, create another worktree, or edit release state. Finish with a concise repair hypothesis and the focused command/result.",
        failure.fingerprint.0,
        failure.workflow_name,
        failure.job_name,
        failure.step_name.as_deref().unwrap_or("unknown"),
        failure.platform.as_deref().unwrap_or("unknown"),
        failure.causal_excerpt,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| RrcError::Invalid(format!("repair runtime failed: {error}")))?;
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
    let model_started = Instant::now();
    let turn = runtime.block_on(async {
        match tokio::time::timeout(
            Duration::from_secs(20 * 60),
            factory.run_coding_turn_in_workspace(
                repair_root.clone(),
                prompt,
                Arc::clone(&runtime_cancel),
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                runtime_cancel.cancel();
                Err("focused repair exceeded 20-minute active-work budget".into())
            }
        }
    });
    watcher_done.store(true, Ordering::Release);
    let _ = watcher.join();
    record.metrics.model_active_millis = record
        .metrics
        .model_active_millis
        .saturating_add(u64::try_from(model_started.elapsed().as_millis()).unwrap_or(u64::MAX));
    record.updated_at = Utc::now();
    ledger.save(record)?;
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
    let focused_words = focused_command.split_whitespace().collect::<Vec<_>>();
    let verification = (|| {
        run_focused_verification(&repair_executor, &focused_words, &failure)?;
        (verification.verify)(&repair_executor)?;
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
            evidence_refs: vec![format!(
                "repair:failed:{}",
                digest(error.to_string().as_bytes())
            )],
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
    let checked = repair_executor.command("git", &["diff", "--check"])?;
    if !checked.status.success() {
        return Err(RrcError::Invalid(format!(
            "repair diff check failed: {}",
            bounded_output(&checked.stderr)
        )));
    }
    let diff = repair_executor.command("git", &["diff", "--binary", baseline_tree.trim()])?;
    if !diff.status.success() || diff.stdout.is_empty() {
        return Err(RrcError::Invalid(
            "repair worktree produced no promotable diff".into(),
        ));
    }
    let root_status = controller.command("git", &["status", "--porcelain"])?;
    if !root_status.status.success() || (!local && !root_status.stdout.is_empty()) {
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
            "add",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "registry/agent.json",
        ];
        paths.extend(record.mutation.version_files.iter().map(String::as_str));
        paths.extend(record.mutation.repair_files.iter().map(String::as_str));
        controller.checked("git", &paths)?;
    }
    apply_binary_patch(&controller, &diff.stdout)?;
    if local {
        let paths = controller.checked("git", &["diff", "--cached", "--name-only"])?;
        record.mutation.repair_files = paths.lines().map(str::to_owned).collect();
        record.record_repair(crate::release_recovery::RepairAttempt {
            fingerprint: failure.fingerprint,
            causal_family: family,
            hypothesis: redact_secrets(&assistant_summary),
            source_commit_before: base,
            source_commit_after: None,
            focused_proof: focused_command,
            focused_status: crate::release_recovery::FocusedProofStatus::Passed,
            evidence_refs: vec![
                format!("repair:patch:{}", digest(&diff.stdout)),
                "repair:local-gates".into(),
            ],
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
    let patch_digest = digest(&diff.stdout);
    apply_controller_event(
        record,
        ReleaseControllerEvent::VerifiedRepair(crate::release_recovery::RepairAttempt {
            fingerprint: failure.fingerprint,
            causal_family: format!("{}:{}", failure.workflow_name, failure.job_name),
            hypothesis: redact_secrets(&assistant_summary)
                .chars()
                .take(1024)
                .collect(),
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
pub(crate) fn repair_tool_registry() -> vesper_agent::ToolRegistry {
    struct RepairTools(vesper_agent::ToolRegistry);
    impl vesper_agent::ToolService for RepairTools {
        fn definitions(&self) -> Vec<vesper_domain::ToolDefinition> {
            self.0
                .definitions_for(vesper_domain::SessionOperatingMode::Code)
        }
        fn execute<'a>(
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
            self.0.execute(call, context)
        }
    }
    vesper_agent::ToolRegistry::empty().with_service(Arc::new(RepairTools(
        vesper_agent::ToolRegistry::parity_default(),
    )))
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
    let output = executor.command(words[0], &words[1..])?;
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
    regex::Regex::new(r"thread '([^']+)' panicked")
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
                        let excerpt =
                            crate::release_recovery::first_causal_excerpt(&error.to_string());
                        let (class, confidence) =
                            crate::release_recovery::classify_failure(&excerpt, None);
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
                        return Ok(());
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
                .last()
                .is_some_and(|failure| failure.class.infrastructure_like()) =>
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
            let _admission = admit_release_mutation(record, ReleaseMutationKind::Publish)?;
            let tag = record
                .mutation
                .tag_name
                .clone()
                .ok_or_else(|| RrcError::Invalid("release tag is missing".into()))?;
            if let Some(receipt) = executor.publication(
                repository,
                &tag,
                record
                    .release_commit
                    .as_deref()
                    .ok_or_else(|| RrcError::Invalid("publication candidate missing".into()))?,
            )? {
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

fn release_stage_commands(record: &ReleaseRecoveryRecord, repository: &str) -> Vec<String> {
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
        RrcError::MutationBlocked("release mutation requires a host permission port".into())
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
                    return Err(RrcError::MutationBlocked(
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
            return Err(RrcError::MutationBlocked(reason));
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
    if decision.is_allowed() {
        Ok(())
    } else {
        Err(RrcError::MutationBlocked(
            "host permission denied release side effect".into(),
        ))
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
    let root = default_release_root()
        .ok_or_else(|| RrcError::Invalid("no user-owned release state root is available".into()))?;
    let ledger = ReleaseLedger::open(root, &repo_identity)?;
    let active = ACTIVE_RELEASE_WORKERS.get_or_init(|| Mutex::new(HashMap::new()));
    let cancelled = Arc::new(AtomicBool::new(false));
    let owner;
    {
        let mut guard = active
            .lock()
            .map_err(|_| RrcError::Invalid("release worker lock is poisoned".into()))?;
        if let Some(existing) = guard.get(&repo_identity) {
            if existing.load(Ordering::Acquire) {
                return Err(RrcError::Busy);
            }
            return Ok(());
        }
        owner = ledger.acquire_owner()?;
        guard.insert(repo_identity.clone(), Arc::clone(&cancelled));
    }
    let worker_key = repo_identity.clone();
    let spawned = std::thread::Builder::new()
        .name("vesper-release-controller".into())
        .spawn(move || {
            let _owner = owner;
            let result = (|| -> Result<(), RrcError> {
                let root = default_release_root().ok_or_else(|| {
                    RrcError::Invalid("no user-owned release state root is available".into())
                })?;
                let ledger = ReleaseLedger::open(root, &repo_identity)?;
                let _watcher =
                    CheckpointCancellationWatcher::start(ledger.clone(), Arc::clone(&cancelled))?;
                let executor = NativeReleaseExecutor::new(&workspace, Arc::clone(&cancelled))?
                    .with_firewall(
                        repair_factory
                            .as_ref()
                            .and_then(|factory| factory.config.firewall.clone()),
                    );
                let github = GhCliEvidenceAdapter.with_cancellation(Arc::clone(&cancelled));
                let health = CurlGitHubStatusAdapter.with_cancellation(Arc::clone(&cancelled));
                drive_release_worker(
                    ReleaseAdvanceContext {
                        workspace: &workspace,
                        repository: &repository,
                        ledger: &ledger,
                        executor: &executor,
                        github: &github,
                        health: &health,
                        repair_factory: repair_factory.as_ref(),
                        cancelled: Arc::clone(&cancelled),
                    },
                    |delay| {
                        let deadline = Instant::now() + delay;
                        while Instant::now() < deadline {
                            if cancelled.load(Ordering::Acquire) {
                                return false;
                            }
                            thread::sleep(Duration::from_millis(100));
                        }
                        true
                    },
                    |record| {
                        authorize_controller_step(
                            repair_factory.as_ref(),
                            record,
                            &repository,
                            &cancelled,
                        )
                    },
                )
            })();
            if let Err(error) = result {
                if let Ok(Some(mut record)) = ledger.load()
                    && !matches!(
                        record.state,
                        ReleaseRecoveryState::Cancelled
                            | ReleaseRecoveryState::Published
                            | ReleaseRecoveryState::Complete
                            | ReleaseRecoveryState::PausedExternal
                            | ReleaseRecoveryState::Escalated
                    )
                {
                    if let RrcError::RetryBlocked(reason) = &error {
                        record.metrics.retries_rejected =
                            record.metrics.retries_rejected.saturating_add(1);
                        record.metrics.speculative_reruns_prevented = record
                            .metrics
                            .speculative_reruns_prevented
                            .saturating_add(1);
                        if reason.contains("identical failure fingerprint") {
                            record.metrics.repeated_fingerprints_blocked = record
                                .metrics
                                .repeated_fingerprints_blocked
                                .saturating_add(1);
                        }
                    }
                    let commit = record.active_commit().unwrap_or("unknown").to_owned();
                    let _ = record.transition(
                        ReleaseRecoveryState::Escalated,
                        &commit,
                        redact_secrets(&format!("controller stopped: {error}")),
                        vec!["rrc:executor-error".into()],
                        None,
                    );
                    let _ = ledger.save(&record);
                }
                eprintln!(
                    "release controller stopped: {}",
                    redact_secrets(&error.to_string())
                );
            }
            if let Ok(mut guard) = ACTIVE_RELEASE_WORKERS.get().expect("initialized").lock() {
                guard.remove(&repo_identity);
            }
        });
    if let Err(error) = spawned {
        if let Ok(mut guard) = active.lock() {
            guard.remove(&worker_key);
        }
        return Err(error.into());
    }
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
        "patch" => format!(
            "{major}.{minor}.{}",
            patch
                .checked_add(1)
                .ok_or_else(|| RrcError::Invalid("version overflow".into()))?
        ),
        "minor" => format!(
            "{major}.{}.0",
            minor
                .checked_add(1)
                .ok_or_else(|| RrcError::Invalid("version overflow".into()))?
        ),
        "major" => format!(
            "{}.0.0",
            major
                .checked_add(1)
                .ok_or_else(|| RrcError::Invalid("version overflow".into()))?
        ),
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

fn gh_json(
    workspace: &Path,
    cancelled: &AtomicBool,
    args: &[&str],
) -> Result<serde_json::Value, RrcError> {
    let mut command = Command::new("gh");
    command
        .args(args)
        .current_dir(workspace)
        .env_remove("GH_DEBUG");
    let output = run_bounded_command(&mut command, cancelled, Duration::from_secs(30))?;
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

    fn repair_test_factory(
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
                "run_command",
                serde_json::json!({"command":command}),
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
        let session = vesper_testkit::FakeProviderSession::with_scripts(scripts);
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
        for (host_cap, expected_cap) in [(0, 24), (5, 5), (100, 24)] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("src")).unwrap();
            fs::write(root.path().join("Cargo.toml"),
                "[package]\nname = \"release-iteration-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
            let commands = (0..30)
                .map(|index| format!("cargo check --offline --target-dir target/limit-{index}"))
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
        for label in ["fixture.release-a", "fixture.release-b"] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = temp.path().join("workspace");
            fs::create_dir_all(workspace.join("src")).unwrap();
            fs::write(
                workspace.join("Cargo.toml"),
                "[package]\nname=\"repair-composition\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
            )
            .unwrap();
            fs::write(workspace.join(".gitattributes"), "* text eol=lf\n").unwrap();
            fs::write(workspace.join(".gitignore"), "/target/\n").unwrap();
            let broken = "pub fn answer() -> u32 { 0 }\n#[test] fn exact_regression() { assert_eq!(answer(), 42); }\n";
            let repaired = broken.replace("{ 0 }", "{ 42 }");
            fs::write(workspace.join("src/lib.rs"), broken).unwrap();
            let native =
                NativeReleaseExecutor::new(&workspace, Arc::new(AtomicBool::new(false))).unwrap();
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
            record.state = ReleaseRecoveryState::ClassifyingFailure;
            record.mutation.candidate_committed = true;
            record.mutation.candidate_pushed = true;
            record.mutation.candidate_push_ref = Some(format!("origin/main@{base}"));
            record.required_gates = GreenGithub.matrix_for_sha("fixture/repo", &base).unwrap();
            record.required_gates[0].jobs[0].state = crate::release_recovery::JobState::Failure;
            record.required_gates[0].run_state = Some(crate::release_recovery::JobState::Failure);
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
            let (factory, session, _runtime) = repair_test_factory(label, &repaired);
            let gates_observed = std::sync::atomic::AtomicUsize::new(0);
            let verify_fixture_gates = |executor: &NativeReleaseExecutor| {
                gates_observed.fetch_add(1, Ordering::SeqCst);
                assert_ne!(
                    executor.workspace, workspace,
                    "verification must use the isolated worktree"
                );
                executor.checked("cargo", &["test", "--offline"])?;
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
                    verify: &verify_fixture_gates,
                },
            )
            .unwrap();
            assert_eq!(gates_observed.load(Ordering::SeqCst), 1);
            assert_eq!(session.requests().len(), 3);
            assert_eq!(record.state, ReleaseRecoveryState::RetryAdmissible);
            assert_eq!(record.repair_attempts.len(), 1);
            let receipt = &record.repair_attempts[0];
            assert_eq!(receipt.source_commit_before, base);
            assert_ne!(receipt.source_commit_after.as_deref(), Some(base.as_str()));
            assert_eq!(
                receipt.focused_status,
                crate::release_recovery::FocusedProofStatus::Passed
            );
            assert!(!receipt.hypothesis.is_empty());
            assert_eq!(
                fs::read_to_string(workspace.join("src/lib.rs")).unwrap(),
                repaired
            );
            assert!(
                native
                    .checked("git", &["status", "--porcelain"])
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                native
                    .checked("git", &["diff", "--name-only", &base, "HEAD"])
                    .unwrap()
                    .trim(),
                "src/lib.rs"
            );
            assert!(!record.mutation.candidate_pushed);
            assert_eq!(
                record.retry_budget.full_gate_used, 0,
                "promotion is not a remote retry"
            );
            assert!(record.metrics.model_active_millis > 0);
            assert_eq!(ledger.load().unwrap().unwrap(), record);
        }
    }

    #[test]
    fn version_bump_updates_only_workspace_and_internal_path_versions() {
        let source = "[workspace.package]\nversion = \"0.24.4\"\n[workspace.dependencies]\na = { path = \"a\", version = \"=0.24.4\" }\nb = { version = \"=0.24.4\" }\n";
        let updated = update_workspace_manifest(source, "0.24.4", "0.24.5").unwrap();
        assert!(updated.contains("version = \"0.24.5\""));
        assert!(updated.contains("path = \"a\", version = \"=0.24.5\""));
        assert!(updated.contains("b = { version = \"=0.24.4\" }"));
    }

    #[test]
    fn native_version_bump_keeps_all_workspace_dependency_pins_resolvable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
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
        let executor = NativeReleaseExecutor::new(root, Arc::new(AtomicBool::new(false))).unwrap();
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
                admit_release_mutation(&record, ReleaseMutationKind::VersionBump).unwrap(),
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
            admit_release_mutation(&candidate, ReleaseMutationKind::CommitCandidate).unwrap();
        assert!(token.candidate_paths().contains(&"a/Cargo.toml".into()));
    }

    #[test]
    fn registry_bump_updates_version_and_archive_urls() {
        let source = r#"{"version":"0.24.4","archive":"https://example/v0.24.4/a.tgz"}"#;
        let updated = update_registry_manifest(source, "0.24.4", "0.24.5").unwrap();
        assert!(updated.contains("\"version\": \"0.24.5\""));
        assert!(updated.contains("/v0.24.5/a.tgz"));
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
            require_kind(&admission, ReleaseMutationKind::VersionBump)?;
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
            require_kind(&admission, ReleaseMutationKind::CommitCandidate)?;
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }
        fn push_candidate(&self, admission: ReleaseMutationAdmission) -> Result<String, RrcError> {
            require_kind(&admission, ReleaseMutationKind::PushCandidate)?;
            Ok("origin/main@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }
        fn create_and_push_tag(
            &self,
            version: &str,
            _commit: &str,
            admission: ReleaseMutationAdmission,
        ) -> Result<(String, String), RrcError> {
            require_kind(&admission, ReleaseMutationKind::CreateTag)?;
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
                    Err(RrcError::MutationBlocked(_))
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
                assert_eq!(delay, Duration::from_secs(20));
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
                admission: ReleaseMutationAdmission,
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
                _: ReleaseMutationAdmission,
            ) -> Result<String, RrcError> {
                panic!("red local gate cannot commit")
            }
            fn push_candidate(&self, _: ReleaseMutationAdmission) -> Result<String, RrcError> {
                panic!("red local gate cannot push")
            }
            fn create_and_push_tag(
                &self,
                _: &str,
                _: &str,
                _: ReleaseMutationAdmission,
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
        .unwrap();
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
