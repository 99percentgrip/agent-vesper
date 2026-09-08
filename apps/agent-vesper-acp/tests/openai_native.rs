#![cfg(feature = "integration-test-harness")]
#![allow(dead_code)]
use serde_json::{Value, json};
use std::{net::TcpListener, thread, time::Duration};
mod support;
use support::{ProcessHarness, read_http_request, write_sse};

fn sse(events: Vec<Value>) -> String {
    events
        .into_iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect()
}

#[test]
fn native_openai_runs_a_real_harness_tool_and_returns_result_to_responses() {
    tool_round_trip("api-key", false);
}

#[test]
fn native_subscription_runs_the_same_real_harness_tool_without_codex() {
    tool_round_trip("chatgpt", false);
}

#[test]
fn both_native_modes_honor_read_only_permission_and_return_the_denial() {
    for mode in ["api-key", "chatgpt"] {
        tool_round_trip(mode, true);
    }
}

fn tool_round_trip(mode: &'static str, denied: bool) {
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
    let root = process.isolated_root().to_path_buf();
    std::fs::write(root.join("fixture.txt"), "native-openai-tool-canary").unwrap();
    let path = root
        .join(if denied { "denied.txt" } else { "fixture.txt" })
        .to_string_lossy()
        .into_owned();
    let tool = if denied { "write_file" } else { "read_file" };
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let wire = read_http_request(&mut first);
        assert_eq!(
            wire.to_ascii_lowercase()
                .contains("chatgpt-account-id: fixture-account"),
            mode == "chatgpt"
        );
        let body: Value = serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["model"], "gpt-6-astra");
        assert_eq!(body["reasoning"]["effort"], "max");
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["name"] == tool)
        );
        let args = if denied {
            json!({"path":path,"content":"must-not-be-written"})
        } else {
            json!({"path":path})
        }
        .to_string();
        write_sse(
            &mut first,
            &sse(vec![
                json!({"type":"response.created","response":{"id":"resp1"}}),
                json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call1","name":tool,"arguments":""}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":args}),
                json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"call1","name":tool,"arguments":args}}),
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
        let output = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .unwrap_or_else(|| panic!("Missing tool result in fixture request: {}", body["input"]));
        assert!(
            body["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "function_call" && item["call_id"] == "call1")
        );
        assert_eq!(output["call_id"], "call1");
        assert!(
            output["output"].as_str().unwrap().contains(if denied {
                "denied"
            } else {
                "native-openai-tool-canary"
            }),
            "{output}"
        );
        write_sse(
            &mut second,
            &sse(vec![
                json!({"type":"response.output_text.delta","output_index":0,"delta":"Native OpenAI completed."}),
                json!({"type":"response.completed","response":{}}),
            ]),
        );
    });
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    assert_eq!(process.response(1)["result"]["protocolVersion"], 1);
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":root,"mcpServers":[]}}));
    let response = process.response(2);
    let session = response["result"]["sessionId"].as_str().expect("session");
    for (id, control, value) in [
        (8, "provider", "lmstudio"),
        (9, "provider", "openai"),
        (10, "model", "gpt-6-astra"),
        (11, "thought_level", "max"),
        (12, "permission_mode", "read"),
    ] {
        process.send(json!({"jsonrpc":"2.0","id":id,"method":"session/set_config_option","params":{"sessionId":session,"configId":control,"value":value}}));
        let response = process.response(id);
        assert!(response.get("error").is_none(), "{response}");
    }
    process.prompt(
        3,
        session,
        "Read fixture.txt using read_file, then report completion.",
        "openai-user1",
    );
    let result = process.response(3);
    assert_eq!(result["result"]["stopReason"], "end_turn", "{result}");
    assert!(process.transcript().iter().any(|v| {
        v["params"]["update"]["content"]["text"]
            .as_str()
            .is_some_and(|t| t.contains("Native OpenAI completed."))
    }));
    server.join().unwrap();
    assert!(!root.join("denied.txt").exists());
    process.finish();
}
