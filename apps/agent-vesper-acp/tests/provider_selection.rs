//! Stage 9 network-free provider selection proof.
//!
//! Boots the release composition with `AGENT_VESPER_PROVIDER=synthetic` and
//! drives a complete ACP prompt-completion lifecycle through the in-process
//! synthetic provider with no network I/O, no GLM credential, and no fake HTTP
//! server. The deterministic reply must flow from the synthetic session,
//! through the runtime, and out of the ACP adapter. A successful `end_turn`
//! with the synthetic reply is impossible through the GLM adapter (no endpoint
//! is configured), so it also proves the selected provider was actually wired.

// Integration tests pull the shared process harness module, whose helpers are
// consumed by different test binaries; only `critical_environment_keys` is
// needed here, so the rest of `support` is permitted to be dead code in this
// test binary.
#![allow(dead_code)]
// The proof boots the release binary with `AGENT_VESPER_PROVIDER=synthetic`,
// which the composition accepts only under the `integration-test-harness`
// feature (the synthetic adapter must never be selectable in production
// builds). Without the feature the child process exits immediately, so the
// whole file compiles out; the canonical `--all-features` verification runs
// it unchanged.
#![cfg(feature = "integration-test-harness")]

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

mod support;

use support::critical_environment_keys;

const TIMEOUT: Duration = Duration::from_secs(5);

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn synthetic_provider_serves_an_acp_prompt_lifecycle_without_network_io() {
    let temporary_root = tempfile::tempdir().unwrap();
    let temp = temporary_root.path().to_path_buf();

    // HOME alone cannot isolate an OS credential manager. Keep every registered
    // native provider explicitly signed out and workspace discovery private.
    let credentials = temp.join("signed-out.json");
    std::fs::write(
        &credentials,
        serde_json::to_vec(&json!({"credentials": {
            "openai": {"native-auth": "{\"mode\":\"signed-out\"}"},
            "xai": {"native-auth": "{\"mode\":\"signed-out\"}"}
        }}))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&credentials, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    // No ZAI_API_KEY and no AGENT_VESPER_GLM_BASE_URL: synthetic mode must not
    // require GLM credentials or any network endpoint.
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"));
    command
        .current_dir(&temp)
        .env_clear()
        .env("AGENT_VESPER_OPENAI_CREDENTIALS_PATH", &credentials)
        .env("AGENT_VESPER_XAI_CREDENTIALS_PATH", &credentials)
        .env("HOME", &temp)
        .env("XDG_CONFIG_HOME", temp.join("config"))
        .env("XDG_CACHE_HOME", temp.join("cache"))
        .env("XDG_DATA_HOME", temp.join("data"))
        .env("XDG_STATE_HOME", temp.join("state"))
        .env("AGENT_VESPER_PROVIDER", "synthetic")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in critical_environment_keys() {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    let mut child = OwnedChild(command.spawn().unwrap());
    let mut stdin = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let stderr = child.0.stderr.take().unwrap();
    let errors = thread::spawn(move || {
        let mut output = String::new();
        BufReader::new(stderr).read_to_string(&mut output).unwrap();
        output
    });
    let (line_sender, line_receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let _ = line_sender.send(line.unwrap());
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
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":temp,"mcpServers":[]}}),
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
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "ACP process did not exit on EOF");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "ACP process exited with {status}");
    reader.join().unwrap();
    let stderr = errors.join().unwrap();
    assert!(
        !stderr.contains(support::CANARY),
        "secret reached ACP stderr"
    );
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
        let line = receiver.recv_timeout(TIMEOUT).unwrap_or_else(|error| {
            panic!("ACP response {id} did not arrive: {error}; transcript={transcript:?}")
        });
        let value: Value = serde_json::from_str(&line).expect("stdout contained non-JSON text");
        transcript.push(value.clone());
        if value["id"] == id {
            return value;
        }
    }
}
