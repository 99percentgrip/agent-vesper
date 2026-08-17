//! VesperLens — native human-in-the-loop oracle for HTML/UI artifacts.
//!
//! See ADR 0017 for the architectural contract. The public surface is
//! [`VesperLens::review_artifact`] and [`VesperLens::review_file`] serve trusted
//! outer chrome around a sandboxed artifact, wait for authenticated structured
//! feedback, and return a parsed [`LensFeedback`].
//!
//! The module is the first runtime TCP listener inside `vesper-agent`. It
//! is strictly loopback-only (`127.0.0.1:0`) and accepts no configuration that
//! could bind it to a non-loopback interface. File sessions are reusable while
//! the owning process remains alive.

pub mod http;
pub mod injector;
pub mod server;
pub mod types;

pub use injector::{inject_review_overlay, render_interview_artifact};
pub use server::serve_and_collect_feedback;
pub use types::{Action, DomAnnotation, LensAnswer, LensError, LensFeedback, LensQuestion};

use std::time::Duration;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Default review window (30 minutes). After this the listener is dropped
/// and [`VesperLens::review_artifact`] returns [`LensError::Timeout`].
pub const DEFAULT_REVIEW_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// VesperLens entrypoint (ADR 0017).
///
/// Cheap to construct. Inline artifacts use independent sessions; canonical
/// file paths reuse a session for iterative review.
#[derive(Debug, Clone)]
pub struct VesperLens {
    timeout: Duration,
    file_sessions: Arc<Mutex<HashMap<PathBuf, server::ReviewSessionHandle>>>,
}

impl Default for VesperLens {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_REVIEW_TIMEOUT,
            file_sessions: Arc::new(Mutex::new(HashMap::new())),
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
        Self {
            timeout,
            file_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The configured review window.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Start an isolated inline review, call `on_url` once, and block until the
    /// human submits authenticated feedback.
    ///
    /// The returned [`LensFeedback`] is the only data that crosses back
    /// into the VRO planner — the agent never receives the raw HTML again.
    pub async fn review_artifact(
        &self,
        html_payload: &str,
        on_url: impl FnOnce(&str),
    ) -> Result<LensFeedback, LensError> {
        serve_and_collect_feedback(html_payload, on_url, self.timeout).await
    }

    /// Review an existing HTML file inside `workspace_root`. Calls for the
    /// same canonical file reuse one browser session, so feedback remains
    /// queued across cancelled tool futures and subsequent review rounds.
    pub async fn review_file(
        &self,
        file: &Path,
        workspace_root: &Path,
        on_url: impl FnOnce(&str),
    ) -> Result<LensFeedback, LensError> {
        let root = workspace_root.canonicalize().map_err(|error| {
            LensError::InvalidArtifact(format!("workspace root is not accessible: {error}"))
        })?;
        let file = file.canonicalize().map_err(|error| {
            LensError::InvalidArtifact(format!("artifact is not accessible: {error}"))
        })?;
        if !file.starts_with(&root) {
            return Err(LensError::InvalidArtifact(
                "artifact escapes the active workspace".into(),
            ));
        }
        let extension = file
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !extension.eq_ignore_ascii_case("html") && !extension.eq_ignore_ascii_case("htm") {
            return Err(LensError::InvalidArtifact(
                "request_human_review accepts only .html or .htm files".into(),
            ));
        }
        let metadata = std::fs::metadata(&file)?;
        if !metadata.is_file() || metadata.len() > server::MAX_ARTIFACT_BYTES {
            return Err(LensError::InvalidArtifact(format!(
                "artifact must be a file no larger than {} bytes",
                server::MAX_ARTIFACT_BYTES
            )));
        }

        let existing = self
            .file_sessions
            .lock()
            .map_err(|_| {
                LensError::InvalidArtifact("review session registry is unavailable".into())
            })?
            .get(&file)
            .filter(|session| session.is_alive())
            .cloned();
        let (session, is_new) = if let Some(session) = existing {
            (session, false)
        } else {
            let session = server::start_review_session(
                server::ArtifactSource::file(file.clone())?,
                self.timeout,
            )
            .await?;
            self.file_sessions
                .lock()
                .map_err(|_| {
                    LensError::InvalidArtifact("review session registry is unavailable".into())
                })?
                .insert(file.clone(), session.clone());
            (session, true)
        };
        session.begin_round();
        if is_new {
            on_url(session.url());
        }
        let feedback = session.next_feedback(self.timeout).await?;
        if (feedback.end_session || !session.is_alive())
            && let Ok(mut sessions) = self.file_sessions.lock()
        {
            sessions.remove(&file);
        }
        Ok(feedback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn url_parts(url: &str) -> (&str, &str) {
        let without_scheme = url.trim_start_matches("http://");
        let (address, session_path) = without_scheme.split_once('/').unwrap();
        (address, session_path.strip_prefix("s/").unwrap())
    }

    async fn post_approval(url: &str, notes: &str) {
        let (address, token) = url_parts(url);
        let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
        let body = serde_json::json!({"action":"approve","notes":notes}).to_string();
        let request = format!(
            "POST /s/{token}/feedback HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nX-Vesper-Lens-Token: {token}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

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
        let (addr, token) = url_parts(&url);
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let body = r#"{"action":"approve","notes":"lgtm"}"#;
        let req = format!(
            "POST /s/{token}/feedback HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nContent-Type: application/json\r\nX-Vesper-Lens-Token: {token}\r\nContent-Length: {}\r\n\r\n{}",
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

    #[tokio::test]
    async fn file_session_survives_a_cancelled_wait_and_keeps_feedback_queued() {
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("index.html");
        std::fs::write(&file, "<!doctype html><html><body>resumable</body></html>").unwrap();
        let lens = Arc::new(VesperLens::with_timeout(Duration::from_secs(5)));
        let (url_tx, url_rx) = tokio::sync::oneshot::channel();
        let first_lens = Arc::clone(&lens);
        let first_file = file.clone();
        let root = workspace.path().to_path_buf();
        let first = tokio::spawn(async move {
            first_lens
                .review_file(&first_file, &root, |url| {
                    let _ = url_tx.send(url.to_string());
                })
                .await
        });
        let url = url_rx.await.unwrap();
        first.abort();
        post_approval(&url, "queued while no tool waiter owned the session").await;
        let feedback = lens
            .review_file(&file, workspace.path(), |_url| {
                panic!("live file session must be reused")
            })
            .await
            .unwrap();
        assert_eq!(
            feedback.notes,
            "queued while no tool waiter owned the session"
        );
    }
}
