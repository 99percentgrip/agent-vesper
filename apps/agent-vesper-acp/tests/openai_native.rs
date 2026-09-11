#![cfg(feature = "integration-test-harness")]
#![allow(dead_code)]
use serde_json::{Value, json};
use std::{net::TcpListener, thread, time::Duration};
mod support;
use support::{ProcessHarness, read_http_request, write_sse};

fn serve_models(listener: &TcpListener, mode: &str) {
    use std::io::{Read, Write};
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
        let mut buffer = [0; 4096];
        let n = socket.read(&mut buffer).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buffer[..n]);
    }
    let headers = String::from_utf8(bytes).unwrap().to_lowercase();
    assert!(headers.starts_with("get /models"));
    assert!(headers.contains("authorization: bearer fixture-openai-key"));
    assert_eq!(
        headers.contains("chatgpt-account-id: fixture-account"),
        mode == "chatgpt"
    );
    let body = if mode == "chatgpt" {
        json!({"models":[{"slug":"gpt-6-astra","visibility":"list"},{"slug":"gpt-5.3-codex-spark","visibility":"list"},{"slug":"gpt-5.4","visibility":"hide"},{"slug":"unverified-model","visibility":"list"}]})
    } else {
        json!({"data":[{"id":"gpt-6-astra"},{"id":"unverified-model"}]})
    }.to_string();
    write!(socket, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).unwrap();
}

fn sse(events: Vec<Value>) -> String {
    events
        .into_iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect()
}

#[test]
fn usage_reports_native_subscription_windows_without_a_provider_turn() {
    use std::io::{Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [
            ("AGENT_VESPER_PROVIDER", "openai".into()),
            ("AGENT_VESPER_OPENAI_TEST_MODE", "chatgpt".into()),
            (
                "AGENT_VESPER_OPENAI_TEST_URL",
                format!("http://{address}/responses"),
            ),
            ("AGENT_VESPER_FULL_HARNESS", "1".into()),
        ],
    );
    let server = thread::spawn(move || {
        serve_models(&listener, "chatgpt");
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut bytes = Vec::new();
        while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            let mut buffer = [0; 4096];
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        let headers = String::from_utf8(bytes).unwrap().to_lowercase();
        assert!(headers.starts_with("get /usage "));
        assert!(headers.contains("chatgpt-account-id: fixture-account"));
        let body = r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":43,"limit_window_seconds":18000},"secondary_window":{"used_percent":37,"limit_window_seconds":604800}}}"#;
        write!(socket, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    process
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}));
    process.response(1);
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":process.isolated_root(),"mcpServers":[]}}));
    let response = process.response(2);
    let session = response["result"]["sessionId"].as_str().unwrap();
    process.prompt(3, session, "/usage", "usage-test");
    assert!(process.response(3).get("error").is_none());
    let text = process
        .transcript()
        .iter()
        .filter_map(|v| v["params"]["update"]["content"]["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "Model:",
        "Reasoning:",
        "Context window:",
        "plus",
        "57% left",
        "63% left",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    server.join().unwrap();
    process.finish();
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

#[test]
fn spark_native_tool_round_trip_omits_summary() {
    tool_round_trip_model("chatgpt", false, "gpt-5.3-codex-spark", "high");
}
fn tool_round_trip(mode: &'static str, denied: bool) {
    tool_round_trip_model(mode, denied, "gpt-6-astra", "max");
}
fn tool_round_trip_model(
    mode: &'static str,
    denied: bool,
    model: &'static str,
    effort: &'static str,
) {
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
        serve_models(&listener, mode);
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
        assert_eq!(body["model"], model);
        if model == "gpt-5.3-codex-spark" {
            assert!(body["reasoning"].get("summary").is_none());
        }
        assert_eq!(body["reasoning"]["effort"], effort);
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
    let model_options = response["result"]["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "model")
        .unwrap();
    let advertised = model_options["options"].to_string();
    assert!(advertised.contains("gpt-6-astra"), "{model_options}");
    assert!(!advertised.contains("gpt-5.4"), "{model_options}");
    assert!(!advertised.contains("unverified-model"), "{model_options}");
    process.send(json!({"jsonrpc":"2.0","id":7,"method":"session/set_config_option","params":{"sessionId":session,"configId":"model","value":"gpt-5.4"}}));
    assert!(process.response(7).get("error").is_some());
    for (id, control, value) in [
        (8, "provider", "lmstudio"),
        (9, "provider", "openai"),
        (10, "model", model),
        (11, "thought_level", effort),
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
