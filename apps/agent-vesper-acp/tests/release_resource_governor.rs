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
fn acp_process_release_status_uses_governor_for_every_cargo_path() {
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

    process.prompt(3, &session, "/release patch", "resource-governor-start");
    let started = process.response(3);
    assert!(started.get("error").is_none(), "{started}");
    assert!(
        update_text(process.transcript())
            .contains("Release controller started and is preparing the release target."),
        "the ACP command must admit the controller rather than dispatch a provider"
    );
    wait_for_receipt(&receipts, "gate");

    let before_status = process.transcript().len();
    process.prompt(4, &session, "/release status", "resource-governor-status");
    let status = process.response(4);
    assert!(status.get("error").is_none(), "{status}");
    let status_text = update_text(&process.transcript()[before_status..]);
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

    // Stop the controlled first local gate. It returns a nonzero result so
    // the controller settles without a push/tag/publication attempt.
    fs::write(&release, b"stop").expect("release controlled Cargo gate");
    wait_for_worker_settlement(&mut process, &session);
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
            "#!/bin/sh\ntag=gate\ncase \" $* \" in\n  *\" metadata \"*) tag=metadata ;;\n  *\" check \"*) tag=check ;;\nesac\nprintf '%s|%s|%s|%s\\n' \"$tag\" \"${{CARGO_BUILD_JOBS-}}\" \"${{RUST_TEST_THREADS-}}\" \"${{CARGO_TARGET_DIR-}}\" >> '{}'\nif [ \"$tag\" = metadata ]; then\n  printf '{{\\\"packages\\\":[]}}'\n  exit 0\nfi\nif [ \"$tag\" = check ]; then\n  exit 0\nfi\nwhile [ ! -e '{}' ]; do sleep 0.02; done\nexit 7\n",
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
    for id in 5..50 {
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
