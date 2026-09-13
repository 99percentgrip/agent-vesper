//! Guided setup stays a native user command, with ACP-only progress and no provider dispatch.
#![allow(dead_code)]
mod support;
use serde_json::json;
use support::ProcessHarness;

#[test]
fn native_dependency_preview_and_failure_preserve_workspace_and_protocol() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let isolated = tempfile::tempdir().unwrap();
    let mut process = ProcessHarness::spawn_with_environment(
        listener.local_addr().unwrap(),
        [
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            (
                "AGENT_VESPER_COGNITION_ROOT",
                isolated.path().join("cognition").display().to_string(),
            ),
            (
                "AGENT_VESPER_GLOBAL_COGNITION_ROOT",
                isolated.path().join("global").display().to_string(),
            ),
            (
                "VESPER_DOCKER_BIN",
                isolated.path().join("missing-engine").display().to_string(),
            ),
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
    for (id, command) in [
        (3, "/web prepare"),
        (4, "/web prepare confirm"),
        (5, "/web prepare confirm"),
    ] {
        process.prompt(id, &session, command, "dependency-control");
        assert!(process.response(id).get("error").is_none());
        assert!(!root.join(".agent-vesper").exists());
    }
    let text = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(text.contains("Confirm with /web prepare confirm"), "{text}");
    assert!(
        text.contains("Checking the bundled browser package"),
        "{text}"
    );
    let expected = if cfg!(feature = "docker") {
        "driver is missing"
    } else {
        "This build does not include container support"
    };
    assert_eq!(
        text.matches(expected).count(),
        2,
        "both final failures must be streamed: {text}"
    );
    assert!(!text.contains("Browser and isolation ready"), "{text}");
    assert!(
        listener.accept().is_err(),
        "setup must not dispatch a provider request"
    );
}
