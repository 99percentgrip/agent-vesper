//! The real keyboard/event-loop proof uses private state and loopback only.
#![cfg(all(unix, feature = "integration-test-harness"))]

#[test]
fn terminal_message_queue_and_paste_lifecycle() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/message_queue_pty.py");
    let python = std::env::var_os("VESPER_TEST_PYTHON").unwrap_or_else(|| "python3".into());
    let output = std::process::Command::new(python)
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_agent-vesper-tui"))
        .output()
        .expect("run bounded private PTY fixture");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
