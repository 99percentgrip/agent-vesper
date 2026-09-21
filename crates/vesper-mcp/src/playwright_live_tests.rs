//! Explicit local-browser acceptance, excluded from ordinary foundation tests.
use crate::{McpServerConfig, McpSession, McpTransport};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

fn text(result: Value, root: &std::path::Path) -> String {
    assert_ne!(result["isError"], true, "{result}");
    let mut text = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(tail) = text.split("[Snapshot](").nth(1) {
        let path = tail.split(')').next().unwrap();
        let path = std::path::Path::new(path);
        assert!(
            path.starts_with(root),
            "snapshot outside temporary output root"
        );
        text.push_str(&std::fs::read_to_string(path).unwrap());
    }
    text
}

#[test]
#[ignore = "requires explicitly supplied local Node and Playwright MCP CLI; launches isolated Chrome"]
fn real_playwright_navigation_snapshot_click_isolation_close() {
    let node = std::env::var("VESPER_TEST_NODE").expect("explicit Node executable required");
    let cli =
        std::env::var("VESPER_TEST_PLAYWRIGHT_CLI").expect("explicit installed MCP CLI required");
    let output = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while !done.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let _ = stream.read(&mut [0u8; 8192]);
                let body = "<!doctype html><title>Vesper MCP continuity</title><h1>Local fixture</h1><button onclick=\"document.querySelector('p').textContent='Clicked successfully'\">Change</button><p>Not clicked</p>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    let config = McpServerConfig {
        id: "playwright".into(),
        transport: McpTransport::Stdio,
        command: Some(node),
        args: vec![
            cli,
            "--headless".into(),
            "--isolated".into(),
            "--output-dir".into(),
            output.path().to_string_lossy().into_owned(),
            "--file-paths".into(),
            "absolute".into(),
        ],
        url: None,
        auth_env: None,
        label: None,
        created_at: std::time::SystemTime::UNIX_EPOCH,
    };
    let a = McpSession::default();
    let b = McpSession::default();
    let tools = a.tools(&config).unwrap();
    println!(
        "click schema: {:?}",
        tools.iter().find(|t| t.name == "browser_click")
    );
    assert!(tools.iter().any(|t| t.name == "browser_navigate"));
    let navigate = text(
        a.call_tool(
            &config,
            "browser_navigate",
            json!({"url":format!("http://{address}/")}),
        )
        .unwrap(),
        output.path(),
    );
    assert!(navigate.contains("Vesper MCP continuity"), "{navigate}");
    let snapshot = text(
        a.call_tool(&config, "browser_snapshot", json!({})).unwrap(),
        output.path(),
    );
    assert!(snapshot.contains("Local fixture"), "{snapshot}");
    let line = snapshot
        .lines()
        .find(|line| line.contains("button \"Change\""))
        .unwrap();
    let reference = line
        .split("ref=")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap();
    let clicked = text(
        a.call_tool(&config, "browser_click", json!({"target":reference}))
            .unwrap(),
        output.path(),
    );
    assert!(clicked.contains("Clicked successfully"), "{clicked}");
    let independent = text(
        b.call_tool(&config, "browser_snapshot", json!({})).unwrap(),
        output.path(),
    );
    assert!(
        !independent.contains("Clicked successfully") && !independent.contains("Local fixture"),
        "{independent}"
    );
    text(
        a.call_tool(&config, "browser_close", json!({})).unwrap(),
        output.path(),
    );
    text(
        b.call_tool(&config, "browser_close", json!({})).unwrap(),
        output.path(),
    );
    drop(a);
    drop(b);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    println!(
        "PASS: real Playwright navigation -> snapshot -> click; independent owner blank; both browsers closed"
    );
}
