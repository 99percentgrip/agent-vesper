//! Opt-in microphone-free acceptance using Cargo's managed binary path.
#![cfg(unix)]

#[test]
#[ignore = "requires VESPER_TEST_PYTHON pointing to a Python with numpy"]
fn microphone_free_voice_lifecycle() {
    let python = std::env::var("VESPER_TEST_PYTHON").unwrap_or_else(|_| "/usr/bin/python3".into());
    let prerequisite = std::process::Command::new(&python)
        .args(["-c", "import numpy"])
        .status()
        .expect("launch test Python");
    assert!(
        prerequisite.success(),
        "voice acceptance requires numpy in {python}"
    );
    let status = std::process::Command::new(&python)
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/voice_pty.py"))
        .arg(env!("CARGO_BIN_EXE_agent-vesper-tui"))
        .arg(&python)
        .status()
        .expect("launch voice lifecycle Python");
    assert!(status.success(), "voice lifecycle failed: {status}");
}
