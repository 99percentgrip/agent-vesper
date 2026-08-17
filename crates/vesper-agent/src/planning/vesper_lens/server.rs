//! Authenticated loopback review server for VesperLens.
//!
//! Trusted review chrome is served as the top-level document. The artifact is
//! confined to a sandboxed iframe and can communicate only through the owned
//! annotation SDK. Feedback requires an unguessable session route, an exact
//! Host and Origin, JSON content type, and a matching custom token header.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio::time;
use uuid::Uuid;

use super::http::{
    ParsedRequest, build_json_response, build_response, build_response_with_headers,
    try_parse_request,
};
use super::injector::{
    ARTIFACT_SDK_SCRIPT, CHROME_SCRIPT, inject_review_sdk, render_review_chrome,
};
use super::types::{LensError, LensFeedback};

const LOOPBACK_BIND: &str = "127.0.0.1:0";
const READ_BUFFER_CAP: usize = 64 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) enum ArtifactSource {
    Inline(Arc<str>),
    File { index: PathBuf, root: PathBuf },
}

impl ArtifactSource {
    pub(crate) fn inline(html: &str) -> Self {
        Self::Inline(Arc::from(html))
    }

    pub(crate) fn file(index: PathBuf) -> Result<Self, LensError> {
        let root = index
            .parent()
            .ok_or_else(|| LensError::InvalidArtifact("artifact has no parent directory".into()))?
            .to_path_buf();
        Ok(Self::File { index, root })
    }

    fn revision(&self) -> String {
        match self {
            Self::Inline(html) => format!("inline-{}", html.len()),
            Self::File { index, .. } => std::fs::metadata(index).map_or_else(
                |_| "missing".into(),
                |metadata| {
                    let modified = metadata
                        .modified()
                        .unwrap_or(SystemTime::UNIX_EPOCH)
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default();
                    format!(
                        "{}-{}-{}",
                        modified.as_secs(),
                        modified.subsec_nanos(),
                        metadata.len()
                    )
                },
            ),
        }
    }

    fn index_html(&self) -> Result<String, LensError> {
        match self {
            Self::Inline(html) => Ok(html.to_string()),
            Self::File { index, .. } => read_bounded_text(index, MAX_ARTIFACT_BYTES),
        }
    }

    fn asset(&self, relative: &str) -> Result<Option<(Vec<u8>, &'static str)>, LensError> {
        let Self::File { root, .. } = self else {
            return Ok(None);
        };
        let decoded = percent_decode(relative)?;
        let relative_path = Path::new(&decoded);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(LensError::InvalidArtifact(
                "asset path escapes the artifact directory".into(),
            ));
        }
        let candidate = root.join(relative_path);
        let canonical = match candidate.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(LensError::Io(error)),
        };
        let canonical_root = root.canonicalize()?;
        if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
            return Err(LensError::InvalidArtifact(
                "asset symlink escapes the artifact directory".into(),
            ));
        }
        let bytes = read_bounded(&canonical, MAX_ASSET_BYTES)?;
        Ok(Some((bytes, mime_for_path(&canonical))))
    }
}

#[derive(Debug)]
struct SessionInner {
    token: String,
    url: String,
    source: ArtifactSource,
    feedback_rx: Mutex<mpsc::UnboundedReceiver<LensFeedback>>,
    round: AtomicU64,
    alive: AtomicBool,
}

/// Reusable handle to one browser review session. Submitted feedback remains
/// queued even when an individual agent tool future is cancelled.
#[derive(Debug, Clone)]
pub(crate) struct ReviewSessionHandle(Arc<SessionInner>);

impl ReviewSessionHandle {
    pub(crate) fn url(&self) -> &str {
        &self.0.url
    }
    pub(crate) fn begin_round(&self) -> u64 {
        self.0.round.fetch_add(1, Ordering::SeqCst) + 1
    }
    pub(crate) fn is_alive(&self) -> bool {
        self.0.alive.load(Ordering::SeqCst)
    }

    pub(crate) async fn next_feedback(&self, timeout: Duration) -> Result<LensFeedback, LensError> {
        let mut receiver = self.0.feedback_rx.lock().await;
        match time::timeout(timeout, receiver.recv()).await {
            Ok(Some(feedback)) => Ok(feedback),
            Ok(None) => Err(LensError::Disconnected),
            Err(_) => Err(LensError::Timeout),
        }
    }
}

pub(crate) async fn start_review_session(
    source: ArtifactSource,
    inactivity_timeout: Duration,
) -> Result<ReviewSessionHandle, LensError> {
    let listener = TcpListener::bind(LOOPBACK_BIND).await?;
    let address = listener.local_addr()?;
    let token = Uuid::new_v4().simple().to_string();
    let url = format!("http://{address}/s/{token}");
    let (feedback_tx, feedback_rx) = mpsc::unbounded_channel();
    let inner = Arc::new(SessionInner {
        token,
        url,
        source,
        feedback_rx: Mutex::new(feedback_rx),
        round: AtomicU64::new(0),
        alive: AtomicBool::new(true),
    });
    let server_inner = Arc::clone(&inner);
    tokio::spawn(async move {
        run_server(
            listener,
            address.to_string(),
            server_inner,
            feedback_tx,
            inactivity_timeout,
        )
        .await
    });
    Ok(ReviewSessionHandle(inner))
}

/// Compatibility one-shot entrypoint used by inline interviews and tests.
pub async fn serve_and_collect_feedback(
    html: &str,
    on_url: impl FnOnce(&str),
    timeout: Duration,
) -> Result<LensFeedback, LensError> {
    let session = start_review_session(ArtifactSource::inline(html), timeout).await?;
    session.begin_round();
    on_url(session.url());
    session.next_feedback(timeout).await
}

async fn run_server(
    listener: TcpListener,
    expected_host: String,
    session: Arc<SessionInner>,
    feedback_tx: mpsc::UnboundedSender<LensFeedback>,
    inactivity_timeout: Duration,
) {
    loop {
        let accepted = time::timeout(inactivity_timeout, listener.accept()).await;
        let Ok(Ok((mut stream, _))) = accepted else {
            break;
        };
        let should_end = handle_connection(&mut stream, &expected_host, &session, &feedback_tx)
            .await
            .unwrap_or(false);
        if should_end {
            break;
        }
    }
    session.alive.store(false, Ordering::SeqCst);
}

async fn handle_connection(
    stream: &mut TcpStream,
    expected_host: &str,
    session: &Arc<SessionInner>,
    feedback_tx: &mpsc::UnboundedSender<LensFeedback>,
) -> Result<bool, LensError> {
    let mut buffer = Vec::with_capacity(8192);
    let mut chunk = [0_u8; 4096];
    let request = loop {
        match try_parse_request(&buffer) {
            Ok(Some(request)) => break request,
            Ok(None) => {
                let read = stream.read(&mut chunk).await?;
                if read == 0 {
                    return Ok(false);
                }
                if buffer.len() + read > READ_BUFFER_CAP {
                    write_response(
                        stream,
                        build_json_response(413, r#"{"ok":false,"error":"request too large"}"#),
                    )
                    .await?;
                    return Ok(false);
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            Err(error) => {
                write_response(
                    stream,
                    build_json_response(
                        400,
                        &serde_json::json!({"ok":false,"error":error.to_string()}).to_string(),
                    ),
                )
                .await?;
                return Ok(false);
            }
        }
    };

    if request.headers.get("host").map(String::as_str) != Some(expected_host) {
        write_response(
            stream,
            build_json_response(403, r#"{"ok":false,"error":"forbidden host"}"#),
        )
        .await?;
        return Ok(false);
    }

    dispatch(stream, request, expected_host, session, feedback_tx).await
}

async fn dispatch(
    stream: &mut TcpStream,
    request: ParsedRequest,
    expected_host: &str,
    session: &Arc<SessionInner>,
    feedback_tx: &mpsc::UnboundedSender<LensFeedback>,
) -> Result<bool, LensError> {
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let base = format!("/s/{}", session.token);
    if request.method == "GET" && path == "/favicon.ico" {
        write_response(
            stream,
            build_response("204 No Content", "image/x-icon", &[]),
        )
        .await?;
        return Ok(false);
    }
    if request.method == "GET" && (path == "/" || path == base) {
        let chrome = render_review_chrome(&session.token);
        let response = build_response_with_headers(
            "200 OK",
            "text/html",
            chrome.as_bytes(),
            &[
                (
                    "Content-Security-Policy",
                    "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; frame-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
                ),
                ("Cross-Origin-Opener-Policy", "same-origin"),
                ("X-Frame-Options", "DENY"),
            ],
        );
        write_response(stream, response).await?;
        return Ok(false);
    }
    if request.method == "GET" && path == format!("{base}/chrome.js") {
        write_response(
            stream,
            build_response("200 OK", "text/javascript", CHROME_SCRIPT.as_bytes()),
        )
        .await?;
        return Ok(false);
    }
    if request.method == "GET" && path == format!("{base}/sdk.js") {
        write_response(
            stream,
            build_response("200 OK", "text/javascript", ARTIFACT_SDK_SCRIPT.as_bytes()),
        )
        .await?;
        return Ok(false);
    }
    if request.method == "GET" && path == format!("{base}/state") {
        let body = serde_json::json!({"round":session.round.load(Ordering::SeqCst),"revision":session.source.revision()}).to_string();
        write_response(stream, build_json_response(200, &body)).await?;
        return Ok(false);
    }
    let artifact_prefix = format!("{base}/artifact/");
    if request.method == "GET" && path == format!("{artifact_prefix}index.html") {
        match session.source.index_html() {
            Ok(html) => {
                let injected = inject_review_sdk(&html, &format!("{base}/sdk.js"));
                write_response(
                    stream,
                    build_response("200 OK", "text/html", injected.as_bytes()),
                )
                .await?;
            }
            Err(error) => {
                write_response(
                    stream,
                    build_response(
                        "500 Internal Server Error",
                        "text/plain",
                        error.to_string().as_bytes(),
                    ),
                )
                .await?
            }
        }
        return Ok(false);
    }
    if request.method == "GET" && path.starts_with(&artifact_prefix) {
        let relative = &path[artifact_prefix.len()..];
        match session.source.asset(relative) {
            Ok(Some((bytes, mime))) => {
                write_response(stream, build_response("200 OK", mime, &bytes)).await?
            }
            Ok(None) => {
                write_response(
                    stream,
                    build_json_response(404, r#"{"ok":false,"error":"asset not found"}"#),
                )
                .await?
            }
            Err(error) => {
                write_response(
                    stream,
                    build_json_response(
                        403,
                        &serde_json::json!({"ok":false,"error":error.to_string()}).to_string(),
                    ),
                )
                .await?
            }
        }
        return Ok(false);
    }
    if request.method == "POST" && path == format!("{base}/feedback") {
        let expected_origin = format!("http://{expected_host}");
        let content_type_ok = request
            .headers
            .get("content-type")
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"));
        let authenticated = request.headers.get("origin").map(String::as_str)
            == Some(expected_origin.as_str())
            && request
                .headers
                .get("x-vesper-lens-token")
                .map(String::as_str)
                == Some(session.token.as_str())
            && content_type_ok;
        if !authenticated {
            write_response(
                stream,
                build_json_response(403, r#"{"ok":false,"error":"unauthenticated feedback"}"#),
            )
            .await?;
            return Ok(false);
        }
        let feedback: LensFeedback = match serde_json::from_slice(&request.body) {
            Ok(feedback) => feedback,
            Err(error) => {
                write_response(
                    stream,
                    build_json_response(
                        400,
                        &serde_json::json!({"ok":false,"error":error.to_string()}).to_string(),
                    ),
                )
                .await?;
                return Ok(false);
            }
        };
        let should_end = feedback.end_session;
        if feedback_tx.send(feedback).is_err() {
            write_response(
                stream,
                build_json_response(410, r#"{"ok":false,"error":"review session ended"}"#),
            )
            .await?;
            return Ok(true);
        }
        write_response(stream, build_json_response(200, r#"{"ok":true}"#)).await?;
        return Ok(should_end);
    }
    write_response(
        stream,
        build_json_response(404, r#"{"ok":false,"error":"not found"}"#),
    )
    .await?;
    Ok(false)
}

async fn write_response(stream: &mut TcpStream, response: Vec<u8>) -> Result<(), LensError> {
    stream.write_all(&response).await?;
    stream.flush().await?;
    Ok(())
}

fn read_bounded_text(path: &Path, max: u64) -> Result<String, LensError> {
    let bytes = read_bounded(path, max)?;
    String::from_utf8(bytes)
        .map_err(|_| LensError::InvalidArtifact("artifact is not valid UTF-8 HTML".into()))
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, LensError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > max {
        return Err(LensError::InvalidArtifact(format!(
            "{} exceeds the {} byte review limit",
            path.display(),
            max
        )));
    }
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref().take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(LensError::InvalidArtifact(
            "file grew beyond the review limit while reading".into(),
        ));
    }
    Ok(bytes)
}

fn percent_decode(value: &str) -> Result<String, LensError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(LensError::InvalidArtifact(
                    "malformed percent-encoded asset path".into(),
                ));
            }
            let high = hex(bytes[index + 1])?;
            let low = hex(bytes[index + 2])?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output)
        .map_err(|_| LensError::InvalidArtifact("asset path is not UTF-8".into()))
}

fn hex(value: u8) -> Result<u8, LensError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(LensError::InvalidArtifact(
            "malformed percent-encoded asset path".into(),
        )),
    }
}

fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn request(address: &str, raw: String) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(raw.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    fn address_and_token(url: &str) -> (&str, &str) {
        let without_scheme = url.strip_prefix("http://").unwrap();
        let (address, path) = without_scheme.split_once('/').unwrap();
        (address, path.strip_prefix("s/").unwrap())
    }

    #[tokio::test]
    async fn chrome_is_sandboxed_and_feedback_is_authenticated() {
        let session = start_review_session(
            ArtifactSource::inline("<html><body>safe</body></html>"),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        session.begin_round();
        let (address, token) = address_and_token(session.url());
        let chrome = request(
            address,
            format!("GET /s/{token} HTTP/1.1\r\nHost: {address}\r\n\r\n"),
        )
        .await;
        assert!(
            chrome.contains("sandbox=\"allow-scripts allow-forms allow-popups allow-downloads\"")
        );
        assert!(!chrome.contains("allow-same-origin"));
        let body = r#"{"action":"approve","annotations":[],"notes":"","answers":[]}"#;
        let forged = request(address, format!("POST /s/{token}/feedback HTTP/1.1\r\nHost: {address}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}", body.len())).await;
        assert!(forged.starts_with("HTTP/1.1 403"));
        let valid = request(address, format!("POST /s/{token}/feedback HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nX-Vesper-Lens-Token: {token}\r\nContent-Length: {}\r\n\r\n{body}", body.len())).await;
        assert!(valid.starts_with("HTTP/1.1 200"));
        assert_eq!(
            session
                .next_feedback(Duration::from_secs(1))
                .await
                .unwrap()
                .action,
            super::super::Action::Approve
        );
    }

    #[tokio::test]
    async fn host_header_rebinding_is_rejected() {
        let session = start_review_session(
            ArtifactSource::inline("<html></html>"),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        let (address, token) = address_and_token(session.url());
        let response = request(
            address,
            format!("GET /s/{token} HTTP/1.1\r\nHost: attacker.example\r\n\r\n"),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 403"));
    }

    #[test]
    fn percent_decoding_and_mime_are_bounded() {
        assert_eq!(
            percent_decode("assets/my%20image.png").unwrap(),
            "assets/my image.png"
        );
        assert!(percent_decode("bad%2").is_err());
        assert_eq!(mime_for_path(Path::new("x.css")), "text/css");
    }

    #[test]
    fn sibling_assets_are_served_but_traversal_and_symlink_escapes_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let index = root.path().join("index.html");
        std::fs::write(&index, "<html></html>").unwrap();
        std::fs::write(root.path().join("styles.css"), "body{}").unwrap();
        let source = ArtifactSource::file(index).unwrap();
        let (bytes, mime) = source.asset("styles.css").unwrap().unwrap();
        assert_eq!(bytes, b"body{}");
        assert_eq!(mime, "text/css");
        assert!(source.asset("../secret.txt").is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
            std::os::unix::fs::symlink(
                outside.path().join("secret.txt"),
                root.path().join("escape.txt"),
            )
            .unwrap();
            assert!(source.asset("escape.txt").is_err());
        }
    }
}
