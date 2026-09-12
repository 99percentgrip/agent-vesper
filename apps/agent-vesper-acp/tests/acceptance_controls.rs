//! Native ACP acceptance controls use isolated state and never call a provider.
#![allow(dead_code)]
mod support;
use serde_json::json;
use support::ProcessHarness;

#[test]
fn native_acceptance_controls_persist_only_explicit_activation() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cognition = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    let mut process = ProcessHarness::spawn_with_environment(
        listener.local_addr().unwrap(),
        [
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            (
                "AGENT_VESPER_COGNITION_ROOT",
                cognition.path().display().to_string(),
            ),
            (
                "AGENT_VESPER_GLOBAL_COGNITION_ROOT",
                global.path().display().to_string(),
            ),
        ],
    );
    let root = process.isolated_root().join("workspace");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("PRD.md"),
        "The implementation must expose the required behavior in both hosts.",
    )
    .unwrap();
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert!(process.response(1).get("error").is_none());
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":root,"mcpServers":[]}}));
    let session = process.response(2)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    process.prompt(3, &session, "/acceptance status", "default-status");
    assert!(process.response(3).get("error").is_none());
    assert!(!root.join(".agent-vesper").exists());
    for (id, command) in [
        (4, "/settings acceptance on PRD.md"),
        (5, "/acceptance status"),
        (6, "/acceptance export audit.json"),
        (7, "/acceptance stop"),
    ] {
        process.prompt(id, &session, command, &format!("control-{id}"));
        assert!(process.response(id).get("error").is_none());
    }
    let settings = vesper_harness::acceptance_settings::AcceptanceSettings::load(&root).unwrap();
    assert!(!settings.enabled);
    assert!(root.join("audit.json").exists());
    let text = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(text.contains("Acceptance enabled and saved"), "{text}");
    assert!(text.contains("INCOMPLETE"), "{text}");
    assert!(
        !text.contains("Implementation acceptance: VERIFIED"),
        "{text}"
    );
    // Native opt-in no longer requires a manually entered PRD path.
    std::fs::remove_file(root.join(".agent-vesper/acceptance-settings.json")).unwrap();
    process.prompt(8, &session, "/settings acceptance on", "automatic-on");
    assert!(process.response(8).get("error").is_none());
    let automatic = vesper_harness::acceptance_settings::AcceptanceSettings::load(&root).unwrap();
    assert!(automatic.enabled);
    assert!(automatic.prd.is_empty());
    process.prompt(9, &session, "/acceptance status", "automatic-status");
    assert!(process.response(9).get("error").is_none());
    let text = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(text.contains("automatically"), "{text}");
    assert!(text.contains("INCOMPLETE"), "{text}");
    assert!(
        listener.accept().is_err(),
        "controls must not dispatch provider requests"
    );
}
