//! Regression matrix for bounded shell-command settlement.

#[cfg(windows)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use vesper_agent::executor::{ToolContext, ToolExecutor};
use vesper_agent::tools::RunCommand;
use vesper_domain::{
    BoundedString, SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId,
};
use vesper_provider::CancellationSignal;

#[derive(Default)]
struct Cancel(AtomicBool);

impl CancellationSignal for Cancel {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

fn context(root: &std::path::Path, cancellation: Arc<dyn CancellationSignal>) -> ToolContext {
    ToolContext {
        workspace_roots: vec![vesper_domain::WorkspaceRoot {
            name: BoundedString::new("workspace").unwrap(),
            path: BoundedString::new(root.to_string_lossy().to_string()).unwrap(),
            primary: true,
        }],
        firewall: None,
        sandbox: None,
        operating_mode: SessionOperatingMode::Code,
        permission_mode: SessionPermissionMode::Bypass,
        conversation: Vec::new(),
        cancellation,
    }
}

fn call(command: &str, timeout: u64) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("command-settlement").unwrap(),
        tool_id: ToolId::new("run_command").unwrap(),
        arguments: json!({"command": command, "timeout": timeout}),
        extensions: vesper_domain::ExtensionMap::default(),
    }
}

async fn run(command: &str, timeout: u64) -> Result<String, String> {
    let root = tempfile::tempdir().unwrap();
    RunCommand
        .execute(
            &call(command, timeout),
            &context(root.path(), Arc::new(Cancel::default())),
        )
        .await
        .map(|result| result.text.as_str().to_owned())
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
fn pressure_commands() -> [String; 3] {
    [
        "python3 -c 'import sys; sys.stdout.write(\"o\" * 1048576)'".into(),
        "python3 -c 'import sys; sys.stderr.write(\"e\" * 1048576)'".into(),
        "python3 -c 'import sys; [(sys.stdout.write(\"o\"*4096), sys.stdout.flush(), sys.stderr.write(\"e\"*4096), sys.stderr.flush()) for _ in range(128)]'".into(),
    ]
}

#[cfg(windows)]
fn powershell(script: &str) -> String {
    let utf16 = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(utf16)
    )
}

#[cfg(windows)]
fn powershell_descendant(script: &str) -> String {
    let utf16 = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    format!(
        "Start-Process powershell.exe -NoNewWindow -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand','{}')",
        STANDARD.encode(utf16)
    )
}

#[cfg(windows)]
fn pressure_commands() -> [String; 3] {
    [
        powershell("[Console]::Out.Write('o' * 1048576)"),
        powershell("[Console]::Error.Write('e' * 1048576)"),
        powershell(
            "1..128 | ForEach-Object { [Console]::Out.Write('o' * 4096); [Console]::Out.Flush(); [Console]::Error.Write('e' * 4096); [Console]::Error.Flush() }",
        ),
    ]
}

#[cfg(unix)]
fn exact_output_command(count: usize) -> String {
    format!("python3 -c 'print(\"x\" * {count}, end=\"\")'")
}

#[cfg(windows)]
fn exact_output_command(count: usize) -> String {
    powershell(&format!("[Console]::Out.Write('x' * {count})"))
}

#[cfg(unix)]
fn timeout_command() -> String {
    "echo partial; (sleep 2; echo stale > descendant.marker) & while :; do printf x; done".into()
}

#[cfg(windows)]
fn timeout_command() -> String {
    powershell(&format!(
        "Write-Output partial; {}; while ($true) {{ [Console]::Out.Write('x' * 4096) }}",
        powershell_descendant(
            "Start-Sleep -Seconds 2; Set-Content -LiteralPath descendant.marker -Value stale"
        )
    ))
}

#[cfg(unix)]
fn cancellation_command(marker: &str) -> String {
    format!("echo before-cancel; (sleep 2; echo stale > {marker}) & while :; do printf y; done")
}

#[cfg(windows)]
fn cancellation_command(marker: &str) -> String {
    powershell(&format!(
        "Write-Output before-cancel; {}; while ($true) {{ [Console]::Out.Write('y' * 4096) }}",
        powershell_descendant(&format!(
            "Start-Sleep -Seconds 2; Set-Content -LiteralPath {marker} -Value stale"
        ))
    ))
}

#[cfg(unix)]
fn held_pipe_command() -> String {
    "(sleep 2; echo stale > descendant.marker; echo stale) & echo leader-done".into()
}

#[cfg(windows)]
fn held_pipe_command() -> String {
    powershell(&format!(
        "{}; Write-Output leader-done",
        powershell_descendant(
            "Start-Sleep -Seconds 2; Set-Content -LiteralPath descendant.marker -Value stale; [Console]::Out.Write('stale')"
        )
    ))
}

#[cfg(unix)]
fn nonzero_command() -> String {
    "printf out; printf err >&2; exit 7".into()
}

#[cfg(windows)]
fn nonzero_command() -> String {
    powershell("[Console]::Out.Write('out'); [Console]::Error.Write('err'); exit 7")
}

async fn assert_marker_absent_after_cleanup(path: &std::path::Path) {
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert!(
        !path.exists(),
        "owned descendant survived cleanup and wrote {}",
        path.display()
    );
}

#[tokio::test]
async fn drains_large_stdout_stderr_and_alternating_streams() {
    for command in pressure_commands() {
        let started = Instant::now();
        let output = run(&command, 5).await.expect("stream pressure must settle");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(output.contains("output truncated"));
        assert!(output.contains("drained=524288") || output.contains("drained=1048576"));
        assert!(output.len() <= 65_536);
    }
}

#[tokio::test]
async fn exact_and_near_retained_output_boundaries_are_truthful() {
    let below = run(&exact_output_command(65_534), 5).await.unwrap();
    assert_eq!(below.len(), 65_534);
    assert!(!below.contains("output truncated"));

    let exact = run(&exact_output_command(65_536), 5).await.unwrap();
    assert_eq!(exact.len(), 65_536);
    assert!(!exact.contains("output truncated"));

    let over = run(&exact_output_command(65_537), 5).await.unwrap();
    assert!(over.len() <= 65_536);
    assert!(over.contains("stdout drained=65537"));
    assert!(over.contains("output truncated"));
}

#[tokio::test]
async fn historical_instruction_file_and_larger_stress_settle_repeatedly() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("instructions.md"), vec![b'i'; 71_443]).unwrap();
    let ctx = context(root.path(), Arc::new(Cancel::default()));
    #[cfg(unix)]
    let commands = [
        "cat instructions.md",
        "git diff --no-index /dev/null instructions.md; test $? -eq 1",
    ];
    #[cfg(windows)]
    let commands = [
        "type instructions.md",
        "git diff --no-index NUL instructions.md & if errorlevel 2 exit /b 2 & exit /b 0",
    ];
    for command in commands {
        let result = RunCommand
            .execute(&call(command, 5), &ctx)
            .await
            .expect("historical output shape must settle");
        assert!(result.text.as_str().contains("output truncated"));
    }
    let next = RunCommand
        .execute(&call("printf recovered", 5), &ctx)
        .await
        .unwrap();
    assert_eq!(next.text.as_str(), "recovered");
}

#[tokio::test]
async fn timeout_preserves_partial_output_and_cleans_descendants() {
    let root = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let error = RunCommand
        .execute(
            &call(&timeout_command(), 1),
            &context(root.path(), Arc::new(Cancel::default())),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(error.contains("command timed out"));
    assert!(error.contains("cleanup=verified"));
    assert!(error.contains("partial"));
    assert!(error.contains("stdout=") || error.contains("output truncated"));
    assert_marker_absent_after_cleanup(&root.path().join("descendant.marker")).await;
}

#[tokio::test]
async fn cancellation_preserves_partial_output_and_next_command_runs() {
    let root = tempfile::tempdir().unwrap();
    let cancel = Arc::new(Cancel::default());
    let signal: Arc<dyn CancellationSignal> = cancel.clone();
    let ctx = context(root.path(), signal);
    let call = call(&cancellation_command("cancelled.marker"), 20);
    let task = tokio::spawn(async move { RunCommand.execute(&call, &ctx).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.0.store(true, Ordering::Release);
    let error = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("cancellation settlement bound")
        .unwrap()
        .unwrap_err()
        .to_string();
    assert!(error.contains("command cancelled"));
    assert!(error.contains("cleanup=verified"));
    assert!(error.contains("before-cancel"));
    assert_marker_absent_after_cleanup(&root.path().join("cancelled.marker")).await;
    assert_eq!(run("printf after-cancel", 5).await.unwrap(), "after-cancel");
}

#[tokio::test]
async fn dropping_async_caller_still_cleans_owned_process_tree() {
    let root = tempfile::tempdir().unwrap();
    let ready_path = root.path().join("leader.ready");
    let ctx = context(root.path(), Arc::new(Cancel::default()));
    #[cfg(unix)]
    let command =
        "touch leader.ready; (sleep 2; echo stale > dropped.marker) & while :; do printf d; done"
            .to_owned();
    #[cfg(windows)]
    let command = powershell(&format!(
        "Set-Content -LiteralPath leader.ready -Value ready; {}; while ($true) {{ [Console]::Out.Write('d' * 4096) }}",
        powershell_descendant(
            "Start-Sleep -Seconds 2; Set-Content -LiteralPath dropped.marker -Value stale"
        )
    ));
    let call = call(&command, 20);
    let task = tokio::spawn(async move { RunCommand.execute(&call, &ctx).await });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready_path.exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ready_path.exists(), "fixture command did not start");
    task.abort();
    let _ = task.await;
    assert_marker_absent_after_cleanup(&root.path().join("dropped.marker")).await;
    assert_eq!(run("printf after-drop", 5).await.unwrap(), "after-drop");
}

#[tokio::test]
async fn leader_exit_does_not_wait_for_descendant_held_pipe() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context(root.path(), Arc::new(Cancel::default()));
    let started = Instant::now();
    let output = RunCommand
        .execute(&call(&held_pipe_command(), 5), &ctx)
        .await
        .expect("owned descendant must be cleaned after leader exit")
        .text
        .as_str()
        .to_owned();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(output.contains("leader-done"));
    assert!(!output.contains("stale"));
    assert_marker_absent_after_cleanup(&root.path().join("descendant.marker")).await;
}

#[tokio::test]
async fn descendant_writing_after_leader_exit_is_cleaned() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context(root.path(), Arc::new(Cancel::default()));
    #[cfg(unix)]
    let command =
        "(while :; do printf z; done) & (sleep 2; echo stale > writer.marker) & exit 0".to_owned();
    #[cfg(windows)]
    let command = powershell(&format!(
        "{}; {}",
        powershell_descendant("while ($true) { [Console]::Out.Write('z' * 4096) }"),
        powershell_descendant(
            "Start-Sleep -Seconds 2; Set-Content -LiteralPath writer.marker -Value stale"
        )
    ));
    let started = Instant::now();
    let output = RunCommand
        .execute(&call(&command, 5), &ctx)
        .await
        .expect("writer descendant must be cleaned")
        .text
        .as_str()
        .to_owned();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!output.contains("stale"));
    assert_marker_absent_after_cleanup(&root.path().join("writer.marker")).await;
}

#[tokio::test]
async fn nonzero_exit_keeps_output_status_and_failure() {
    let error = run(&nonzero_command(), 5).await.unwrap_err();
    assert!(error.contains("command exited unsuccessfully"));
    assert!(error.contains("exit status: 7") || error.contains("exit code: 7"));
    assert!(error.contains("out"));
    assert!(error.contains("err"));
    assert!(error.contains("cleanup=verified"));
}
