#![cfg(feature = "integration-test-harness")]
#![allow(dead_code)]

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

mod support;
use support::{ProcessHarness, read_http_request, write_sse};

fn sse(events: Vec<Value>) -> String {
    events
        .into_iter()
        .map(|value| format!("data: {value}\n\n"))
        .collect()
}

fn serve_models(listener: &TcpListener) {
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut buffer = [0; 4096];
        let count = socket.read(&mut buffer).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
    let request = String::from_utf8(bytes).unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /language-models http/1.1"));
    assert!(request.contains("x-xai-token-auth: xai-grok-cli"));
    let body = r#"{"models":[{"id":"grok-4.7"}]}"#;
    write!(
        socket,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

fn spawn(listener: &TcpListener) -> ProcessHarness {
    let address = listener.local_addr().unwrap();
    ProcessHarness::spawn_with_environment(
        address,
        [
            ("AGENT_VESPER_PROVIDER", "xai".into()),
            ("AGENT_VESPER_XAI_TEST_MODE", "grok-session".into()),
            (
                "AGENT_VESPER_XAI_TEST_URL",
                format!("http://{address}/responses"),
            ),
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
            ("AGENT_VESPER_VRO_ENABLED", "0".into()),
            ("AGENT_VESPER_XAI_TEST_STALE_HOSTED", "1".into()),
        ],
    )
}

fn initialize(process: &mut ProcessHarness) -> String {
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert_eq!(process.response(1)["result"]["protocolVersion"], 1);
    process.send(json!({
        "jsonrpc":"2.0",
        "id":2,
        "method":"session/new",
        "params":{"cwd":process.isolated_root(),"mcpServers":[]}
    }));
    let response = process.response(2);
    let session = response["result"]["sessionId"]
        .as_str()
        .expect("session")
        .to_owned();
    process.send(json!({
        "jsonrpc":"2.0",
        "id":9,
        "method":"session/set_config_option",
        "params":{"sessionId":session,"configId":"permission_mode","value":"bypass"}
    }));
    let response = process.response(9);
    assert!(response.get("error").is_none(), "{response}");
    session
}

#[test]
fn grok_session_plain_hello_reaches_transport_with_full_code_registry() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut process = spawn(&listener);
    let server = thread::spawn(move || {
        serve_models(&listener);
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let wire = read_http_request(&mut socket);
        let headers = wire.split_once("\r\n\r\n").unwrap().0.to_ascii_lowercase();
        assert!(headers.contains("x-xai-token-auth: xai-grok-cli"));
        let body: Value = serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
        let cache_key = body["prompt_cache_key"]
            .as_str()
            .expect("production AgentLoop must route a stable prompt-cache key")
            .to_owned();
        assert!(cache_key.starts_with("vesper-conversation-"));
        assert!(headers.contains(&format!("x-grok-conv-id: {cache_key}")));
        assert_eq!(body["model"], "grok-4.7");
        assert_eq!(body["reasoning"]["effort"], "high");
        let names = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for required in [
            "read_file",
            "list_directory",
            "search_files",
            "grep",
            "write_file",
            "edit_file",
            "apply_patch",
            "run_command",
            "update_plan",
        ] {
            assert!(names.contains(required), "missing shared tool {required}");
        }
        assert!(body["tools"].as_array().unwrap().iter().all(|tool| {
            !matches!(
                tool["type"].as_str(),
                Some("web_search" | "x_search" | "code_interpreter" | "file_search" | "mcp")
            )
        }));
        write_sse(
            &mut socket,
            &sse(vec![
                json!({"type":"response.created","response":{"id":"resp-hello"}}),
                json!({"type":"response.output_text.delta","output_index":0,"delta":"hello from xAI"}),
                json!({"type":"response.completed","response":{}}),
            ]),
        );
    });
    let session = initialize(&mut process);
    process.prompt(3, &session, "hello", "xai-hello");
    let result = process.response(3);
    assert_eq!(result["result"]["stopReason"], "end_turn", "{result}");
    assert!(process.transcript().iter().any(|value| {
        value["params"]["update"]["content"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("hello from xAI"))
    }));
    server.join().unwrap();
    process.finish();
}

#[test]
fn grok_session_read_file_executes_once() {
    tool_round_trip("read_file");
}

#[test]
fn grok_session_run_command_executes_once() {
    tool_round_trip("run_command");
}

fn tool_round_trip(tool: &'static str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut process = spawn(&listener);
    let root = process.isolated_root().to_path_buf();
    let input = root.join("fixture.txt");
    let marker = root.join("command-marker.txt");
    std::fs::write(&input, "xai-read-canary").unwrap();
    let arguments = if tool == "read_file" {
        json!({"path":input}).to_string()
    } else {
        json!({"command":format!("printf x >> '{}'", marker.display())}).to_string()
    };
    let expected = (tool == "read_file").then_some("xai-read-canary");
    let server = thread::spawn(move || {
        serve_models(&listener);
        let (mut first, _) = listener.accept().unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let wire = read_http_request(&mut first);
        let body: Value = serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
        let cache_key = body["prompt_cache_key"]
            .as_str()
            .expect("production AgentLoop must route a stable prompt-cache key")
            .to_owned();
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == tool)
        );
        write_sse(
            &mut first,
            &sse(vec![
                json!({"type":"response.created","response":{"id":"resp-tool"}}),
                json!({"type":"response.output_item.done","output_index":1,"item":{"type":"reasoning","id":"reasoning-once","summary":[],"encrypted_content":"opaque-round-trip-canary"}}),
                json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call-once","name":tool,"arguments":""}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":arguments}),
                json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"call-once","name":tool,"arguments":arguments}}),
                json!({"type":"response.completed","response":{}}),
            ]),
        );
        drop(first);
        let (mut second, _) = listener.accept().unwrap();
        second
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let wire = read_http_request(&mut second);
        let body: Value = serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["prompt_cache_key"], cache_key);
        let outputs = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output" && item["call_id"] == "call-once")
            .collect::<Vec<_>>();
        assert_eq!(
            outputs.len(),
            1,
            "tool result was not continued exactly once"
        );
        assert!(
            body["input"].as_array().unwrap().iter().any(|item| {
                item["type"] == "reasoning"
                    && item["encrypted_content"] == "opaque-round-trip-canary"
            }),
            "opaque reasoning was not consumed by the next production request"
        );
        let output = outputs[0]["output"].as_str().unwrap();
        if let Some(expected) = expected {
            assert!(output.contains(expected));
        }
        write_sse(
            &mut second,
            &sse(vec![
                json!({"type":"response.output_text.delta","output_index":0,"delta":"tool complete"}),
                json!({"type":"response.completed","response":{}}),
            ]),
        );
    });
    let session = initialize(&mut process);
    process.prompt(
        3,
        &session,
        &format!("Use {tool} exactly once."),
        "xai-tool",
    );
    let result = process.response(3);
    assert_eq!(result["result"]["stopReason"], "end_turn", "{result}");
    server.join().unwrap();
    if tool == "run_command" {
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "x");
    }
    process.finish();
}
