// Integration tests pull the shared process harness module, whose helpers are
// consumed by different test binaries; only `critical_environment_keys` is
// needed here, so the rest of `support` is permitted to be dead code in this
// test binary.
#![allow(dead_code)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

mod support;

use support::critical_environment_keys;

const CANARY: &str = "vesper-stage4-secret-canary";

#[test]
fn stdio_transcript_reaches_real_glm_adapter_with_protocol_pure_stdout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        assert!(request.starts_with("POST /v4/chat/completions HTTP/1.1"));
        assert!(request.contains("authorization: Bearer vesper-stage4-secret-canary"));
        assert!(request.contains("\"content\":\"hello\""));
        let body = concat!(
            "data: {\"id\":\"fixture-response\",\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}],",
            "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        stream.flush().unwrap();

        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        assert!(request.contains("\"content\":\"return a tool call\""));
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",",
            "\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"x.txt\\\"}\"}}]},",
            "\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        stream.flush().unwrap();
    });

    let temp = std::env::temp_dir().join(format!("agent-vesper-stage4-{}", std::process::id()));
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
        .env("AGENT_VESPER_GLM_BASE_URL", format!("http://{address}/v4"))
        .env("AGENT_VESPER_ALLOW_INSECURE_LOOPBACK", "1")
        .env("AGENT_VESPER_FULL_HARNESS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Inherit platform-critical env vars (Windows Winsock/DLL search paths,
    // macOS temp/subprocess resolution, Linux PATH). ZAI_API_KEY and
    // AGENT_VESPER_* are set explicitly above, so secret isolation is intact.
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
    assert_eq!(
        initialize["result"]["agentCapabilities"]["promptCapabilities"]["audio"],
        false
    );

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"authenticate","params":{"methodId":"zai-api-key-setup"}}),
    );
    let auth = response_for(&line_receiver, 2, &mut Vec::new());
    assert!(auth.get("error").is_none());

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":3,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}),
    );
    let session_response = response_for(&line_receiver, 3, &mut Vec::new());
    let session = session_response["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    // The catalog advertisement follows the session/new response; consume it
    // here so the later prompt-turn transcript assertion sees only turn
    // updates (clients drop pre-response advertisements, so this ordering is
    // the shipped contract).
    let advertisement = loop {
        let line = line_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("catalog advertisement after session/new response");
        assert!(!line.contains(CANARY), "secret reached ACP stdout");
        let value: Value = serde_json::from_str(&line).expect("stdout contained non-JSON text");
        if value["params"]["update"]["sessionUpdate"] == "available_commands_update" {
            break value;
        }
    };
    assert_eq!(
        advertisement["params"]["update"]["availableCommands"]
            .as_array()
            .map(|commands| commands.len()),
        Some(44),
        "the post-response advertisement carries frozen plus host-parity commands \
         (28 frozen + 14 host-parity, including /sandbox)"
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"session/prompt",
            "params":{
                "sessionId":session,
                "prompt":[{"type":"text","text":"hello"}],
                "_meta":{"userMessageId":"client-message-4"}
            }
        }),
    );
    let mut transcript = Vec::new();
    let prompt = response_for(&line_receiver, 4, &mut transcript);
    assert_eq!(prompt["result"]["userMessageId"], "client-message-4");
    assert_eq!(prompt["result"]["stopReason"], "end_turn");
    let update_kinds: Vec<_> = transcript
        .iter()
        .filter_map(|value| {
            value["params"]["update"]["sessionUpdate"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(
        update_kinds,
        ["agent_thought_chunk", "agent_message_chunk", "usage_update"]
    );

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":5,"method":"session/list","params":{}}),
    );
    let listed = response_for(&line_receiver, 5, &mut transcript);
    assert_eq!(listed["result"]["sessions"][0]["sessionId"], session);
    assert_eq!(listed["result"]["sessions"][0]["cwd"], "/tmp");

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":6,"method":"session/load","params":{"sessionId":session,"cwd":"/tmp","mcpServers":[]}}),
    );
    let loaded = response_for(&line_receiver, 6, &mut transcript);
    assert!(loaded.get("error").is_none());
    let replay: Vec<_> = transcript
        .iter()
        .rev()
        .take_while(|value| value["id"] != 5)
        .filter_map(|value| {
            value["params"]["update"]["sessionUpdate"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    assert!(replay.contains(&"user_message_chunk".to_owned()));
    assert!(replay.contains(&"agent_message_chunk".to_owned()));

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":7,"method":"session/resume","params":{"sessionId":session,"cwd":"/tmp","mcpServers":[]}}),
    );
    assert!(
        response_for(&line_receiver, 7, &mut transcript)
            .get("error")
            .is_none()
    );

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":8,"method":"session/fork","params":{"sessionId":session,"cwd":"/tmp"}}),
    );
    let forked = response_for(&line_receiver, 8, &mut transcript);
    let child_session = forked["result"]["sessionId"].as_str().unwrap();
    assert_ne!(child_session, session);

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":9,"method":"session/close","params":{"sessionId":child_session}}),
    );
    assert!(
        response_for(&line_receiver, 9, &mut transcript)
            .get("error")
            .is_none()
    );

    send(
        &mut stdin,
        json!({
            "jsonrpc":"2.0",
            "id":10,
            "method":"session/prompt",
            "params":{
                "sessionId":session,
                "prompt":[{"type":"text","text":"return a tool call"}],
                "_meta":{"userMessageId":"tool-message-10"}
            }
        }),
    );
    let tool_prompt = response_for(&line_receiver, 10, &mut transcript);
    assert_eq!(tool_prompt["result"]["stopReason"], "end_turn");
    assert!(transcript.iter().any(|value| {
        value["params"]["update"]["sessionUpdate"] == "tool_call"
            && value["params"]["update"]["toolCallId"] == "call-1"
    }));
    assert!(transcript.iter().any(|value| {
        value["params"]["update"]["sessionUpdate"] == "tool_call_update"
            && value["params"]["update"]["toolCallId"] == "call-1"
            && value["params"]["update"]["status"] == "failed"
    }));

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
    server.join().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(!stderr.contains(CANARY));
    assert!(
        transcript
            .iter()
            .all(|value| value.get("jsonrpc") == Some(&Value::String("2.0".into())))
    );
}

#[test]
fn empty_prompt_and_slash_commands_never_dispatch_provider() {
    let temp = std::env::temp_dir().join(format!(
        "agent-vesper-stage4-no-dispatch-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"));
    command
        .env_clear()
        .env("HOME", &temp)
        .env("XDG_CONFIG_HOME", temp.join("config"))
        .env("XDG_CACHE_HOME", temp.join("cache"))
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
    let _ = response_for(&line_receiver, 1, &mut Vec::new());
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":11,"method":"authenticate","params":{"methodId":"unknown-method"}}),
    );
    assert!(
        response_for(&line_receiver, 11, &mut Vec::new())
            .get("error")
            .is_some()
    );
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":12,"method":"session/load","params":{"sessionId":"missing-session","cwd":"/tmp","mcpServers":[]}}),
    );
    assert!(
        response_for(&line_receiver, 12, &mut Vec::new())
            .get("error")
            .is_none(),
        "source-compatible missing loads create only an ephemeral session"
    );
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}),
    );
    let mut new_session_transcript = Vec::new();
    let session =
        response_for(&line_receiver, 2, &mut new_session_transcript)["result"]["sessionId"]
            .as_str()
            .unwrap()
            .to_owned();
    // Client-registration contract: no catalog advertisement may precede the
    // session/new response — ACP clients register the session only when the
    // response is processed and drop `session/update` notifications for
    // unregistered sessions (Zed: "unknown session"), which left the
    // slash-command menu permanently empty.
    let pre_response_ads = new_session_transcript
        .iter()
        .filter(|value| value["params"]["update"]["sessionUpdate"] == "available_commands_update")
        .count();
    assert_eq!(
        pre_response_ads, 0,
        "session/new must not advertise before the response; clients drop unknown-session updates"
    );
    let mut advertisements: Vec<Value> = Vec::new();
    for _ in 0..8 {
        let line = line_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("catalog advertisement after the session/new response");
        assert!(!line.contains(CANARY), "secret reached ACP stdout");
        let value: Value = serde_json::from_str(&line).expect("stdout contained non-JSON text");
        if value["params"]["update"]["sessionUpdate"] == "available_commands_update" {
            advertisements.push(value.clone());
            break;
        }
    }
    // The production composition appends 13 implemented host-parity commands
    // after the 28 frozen entries and advertises the combined catalog once.
    assert_eq!(
        advertisements.len(),
        1,
        "session/new must advertise the catalog exactly once, after the response"
    );
    let advertised: Vec<&str> = advertisements[0]["params"]["update"]["availableCommands"]
        .as_array()
        .expect("availableCommands is an array")
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect();
    // 28 frozen oracle entries + 16 host-parity extensions (VRO-13 PR-5:
    // /daemon + /watch). The catalog is advertised once, in order, first
    // help → last watch.
    assert_eq!(advertised.len(), 44);
    assert_eq!(advertised_first_and_last(&advertised), ("help", "watch"));
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":session,"prompt":[],"_meta":{"userMessageId":"empty-message"}}}),
    );
    let empty = response_for(&line_receiver, 3, &mut Vec::new());
    assert_eq!(empty["result"]["userMessageId"], "empty-message");
    assert_eq!(empty["result"]["stopReason"], "end_turn");

    // Catalog commands execute in the harness without any provider dispatch
    // (fixtures/acp/slash-command parity: /help answers with the oracle's
    // condensed help text as a normal end_turn response).
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{"sessionId":session,"prompt":[{"type":"text","text":"/help"}]}}),
    );
    let mut help_transcript = Vec::new();
    let help = response_for(&line_receiver, 4, &mut help_transcript);
    assert!(
        help.get("error").is_none(),
        "/help must answer normally, got {help}"
    );
    assert_eq!(help["result"]["stopReason"], "end_turn");
    let help_chunk = help_transcript
        .iter()
        .filter(|value| value["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
        .map(|value| {
            value["params"]["update"]["content"]["text"]
                .as_str()
                .unwrap_or("")
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(
        help_chunk.starts_with("⌨️ **Harness Commands**"),
        "/help must stream the oracle help text, got {help_chunk:?}"
    );

    // Unknown slash commands get the oracle's bounded unknown-command text —
    // never a provider dispatch and never a protocol error.
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":5,"method":"session/prompt","params":{"sessionId":session,"prompt":[{"type":"text","text":"/future-command"}]}}),
    );
    let mut unknown_transcript = Vec::new();
    let unknown = response_for(&line_receiver, 5, &mut unknown_transcript);
    assert!(
        unknown.get("error").is_none(),
        "unknown slash commands must answer normally, got {unknown}"
    );
    let unknown_chunk = unknown_transcript
        .iter()
        .filter(|value| value["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
        .map(|value| {
            value["params"]["update"]["content"]["text"]
                .as_str()
                .unwrap_or("")
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(
        unknown_chunk.starts_with("Unknown command: /future-command"),
        "unknown slash commands must stream the oracle fallback text, got {unknown_chunk:?}"
    );

    drop(stdin);
    wait_for_exit(&mut child);
    reader.join().unwrap();
}

#[test]
fn host_owned_slash_commands_reach_real_stores_with_tui_parity() {
    // End-to-end regression for the ACP/TUI parity gap: /skills must read
    // the cross-project global skill layer, /checkpoint + /sessions + /
    // lineage must hit the durable checkpoint stores, /curator must curate
    // instead of answering "unknown command", and /compact must operate on
    // the engine's own history — no provider dispatch anywhere.
    let tag = format!("agent-vesper-acp-parity-{}", std::process::id());
    let temp = std::env::temp_dir().join(&tag);
    let global_memory = temp.join("global-memory");
    let project_memory = temp.join("project-memory");
    let checkpoint_root = temp.join("checkpoints");
    let mcp_root = temp.join("mcp");
    let workspace = temp.join("workspace");
    std::fs::create_dir_all(global_memory.join("skills")).unwrap();
    std::fs::create_dir_all(&project_memory).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        global_memory
            .join("skills")
            .join("cross-project-release.md"),
        "---\ndescription: ship the release end to end\n---\nbody",
    )
    .unwrap();
    std::fs::write(workspace.join("tracked.txt"), "parity payload").unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"));
    command
        .env_clear()
        .env("HOME", &temp)
        .env("XDG_CONFIG_HOME", temp.join("config"))
        .env("XDG_CACHE_HOME", temp.join("cache"))
        .env("AGENT_VESPER_MEMORY_ROOT", &project_memory)
        .env("AGENT_VESPER_GLOBAL_MEMORY_ROOT", &global_memory)
        .env("AGENT_VESPER_CHECKPOINT_ROOT", &checkpoint_root)
        .env("AGENT_VESPER_MCP_ROOT", &mcp_root)
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
    let _ = response_for(&line_receiver, 1, &mut Vec::new());
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
            "cwd": workspace.to_string_lossy(), "mcpServers":[]
        }}),
    );
    let session = response_for(&line_receiver, 2, &mut Vec::new())["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    let mut run_command = |text: &str| -> String {
        static PROMPT_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(200);
        let id = PROMPT_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
                "sessionId": session,
                "prompt":[{"type":"text","text":text}]
            }}),
        );
        let mut transcript = Vec::new();
        let response = response_for(&line_receiver, id, &mut transcript);
        assert!(
            response.get("error").is_none(),
            "{text} must answer normally, got {response}"
        );
        transcript
            .iter()
            .filter(|value| value["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
            .map(|value| {
                value["params"]["update"]["content"]["text"]
                    .as_str()
                    .unwrap_or("")
            })
            .collect::<Vec<_>>()
            .join("")
    };

    // /skills reads the GLOBAL layer — the exact regression from the field
    // report ("used slash skills: No learned skills").
    let skills = run_command("/skills");
    assert!(
        skills.contains("**Learned skills**") && skills.contains("ship the release end to end"),
        "/skills must list global learned skills, got {skills:?}"
    );

    // /checkpoint snapshots the session workspace root through the durable
    // ledger; /sessions and /lineage then resolve the seeded record.
    let checkpoint = run_command("/checkpoint parity-label");
    assert!(
        checkpoint.starts_with("checkpoint: ") && checkpoint.contains("captured"),
        "got {checkpoint:?}"
    );
    assert!(checkpoint.contains("parity-label"));
    let sessions = run_command("/sessions");
    assert!(
        sessions.starts_with("sessions: 1 session(s)"),
        "got {sessions:?}"
    );
    let lineage = run_command("/lineage");
    assert!(lineage.starts_with("lineage: 1 hop(s)"), "got {lineage:?}");

    // /curator is a catalog command and must curate — never the
    // unknown-command fallback.
    let curator = run_command("/curator");
    assert!(curator.starts_with("curator: removed"), "got {curator:?}");

    // /compact operates on the engine's own conversation state.
    let compact = run_command("/compact");
    assert!(compact.starts_with("compact: "), "got {compact:?}");

    // /mcp and /plugins report the real (empty) registries.
    let mcp = run_command("/mcp");
    assert_eq!(mcp, "mcp: (no servers configured)");
    let plugins = run_command("/plugins");
    assert_eq!(plugins, "plugins: (no plugins loaded)");

    drop(stdin);
    wait_for_exit(&mut child);
    reader.join().unwrap();
    let _ = std::fs::remove_dir_all(&temp);
}

fn advertised_first_and_last<'a>(advertised: &[&'a str]) -> (&'a str, &'a str) {
    (
        advertised.first().copied().unwrap_or_default(),
        advertised.last().copied().unwrap_or_default(),
    )
}
#[test]
fn malformed_input_exits_without_stdout_contamination() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"));
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in critical_environment_keys() {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{not-json}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        let _: Value = serde_json::from_str(line).expect("malformed input polluted ACP stdout");
    }
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains(CANARY));
}

#[test]
fn cancellation_after_reasoning_emits_no_post_cancel_content() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _request = read_http_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
             data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":\"visible-before-cancel\"}}}}]}}\n\n"
        )
        .unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(300));
        let _ = write!(
            stream,
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"must-not-escape\"}},\"finish_reason\":\"stop\"}}]}}\n\n\
             data: [DONE]\n\n"
        );
        let _ = stream.flush();
    });

    let temp =
        std::env::temp_dir().join(format!("agent-vesper-stage4-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-acp"));
    command
        .env_clear()
        .env("HOME", &temp)
        .env("ZAI_API_KEY", CANARY)
        .env("AGENT_VESPER_GLM_BASE_URL", format!("http://{address}/v4"))
        .env("AGENT_VESPER_ALLOW_INSECURE_LOOPBACK", "1")
        .env("AGENT_VESPER_FULL_HARNESS", "0")
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
    let _ = response_for(&line_receiver, 1, &mut Vec::new());
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
        json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":session,"prompt":[{"type":"text","text":"cancel me"}],"_meta":{"userMessageId":"cancel-message"}}}),
    );
    let mut transcript = Vec::new();
    loop {
        let line = line_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!line.contains(CANARY));
        let value: Value = serde_json::from_str(&line).unwrap();
        let thought = value["params"]["update"]["sessionUpdate"] == "agent_thought_chunk";
        transcript.push(value);
        if thought {
            break;
        }
    }
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}),
    );
    let cancelled = response_for(&line_receiver, 3, &mut transcript);
    assert_eq!(cancelled["result"]["stopReason"], "cancelled");
    assert!(!transcript.iter().any(|value| {
        value["params"]["update"]["content"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("must-not-escape"))
    }));
    drop(stdin);
    wait_for_exit(&mut child);
    reader.join().unwrap();
    server.join().unwrap();
}

#[test]
fn session_new_with_client_mcp_servers_creates_session() {
    // Oracle parity: clients such as Zed attach every configured MCP server to
    // each `session/new`. The agent must accept and ignore them (the frozen
    // Python oracle never rejects a session over `mcp_servers`) instead of
    // answering `-32602 Invalid params`, which made the agent unloadable in
    // Zed for any user with MCP servers configured. No provider call happens
    // during session creation, so no loopback fixture is needed.
    let temp = std::env::temp_dir().join(format!("agent-vesper-mcp-parity-{}", std::process::id()));
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
        .env("AGENT_VESPER_FULL_HARNESS", "0")
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

    // Zed-style stdio server (untagged variant) plus a typed http server.
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
            "cwd":"/tmp",
            "mcpServers":[
                {"name":"github","command":"/usr/bin/npx","args":["-y","@github/mcp-server"],"env":[{"name":"GITHUB_TOKEN","value":"x"}]},
                {"type":"http","name":"docs","url":"https://mcp.example.test/sse"}
            ]
        }}),
    );
    let mut transcript = Vec::new();
    let session_response = response_for(&line_receiver, 2, &mut transcript);
    assert!(
        session_response.get("error").is_none(),
        "session/new must not reject client-declared MCP servers: {session_response}"
    );
    assert!(
        session_response["result"]["sessionId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "session/new must return a session id: {session_response}"
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

fn read_http_request(stream: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "HTTP request ended before headers");
        bytes.extend_from_slice(&buffer[..read]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap();
        if bytes.len() >= header_end + content_length {
            return String::from_utf8_lossy(&bytes[..header_end + content_length]).into_owned();
        }
    }
}

fn wait_for_exit(child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        assert!(Instant::now() < deadline, "ACP process did not exit on EOF");
        thread::sleep(Duration::from_millis(10));
    }
}
