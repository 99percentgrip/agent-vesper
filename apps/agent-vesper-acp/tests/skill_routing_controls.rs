//! Real protocol controls; no provider calls or real user-state writes.
#![allow(dead_code)]
mod support;
use serde_json::json;
use std::{net::TcpListener, sync::mpsc, thread};
use support::{ProcessHarness, read_http_request, successful_body, write_sse};

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

#[test]
fn ordinary_use_the_prose_reaches_the_acp_provider_once_and_session_continues() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (requests_tx, requests_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        for answer in ["first ordinary reply", "second ordinary reply"] {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            requests_tx.send(request).unwrap();
            write_sse(&mut stream, &successful_body(answer));
        }
    });

    let mut process = ProcessHarness::spawn_with_environment(
        address,
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
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"authenticate","params":{"methodId":"zai-api-key-setup"}}));
    assert!(process.response(2).get("error").is_none());
    process.send(json!({"jsonrpc":"2.0","id":3,"method":"session/new","params":{"cwd":root,"mcpServers":[]}}));
    let session = process.response(3)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    let ordinary =
        "Use the actual repository review links.\nExplain tools, permissions, skills and memory.";
    process.prompt(4, &session, ordinary, "ordinary-prose");
    let first = process.response(4);
    assert!(first.get("error").is_none(), "{first}");
    process.prompt(
        5,
        &session,
        "Confirm the session accepted a later turn.",
        "later-turn",
    );
    let second = process.response(5);
    assert!(second.get("error").is_none(), "{second}");

    let requests = [requests_rx.recv().unwrap(), requests_rx.recv().unwrap()];
    assert!(
        requests[0].contains("Use the actual repository review links.\\nExplain tools, permissions, skills and memory."),
        "ordinary prompt was not preserved on the ACP provider wire: {}",
        requests[0]
    );
    assert!(requests[1].contains("Confirm the session accepted a later turn."));
    assert_eq!(requests_rx.try_iter().count(), 0);
    let output = support::update_texts(process.transcript(), "agent_message_chunk").join("\n");
    assert!(output.contains("first ordinary reply"), "{output}");
    assert!(output.contains("second ordinary reply"), "{output}");
    assert!(!output.contains("skill routing failed"), "{output}");
    server.join().unwrap();
}
