//! Real-host rejection handling with synthetic credentials and loopback traffic.
#![cfg(feature = "integration-test-harness")]
#![allow(dead_code)]
use serde_json::json;
use std::{io::Write, net::TcpListener, thread, time::Duration};
mod support;
use support::{ProcessHarness, read_http_request};

#[test]
fn native_rejections_are_safe_and_actionable_in_both_authentication_modes() {
    for mode in ["api-key", "chatgpt"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut process = ProcessHarness::spawn_with_environment(
            address,
            [
                ("AGENT_VESPER_PROVIDER", "openai".into()),
                ("AGENT_VESPER_OPENAI_TEST_MODE", mode.into()),
                (
                    "AGENT_VESPER_OPENAI_TEST_URL",
                    format!("http://{address}/responses"),
                ),
                ("AGENT_VESPER_FULL_HARNESS", "1".into()),
                ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            ],
        );
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
            let wire = read_http_request(&mut socket);
            assert_eq!(
                wire.to_ascii_lowercase()
                    .contains("chatgpt-account-id: fixture-account"),
                mode == "chatgpt"
            );
            let body = json!({"error": {
                "code": "context_length_exceeded", "param": "input",
                "message": "private-provider-prose-canary"
            }})
            .to_string();
            write!(socket, "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        process.send(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}),
        );
        process.response(1);
        process.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":process.isolated_root(),"mcpServers":[]}}));
        let response = process.response(2);
        let session = response["result"]["sessionId"].as_str().unwrap();
        process.prompt(3, session, "Return a short answer.", "rejection-test");
        let rejection = process.response(3);
        assert_eq!(
            rejection["error"]["data"],
            "provider turn failed: ContextLimit"
        );
        server.join().unwrap();
        let (transcript, stderr) = process.finish_and_capture();
        let encoded = serde_json::to_string(&transcript).unwrap();
        assert!(!encoded.contains("private-provider-prose-canary"));
        assert!(!stderr.contains("private-provider-prose-canary"));
    }
}
