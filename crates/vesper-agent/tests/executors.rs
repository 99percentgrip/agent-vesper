//! Real-executor integration tests (ADR 0010, Tier C Phase 3/4).
//!
//! Each executor runs against a fresh `tempfile` workspace root so the tests
//! prove (a) real filesystem/shell I/O and (b) path confinement rejects
//! escape attempts — without touching anything outside the temp root.

use serde_json::json;
use std::fs;
use vesper_agent::executor::{ToolContext, ToolExecutor};
use vesper_agent::tools::{
    ApplyPatch, EditFile, Grep, ListDirectory, ReadFile, RunCommand, SearchFiles, UpdatePlan,
    WriteFile,
};
use vesper_domain::{
    BoundedString, SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId,
};
use vesper_testkit::FakeProviderSession;

fn root_context(root: &std::path::Path) -> ToolContext {
    let roots = vec![vesper_domain::WorkspaceRoot {
        name: BoundedString::new("workspace").unwrap(),
        path: BoundedString::new(root.to_string_lossy().to_string()).unwrap(),
        primary: true,
    }];
    // Build an uncancellable context with the temp root as the primary root.
    let _ = FakeProviderSession::default(); // link the testkit surface
    vesper_agent::tools::stub_context(
        roots,
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    )
}

fn call(tool: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new(tool).unwrap(),
        arguments: args,
        extensions: vesper_domain::ExtensionMap::default(),
    }
}

#[tokio::test]
async fn read_file_returns_confined_file_contents() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), "hello vesper").unwrap();
    let context = root_context(root.path());
    let result = ReadFile
        .execute(&call("read_file", json!({"path": "note.txt"})), &context)
        .await
        .unwrap();
    assert_eq!(result.text.as_str(), "hello vesper");
}

#[tokio::test]
async fn write_file_creates_and_overwrites_on_disk() {
    let root = tempfile::tempdir().unwrap();
    let context = root_context(root.path());
    WriteFile
        .execute(
            &call(
                "write_file",
                json!({"path": "out/nested.txt", "content": "first"}),
            ),
            &context,
        )
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("out/nested.txt")).unwrap(),
        "first"
    );

    WriteFile
        .execute(
            &call(
                "write_file",
                json!({"path": "out/nested.txt", "content": "second"}),
            ),
            &context,
        )
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("out/nested.txt")).unwrap(),
        "second"
    );
}

#[tokio::test]
async fn edit_file_replaces_a_unique_block_on_disk() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("code.rs"), "fn old() {}\n").unwrap();
    let context = root_context(root.path());
    EditFile
        .execute(
            &call(
                "edit_file",
                json!({"path": "code.rs", "old_text": "fn old() {}", "new_text": "fn new() {}"}),
            ),
            &context,
        )
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("code.rs")).unwrap(),
        "fn new() {}\n"
    );
}

#[tokio::test]
async fn list_directory_enumerates_entries() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a").unwrap();
    fs::write(root.path().join("b.txt"), "b").unwrap();
    let context = root_context(root.path());
    let result = ListDirectory
        .execute(&call("list_directory", json!({})), &context)
        .await
        .unwrap();
    let listing = result.text.as_str().to_string();
    assert!(listing.contains("a.txt"));
    assert!(listing.contains("b.txt"));
}

#[tokio::test]
async fn search_files_matches_a_glob_pattern() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.rs"), "").unwrap();
    fs::write(root.path().join("b.txt"), "").unwrap();
    let context = root_context(root.path());
    let result = SearchFiles
        .execute(
            &call("search_files", json!({"pattern": "**/*.rs"})),
            &context,
        )
        .await
        .unwrap();
    let matches = result.text.as_str().to_string();
    assert!(matches.contains("a.rs"), "matches: {matches}");
    assert!(
        !matches.contains("b.txt"),
        "txt must not match *.rs: {matches}"
    );
}

#[tokio::test]
async fn grep_finds_matching_lines_with_line_numbers() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("g.txt"), "alpha\nbeta\nGAMMA\ndelta\n").unwrap();
    let context = root_context(root.path());
    let result = Grep
        .execute(
            &call("grep", json!({"pattern": "(?i)gamma", "include": "*.txt"})),
            &context,
        )
        .await
        .unwrap();
    let hits = result.text.as_str().to_string();
    assert!(hits.contains("g.txt:3:"), "hits: {hits}");
    assert!(hits.contains("GAMMA"));
}

#[tokio::test]
async fn run_command_executes_in_the_confined_workspace() {
    let root = tempfile::tempdir().unwrap();
    let context = root_context(root.path());
    let result = RunCommand
        .execute(
            &call("run_command", json!({"command": "echo vesper-shell"})),
            &context,
        )
        .await
        .unwrap();
    assert!(
        result.text.as_str().contains("vesper-shell"),
        "shell output must surface: {}",
        result.text.as_str()
    );
}

#[tokio::test]
async fn run_command_enforces_its_timeout() {
    let root = tempfile::tempdir().unwrap();
    let context = root_context(root.path());
    let result = RunCommand
        .execute(
            &call("run_command", json!({"command": "sleep 5", "timeout": 1})),
            &context,
        )
        .await
        .unwrap();
    assert!(
        result.text.as_str().contains("timed out"),
        "a 1s timeout must kill `sleep 5`: {}",
        result.text.as_str()
    );
}

#[tokio::test]
async fn mutating_tools_reject_path_escape_attempts() {
    let root = tempfile::tempdir().unwrap();
    let context = root_context(root.path());
    let error = WriteFile
        .execute(
            &call(
                "write_file",
                json!({"path": "../../../etc/vesper-escape", "content": "x"}),
            ),
            &context,
        )
        .await
        .expect_err("escape must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("escapes the workspace root"),
        "confinement must explain the escape: {message}"
    );
}

#[tokio::test]
async fn read_only_tools_also_reject_path_escape() {
    let root = tempfile::tempdir().unwrap();
    let context = root_context(root.path());
    assert!(
        ReadFile
            .execute(
                &call("read_file", json!({"path": "../../etc/passwd"})),
                &context
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn update_plan_writes_the_plan_artifact_and_returns_markdown() {
    let root = tempfile::tempdir().unwrap();
    let context = root_context(root.path());
    let result = UpdatePlan
        .execute(
            &call(
                "update_plan",
                json!({
                    "tasks": [
                        {"content": "Scaffold", "status": "in_progress", "priority": "high"},
                        {"content": "Ship", "status": "pending", "priority": "medium"}
                    ]
                }),
            ),
            &context,
        )
        .await
        .unwrap();
    // The plan markdown is returned so the loop can drive the REVIEW transition.
    let markdown = result.text.as_str().to_string();
    assert!(markdown.contains("# Plan"));
    assert!(markdown.contains("Scaffold"));
    // And persisted to the confined `.agent/plan.md` artifact.
    let on_disk = fs::read_to_string(root.path().join(".agent/plan.md")).unwrap();
    assert!(on_disk.contains("Ship"));
}

#[tokio::test]
async fn apply_patch_updates_the_target_file() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("p.txt"), "header\nalpha\nfooter\n").unwrap();
    let context = root_context(root.path());
    // Correct single-hunk unified diff: context + removed + added.
    let patch = " header\n-alpha\n+beta\n footer\n";
    ApplyPatch
        .execute(
            &call("apply_patch", json!({"path": "p.txt", "patch": patch})),
            &context,
        )
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("p.txt")).unwrap(),
        "header\nbeta\nfooter\n"
    );
}
