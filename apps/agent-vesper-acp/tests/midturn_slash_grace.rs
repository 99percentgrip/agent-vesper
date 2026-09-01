//! Mid-turn slash-command grace (CANCEL_GRACE regression suite).
//!
//! Editors interrupt a running turn by sending `session/cancel` immediately
//! followed by the new prompt — even when that prompt is an informational
//! slash command (`/status`, `/usage`, `/max-iterations`, …). The adapter
//! holds engine-session cancels for a short grace window: a safe slash
//! prompt arriving inside the window aborts the cancel (the turn keeps
//! working and the slash answers concurrently), while any other prompt or
//! grace expiry still performs the cancel.
//!
//! These tests drive the REAL release composition binary over stdio with a
//! slow loopback GLM server so the turn is genuinely in flight when the
//! cancel+slash pair arrives.

// Shared process-test helpers: this binary uses only a subset, so unused
// helpers are allowed rather than deleted (other binaries use them).
#[allow(dead_code)]
mod support;

use std::{io::Write, net::TcpListener, thread, time::Duration};

use serde_json::json;
use support::ProcessHarness;

/// Serves completions in arrival order: one per `(delay, answer)` pair.
/// Each connection is served on its own thread so a cancelled turn's dead
/// socket (broken pipe on write) can never kill the remaining script.
fn serve_completions(
    listener: TcpListener,
    script: Vec<(Duration, &'static str)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for (delay, answer) in script {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => return,
            };
            thread::spawn(move || {
                let _ = support::read_http_request(&mut stream);
                thread::sleep(delay);
                // A cancelled client may already be gone: write tolerantly
                // (`write_sse` unwraps by contract for the other suites).
                let body = support::successful_body(answer);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: \
                     {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            });
        }
    })
}

/// Serves exactly one completion, delaying the SSE body so the agent turn
/// stays in flight for the whole test window.
fn serve_one_slow_completion(
    listener: TcpListener,
    delay: Duration,
    answer: &'static str,
) -> thread::JoinHandle<()> {
    serve_completions(listener, vec![(delay, answer)])
}

fn cancel(process: &mut ProcessHarness, session: &str) {
    process.send(json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {"sessionId": session}
    }));
}

/// The editor interrupt pattern (cancel + safe slash) must NOT stop the
/// running turn: the slash answers immediately and the turn still completes
/// with `end_turn`.
#[test]
fn safe_slash_during_editor_interrupt_keeps_the_turn_running() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let _server = serve_one_slow_completion(
        listener,
        Duration::from_millis(1500),
        "slow-answer-complete",
    );

    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [("AGENT_VESPER_FULL_HARNESS", "1".to_owned())],
    );
    let session = process.initialize_and_new_session();

    // Turn in flight (provider sleeps 1.5s).
    process.prompt(10, &session, "slow work", "slow-message");
    thread::sleep(Duration::from_millis(400));

    // Editor interrupt pattern: cancel + informational slash back-to-back.
    cancel(&mut process, &session);
    process.prompt(11, &session, "/status", "status-message");

    // The slash answers quickly, without waiting for the turn.
    let status = process.response(11);
    assert_eq!(
        status["result"]["stopReason"], "end_turn",
        "the mid-turn slash must answer as its own turn: {status}"
    );

    // The interrupted turn SURVIVES the cancel and completes normally.
    let turn = process.response(10);
    assert_eq!(
        turn["result"]["stopReason"], "end_turn",
        "a safe slash must not cancel the running turn: {turn}"
    );
    let texts = support::update_texts(process.transcript(), "agent_message_chunk");
    assert!(
        texts.iter().any(|text| text.contains("Model")),
        "the /status answer must be inserted into the session context: {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("slow-answer-complete")),
        "the running turn's content must still stream: {texts:?}"
    );
}

/// `/usage` has its own provider/quota path, so cover it directly rather
/// than relying on the generic `/status` classifier regression.
#[test]
fn usage_during_editor_interrupt_keeps_the_turn_running() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let _server = serve_one_slow_completion(
        listener,
        Duration::from_millis(1500),
        "usage-survivor-complete",
    );
    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [("AGENT_VESPER_FULL_HARNESS", "1".to_owned())],
    );
    let session = process.initialize_and_new_session();
    process.prompt(
        40,
        &session,
        "slow quota-adjacent work",
        "slow-usage-message",
    );
    thread::sleep(Duration::from_millis(400));
    cancel(&mut process, &session);
    process.prompt(41, &session, "/usage", "usage-message");

    let usage = process.response(41);
    assert_eq!(usage["result"]["stopReason"], "end_turn", "{usage}");
    let turn = process.response(40);
    assert_eq!(turn["result"]["stopReason"], "end_turn", "{turn}");
    let texts = support::update_texts(process.transcript(), "agent_message_chunk");
    assert!(
        texts.iter().any(|text| text.contains("usage:")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("usage-survivor-complete")),
        "{texts:?}"
    );
}

/// Next-turn iteration controls are concurrent-safe too. Disabling the user
/// cap must answer immediately without cancelling the already-running turn.
#[test]
fn max_iterations_disable_during_work_keeps_the_turn_running() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let _server = serve_one_slow_completion(
        listener,
        Duration::from_millis(1500),
        "iteration-control-survivor",
    );
    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [("AGENT_VESPER_FULL_HARNESS", "1".to_owned())],
    );
    let session = process.initialize_and_new_session();
    process.prompt(50, &session, "slow implementation", "slow-control-message");
    thread::sleep(Duration::from_millis(400));
    cancel(&mut process, &session);
    process.prompt(
        51,
        &session,
        "/max-iterations disable",
        "max-iterations-message",
    );

    let control = process.response(51);
    assert_eq!(control["result"]["stopReason"], "end_turn", "{control}");
    let turn = process.response(50);
    assert_eq!(turn["result"]["stopReason"], "end_turn", "{turn}");
    let texts = support::update_texts(process.transcript(), "agent_message_chunk");
    assert!(
        texts.iter().any(|text| text.contains("cap disabled")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("iteration-control-survivor")),
        "{texts:?}"
    );
}

/// A genuine stop (cancel with NO follow-up prompt) must still cancel once
/// the grace window expires — the grace only exists for the slash case.
#[test]
fn cancel_without_follow_up_prompt_still_cancels_after_grace() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let _server =
        serve_one_slow_completion(listener, Duration::from_millis(2500), "should-not-complete");

    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [("AGENT_VESPER_FULL_HARNESS", "1".to_owned())],
    );
    let session = process.initialize_and_new_session();

    process.prompt(20, &session, "cancel me for real", "cancel-message");
    thread::sleep(Duration::from_millis(400));
    cancel(&mut process, &session);

    let turn = process.response(20);
    assert_eq!(
        turn["result"]["stopReason"], "cancelled",
        "a cancel without a safe-slash follow-up must still stop the turn: {turn}"
    );
}

/// A NON-slash prompt during the grace window is a genuine interrupt: the
/// cancel executes before the new prompt dispatches.
#[test]
fn non_slash_prompt_during_grace_executes_the_cancel() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // Turn 30 is slow (stays in flight); the replacement turn 31 gets an
    // immediate answer from the second scripted completion.
    let _server = serve_completions(
        listener,
        vec![
            (Duration::from_millis(2500), "should-not-complete"),
            (Duration::from_millis(50), "replacement-answer"),
        ],
    );

    let mut process = ProcessHarness::spawn_with_environment(
        address,
        [("AGENT_VESPER_FULL_HARNESS", "1".to_owned())],
    );
    let session = process.initialize_and_new_session();

    process.prompt(30, &session, "turn to interrupt", "interrupted-message");
    thread::sleep(Duration::from_millis(400));
    cancel(&mut process, &session);
    // A real message (not a safe slash) follows the cancel: interrupt.
    process.prompt(
        31,
        &session,
        "actually do this instead",
        "replacement-message",
    );

    let interrupted = process.response(30);
    assert_eq!(
        interrupted["result"]["stopReason"], "cancelled",
        "a non-slash prompt must execute the pending cancel: {interrupted}"
    );
    let replacement = process.response(31);
    assert_eq!(
        replacement["result"]["stopReason"], "end_turn",
        "the replacement prompt still runs: {replacement}"
    );
}
