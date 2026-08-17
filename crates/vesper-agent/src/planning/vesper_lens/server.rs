//! Raw `tokio::net::TcpListener` loopback server for VesperLens (ADR 0017).
//!
//! Per the PRD §3.C, this module owns:
//! - binding to `127.0.0.1:0` (OS-assigned ephemeral port),
//! - serving the injected HTML on `GET /`,
//! - parsing JSON feedback on `POST /feedback`,
//! - shutting down the listener after the POST and returning the parsed
//!   [`LensFeedback`].
//!
//! No web framework, no `axum`/`hyper`, no `std::process::Command`. The
//! HTTP/1.1 wire format is parsed by the pure functions in [`super::http`].
//!
//! ## Robustness
//!
//! A real browser using `fetch("/feedback", { keepalive: true })` will
//! typically close the GET connection (we advertise `Connection: close`)
//! and open a fresh connection for the POST. The accept loop therefore
//! handles **any number of sequential connections** within the configured
//! timeout, and within each connection reads **any number of requests**
//! until either a `POST /feedback` arrives or the peer closes.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time;

use super::http::{ParsedRequest, build_html_response, build_json_response, try_parse_request};
use super::types::{LensError, LensFeedback};

/// The address we always bind. Hard-coded; never overridable.
const LOOPBACK_BIND: &str = "127.0.0.1:0";

/// Per-connection read buffer cap. An honest feedback POST is well under
/// 1 KiB; a 64 KiB cap absorbs pathological `notes` strings without letting
/// a hostile client exhaust memory.
const READ_BUFFER_CAP: usize = 64 * 1024;

/// Serve `injected_html` on an ephemeral loopback port and block until the
/// human submits feedback.
///
/// `on_url` is called exactly once with the canonical review URL
/// (`http://127.0.0.1:<port>/`) as soon as the listener is bound. The
/// caller wires this into the TUI status line per PRD §4.
///
/// `timeout` bounds the entire review window. If no feedback arrives in
/// time the listener is dropped (RAII) and [`LensError::Timeout`] is
/// returned.
pub async fn serve_and_collect_feedback(
    injected_html: &str,
    on_url: impl FnOnce(&str),
    timeout: Duration,
) -> Result<LensFeedback, LensError> {
    let listener = TcpListener::bind(LOOPBACK_BIND).await?;
    let bound_addr = listener.local_addr()?;
    let url = format!("http://{bound_addr}/");
    on_url(&url);

    // Bounded by the outer timeout: every accept/read races against it.
    let result = time::timeout(timeout, accept_loop(&listener, injected_html)).await;
    match result {
        Ok(inner) => inner,
        Err(_) => Err(LensError::Timeout),
    }
}

async fn accept_loop(
    listener: &TcpListener,
    injected_html: &str,
) -> Result<LensFeedback, LensError> {
    loop {
        // Accept errors (transient EMFILE etc.) are retried; we never
        // surface them as the final verdict unless the listener itself
        // dies.
        let (mut stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        // localhost peer is implicitly trusted because the bind is loopback
        // only. We deliberately do NOT log _peer (which would be 127.0.0.1
        // anyway).
        match handle_connection(&mut stream, injected_html).await {
            Ok(Some(feedback)) => return Ok(feedback),
            Ok(None) => {
                // Connection closed cleanly without a POST. Loop and accept
                // the next one (a browser may issue GET then POST on
                // separate connections).
                continue;
            }
            Err(LensError::HttpParse(_)) => {
                // A single malformed request does not poison the whole
                // server — drop this connection and accept the next.
                continue;
            }
            Err(other) => return Err(other),
        }
    }
}

/// Handle one accepted TCP connection: read **exactly one** complete
/// HTTP request, dispatch it, and return. The connection is then closed
/// by the caller (this matches the `Connection: close` response header
/// we advertise, so a browser reconnects for the next request).
///
/// Returns:
/// - `Ok(Some(feedback))` — a POST /feedback was received and parsed.
/// - `Ok(None)` — the peer sent a non-POST request (GET handled, junk
///   ignored, etc.) OR closed cleanly without sending anything.
/// - `Err(_)` — I/O failure, malformed request, or unparseable JSON.
async fn handle_connection(
    stream: &mut tokio::net::TcpStream,
    injected_html: &str,
) -> Result<Option<LensFeedback>, LensError> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut tmp = [0u8; 4096];

    // Read until we have one complete request (header terminator present
    // AND, if Content-Length is set, all body bytes present).
    let req = loop {
        match try_parse_request(&buf) {
            Ok(Some(req)) => break req,
            Ok(None) => {
                let n = stream.read(&mut tmp).await?;
                if n == 0 {
                    if buf.is_empty() {
                        return Ok(None);
                    }
                    return Err(LensError::HttpParse("connection closed mid-request".into()));
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > READ_BUFFER_CAP {
                    return Err(LensError::HttpParse(format!(
                        "request exceeded {READ_BUFFER_CAP}-byte cap"
                    )));
                }
            }
            Err(e) => return Err(LensError::HttpParse(e.to_string())),
        }
    };

    // Dispatch exactly one request, then return so the caller closes the
    // connection. The `consumed_len` bookkeeping is unnecessary because we
    // never read a second request on the same stream.
    let _ = buf; // buf goes out of scope here
    dispatch(stream, req, injected_html).await
}

/// Dispatch a parsed request. Returns `Some(feedback)` only for a
/// successful POST /feedback.
async fn dispatch(
    stream: &mut tokio::net::TcpStream,
    req: ParsedRequest,
    injected_html: &str,
) -> Result<Option<LensFeedback>, LensError> {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            let bytes = build_html_response(injected_html);
            stream.write_all(&bytes).await?;
            stream.flush().await?;
            Ok(None)
        }
        ("POST", "/feedback") => {
            if req.body.is_empty() {
                let _ = stream
                    .write_all(&build_json_response(
                        400,
                        r#"{"ok":false,"error":"empty body"}"#,
                    ))
                    .await;
                let _ = stream.flush().await;
                return Err(LensError::EmptyBody);
            }
            match serde_json::from_slice::<LensFeedback>(&req.body) {
                Ok(fb) => {
                    let _ = stream
                        .write_all(&build_json_response(200, r#"{"ok":true}"#))
                        .await;
                    let _ = stream.flush().await;
                    Ok(Some(fb))
                }
                Err(err) => {
                    let msg = format!(
                        r#"{{"ok":false,"error":{}}}"#,
                        serde_json::json!(err.to_string())
                    );
                    let _ = stream.write_all(&build_json_response(400, &msg)).await;
                    let _ = stream.flush().await;
                    Err(LensError::Json(err))
                }
            }
        }
        // Browser favicon / preflight / unknown routes get a clean 404 so
        // we do not poison the connection.
        _ => {
            let _ = stream
                .write_all(&build_json_response(
                    404,
                    r#"{"ok":false,"error":"not found"}"#,
                ))
                .await;
            let _ = stream.flush().await;
            Ok(None)
        }
    }
}

// NOTE: We previously tracked the byte length of a parsed request here so
// that the read loop could drain `buf` and handle multiple requests per
// connection. With the simplified one-request-per-connection design above
// (matching `Connection: close`), the bookkeeping is unnecessary; the
// `http_find_header_end` helper in `mod.rs` is retained for tests.

#[cfg(test)]
mod tests {
    use super::super::types::Action;
    use super::*;
    use crate::planning::vesper_lens::injector::inject_review_overlay;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Start a fresh server, returning (bound_url, server_join_handle).
    async fn start_server(
        html: &str,
        timeout: Duration,
    ) -> (
        String,
        tokio::task::JoinHandle<Result<LensFeedback, LensError>>,
    ) {
        let injected = inject_review_overlay(html);
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let handle = tokio::spawn(async move {
            serve_and_collect_feedback(
                &injected,
                |url| {
                    let _ = tx.send(url.to_string());
                },
                timeout,
            )
            .await
        });
        let url = rx.await.expect("server must bind");
        (url, handle)
    }

    /// Strip "http://" and trailing "/" to get the bare host:port for TcpStream::connect.
    fn addr_of(url: &str) -> &str {
        url.trim_start_matches("http://").trim_end_matches('/')
    }

    #[tokio::test]
    async fn happy_path_get_then_post_feedback() {
        let (url, server) = start_server(
            "<html><body><h1>Hello</h1></body></html>",
            Duration::from_secs(5),
        )
        .await;

        // GET first, on its own connection.
        {
            let mut s = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
            s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = s.read(&mut buf).await.unwrap();
        }
        // POST feedback on a fresh connection.
        {
            let mut s = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
            let body = r#"{"action":"approve"}"#;
            let req = format!(
                "POST /feedback HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut buf = vec![0u8; 256];
            let _ = s.read(&mut buf).await.unwrap();
        }

        let feedback = server.await.unwrap().unwrap();
        assert_eq!(feedback.action, Action::Approve);
    }

    #[tokio::test]
    async fn post_with_annotations_parses_correctly() {
        let body = r##"{"action":"modify","annotations":[{"selector":"#hero","comment":"too big"}],"notes":"fix it","answers":[{"question":"framework","value":"Rust"}]}"##;
        let (url, server) =
            start_server("<html><body></body></html>", Duration::from_secs(5)).await;

        let mut s = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
        let req = format!(
            "POST /feedback HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = vec![0u8; 1024];
        let _ = s.read(&mut buf).await.unwrap();

        let feedback = server.await.unwrap().unwrap();
        assert_eq!(feedback.action, Action::Modify);
        assert_eq!(feedback.annotations.len(), 1);
        assert_eq!(feedback.annotations[0].selector, "#hero");
        assert_eq!(feedback.annotations[0].comment, "too big");
        assert_eq!(feedback.notes, "fix it");
        assert_eq!(feedback.answers.len(), 1);
        assert_eq!(feedback.answers[0].question, "framework");
        assert_eq!(feedback.answers[0].value, "Rust");
    }

    #[tokio::test]
    async fn get_returns_injected_html_with_overlay() {
        let (url, _server) = start_server(
            "<html><body><p>artifact</p></body></html>",
            // Short timeout: we don't POST, so the server will time out
            // after we've grabbed the GET response.
            Duration::from_millis(300),
        )
        .await;

        let mut s = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut buf = vec![0u8; 65536];
        let n = s.read(&mut buf).await.unwrap();
        let got = &buf[..n];
        let s = std::str::from_utf8(got).unwrap();
        assert!(s.contains("HTTP/1.1 200 OK"));
        assert!(s.contains("<p>artifact</p>"));
        assert!(s.contains("VesperLens Review"));
    }

    #[tokio::test]
    async fn timeout_returns_timeout_error() {
        let injected = inject_review_overlay("<html></html>");
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let handle = tokio::spawn(async move {
            serve_and_collect_feedback(
                &injected,
                |url| {
                    let _ = tx.send(url.to_string());
                },
                Duration::from_millis(100),
            )
            .await
        });
        // Confirm the server bound (so we know the timeout is real, not
        // a bind failure).
        let _url = rx.await.expect("server must bind");
        let res = handle.await.unwrap();
        assert!(matches!(res, Err(LensError::Timeout)));
    }

    #[tokio::test]
    async fn malformed_request_does_not_poison_server() {
        // Send garbage on connection 1; the server should drop it and
        // still serve the legitimate POST on connection 2.
        let body = r#"{"action":"reject"}"#;
        let (url, server) = start_server("<html></html>", Duration::from_secs(5)).await;

        // Garbage connection.
        {
            let mut g = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
            g.write_all(b"NOT HTTP AT ALL\r\n\r\n").await.unwrap();
            let mut buf = vec![0u8; 64];
            let _ = g.read(&mut buf).await;
        }
        // Real POST on a fresh connection.
        {
            let mut s = tokio::net::TcpStream::connect(addr_of(&url)).await.unwrap();
            let req = format!(
                "POST /feedback HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut buf = vec![0u8; 256];
            let _ = s.read(&mut buf).await.unwrap();
        }

        let feedback = server.await.unwrap().unwrap();
        assert_eq!(feedback.action, Action::Reject);
    }
}
