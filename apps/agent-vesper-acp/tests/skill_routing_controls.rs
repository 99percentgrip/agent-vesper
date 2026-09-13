//! Real protocol controls; no provider calls or real user-state writes.
#![allow(dead_code)]
mod support;
use serde_json::json;
use support::ProcessHarness;

#[test]
fn native_skill_controls_save_explicitly_and_do_not_dispatch_provider() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut process = ProcessHarness::spawn_with_environment(
        listener.local_addr().unwrap(),
        [
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
        ],
    );
    let root = process.isolated_root().join("workspace");
    std::fs::create_dir(&root).unwrap();
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert!(process.response(1).get("error").is_none());
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":root,"mcpServers":[]}}));
    let session = process.response(2)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    process.prompt(3, &session, "/skills settings status", "status");
    assert!(process.response(3).get("error").is_none());
    assert!(!root.join(".agent-vesper").exists());
    for (id, command) in [
        (4, "/skills settings save mode enhanced"),
        (5, "/skills settings save disable ledger-audit"),
        (6, "/skills settings status"),
    ] {
        process.prompt(id, &session, command, &format!("control-{id}"));
        assert!(process.response(id).get("error").is_none());
    }
    let saved = vesper_harness::skill_routing_settings::load(&root).unwrap();
    assert_eq!(
        saved.mode,
        vesper_harness::skill_routing_settings::RoutingMode::Enhanced
    );
    assert!(saved.disabled.contains("ledger-audit"));
    let text = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(text.contains("Skills settings saved"), "{text}");
    assert!(text.contains("Enhanced"), "{text}");
    assert!(
        listener.accept().is_err(),
        "settings must not call providers"
    );
}
