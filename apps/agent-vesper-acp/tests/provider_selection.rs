//! Stage 9 network-free provider selection proof.
//!
//! Boots the release composition with `AGENT_VESPER_PROVIDER=synthetic` and
//! drives a complete ACP prompt-completion lifecycle through the in-process
//! synthetic provider with no network I/O, no GLM credential, and no fake HTTP
//! server. The deterministic reply must flow from the synthetic session,
//! through the runtime, and out of the ACP adapter. A successful `end_turn`
//! with the synthetic reply is impossible through the GLM adapter (no endpoint
//! is configured), so it also proves the selected provider was actually wired.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn synthetic_provider_serves_an_acp_prompt_lifecycle_without_network_io() {
    let temp = std::env::temp_dir().join(format!(
        "agent-vesper-stage9-synthetic-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();

    // No ZAI_API_KEY and no AGENT_VESPER_GLM_BASE_URL: synthetic mode must not
    // require GLM credentials or any network endpoint.
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"))
        .env_clear()
        .env("HOME", &temp)
        .env("XDG_CONFIG_HOME", temp.join("config"))
        .env("XDG_CACHE_HOME", temp.join("cache"))
        .env("XDG_DATA_HOME", temp.join("data"))
        .env("XDG_STATE_HOME", temp.join("state"))
        .env("AGENT_VESPER_PROVIDER", "synthetic")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
    let session = response_for(&line_receiver, 2, &mut Vec::new())["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"session/prompt",
            "params":{
                "sessionId":session,
                "prompt":[{"type":"text","text":"any input"}],
                "_meta":{"userMessageId":"synthetic-message-3"}
            }
        }),
    );
    let mut transcript = Vec::new();
    let prompt = response_for(&line_receiver, 3, &mut transcript);
    assert!(prompt.get("error").is_none(), "prompt errored: {prompt:?}");
    assert_eq!(prompt["result"]["userMessageId"], "synthetic-message-3");
    assert_eq!(prompt["result"]["stopReason"], "end_turn");

    // The deterministic synthetic reply must reach the ACP update stream as an
    // agent_message_chunk whose content carries the configured reply text.
    let reply = transcript
        .iter()
        .filter_map(|value| value["params"]["update"]["content"]["text"].as_str())
        .collect::<String>();
    assert!(
        transcript
            .iter()
            .any(|value| value["params"]["update"]["sessionUpdate"] == "agent_message_chunk"),
        "synthetic content did not surface as an agent_message_chunk: {transcript:?}"
    );
    assert!(
        reply.contains("synthetic-ok"),
        "synthetic reply did not reach stdout: got {reply:?}"
    );

    drop(stdin);
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "ACP process did not exit on EOF");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "ACP process exited with {status}");
    reader.join().unwrap();
    // Every stdout line must be ACP JSON-RPC; nothing else may contaminate it.
    assert!(
        transcript
            .iter()
            .all(|value| value.get("jsonrpc") == Some(&Value::String("2.0".into()))),
        "non-JSON-RPC line reached stdout"
    );
}

fn send(stdin: &mut impl Write, value: Value) {
    serde_json::to_writer(&mut *stdin, &value).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
}

fn response_for(receiver: &mpsc::Receiver<String>, id: u64, transcript: &mut Vec<Value>) -> Value {
    loop {
        let line = receiver.recv_timeout(TIMEOUT).unwrap();
        let value: Value = serde_json::from_str(&line).expect("stdout contained non-JSON text");
        transcript.push(value.clone());
        if value["id"] == id {
            return value;
        }
    }
}
