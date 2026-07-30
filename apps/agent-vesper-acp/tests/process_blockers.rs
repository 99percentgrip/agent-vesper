#[path = "support/persistence_vectors.rs"]
mod persistence_vectors;
mod support;

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use serde_json::json;
use support::{
    ProcessHarness, read_http_request, successful_body, terminal_count, update_texts, write_sse,
};

#[test]
fn retry_before_visible_output_uses_two_requests_and_one_terminal() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let request = read_http_request(&mut first);
        assert!(request.contains("\"content\":\"retry-vector\""));
        observed.fetch_add(1, Ordering::SeqCst);
        write!(
            first,
            "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nretry-after: 0\r\nconnection: close\r\n\r\n"
        )
        .unwrap();
        first.flush().unwrap();

        let (mut second, _) = listener.accept().unwrap();
        let request = read_http_request(&mut second);
        assert!(request.contains("\"content\":\"retry-vector\""));
        observed.fetch_add(1, Ordering::SeqCst);
        write_sse(&mut second, &successful_body("retry-ok"));
    });

    let mut process = ProcessHarness::spawn(address);
    let session = process.initialize_and_new_session();
    process.prompt(10, &session, "retry-vector", "retry-message");
    let response = process.response(10);
    assert_eq!(response["result"]["userMessageId"], "retry-message");
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert_eq!(
        update_texts(process.transcript(), "agent_message_chunk"),
        ["retry-ok"]
    );
    assert_eq!(terminal_count(process.transcript(), 10), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    process.finish();
    server.join().unwrap();
}

#[test]
fn output_limit_continuation_is_one_acp_turn_with_cumulative_usage() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server_captured = Arc::clone(&captured);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        server_captured
            .lock()
            .unwrap()
            .push(read_http_request(&mut first));
        let first_body = concat!(
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"part\"},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        write_sse(&mut first, first_body);

        let (mut second, _) = listener.accept().unwrap();
        let request = read_http_request(&mut second);
        assert!(
            request.contains("Continue exactly where you left off. Do not repeat or summarize.")
        );
        server_captured.lock().unwrap().push(request);
        let second_body = concat!(
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":1,\"total_tokens\":5}}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"rest\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        write_sse(&mut second, second_body);
    });

    let mut process = ProcessHarness::spawn(address);
    let session = process.initialize_and_new_session();
    process.prompt(11, &session, "continuation-vector", "continuation-message");
    let response = process.response(11);
    assert_eq!(response["result"]["userMessageId"], "continuation-message");
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert_eq!(
        update_texts(process.transcript(), "agent_message_chunk").concat(),
        "part\nrest"
    );
    let usages = process
        .transcript()
        .iter()
        .filter(|value| value["params"]["update"]["sessionUpdate"] == "usage_update")
        .collect::<Vec<_>>();
    assert_eq!(usages.len(), 2);
    assert_eq!(usages[1]["params"]["update"]["used"], 10);
    assert_eq!(terminal_count(process.transcript(), 11), 1);
    assert_eq!(captured.lock().unwrap().len(), 2);
    process.finish();
    server.join().unwrap();
}

#[test]
fn post_output_interruption_does_not_replay_and_session_recovers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let _ = read_http_request(&mut first);
        observed.fetch_add(1, Ordering::SeqCst);
        let partial = "data: {\"choices\":[{\"delta\":{\"content\":\"partial-visible\"}}]}\n\n";
        write_sse(&mut first, partial);

        let (mut second, _) = listener.accept().unwrap();
        let _ = read_http_request(&mut second);
        observed.fetch_add(1, Ordering::SeqCst);
        write_sse(&mut second, &successful_body("recovered"));
    });

    let mut process = ProcessHarness::spawn(address);
    let session = process.initialize_and_new_session();
    process.prompt(12, &session, "interrupt-vector", "interrupt-message");
    let interrupted = process.response(12);
    assert_eq!(interrupted["result"]["stopReason"], "refusal");
    assert_eq!(
        update_texts(process.transcript(), "agent_message_chunk"),
        ["partial-visible"]
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);

    process.prompt(13, &session, "recovery-vector", "recovery-message");
    let recovered = process.response(13);
    assert_eq!(recovered["result"]["stopReason"], "end_turn");
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(terminal_count(process.transcript(), 12), 1);
    assert_eq!(terminal_count(process.transcript(), 13), 1);
    process.finish();
    server.join().unwrap();
}

#[test]
fn cancellation_before_dispatch_observes_zero_http_requests() {
    let provider = TcpListener::bind("127.0.0.1:0").unwrap();
    provider.set_nonblocking(true).unwrap();
    let provider_address = provider.local_addr().unwrap();
    let gate = TcpListener::bind("127.0.0.1:0").unwrap();
    let gate_address = gate.local_addr().unwrap();
    let (ready_sender, ready_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let gate_thread = thread::spawn(move || {
        let (mut first, _) = gate.accept().unwrap();
        let mut signal = [0_u8; 23];
        first.read_exact(&mut signal).unwrap();
        ready_sender.send(()).unwrap();
        release_receiver.recv().unwrap();
        first.write_all(b"c").unwrap();
        let (mut second, _) = gate.accept().unwrap();
        second.read_exact(&mut signal).unwrap();
        second.write_all(b"r").unwrap();
    });

    let mut process = ProcessHarness::spawn_test_driver(provider_address, gate_address);
    let session = process.initialize_and_new_session();
    process.prompt(
        14,
        &session,
        "cancel-before-dispatch",
        "cancel-before-message",
    );
    ready_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    process.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}));
    process.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}));
    release_sender.send(()).unwrap();
    let cancelled = process.response(14);
    assert_eq!(cancelled["result"]["stopReason"], "cancelled");
    assert!(
        provider.accept().is_err(),
        "provider dispatch occurred before cancellation"
    );
    assert!(update_texts(process.transcript(), "agent_message_chunk").is_empty());

    let provider_thread = thread::spawn(move || {
        provider.set_nonblocking(false).unwrap();
        let (mut stream, _) = provider.accept().unwrap();
        let _ = read_http_request(&mut stream);
        write_sse(&mut stream, &successful_body("after-cancel"));
    });
    process.prompt(15, &session, "after-cancel", "after-cancel-message");
    assert_eq!(process.response(15)["result"]["stopReason"], "end_turn");
    assert_eq!(terminal_count(process.transcript(), 14), 1);
    assert_eq!(terminal_count(process.transcript(), 15), 1);
    process.finish();
    gate_thread.join().unwrap();
    provider_thread.join().unwrap();
}

#[test]
fn separate_sessions_execute_provider_requests_concurrently() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let release_a = Arc::new(Barrier::new(2));
    let maximum_server = Arc::clone(&maximum);
    let active_server = Arc::clone(&active);
    let release_server = Arc::clone(&release_a);
    let server = thread::spawn(move || {
        let mut workers = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let current = active_server.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_server.fetch_max(current, Ordering::SeqCst);
            let release = Arc::clone(&release_server);
            let active = Arc::clone(&active_server);
            workers.push(thread::spawn(move || {
                let request = read_http_request(&mut stream);
                if request.contains("\"content\":\"session-a\"") {
                    release.wait();
                    write_sse(&mut stream, &successful_body("answer-a"));
                } else {
                    assert!(request.contains("\"content\":\"session-b\""));
                    write_sse(&mut stream, &successful_body("answer-b"));
                    release.wait();
                }
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
    });

    let mut process = ProcessHarness::spawn(address);
    let session_a = process.initialize_and_new_session();
    process.send(json!({"jsonrpc":"2.0","id":3,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}));
    let session_b = process.response(3)["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    process.prompt(20, &session_a, "session-a", "message-a");
    process.prompt(21, &session_b, "session-b", "message-b");
    let b = process.response(21);
    assert_eq!(b["result"]["userMessageId"], "message-b");
    let a = process.response(20);
    assert_eq!(a["result"]["userMessageId"], "message-a");
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_eq!(terminal_count(process.transcript(), 20), 1);
    assert_eq!(terminal_count(process.transcript(), 21), 1);
    process.finish();
    server.join().unwrap();
}

#[test]
fn prompts_in_one_session_are_serialized_and_history_reaches_second() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (first_arrived_sender, first_arrived_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let (check_sender, check_receiver) = mpsc::channel();
    let (serialized_sender, serialized_receiver) = mpsc::channel();
    let max_active = Arc::new(AtomicUsize::new(0));
    let server_max = Arc::clone(&max_active);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let first_request = read_http_request(&mut first);
        assert!(first_request.contains("\"content\":\"serial-a\""));
        server_max.store(1, Ordering::SeqCst);
        first_arrived_sender.send(()).unwrap();
        check_receiver.recv().unwrap();
        listener.set_nonblocking(true).unwrap();
        serialized_sender.send(listener.accept().is_err()).unwrap();
        listener.set_nonblocking(false).unwrap();
        release_receiver.recv().unwrap();
        write_sse(&mut first, &successful_body("serial-answer-a"));

        let (mut second, _) = listener.accept().unwrap();
        let second_request = read_http_request(&mut second);
        assert!(second_request.contains("\"content\":\"serial-b\""));
        assert!(second_request.contains("serial-answer-a"));
        write_sse(&mut second, &successful_body("serial-answer-b"));
    });

    let mut process = ProcessHarness::spawn(address);
    let session = process.initialize_and_new_session();
    process.prompt(30, &session, "serial-a", "serial-message-a");
    first_arrived_receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    process.prompt(31, &session, "serial-b", "serial-message-b");
    check_sender.send(()).unwrap();
    assert!(
        serialized_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
        "second same-session prompt reached provider before first terminated"
    );
    assert_eq!(max_active.load(Ordering::SeqCst), 1);
    release_sender.send(()).unwrap();
    assert_eq!(
        process.response(30)["result"]["userMessageId"],
        "serial-message-a"
    );
    assert_eq!(
        process.response(31)["result"]["userMessageId"],
        "serial-message-b"
    );
    assert_eq!(terminal_count(process.transcript(), 30), 1);
    assert_eq!(terminal_count(process.transcript(), 31), 1);
    process.finish();
    server.join().unwrap();
}

#[test]
fn slow_stdout_reader_backpressures_without_dropping_visible_events() {
    const CHUNKS: usize = 6_000;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_http_request(&mut stream);
        let mut body = String::with_capacity(CHUNKS * 600);
        for index in 0..CHUNKS {
            let content = format!("{index:05}:{}", "x".repeat(480));
            body.push_str("data: {\"choices\":[{\"delta\":{\"content\":");
            body.push_str(&serde_json::to_string(&content).unwrap());
            body.push_str("}}]}\n\n");
        }
        body.push_str(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        );
        write_sse(&mut stream, &body);
    });

    let mut process = ProcessHarness::spawn(address);
    let session = process.initialize_and_new_session();
    #[cfg(target_os = "linux")]
    let baseline = support::rss_kib(process.pid());
    process.pause_stdout();
    process.prompt(40, &session, "slow-reader", "slow-reader-message");
    thread::sleep(Duration::from_millis(300));
    #[cfg(target_os = "linux")]
    {
        let held = support::rss_kib(process.pid());
        assert!(
            held <= baseline + 24 * 1024,
            "ACP child RSS grew beyond bounded ceiling: baseline={baseline} KiB held={held} KiB"
        );
    }
    process.resume_stdout();
    assert_eq!(process.response(40)["result"]["stopReason"], "end_turn");
    let deltas = update_texts(process.transcript(), "agent_message_chunk");
    assert_eq!(deltas.len(), CHUNKS);
    for (index, delta) in deltas.iter().enumerate() {
        assert!(delta.starts_with(&format!("{index:05}:")));
    }
    assert_eq!(terminal_count(process.transcript(), 40), 1);
    process.finish();
    server.join().unwrap();
}

#[test]
fn cancellation_remains_responsive_while_stdout_is_backpressured() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (pressured_sender, pressured_receiver) = mpsc::channel();
    let (closed_sender, closed_receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_http_request(&mut stream);
        stream
            .set_write_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n"
        )
        .unwrap();
        let chunk = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
            serde_json::to_string(&"p".repeat(960)).unwrap()
        );
        loop {
            if stream.write_all(chunk.as_bytes()).is_err() {
                pressured_sender.send(()).unwrap();
                break;
            }
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match stream.write_all(chunk.as_bytes()) {
                Err(error)
                    if !matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    closed_sender.send(()).unwrap();
                    break;
                }
                _ if std::time::Instant::now() >= deadline => {
                    panic!("provider connection did not close after backpressured cancellation");
                }
                _ => thread::yield_now(),
            }
        }
    });

    let mut process = ProcessHarness::spawn(address);
    let session = process.initialize_and_new_session();
    process.pause_stdout();
    process.prompt(41, &session, "cancel-under-pressure", "pressure-message");
    pressured_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    process.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}));
    closed_receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    process.resume_stdout();
    assert_eq!(process.response(41)["result"]["stopReason"], "cancelled");
    assert_eq!(terminal_count(process.transcript(), 41), 1);
    process.finish();
    server.join().unwrap();
}
