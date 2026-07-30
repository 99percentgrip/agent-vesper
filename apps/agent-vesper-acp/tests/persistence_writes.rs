//! Stage 7 end-to-end write wiring: transactional persistence through the ACP
//! lifecycle and graceful write-failure handling.
//!
//! These tests spawn the real release binary with session writes enabled and
//! exercise complete round-trips (new session -> prompt -> transactional save
//! -> separate process resumes -> state fidelity) plus a bounded write failure
//! that must surface a sanitized, stable ACP error reason without crashing the
//! dispatcher.

// Integration tests pull the shared process harness module, whose helpers are
// consumed by different test binaries; only `ProcessHarness` is needed here.
#![allow(dead_code)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use serde_json::{Value, json};

mod support;

use support::ProcessHarness;

const CANARY: &str = "vesper-stage7-secret-canary";
const READ_ENABLE: &str = "AGENT_VESPER_ENABLE_SESSION_READS";
const VESPER_READ_ENABLE: &str = "AGENT_VESPER_ENABLE_VESPER_SESSION_READS";
const READ_ROOT: &str = "AGENT_VESPER_SESSION_ROOT";
const WRITE_ENABLE: &str = "AGENT_VESPER_ENABLE_SESSION_WRITES";
const WRITE_ROOT: &str = "AGENT_VESPER_SESSION_WRITE_ROOT";
const WRITE_MAX_BYTES: &str = "AGENT_VESPER_SESSION_WRITE_MAX_BYTES";

/// A deterministic single-shot SSE completion that the GLM adapter accepts as a
/// finished turn. The assistant content (`answer`) is the fidelity marker the
/// round-trip test looks for after resume.
const PROMPT_SSE_BODY: &str = concat!(
    "data: {\"id\":\"fixture-response\",\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}],",
    "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n",
    "data: [DONE]\n\n"
);

fn unique_root(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "vesper-stage7-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

/// Spawns a loopback GLM completion server that serves exactly one prompt turn.
fn serve_one_prompt_completion(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut sink = [0u8; 4096];
        let _ = stream.read(&mut sink);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            PROMPT_SSE_BODY.len(),
            PROMPT_SSE_BODY
        )
        .unwrap();
        stream.flush().unwrap();
    })
}

fn send_request(harness: &mut ProcessHarness, id: u64, method: &str, params: Value) {
    harness.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
}

fn initialize(harness: &mut ProcessHarness) {
    send_request(harness, 1, "initialize", json!({"protocolVersion":1}));
    let response = harness.response(1);
    assert_eq!(response["result"]["protocolVersion"], 1);
}

fn authenticate(harness: &mut ProcessHarness, id: u64) {
    send_request(
        harness,
        id,
        "authenticate",
        json!({"methodId":"zai-api-key-setup"}),
    );
    let response = harness.response(id);
    assert!(
        response.get("error").is_none(),
        "authenticate failed: {response}"
    );
}

fn new_session(harness: &mut ProcessHarness, id: u64) -> String {
    send_request(
        harness,
        id,
        "session/new",
        json!({"cwd":"/tmp","mcpServers":[]}),
    );
    let response = harness.response(id);
    response["result"]["sessionId"]
        .as_str()
        .expect("session id present")
        .to_owned()
}

fn prompt(harness: &mut ProcessHarness, id: u64, session: &str, text: &str, message_id: &str) {
    harness.prompt(id, session, text, message_id);
}

/// Round-trip: process one persists a prompted session transactionally, then a
/// fresh process two resumes it and the persisted history is replayed verbatim.
#[test]
fn prompt_persists_transactionally_and_resumes_across_processes() {
    let vesper_root = unique_root("writes");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = serve_one_prompt_completion(listener);

    let mut first = ProcessHarness::spawn_with_environment(
        address,
        [
            (READ_ENABLE, "1".into()),
            (VESPER_READ_ENABLE, "1".into()),
            (READ_ROOT, vesper_root.to_string_lossy().into_owned()),
            (WRITE_ENABLE, "1".into()),
            (WRITE_ROOT, vesper_root.to_string_lossy().into_owned()),
        ],
    );

    initialize(&mut first);
    authenticate(&mut first, 2);
    let session = new_session(&mut first, 3);
    prompt(&mut first, 4, &session, "hello", "client-message-4");
    let prompt_response = first.response(4);
    assert!(
        prompt_response.get("error").is_none(),
        "prompt should succeed with persistence: {prompt_response}"
    );
    assert_eq!(prompt_response["result"]["stopReason"], "end_turn");

    let record_path = vesper_root.join(format!("{session}.json"));
    let sidecar_path = vesper_root.join(format!("{session}.meta"));
    assert!(
        record_path.exists(),
        "transactional write must commit the session record"
    );
    assert!(
        sidecar_path.exists(),
        "transactional write must commit the derived sidecar"
    );
    let record_bytes = fs::read(&record_path).unwrap();
    let record: Value = serde_json::from_slice(&record_bytes).unwrap();
    assert_eq!(record["session_id"], session);
    assert_eq!(record["format"], "agent-vesper-session");
    assert_eq!(record["version"], 1);

    let (transcript, stderr) = first.finish_and_capture();
    assert!(!stderr.contains(CANARY), "secret reached stderr");
    let wrote_chunks: Vec<&str> = transcript
        .iter()
        .filter_map(|value| value["params"]["update"]["sessionUpdate"].as_str())
        .collect();
    assert!(
        wrote_chunks.contains(&"agent_message_chunk"),
        "prompt should have streamed an assistant message"
    );
    server.join().unwrap();

    // Process two: fresh process, reads only, resumes the persisted session.
    let resume_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let resume_address = resume_listener.local_addr().unwrap();
    let mut second = ProcessHarness::spawn_with_environment(
        resume_address,
        [
            (READ_ENABLE, "1".into()),
            (VESPER_READ_ENABLE, "1".into()),
            (READ_ROOT, vesper_root.to_string_lossy().into_owned()),
        ],
    );

    initialize(&mut second);
    send_request(&mut second, 2, "session/list", json!({}));
    let listed = second.response(2);
    assert_eq!(listed["result"]["sessions"][0]["sessionId"], session);

    send_request(
        &mut second,
        3,
        "session/resume",
        json!({"sessionId":session,"cwd":"/tmp","mcpServers":[]}),
    );
    let resumed = second.response(3);
    assert!(
        resumed.get("error").is_none(),
        "resume of a persisted session must succeed: {resumed}"
    );

    let replayed: Vec<&str> = second
        .transcript()
        .iter()
        .filter_map(|value| value["params"]["update"]["sessionUpdate"].as_str())
        .collect();
    assert!(
        replayed.contains(&"agent_message_chunk"),
        "resume must replay the persisted assistant message"
    );
    assert!(
        replayed.contains(&"user_message_chunk"),
        "resume must replay the persisted user message"
    );

    let (_, stderr2) = second.finish_and_capture();
    assert!(!stderr2.contains(CANARY), "secret reached stderr on resume");

    let _ = fs::remove_dir_all(&vesper_root);
}

/// A bounded write failure surfaces a sanitized, stable ACP error reason and
/// the dispatcher keeps serving subsequent requests without crashing.
#[test]
fn oversized_write_surfaces_stable_reason_without_crashing_dispatcher() {
    let vesper_root = unique_root("oversized");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = serve_one_prompt_completion(listener);

    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [
            (READ_ENABLE, "1".into()),
            (VESPER_READ_ENABLE, "1".into()),
            (READ_ROOT, vesper_root.to_string_lossy().into_owned()),
            (WRITE_ENABLE, "1".into()),
            (WRITE_ROOT, vesper_root.to_string_lossy().into_owned()),
            // Any real session serializes far above 8 bytes, so the write
            // fails the bound before touching the filesystem.
            (WRITE_MAX_BYTES, "8".into()),
        ],
    );

    initialize(&mut process);
    authenticate(&mut process, 2);
    let session = new_session(&mut process, 3);
    prompt(&mut process, 4, &session, "hello", "client-message-4");
    let prompt_response = process.response(4);
    let error = prompt_response
        .get("error")
        .expect("oversized write must surface a sanitized error");
    assert_eq!(error["code"], -32602, "must map to invalid_params");
    assert_eq!(
        error["data"]["reason"], "persistent-session-rejected-by-bounds",
        "write failure must carry a stable sanitized reason: {error}"
    );

    // No record or temp should have reached the canonical root.
    let entries: Vec<_> = fs::read_dir(&vesper_root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        entries.iter().all(|name| !name.ends_with(".json")),
        "failed write must leave no session record: {entries:?}"
    );

    // The dispatcher must survive: a subsequent list returns the live session.
    send_request(&mut process, 5, "session/list", json!({}));
    let listed = process.response(5);
    assert!(
        listed.get("error").is_none(),
        "dispatcher must keep serving after a write failure: {listed}"
    );
    assert_eq!(listed["result"]["sessions"][0]["sessionId"], session);

    let (_, stderr) = process.finish_and_capture();
    assert!(
        !stderr.contains(CANARY),
        "secret reached stderr on write failure"
    );

    server.join().unwrap();
    let _ = fs::remove_dir_all(&vesper_root);
}
