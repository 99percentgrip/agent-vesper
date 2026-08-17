//! VesperLens — native human-in-the-loop oracle for HTML/UI artifacts.
//!
//! See ADR 0017 for the architectural contract. The public surface is
//! [`VesperLens::review_artifact`]: given an HTML payload, it injects the
//! review overlay, starts a loopback HTTP server, waits for the human to
//! POST structured feedback, and returns a parsed [`LensFeedback`].
//!
//! The module is the first runtime TCP listener inside `vesper-agent`. It
//! is strictly loopback-only (`127.0.0.1:0`), single-turn, and accepts no
//! configuration that could bind it to a non-loopback interface.

pub mod http;
pub mod injector;
pub mod server;
pub mod types;

pub use injector::{inject_review_overlay, render_interview_artifact};
pub use server::serve_and_collect_feedback;
pub use types::{Action, DomAnnotation, LensAnswer, LensError, LensFeedback, LensQuestion};

use std::time::Duration;

/// Default review window (30 minutes). After this the listener is dropped
/// and [`VesperLens::review_artifact`] returns [`LensError::Timeout`].
pub const DEFAULT_REVIEW_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// VesperLens entrypoint (ADR 0017).
///
/// Cheap to construct. Each call to [`Self::review_artifact`] runs one
/// independent review session on its own ephemeral port.
#[derive(Debug, Clone)]
pub struct VesperLens {
    timeout: Duration,
}

impl Default for VesperLens {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_REVIEW_TIMEOUT,
        }
    }
}

impl VesperLens {
    /// Construct a VesperLens with the default 30-minute review window.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a VesperLens with a custom review window. Useful for
    /// tests (short timeouts) or batch runs (longer ceilings).
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// The configured review window.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Inject the review overlay into `html_payload`, start the loopback
    /// server, call `on_url` exactly once with the canonical review URL,
    /// and block until the human submits feedback.
    ///
    /// The returned [`LensFeedback`] is the only data that crosses back
    /// into the VRO planner — the agent never receives the raw HTML again.
    pub async fn review_artifact(
        &self,
        html_payload: &str,
        on_url: impl FnOnce(&str),
    ) -> Result<LensFeedback, LensError> {
        let injected = inject_review_overlay(html_payload);
        serve_and_collect_feedback(&injected, on_url, self.timeout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn default_timeout_is_30_minutes() {
        assert_eq!(VesperLens::new().timeout(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn with_timeout_round_trips() {
        let lens = VesperLens::with_timeout(Duration::from_secs(7));
        assert_eq!(lens.timeout(), Duration::from_secs(7));
    }

    #[tokio::test]
    async fn review_artifact_end_to_end() {
        // End-to-end through the public surface: construct a VesperLens,
        // call review_artifact, capture the URL via the callback, then
        // drive the server with a TcpStream client.
        let lens = VesperLens::with_timeout(Duration::from_secs(5));
        let html = "<html><body><h1>End to end</h1></body></html>";

        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        // `async move` captures `lens` by value, satisfying the `'static`
        // requirement of `tokio::spawn`. The spawned task runs the server
        // concurrently with the client work below — critical, because the
        // client's POST is what unblocks the server's accept().
        let review = tokio::spawn(async move {
            lens.review_artifact(html, move |url| {
                let _ = tx.send(url.to_string());
            })
            .await
        });

        // Wait for the server task to bind and announce its URL.
        let url = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .expect("server did not bind in 2s")
            .expect("on_url callback dropped");

        // Client: connect, POST feedback, drain ack.
        let addr = url.trim_start_matches("http://").trim_end_matches('/');
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let body = r#"{"action":"approve","notes":"lgtm"}"#;
        let req = format!(
            "POST /feedback HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        client.write_all(req.as_bytes()).await.unwrap();
        let mut buf = vec![0u8; 256];
        let _ = client.read(&mut buf).await;

        let feedback = review.await.unwrap().unwrap();
        assert_eq!(feedback.action, Action::Approve);
        assert_eq!(feedback.notes, "lgtm");
    }
}
