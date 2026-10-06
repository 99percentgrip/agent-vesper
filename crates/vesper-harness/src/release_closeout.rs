//! Immutable-publication closeout: existing registry PR and local execution report.
use crate::release_recovery::{ReleaseRecoveryRecord, RrcError, stable_semver_components};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

const UPSTREAM: &str = "agentclientprotocol/registry";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryTarget {
    pub pull_request: u64,
    pub repository: String,
    pub branch: String,
    pub path: String,
    pub original_blob: String,
    pub manifest: String,
}

impl RegistryTarget {
    pub fn endpoint(&self) -> String {
        format!("repos/{}/contents/{}", self.repository, self.path)
    }
    pub fn read_endpoint(&self) -> String {
        format!("{}?ref={}", self.endpoint(), query_encode(&self.branch))
    }
    pub fn pull_request_url(&self) -> String {
        format!("https://github.com/{UPSTREAM}/pull/{}", self.pull_request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseoutReceipt {
    pub report: String,
    pub registry_url: Option<String>,
    pub registry_blob: Option<String>,
}

fn invalid(detail: &str) -> RrcError {
    RrcError::Invalid(format!("release closeout: {detail}"))
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, RrcError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("missing required registry/publication field"))
}
fn safe_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
fn query_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub(crate) fn decode_content(value: &Value) -> Result<Value, RrcError> {
    if value["type"] != "file" || value["encoding"] != "base64" {
        return Err(invalid("registry entry is not an ordinary encoded file"));
    }
    let encoded = required(value, "content")?
        .split_whitespace()
        .collect::<String>();
    if encoded.len() > 65536 {
        return Err(invalid("registry entry exceeds bound"));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| invalid("malformed registry content encoding"))?;
    serde_json::from_slice(&bytes).map_err(RrcError::Json)
}

pub(crate) fn prepare_registry(
    mut manifest: Value,
    version: &str,
    repository: &str,
    pull_request: u64,
    mut fetch: impl FnMut(&str) -> Result<Value, RrcError>,
) -> Result<RegistryTarget, RrcError> {
    if manifest["id"] != "agent-vesper"
        || manifest["version"] != version
        || stable_semver_components(version).is_none()
    {
        return Err(invalid(
            "canonical registry identity/version does not match verified release",
        ));
    }
    let pr = fetch(&format!("repos/{UPSTREAM}/pulls/{pull_request}"))?;
    if pr["state"] != "open"
        || pr["base"]["repo"]["full_name"] != UPSTREAM
        || pr["head"]["repo"]["fork"] != true
    {
        return Err(invalid(
            "existing registry PR must be open and target the registry upstream",
        ));
    }
    let fork = required(&pr["head"]["repo"], "full_name")?.to_owned();
    if fork.split('/').count() != 2 || !fork.split('/').all(safe_component) {
        return Err(invalid("invalid registry fork identity"));
    }
    let branch = required(&pr["head"], "ref")?.to_owned();
    if branch == "main" || branch == "master" || !branch.split('/').all(safe_component) {
        return Err(invalid("invalid registry PR branch"));
    }
    let fork_metadata = fetch(&format!("repos/{fork}"))?;
    if fork_metadata["parent"]["full_name"] != UPSTREAM {
        return Err(invalid("registry fork parent mismatch"));
    }
    let release = fetch(&format!("repos/{repository}/releases/tags/v{version}"))?;
    if release["tag_name"] != format!("v{version}")
        || release["draft"] != false
        || release["prerelease"] != false
    {
        return Err(invalid("release is not the verified stable publication"));
    }
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| invalid("release assets missing"))?;
    let binaries = manifest["distribution"]["binary"]
        .as_object_mut()
        .ok_or_else(|| invalid("binary distributions missing"))?;
    if binaries.len() != 5 {
        return Err(invalid("five native registry distributions required"));
    }
    for (platform, binary) in binaries {
        if ![
            "darwin-x86_64",
            "darwin-aarch64",
            "linux-x86_64",
            "linux-aarch64",
            "windows-x86_64",
        ]
        .contains(&platform.as_str())
        {
            return Err(invalid("unexpected registry platform"));
        }
        let url = required(binary, "archive")?;
        let extension = if platform == "windows-x86_64" {
            "zip"
        } else {
            "tar.gz"
        };
        let expected = format!(
            "https://github.com/{repository}/releases/download/v{version}/agent-vesper-acp-{platform}.{extension}"
        );
        if url != expected {
            return Err(invalid(
                "registry archive URL is not the exact published asset",
            ));
        }
        let matches = assets
            .iter()
            .filter(|asset| asset["browser_download_url"] == url)
            .collect::<Vec<_>>();
        if matches.len() != 1 || matches[0]["size"].as_u64().unwrap_or(0) == 0 {
            return Err(invalid("unique nonempty published archive missing"));
        }
        let digest = required(matches[0], "digest")?
            .strip_prefix("sha256:")
            .ok_or_else(|| invalid("archive server digest missing"))?;
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("invalid archive server digest"));
        }
        binary["sha256"] = json!(digest);
    }
    let path = "agent-vesper/agent.json".to_owned();
    let current = fetch(&format!(
        "repos/{fork}/contents/{path}?ref={}",
        query_encode(&branch)
    ))?;
    let existing = decode_content(&current)?;
    if existing["id"] != "agent-vesper" {
        return Err(invalid("existing PR entry has a different agent identity"));
    }
    let original_blob = required(&current, "sha")?.to_owned();
    if original_blob.len() != 40 || !original_blob.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("invalid existing registry blob"));
    }
    Ok(RegistryTarget {
        pull_request,
        repository: fork,
        branch,
        path,
        original_blob,
        manifest: serde_json::to_string_pretty(&manifest)? + "\n",
    })
}

pub(crate) fn update_registry(
    target: &RegistryTarget,
    allow_mutation: bool,
    mut api: impl FnMut(&[&str]) -> Result<Value, RrcError>,
) -> Result<String, RrcError> {
    let endpoint = target.read_endpoint();
    let current = api(&["api", &endpoint])?;
    let desired: Value = serde_json::from_str(&target.manifest)?;
    if decode_content(&current)? == desired {
        return Ok(required(&current, "sha")?.to_owned());
    }
    if !allow_mutation {
        return Err(RrcError::MutationBlocked(
            "registry update receipt remains uncertain; no write replayed".into(),
        ));
    }
    if required(&current, "sha")? != target.original_blob {
        return Err(invalid(
            "registry PR changed since closeout admission; no concurrent edits overwritten",
        ));
    }
    let message = format!("Update agent-vesper to v{}", required(&desired, "version")?);
    let content = format!("content={}", STANDARD.encode(target.manifest.as_bytes()));
    let branch = format!("branch={}", target.branch);
    let blob = format!("sha={}", target.original_blob);
    let message = format!("message={message}");
    let endpoint = target.endpoint();
    api(&[
        "api", "--method", "PUT", &endpoint, "-f", &content, "-f", &branch, "-f", &blob, "-f",
        &message,
    ])?;
    // Never trust an acknowledged write as verification.
    let endpoint = target.read_endpoint();
    let observed = api(&["api", &endpoint])?;
    if decode_content(&observed)? != desired {
        return Err(invalid(
            "registry readback does not match published manifest and digests",
        ));
    }
    Ok(required(&observed, "sha")?.to_owned())
}

pub fn summary(record: &ReleaseRecoveryRecord) -> Option<String> {
    if record.state != crate::release_recovery::ReleaseRecoveryState::Complete
        || !record.mutation.publication_verified
    {
        return None;
    }
    let receipt = record.mutation.closeout_receipt.as_ref()?;
    let version = record.release_version.as_deref()?;
    let commit = record.release_commit.as_deref()?;
    let jobs = record
        .required_gates
        .iter()
        .map(|g| g.jobs.len())
        .sum::<usize>();
    let local = if record.mutation.local_gates.is_empty() {
        "not executed in this closeout epoch".to_owned()
    } else {
        format!(
            "{}/{} passed",
            record
                .mutation
                .local_gates
                .iter()
                .filter(|gate| gate.state == crate::release_recovery::SettlementState::Succeeded)
                .count(),
            record.mutation.local_gates.len()
        )
    };
    Some(format!(
        "Release v{version} completed.\n\n- Commit: {commit}\n- Local gates: {local}; current exact-SHA CI: {jobs} jobs verified.\n- Published inventory: {} assets; required archive/checksum verification passed.\n- Current main: {} (green).\n- Registry: {}\n- Execution report: {}\n\nPublication and registry/report delivery are complete. Full PRD coverage is tracked separately.",
        record.mutation.published_asset_names.len(),
        record.current_main.as_deref().unwrap_or("unknown"),
        receipt
            .registry_url
            .as_deref()
            .unwrap_or("not applicable: no registry manifest"),
        receipt.report
    ))
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, RrcError> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        if !safe_component(part) {
            return Err(invalid("invalid closeout artifact path"));
        }
        path.push(part);
        if let Ok(meta) = fs::symlink_metadata(&path)
            && meta.file_type().is_symlink()
        {
            return Err(invalid("closeout artifact cannot traverse symlinks"));
        }
    }
    Ok(path)
}

pub(crate) fn write_report(
    root: &Path,
    record: &ReleaseRecoveryRecord,
    registry_blob: Option<String>,
) -> Result<CloseoutReceipt, RrcError> {
    let version = record
        .release_version
        .as_deref()
        .ok_or_else(|| invalid("release version missing"))?;
    if stable_semver_components(version).is_none() {
        return Err(invalid("invalid report version"));
    }
    let relative = if record.current_main != record.release_commit {
        let main = record
            .current_main
            .as_deref()
            .ok_or_else(|| invalid("main report identity missing"))?;
        if main.len() != 40 || !main.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("main report identity must be a full commit SHA"));
        }
        format!("docs/foundation/release-v{version}-main-{main}-closeout.md")
    } else {
        format!("docs/foundation/release-v{version}-closeout.md")
    };
    let destination = checked_path(root, &relative)?;
    let receipt = CloseoutReceipt {
        report: destination.to_string_lossy().into_owned(),
        registry_url: record
            .mutation
            .closeout_registry
            .as_ref()
            .map(RegistryTarget::pull_request_url),
        registry_blob,
    };
    let mut completed = record.clone();
    completed.mutation.closeout_receipt = Some(receipt.clone());
    let summary = summary(&completed)
        .ok_or_else(|| invalid("completion summary lacks publication provenance"))?;
    // Exclude heartbeat, journal, and observation timestamps so a restart after
    // file creation can verify the same report without overwriting user edits.
    let mut evidence = json!({
        "repository": record.repo_identity,
        "epoch": record.epoch_id,
        "version": record.release_version,
        "release_commit": record.release_commit,
        "current_main": record.current_main,
        "publication_verified": record.mutation.publication_verified,
        "published_assets": record.mutation.published_asset_names,
        "local_gates": record.mutation.local_gates,
        "main_gates": record.required_gates,
        "changed_version_files": record.mutation.version_files,
        "changed_repair_files": record.mutation.repair_files,
        "failures": record.failures,
        "repair_attempts": record.repair_attempts,
        "repair_admissions": record.mutation.repair_admissions,
        "retry_budget": record.retry_budget,
        "state_changes": record.state_changes,
        "transitions": record.transitions,
        "metrics": record.metrics,
        "receipt": receipt,
    });
    crate::release_recovery::redact_persisted_value(&mut evidence);
    let evidence = serde_json::to_string_pretty(&evidence)?;
    if evidence.len() > 2 * 1024 * 1024 {
        return Err(invalid("closeout report evidence exceeds bound"));
    }
    let registry_method = if record.mutation.closeout_registry.is_some() {
        "Registry delivery used the existing PR branch, an expected-blob contents update and independent readback; no PR was created, replaced or merged."
    } else {
        "No registry manifest applies; no registry PR write was performed."
    };
    let body = format!(
        "# Release v{version} closeout\n\n## Objective and status\n\n{summary}\n\n## Methods and commands\n\nNative RRC reobserved current main and the complete exact-SHA workflow matrix. {registry_method} Reports and links remain local.\n\n## Files and exact evidence\n\n```json\n{evidence}\n```\n\n## Deviations, unresolved items and readiness effect\n\nUpstream PR merge remains maintainer-owned. Foundation fixtures do not establish live provider effectiveness. This report certifies the observed release and closeout, not complete implementation/PRD parity. The published tag and assets remain immutable.\n"
    );
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if destination.exists() {
        if fs::read_to_string(&destination)? != body {
            return Err(invalid(
                "existing execution report differs; user edits preserved",
            ));
        }
    } else {
        super::release_executor::atomic_write(&destination, body.as_bytes())?;
    }
    let report_name = relative.rsplit('/').next().expect("report filename");
    for (file, link) in [
        (
            "docs/foundation/evidence-index.md",
            format!(
                "\n- [Release v{version} closeout]({report_name}) — native publication, main-health, registry and completion receipts.\n"
            ),
        ),
        (
            "docs/Agent_Vesper_Release_Recovery_Controller_PRD.md",
            format!(
                "\n[Release v{version} closeout evidence](foundation/{report_name}) records this release; full PRD parity remains separately evidenced.\n"
            ),
        ),
    ] {
        let path = checked_path(root, file)?;
        if path.exists() {
            let mut text = fs::read_to_string(&path)?;
            let target = if file == "docs/foundation/evidence-index.md" {
                report_name.to_owned()
            } else {
                format!("foundation/{report_name}")
            };
            if !text.contains(&format!("]({target})")) {
                text.push_str(&link);
                super::release_executor::atomic_write(&path, text.as_bytes())?;
            }
        }
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(value: &Value, sha: &str) -> Value {
        json!({"type":"file", "encoding":"base64", "content":STANDARD.encode(serde_json::to_vec(value).unwrap()), "sha":sha})
    }
    fn plan_with(mut change: impl FnMut(&str, &mut Value)) -> Result<RegistryTarget, RrcError> {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../registry/agent.json")).unwrap();
        let version = manifest["version"].as_str().unwrap().to_owned();
        let assets = manifest["distribution"]["binary"].as_object().unwrap().values()
            .map(|b| json!({"browser_download_url":b["archive"],"size":42,"digest":format!("sha256:{}", "a".repeat(64))})).collect::<Vec<_>>();
        let mut previous = manifest.clone();
        previous["version"] = json!("0.24.5");
        prepare_registry(
            manifest,
            &version,
            "99percentgrip/agent-vesper",
            539,
            |endpoint| {
                let mut response = if endpoint.contains("/pulls/") {
                    json!({"state":"open","base":{"repo":{"full_name":UPSTREAM}},"head":{"ref":"agent-vesper/v0.20.51","repo":{"full_name":"99percentgrip/registry","fork":true}}})
                } else if endpoint.contains("/releases/") {
                    json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":false,"assets":assets})
                } else if endpoint.contains("/contents/") {
                    content(&previous, &"b".repeat(40))
                } else {
                    json!({"parent":{"full_name":UPSTREAM}})
                };
                change(endpoint, &mut response);
                Ok(response)
            },
        )
    }
    #[test]
    fn closeout_registry_targets_existing_pr_with_exact_asset_digests() {
        let plan = plan_with(|_, _| {}).unwrap();
        assert_eq!(plan.pull_request, 539);
        assert_eq!(plan.branch, "agent-vesper/v0.20.51");
        assert_eq!(plan.path, "agent-vesper/agent.json");
        let desired: Value = serde_json::from_str(&plan.manifest).unwrap();
        for binary in desired["distribution"]["binary"]
            .as_object()
            .unwrap()
            .values()
        {
            assert_eq!(binary["sha256"], "a".repeat(64));
        }
    }
    #[test]
    fn closeout_registry_refuses_closed_or_unrelated_pr_and_invalid_assets() {
        for defect in 0..5 {
            assert!(
                plan_with(|endpoint, response| {
                    match defect {
                        0 if endpoint.contains("/pulls/") => response["state"] = json!("closed"),
                        1 if endpoint.contains("/pulls/") => {
                            response["head"]["ref"] = json!("main")
                        }
                        2 if !endpoint.contains("/pulls/")
                            && endpoint == "repos/99percentgrip/registry" =>
                        {
                            response["parent"]["full_name"] = json!("other/registry")
                        }
                        3 if endpoint.contains("/releases/") => {
                            response["assets"][0]["digest"] = json!("sha256:invalid")
                        }
                        4 if endpoint.contains("/releases/") => response["draft"] = json!(true),
                        _ => {}
                    }
                })
                .is_err(),
                "defect {defect} must refuse"
            );
        }
    }
    #[test]
    fn closeout_registry_verifies_write_by_independent_readback() {
        let target = plan_with(|_, _| {}).unwrap();
        let desired: Value = serde_json::from_str(&target.manifest).unwrap();
        let mut observed = desired.clone();
        observed["version"] = json!("0.24.5");
        let mut writes = 0;
        let blob = update_registry(&target, true, |args| {
            if args.contains(&"PUT") {
                writes += 1;
                assert!(args.contains(&target.endpoint().as_str()));
                assert!(args.contains(&format!("branch={}", target.branch).as_str()));
                observed = desired.clone();
                Ok(json!({}))
            } else {
                Ok(content(
                    &observed,
                    if writes == 0 {
                        &target.original_blob
                    } else {
                        "cccccccccccccccccccccccccccccccccccccccc"
                    },
                ))
            }
        })
        .unwrap();
        assert_eq!(writes, 1);
        assert_eq!(blob, "c".repeat(40));
    }
    #[test]
    fn closeout_registry_uncertain_write_never_replays() {
        let target = plan_with(|_, _| {}).unwrap();
        let mut desired: Value = serde_json::from_str(&target.manifest).unwrap();
        let recovered = update_registry(&target, false, |args| {
            assert!(!args.contains(&"PUT"));
            Ok(content(&desired, &"c".repeat(40)))
        })
        .unwrap();
        assert_eq!(recovered, "c".repeat(40));
        desired["version"] = json!("0.24.5");
        assert!(matches!(
            update_registry(&target, false, |args| {
                assert!(!args.contains(&"PUT"));
                Ok(content(&desired, &target.original_blob))
            }),
            Err(RrcError::MutationBlocked(_))
        ));
    }
    #[test]
    fn closeout_registry_preserves_concurrent_edit_and_rejects_false_write_receipt() {
        let target = plan_with(|_, _| {}).unwrap();
        let mut old: Value = serde_json::from_str(&target.manifest).unwrap();
        old["version"] = json!("0.24.5");
        assert!(
            update_registry(&target, true, |args| {
                assert!(!args.contains(&"PUT"));
                Ok(content(&old, &"d".repeat(40)))
            })
            .is_err()
        );
        assert!(
            update_registry(&target, true, |_| Ok(content(&old, &target.original_blob))).is_err()
        );
    }
    #[test]
    fn closeout_later_main_has_a_distinct_report_without_overwriting_publication() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("docs/foundation")).unwrap();
        fs::write(
            root.path().join("docs/foundation/evidence-index.md"),
            "# Evidence\n",
        )
        .unwrap();
        fs::write(
            root.path()
                .join("docs/Agent_Vesper_Release_Recovery_Controller_PRD.md"),
            "# PRD\n",
        )
        .unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", &"a".repeat(40))
                .unwrap();
        record.state = crate::release_recovery::ReleaseRecoveryState::Complete;
        record.release_version = Some("0.24.6".into());
        record.release_commit = Some("a".repeat(40));
        record.current_main = record.release_commit.clone();
        record.mutation.publication_verified = true;
        let first = write_report(root.path(), &record, None).unwrap();
        let first_bytes = fs::read(&first.report).unwrap();
        record.current_main = Some("b".repeat(40));
        let later = write_report(root.path(), &record, None).unwrap();
        assert_ne!(later.report, first.report);
        assert!(later.report.contains(&format!("main-{}", "b".repeat(40))));
        assert_eq!(fs::read(&first.report).unwrap(), first_bytes);
        assert_eq!(write_report(root.path(), &record, None).unwrap(), later);
        let filename = Path::new(&later.report)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        for path in [
            "docs/foundation/evidence-index.md",
            "docs/Agent_Vesper_Release_Recovery_Controller_PRD.md",
        ] {
            let text = fs::read_to_string(root.path().join(path)).unwrap();
            assert_eq!(
                text.matches(filename).count(),
                1,
                "the new report must be linked exactly once"
            );
        }
    }

    #[test]
    fn closeout_report_restart_is_idempotent_and_preserves_user_edits() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("docs/foundation")).unwrap();
        fs::write(
            root.path().join("docs/foundation/evidence-index.md"),
            "# Evidence\n",
        )
        .unwrap();
        fs::write(
            root.path()
                .join("docs/Agent_Vesper_Release_Recovery_Controller_PRD.md"),
            "# PRD\n",
        )
        .unwrap();
        let mut record =
            crate::release_recovery::start_release("fixture", "patch", "main", &"a".repeat(40))
                .unwrap();
        record.state = crate::release_recovery::ReleaseRecoveryState::Complete;
        record.release_version = Some("0.24.6".into());
        record.release_commit = Some("a".repeat(40));
        record.current_main = record.release_commit.clone();
        record.mutation.publication_verified = true;
        record.mutation.published_asset_names = (0..100)
            .map(|index| format!("fixture-asset-{index}-{}", "x".repeat(50)))
            .collect();
        let receipt = write_report(root.path(), &record, None).unwrap();
        let document = fs::read_to_string(&receipt.report).unwrap();
        let evidence = document
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap();
        assert!(evidence.len() > 4096);
        let parsed = serde_json::from_str::<Value>(evidence)
            .expect("full report evidence must remain valid JSON");
        for key in [
            "failures",
            "repair_attempts",
            "repair_admissions",
            "retry_budget",
            "metrics",
            "changed_repair_files",
        ] {
            assert!(
                parsed.get(key).is_some(),
                "release report must retain {key}"
            );
        }
        record.note_progress_milestone("New heartbeat unrelated to report evidence");
        record.mutation.in_flight_operation = Some("Closeout".into());
        assert_eq!(write_report(root.path(), &record, None).unwrap(), receipt);
        let index =
            fs::read_to_string(root.path().join("docs/foundation/evidence-index.md")).unwrap();
        assert_eq!(index.matches("release-v0.24.6-closeout.md").count(), 1);
        fs::write(&receipt.report, "User edits\n").unwrap();
        assert!(write_report(root.path(), &record, None).is_err());
        assert_eq!(fs::read_to_string(&receipt.report).unwrap(), "User edits\n");
        assert!(summary(&record).is_none());
        record.mutation.closeout_receipt = Some(receipt);
        let final_summary = summary(&record).unwrap();
        assert!(final_summary.contains("completed"));
        assert!(final_summary.contains("not executed in this closeout epoch"));
        assert!(!final_summary.contains("0/0 passed"));
    }
}
