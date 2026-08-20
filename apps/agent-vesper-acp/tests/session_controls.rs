//! Real-process coverage for the ACP footer control surface.
//!
//! Zed renders the chat-footer selectors (model picker, reasoning dial, API
//! plan, token-counter context) from the `configOptions` advertised on
//! `session/new`/`load`/`resume`/`set_config_option`. These tests boot the
//! real binary and prove the provider-routed controls are advertised,
//! validated, and applied — no provider HTTP is needed because session
//! creation and config-option handling never dispatch a prompt turn.

#![allow(dead_code)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

mod support;

use support::critical_environment_keys;

const CANARY: &str = "vesper-controls-secret-canary";

fn option_ids(response: &Value) -> Vec<String> {
    response["result"]["configOptions"]
        .as_array()
        .expect("configOptions array")
        .iter()
        .map(|option| option["id"].as_str().expect("option id").to_owned())
        .collect()
}

fn option(response: &Value, id: &str) -> Value {
    response["result"]["configOptions"]
        .as_array()
        .expect("configOptions array")
        .iter()
        .find(|option| option["id"] == id)
        .unwrap_or_else(|| panic!("missing config option {id}"))
        .clone()
}

fn option_values(response: &Value, id: &str) -> Vec<String> {
    option(response, id)["options"]
        .as_array()
        .expect("options array")
        .iter()
        .map(|choice| {
            choice["value"]
                .as_str()
                .or_else(|| choice["id"].as_str())
                .expect("choice value")
                .to_owned()
        })
        .collect()
}

#[test]
fn footer_controls_are_advertised_and_settable_end_to_end() {
    let temp = std::env::temp_dir().join(format!(
        "agent-vesper-controls-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("t")
    ));
    std::fs::create_dir_all(&temp).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"));
    command
        .env_clear()
        .env("HOME", &temp)
        .env("XDG_CONFIG_HOME", temp.join("config"))
        .env("XDG_CACHE_HOME", temp.join("cache"))
        .env("XDG_DATA_HOME", temp.join("data"))
        .env("XDG_STATE_HOME", temp.join("state"))
        .env("ZAI_API_KEY", CANARY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in critical_environment_keys() {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (line_sender, line_receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            line_sender.send(line.unwrap()).unwrap();
        }
    });

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}),
    );
    let initialize = response_for(&line_receiver, 1, &mut Vec::new());
    assert_eq!(initialize["result"]["protocolVersion"], 1);

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}),
    );
    let session_response = response_for(&line_receiver, 2, &mut Vec::new());
    let session = session_response["result"]["sessionId"].as_str().unwrap();

    // The full footer surface is advertised on session/new: model picker
    // first (oracle parity), then reasoning, plan, generation style,
    // auxiliary model, MoA, and permissions.
    assert_eq!(
        option_ids(&session_response),
        [
            "model",
            "thought_level",
            "api_endpoint",
            "generation_profile",
            "auxiliary_model",
            "mixture_mode",
            "permission_mode",
        ]
    );
    let model = option(&session_response, "model");
    assert_eq!(model["category"], "model");
    assert_eq!(model["currentValue"], "glm-5.3");
    assert_eq!(
        option_values(&session_response, "model"),
        ["__moa__", "glm-5.3", "glm-5.2", "glm-5-turbo", "glm-4.7"],
        "coding plan excludes vision models and leads with the MoA picker"
    );
    let thought = option(&session_response, "thought_level");
    assert_eq!(thought["category"], "thought_level");
    assert_eq!(
        option_values(&session_response, "thought_level"),
        ["disabled", "enabled", "high", "max"],
        "glm-5.3 is a deep-reasoning model: all four levels are offered"
    );
    assert_eq!(
        option_values(&session_response, "api_endpoint"),
        ["coding", "standard", "bigmodel"]
    );
    assert_eq!(
        option_values(&session_response, "permission_mode"),
        ["ask", "bypass", "read"]
    );

    // Footer model pick: set glm-5.2 and see the advertised current value
    // change on the response (and no error).
    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0","id":3,"method":"session/set_config_option",
            "params":{"sessionId":session,"configId":"model","value":"glm-5.2"}
        }),
    );
    let model_set = response_for(&line_receiver, 3, &mut Vec::new());
    assert!(
        model_set.get("error").is_none(),
        "model selection failed: {model_set}"
    );
    assert_eq!(option(&model_set, "model")["currentValue"], "glm-5.2");

    // Reasoning dial: set max through the built-in reasoning override.
    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0","id":4,"method":"session/set_config_option",
            "params":{"sessionId":session,"configId":"thought_level","value":"max"}
        }),
    );
    let thought_set = response_for(&line_receiver, 4, &mut Vec::new());
    assert!(
        thought_set.get("error").is_none(),
        "thought level failed: {thought_set}"
    );
    assert_eq!(option(&thought_set, "thought_level")["currentValue"], "max");

    // Permission selector round-trips the advertised oracle value `read`.
    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0","id":5,"method":"session/set_config_option",
            "params":{"sessionId":session,"configId":"permission_mode","value":"read"}
        }),
    );
    let permission_set = response_for(&line_receiver, 5, &mut Vec::new());
    assert!(
        permission_set.get("error").is_none(),
        "permission selection failed: {permission_set}"
    );
    assert_eq!(
        option(&permission_set, "permission_mode")["currentValue"],
        "read"
    );

    // Unadvertised model ids are rejected fail-closed.
    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0","id":6,"method":"session/set_config_option",
            "params":{"sessionId":session,"configId":"model","value":"gpt-9"}
        }),
    );
    let rejected = response_for(&line_receiver, 6, &mut Vec::new());
    assert!(
        rejected.get("error").is_some(),
        "invented model must be rejected: {rejected}"
    );
    assert_eq!(
        rejected["error"]["data"]["reason"],
        "unsupported-session-config-value"
    );

    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "ACP process did not exit on EOF");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    reader.join().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(!stderr.contains(CANARY));
}

fn send(stdin: &mut impl Write, value: Value) {
    serde_json::to_writer(&mut *stdin, &value).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
}

fn response_for(receiver: &mpsc::Receiver<String>, id: u64, transcript: &mut Vec<Value>) -> Value {
    loop {
        let line = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!line.contains(CANARY), "secret reached ACP stdout");
        let value: Value = serde_json::from_str(&line).expect("stdout contained non-JSON text");
        transcript.push(value.clone());
        if value["id"] == id {
            return value;
        }
    }
}
