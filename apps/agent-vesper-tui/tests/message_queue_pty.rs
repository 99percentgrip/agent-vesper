//! The real keyboard/event-loop proof uses private state and loopback only.
#![cfg(all(unix, feature = "integration-test-harness"))]

fn run_fixture(name: &str, binary: bool) {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name);
    let python = std::env::var_os("VESPER_TEST_PYTHON").unwrap_or_else(|| "python3".into());
    let log = tempfile::NamedTempFile::new().unwrap();
    let stdout = log.reopen().unwrap();
    let stderr = stdout.try_clone().unwrap();
    let mut command = Command::new(python);
    command.arg(script).process_group(0);
    if binary {
        command.arg(env!("CARGO_BIN_EXE_agent-vesper-tui"));
    }
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("run bounded private PTY fixture");
    let deadline = Instant::now() + Duration::from_secs(270);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            // The fixture handles TERM through its ordinary cleanup path.
            let _ = Command::new("kill")
                .args(["-TERM", &format!("-{}", child.id())])
                .status();
            let cleanup_deadline = Instant::now() + Duration::from_secs(15);
            while child.try_wait().unwrap().is_none() && Instant::now() < cleanup_deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            if child.try_wait().unwrap().is_none() {
                let _ = Command::new("kill")
                    .args(["-KILL", &format!("-{}", child.id())])
                    .status();
            }
            let _ = child.wait();
            panic!(
                "PTY fixture exceeded 270 seconds: {}",
                std::fs::read_to_string(log.path()).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        status.success(),
        "{}",
        std::fs::read_to_string(log.path()).unwrap()
    );
}

#[test]
fn terminal_message_queue_and_paste_lifecycle() {
    run_fixture("message_queue_pty.py", true);
}

#[test]
fn terminal_writer_handles_backpressure_and_partial_writes() {
    run_fixture("test_terminal_writer.py", false);
}

#[test]
fn terminal_show_more_restore_copy_and_queue_are_distinct() {
    run_fixture("message_journal_ux_pty.py", true);
}
