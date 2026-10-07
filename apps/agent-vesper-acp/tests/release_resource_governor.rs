//! ACP process acceptance for the RRC Host Resource Governor.
//!
//! The integration-only ACP driver enables a deterministic permissive policy;
//! dedicated governor tests retain live Linux and constrained-host coverage. A
//! controlled `cargo` wrapper keeps the first local gate alive long enough to
//! query `/release status`, proving that the production RRC path exports
//! resource telemetry and inherits the governor's bounded Cargo environment.

#![cfg(all(target_os = "linux", feature = "integration-test-harness"))]
#![allow(dead_code)]

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use serde_json::json;
use support::ProcessHarness;

struct GateRelease(PathBuf);

impl Drop for GateRelease {
    fn drop(&mut self) {
        // If an assertion fails before normal settlement, release the owned
        // wrapper so no test-local subprocess is left waiting.
        let _ = fs::write(&self.0, b"stop");
    }
}

#[test]
fn controlled_cargo_metadata_emits_valid_json() {
    let fixture = tempfile::tempdir().expect("temporary controlled Cargo fixture");
    let cargo = fixture.path().join("cargo");
    write_controlled_cargo(
        &cargo,
        &fixture.path().join("receipts"),
        &fixture.path().join("release"),
    );
    let output = Command::new(&cargo)
        .arg("metadata")
        .output()
        .expect("run metadata wrapper");
    assert!(output.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("controlled metadata must be valid JSON under the host POSIX shell");
    assert_eq!(metadata, json!({"packages": []}));
}

#[test]
fn acp_process_release_status_uses_governor_for_every_cargo_path() {
    governed_release_process("/release patch", false);
}

#[test]
fn acp_natural_release_inherits_session_permission_and_governor() {
    governed_release_process(
        "Release this completed work with a patch version bump.",
        false,
    );
}

#[test]
fn acp_release_approval_keeps_owner_and_completes_cancellation() {
    governed_release_process("/release patch", true);
}

#[test]
fn acp_release_rejection_and_unadvertised_approval_finish_without_mutation() {
    use vesper_harness::release_recovery::*;
    for choice in ["reject-once", "allow-always", "invented-option"] {
        let fixture = tempfile::tempdir().unwrap();
        let workspace = fixture.path().join("workspace");
        let state_root = fixture.path().join("release-state");
        create_release_fixture(&workspace);
        let identity = repository_identity_for_workspace(&workspace).unwrap();
        let before = fs::read(workspace.join("Cargo.toml")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut process = ProcessHarness::spawn_resource_governor_test_driver(
            listener.local_addr().unwrap(),
            [(
                "AGENT_VESPER_RELEASE_ROOT",
                state_root.display().to_string(),
            )],
        );
        let session = process.initialize_and_new_session_in(&workspace);
        // Keep the native default Ask; no fabricated approval or model call.
        process.prompt(3, &session, "/release patch", "release-rejection");
        answer_release_permission(&mut process, &session, choice);
        let response = process.response(3);
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(support::terminal_count(process.transcript(), 3), 1);
        let text = update_text(process.transcript());
        assert!(text.contains("ACP client rejected permission"), "{text}");
        assert!(text.contains("Release unfinished"), "{text}");
        let record = ReleaseLedger::open(&state_root, &identity)
            .unwrap()
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(record.state, ReleaseRecoveryState::LocalVerification);
        assert_eq!(record.liveness.state, ReleaseLivenessState::Failed);
        assert!(record.failures.is_empty());
        assert!(record.repair_attempts.is_empty());
        assert!(record.mutation.in_flight_operation.is_none());
        assert!(record.mutation.version_after.is_none());
        assert!(!record.mutation.candidate_pushed && !record.mutation.tag_pushed);
        process.finish();
        assert_eq!(fs::read(workspace.join("Cargo.toml")).unwrap(), before);
        assert!(
            listener.accept().is_err(),
            "authorization must not dispatch a provider"
        );
    }
}

#[test]
fn acp_release_pending_approval_cancel_ignores_late_allow() {
    use vesper_harness::release_recovery::*;
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("workspace");
    let state_root = fixture.path().join("release-state");
    create_release_fixture(&workspace);
    let identity = repository_identity_for_workspace(&workspace).unwrap();
    let before = fs::read(workspace.join("Cargo.toml")).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut process = ProcessHarness::spawn_resource_governor_test_driver(
        listener.local_addr().unwrap(),
        [(
            "AGENT_VESPER_RELEASE_ROOT",
            state_root.display().to_string(),
        )],
    );
    let session = process.initialize_and_new_session_in(&workspace);
    process.prompt(3, &session, "/release patch", "release-pending-cancel");
    let pending = loop {
        let message = process.next();
        if message["method"] == "session/request_permission" {
            break message;
        }
    };
    process.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}));
    let response = process.response(3);
    assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
    assert_eq!(support::terminal_count(process.transcript(), 3), 1);
    process.send(json!({"jsonrpc":"2.0","id":pending["id"],"result":{
        "outcome":{"outcome":"selected","optionId":"allow-once"}
    }}));
    process.prompt(4, &session, "/release status", "release-after-late-allow");
    assert!(process.response(4).get("error").is_none());
    let record = ReleaseLedger::open(&state_root, &identity)
        .unwrap()
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(record.state, ReleaseRecoveryState::Cancelled);
    assert!(record.mutation.version_after.is_none());
    assert!(!record.mutation.candidate_pushed && !record.mutation.tag_pushed);
    process.finish();
    assert_eq!(fs::read(workspace.join("Cargo.toml")).unwrap(), before);
    assert!(listener.accept().is_err());
}

fn answer_release_permission(process: &mut ProcessHarness, session: &str, choice: &str) {
    loop {
        let message = process.next();
        if message["method"] != "session/request_permission" {
            continue;
        }
        assert_eq!(message["params"]["sessionId"], session);
        assert!(
            message["params"]["toolCall"]["title"]
                .as_str()
                .unwrap()
                .contains("release_controller")
        );
        let options = message["params"]["options"].as_array().unwrap();
        assert!(options.iter().any(|o| o["optionId"] == "allow-once"));
        assert!(!options.iter().any(|o| o["optionId"] == "allow-always"));
        process.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{
            "outcome":{"outcome":"selected","optionId":choice}
        }}));
        return;
    }
}

#[test]
fn acp_completed_release_delivers_final_receipt_without_provider_or_new_release() {
    use vesper_harness::{release_closeout::CloseoutReceipt, release_recovery::*};
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("workspace");
    let state_root = fixture.path().join("release-state");
    create_release_fixture(&workspace);
    let identity = repository_identity_for_workspace(&workspace).unwrap();
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let report = workspace.join("docs/foundation/release-fixture-closeout.md");
    fs::write(&report, "Controlled completed release receipt\n").unwrap();
    let mut record = start_release(&identity, "patch", "main", &head).unwrap();
    record.state = ReleaseRecoveryState::Complete;
    record.release_version = Some("0.1.1".into());
    record.release_commit = Some(head.clone());
    record.current_main = Some(head);
    record.mutation.publication_verified = true;
    record.mutation.closeout_receipt = Some(CloseoutReceipt {
        report: report.to_string_lossy().into_owned(),
        registry_url: None,
        registry_blob: None,
    });
    record.note_progress_milestone("Fixture release closeout verified");
    ReleaseLedger::open(&state_root, &identity)
        .unwrap()
        .save(&record)
        .unwrap();
    let before = fs::read(workspace.join("Cargo.toml")).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut process = ProcessHarness::spawn_resource_governor_test_driver(
        listener.local_addr().unwrap(),
        [(
            "AGENT_VESPER_RELEASE_ROOT",
            state_root.display().to_string(),
        )],
    );
    let session = process.initialize_and_new_session_in(&workspace);
    process.prompt(3, &session, "/release resume", "fixture-closeout-delivery");
    let response = process.response(3);
    assert!(response.get("error").is_none(), "{response}");
    let text = update_text(process.transcript());
    assert_eq!(
        text.matches("Release v0.1.1 completed.").count(),
        1,
        "{text}"
    );
    assert!(
        text.contains(&report.to_string_lossy().to_string()),
        "{text}"
    );
    assert_eq!(support::terminal_count(process.transcript(), 3), 1);
    process.finish();
    assert_eq!(fs::read(workspace.join("Cargo.toml")).unwrap(), before);
    assert!(
        listener.accept().is_err(),
        "completion must not dispatch a model"
    );
}

#[test]
fn acp_conflicting_published_target_returns_clarification_without_observing_old_worker() {
    use vesper_harness::release_recovery::*;
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("workspace");
    let state_root = fixture.path().join("release-state");
    create_release_fixture(&workspace);
    let identity = repository_identity_for_workspace(&workspace).unwrap();
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let mut record = start_release(&identity, "patch", "main", &head).unwrap();
    record.state = ReleaseRecoveryState::Published;
    record.release_version = Some("0.1.1".into());
    record.release_commit = Some(head);
    record.mutation.publication_verified = true;
    record.mutation.version_after = Some("0.1.1".into());
    record.liveness.state = ReleaseLivenessState::Active;
    record.liveness.owner_pid = Some(std::process::id());
    let ledger = ReleaseLedger::open(&state_root, &identity).unwrap();
    ledger.save(&record).unwrap();
    let ledger_path = fs::read_dir(&state_root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let before = fs::read(&ledger_path).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut process = ProcessHarness::spawn_resource_governor_test_driver(
        listener.local_addr().unwrap(),
        [(
            "AGENT_VESPER_RELEASE_ROOT",
            state_root.display().to_string(),
        )],
    );
    let session = process.initialize_and_new_session_in(&workspace);
    process.prompt(
        3,
        &session,
        "/release 0.1.2",
        "fixture-conflicting-publication",
    );
    let response = process.response(3);
    assert!(response.get("error").is_none(), "{response}");
    let text = update_text(process.transcript());
    assert!(
        text.contains("Which release should RRC continue?"),
        "{text}"
    );
    assert_eq!(support::terminal_count(process.transcript(), 3), 1);
    process.finish();
    assert_eq!(fs::read(&ledger_path).unwrap(), before);
    assert!(listener.accept().is_err());
}

fn governed_release_process(request: &str, ask: bool) {
    let fixture = tempfile::tempdir().expect("temporary ACP RRC fixture");
    let workspace = fixture.path().join("workspace");
    let release_root = fixture.path().join("release-state");
    let cargo_bin = fixture.path().join("controlled-bin");
    let receipts = fixture.path().join("cargo-receipts");
    let release = fixture.path().join("release-cargo");
    create_release_fixture(&workspace);
    fs::create_dir_all(&cargo_bin).expect("create controlled command directory");
    write_controlled_cargo(&cargo_bin.join("cargo"), &receipts, &release);
    let _release = GateRelease(release.clone());

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking provider listener");
    let inherited_path = std::env::var("PATH").expect("test runner PATH");
    let path = format!("{}:{inherited_path}", cargo_bin.display());
    let mut process = ProcessHarness::spawn_resource_governor_test_driver(
        listener.local_addr().expect("listener address"),
        [
            (
                "AGENT_VESPER_RELEASE_ROOT",
                release_root.display().to_string(),
            ),
            ("PATH", path),
        ],
    );
    let session = process.initialize_and_new_session_in(&workspace);

    // This fixture owns temporary mutations explicitly. Exercise the actual
    // persisted session control; release composition must inherit that choice.
    process.send(json!({
        "jsonrpc":"2.0", "id":20, "method":"session/set_config_option",
        "params":{"sessionId":session,"configId":"permission_mode","value":if ask {"ask"} else {"bypass"}}
    }));
    let configured = process.response(20);
    assert!(configured.get("error").is_none(), "{configured}");

    process.prompt(3, &session, request, "resource-governor-start");
    if ask {
        // Version preparation and the next local gate are distinct owned steps.
        // Keep servicing protocol requests until both one-time approvals settle.
        answer_release_permission(&mut process, &session, "allow-once");
        answer_release_permission(&mut process, &session, "allow-once");
    }
    // Admission streams progress but the request remains active to own RRC.
    // Concurrent status/cancel must still work while the Cargo gate is blocked.
    wait_for_receipt(&receipts, "gate");

    let before_status = process.transcript().len();
    process.prompt(4, &session, "/release status", "resource-governor-status");
    let status = process.response(4);
    assert!(status.get("error").is_none(), "{status}");
    let status_text = update_text(&process.transcript()[before_status..]);
    assert_eq!(
        support::terminal_count(process.transcript(), 3),
        0,
        "release prompt must not terminate its ACP owner at admission"
    );
    for expected in [
        "RUN                  Local verification",
        "Current process      cargo",
        "RESOURCES",
        "RAM avail",
        "Cgroup",
        "RRC RSS",
        "Swap",
        "Swap trend",
        "Zram",
        "Memory PSI",
        "Gate need",
        "Disk free",
        "Cargo jobs  1",
        "Pressure    Normal",
        "Action      verification admitted: current headroom",
    ] {
        assert!(
            status_text.contains(expected),
            "missing {expected:?} in:\n{status_text}"
        );
    }
    assert!(
        !status_text.contains("Swap gate"),
        "logical swap occupancy must not be projected as a fixed gate: {status_text}"
    );
    assert!(
        status_text
            .lines()
            .filter(|line| line.trim_start().starts_with("Progress"))
            .all(|line| !line.contains('%')),
        "release progress must retain counts rather than invent a percentage: {status_text}"
    );

    let receipt_text = fs::read_to_string(&receipts).expect("governed Cargo receipts");
    let expected_target_root = release_root.join("host-resources/targets");
    for cargo_path in ["check", "metadata", "gate"] {
        let line = receipt_text
            .lines()
            .find(|line| line.starts_with(&format!("{cargo_path}|")))
            .unwrap_or_else(|| panic!("missing {cargo_path} receipt in {receipt_text:?}"));
        let fields = line.split('|').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "malformed receipt {line:?}");
        assert_eq!(
            fields[1], "1",
            "Cargo jobs must be governor-bounded: {line}"
        );
        assert_eq!(
            fields[2], "1",
            "test threads must be governor-bounded: {line}"
        );
        assert!(
            Path::new(fields[3]).starts_with(&expected_target_root),
            "Cargo must use the controller-managed target cache: {line}"
        );
    }
    assert!(
        !workspace.join("target").exists(),
        "the source workspace must never become the RRC Cargo cache"
    );

    // Cancel through the real host/controller before releasing the wrapper;
    // a genuine failed gate would correctly admit a provider-backed repair.
    process.prompt(5, &session, "/release cancel", "resource-governor-cancel");
    let cancelled = process.response(5);
    assert!(cancelled.get("error").is_none(), "{cancelled}");
    fs::write(&release, b"stop").expect("release controlled Cargo gate");
    wait_for_worker_settlement(&mut process, &session);
    let settled = process.response(3);
    assert!(settled.get("error").is_none(), "{settled}");
    assert_eq!(support::terminal_count(process.transcript(), 3), 1);
    process.finish();
    assert!(
        listener.accept().is_err(),
        "release controls/status must not dispatch a provider request"
    );
}

fn create_release_fixture(workspace: &Path) {
    fs::create_dir_all(workspace.join("member")).expect("create member directory");
    fs::create_dir_all(workspace.join("registry")).expect("create registry directory");
    fs::create_dir_all(workspace.join("docs/foundation")).expect("create evidence directory");
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write root manifest");
    fs::write(
        workspace.join("member/Cargo.toml"),
        "[package]\nname = \"rrc-resource-fixture\"\nversion.workspace = true\nedition.workspace = true\n",
    )
    .expect("write member manifest");
    fs::write(workspace.join("Cargo.lock"), "version = 3\n").expect("write lockfile");
    fs::write(
        workspace.join("registry/agent.json"),
        "{\n  \"version\": \"0.1.0\"\n}\n",
    )
    .expect("write registry");
    fs::write(
        workspace.join("docs/foundation/objective-evidence.md"),
        "# RRC resource governor process fixture\n",
    )
    .expect("write evidence fixture");
    fs::write(
        workspace.join("docs/foundation/release-objective-provenance.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "objective_id": "rrc-resource-governor-process-test",
            "objective_label": "RRC resource governor process acceptance",
            "variant_label": "Controlled ACP status fixture",
            "completed_at": "2026-09-30T00:00:00Z",
            "evidence_reports": ["docs/foundation/objective-evidence.md"]
        }))
        .expect("serialize objective provenance"),
    )
    .expect("write objective provenance");
    run_git(workspace, &["init", "--initial-branch=main"]);
    run_git(
        workspace,
        &["config", "user.email", "rrc-fixture@example.invalid"],
    );
    run_git(workspace, &["config", "user.name", "RRC Fixture"]);
    run_git(workspace, &["add", "."]);
    run_git(workspace, &["commit", "-m", "resource governor fixture"]);
    run_git(
        workspace,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/vesper/fixture.git",
        ],
    );
}

fn write_controlled_cargo(path: &Path, receipts: &Path, release: &Path) {
    let quote = |value: &Path| value.display().to_string().replace('\'', "'\"'\"'");
    fs::write(
        path,
        format!(
            "#!/bin/sh\ntag=gate\ncase \" $* \" in\n  *\" metadata \"*) tag=metadata ;;\n  *\" check \"*) tag=check ;;\nesac\nprintf '%s|%s|%s|%s\\n' \"$tag\" \"${{CARGO_BUILD_JOBS-}}\" \"${{RUST_TEST_THREADS-}}\" \"${{CARGO_TARGET_DIR-}}\" >> '{}'\nif [ \"$tag\" = metadata ]; then\n  printf '%s\\n' '{{\"packages\":[]}}'\n  exit 0\nfi\nif [ \"$tag\" = check ]; then\n  exit 0\nfi\nwhile [ ! -e '{}' ]; do sleep 0.02; done\nexit 7\n",
            quote(receipts),
            quote(release),
        ),
    )
    .expect("write controlled Cargo command");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("make controlled Cargo executable");
}

fn run_git(workspace: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .unwrap_or_else(|error| panic!("spawn git {}: {error}", args.join(" ")));
    assert!(
        output.status.success(),
        "git {} failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_for_receipt(receipts: &Path, name: &str) {
    // The integration policy supplies deterministic capacity, so the
    // controlled Cargo process must record promptly even on a pressured shared
    // runner. Keep this bound short enough to expose lifecycle regressions.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read_to_string(receipts).ok().is_some_and(|text| {
            text.lines()
                .any(|line| line.starts_with(&format!("{name}|")))
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "controlled Cargo did not record {name} before its scheduling deadline; receipts={:?}",
            fs::read_to_string(receipts).ok()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_worker_settlement(process: &mut ProcessHarness, session: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    for id in 6..50 {
        let start = process.transcript().len();
        process.prompt(
            id,
            session,
            "/release status",
            "resource-governor-settlement",
        );
        let response = process.response(id);
        assert!(response.get("error").is_none(), "{response}");
        let text = update_text(&process.transcript()[start..]);
        if !text.contains("\nRUN                  ") {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "RRC worker did not settle after controlled Cargo failed: {text}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!("RRC worker did not settle within the bounded status polls");
}

fn update_text(values: &[serde_json::Value]) -> String {
    support::update_texts(values, "agent_message_chunk").join("")
}
