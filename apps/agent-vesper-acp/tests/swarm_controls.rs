//! Real ACP process activation and session-local draft/refusal acceptance.
#![cfg(feature = "swarm")]
#![allow(dead_code)]
mod support;
use serde_json::json;
use support::ProcessHarness;

#[test]
fn native_settings_save_cancel_and_refusals_never_dispatch_a_provider() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cognition = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    std::fs::write(
        cognition.path().join("embedding.json"),
        r#"{"source":"local"}"#,
    )
    .unwrap();
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
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert!(process.response(1).get("error").is_none());
    let mut sessions = Vec::new();
    for id in [2, 3] {
        process.send(json!({"jsonrpc":"2.0","id":id,"method":"session/new","params":{"cwd":root,"mcpServers":[]}}));
        sessions.push(
            process.response(id)["result"]["sessionId"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    for (index, session, command) in [
        (4, 0, "/swarm run disabled goal"),
        (5, 0, "/settings swarm enabled on"),
        (6, 1, "/settings swarm save"),
        (7, 0, "/settings swarm cancel"),
    ] {
        process.prompt(
            index,
            &sessions[session],
            command,
            &format!("control-{index}"),
        );
        assert!(process.response(index).get("error").is_none());
    }
    assert!(!root.join(".agent-vesper/swarm-settings.json").exists());
    for (index, command) in [
        (8, "/settings swarm enabled on"),
        (9, "/settings swarm save"),
        (10, "/swarm run embedding unavailable"),
    ] {
        process.prompt(index, &sessions[0], command, &format!("control-{index}"));
        assert!(process.response(index).get("error").is_none());
    }
    let saved = vesper_harness::swarm_settings::load(&root).unwrap();
    assert!(saved.enabled);
    assert_eq!(saved.drivers, 3);
    let text = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(text.contains("Swarm is off"), "{text}");
    assert!(text.contains("Open swarm settings before saving"), "{text}");
    assert!(text.contains("real embedding source"), "{text}");
    assert!(
        !std::fs::read_dir(root.join(".agent-vesper"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("swarm-run-"))
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    process.finish();
}
