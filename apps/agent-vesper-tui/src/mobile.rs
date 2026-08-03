//! Credential-free mobile approval companion.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use qrcode::{QrCode, render::unicode::Dense1x2};
use rand::{RngCore, rngs::OsRng};

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_ADVERTISED_URL_BYTES: usize = 2 * 1024;
const APPROVAL_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileDecision {
    pub approval_id: String,
    pub approved: bool,
}

#[derive(Debug, Clone)]
struct PendingApproval {
    id: String,
    expires: Instant,
}

pub struct MobileServer {
    stop: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<PendingApproval>>>,
    decisions: mpsc::Receiver<MobileDecision>,
    thread: Option<JoinHandle<()>>,
    pairing_url: String,
    phone_reachable: bool,
}

impl MobileServer {
    pub fn start_from_environment() -> Result<Self, String> {
        let bind =
            std::env::var("AGENT_VESPER_MOBILE_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into());
        let allow_public = std::env::var("AGENT_VESPER_MOBILE_ALLOW_PUBLIC").as_deref() == Ok("1");
        let public_url = std::env::var("AGENT_VESPER_MOBILE_URL").ok();
        Self::start(&bind, allow_public, public_url.as_deref())
    }

    pub fn start(bind: &str, allow_public: bool, public_url: Option<&str>) -> Result<Self, String> {
        let host = bind
            .rsplit_once(':')
            .map(|(host, _)| host)
            .ok_or_else(|| "mobile bind must be host:port".to_string())?;
        if !matches!(host, "127.0.0.1" | "localhost" | "0.0.0.0") {
            return Err("mobile bind must be loopback or explicitly acknowledged 0.0.0.0".into());
        }
        let public = host == "0.0.0.0";
        if public && !allow_public {
            return Err("refusing 0.0.0.0 without AGENT_VESPER_MOBILE_ALLOW_PUBLIC=1".into());
        }
        let listener = TcpListener::bind(bind).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let advertised = if public {
            public_url
                .ok_or_else(|| "public mobile bind requires AGENT_VESPER_MOBILE_URL".to_string())?
                .trim_end_matches('/')
                .to_owned()
        } else {
            format!("http://127.0.0.1:{}", address.port())
        };
        if !advertised.starts_with("http://") && !advertised.starts_with("https://") {
            return Err("AGENT_VESPER_MOBILE_URL must start with http:// or https://".into());
        }
        if advertised.len() > MAX_ADVERTISED_URL_BYTES {
            return Err("AGENT_VESPER_MOBILE_URL is too long".into());
        }
        let pair_token = random_token();
        let pairing_url = format!("{advertised}/?pair={pair_token}");
        let stop = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(Mutex::new(None));
        let (decision_tx, decisions) = mpsc::channel();
        let thread_stop = Arc::clone(&stop);
        let thread_pending = Arc::clone(&pending);
        let thread_pair = pair_token.clone();
        let thread = std::thread::Builder::new()
            .name("vesper-mobile-approval".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_connection(stream, &thread_pair, &thread_pending, &decision_tx)
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock
                                    | std::io::ErrorKind::Interrupted
                                    | std::io::ErrorKind::ConnectionAborted
                            ) =>
                        {
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            stop,
            pending,
            decisions,
            thread: Some(thread),
            pairing_url,
            phone_reachable: public,
        })
    }

    pub fn pairing_url(&self) -> &str {
        &self.pairing_url
    }

    pub fn pairing_qr(&self) -> Option<String> {
        self.phone_reachable
            .then(|| QrCode::new(self.pairing_url.as_bytes()).ok())
            .flatten()
            .map(|code| {
                code.render::<Dense1x2>()
                    .dark_color(Dense1x2::Light)
                    .light_color(Dense1x2::Dark)
                    .build()
            })
    }

    pub fn register_approval(&self) -> String {
        let id = random_token();
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(PendingApproval {
                id: id.clone(),
                expires: Instant::now() + APPROVAL_TTL,
            });
        }
        id
    }

    pub fn clear_approval(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = None;
        }
    }

    pub fn try_decision(&self) -> Option<MobileDecision> {
        self.decisions.try_recv().ok()
    }
}

impl Drop for MobileServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn random_token() -> String {
    let mut bytes = [0_u8; 24];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn handle_connection(
    mut stream: TcpStream,
    pair_token: &str,
    pending: &Mutex<Option<PendingApproval>>,
    decisions: &mpsc::Sender<MobileDecision>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let Ok(bytes) = read_request(&mut stream) else {
        return;
    };
    let Ok(request) = std::str::from_utf8(&bytes) else {
        return;
    };
    let response = route_request(request, pair_token, pending, decisions);
    respond(
        &mut stream,
        response.status,
        response.content_type,
        &response.body,
    );
}

struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

fn route_request(
    request: &str,
    pair_token: &str,
    pending: &Mutex<Option<PendingApproval>>,
    decisions: &mpsc::Sender<MobileDecision>,
) -> HttpResponse {
    let Some(first) = request.lines().next() else {
        return text_response("400 Bad Request", "Missing request line");
    };
    let mut fields = first.split_whitespace();
    let method = fields.next().unwrap_or_default();
    let target = fields.next().unwrap_or_default();
    if method == "GET" && target.starts_with("/?pair=") {
        if target.trim_start_matches("/?pair=") == pair_token {
            return HttpResponse {
                status: "200 OK",
                content_type: "text/html; charset=utf-8",
                body: PWA_HTML.to_owned(),
            };
        } else {
            return text_response("404 Not Found", "Not found");
        }
    }
    if method == "GET" && target.starts_with("/pending?pair=") {
        if target.trim_start_matches("/pending?pair=") != pair_token {
            return text_response("404 Not Found", "Not found");
        }
        let id = pending
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .filter(|approval| approval.expires >= Instant::now())
            .map(|approval| approval.id)
            .unwrap_or_default();
        return HttpResponse {
            status: "200 OK",
            content_type: "application/json",
            body: format!("{{\"approval\":\"{id}\"}}"),
        };
    }
    if method == "POST" && target.starts_with("/approve/") {
        let id = target.trim_start_matches("/approve/");
        let Some(body) = request.split("\r\n\r\n").nth(1) else {
            return text_response("400 Bad Request", "Missing body");
        };
        let approved = match serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("approved").and_then(serde_json::Value::as_bool))
        {
            Some(value) => value,
            None => {
                return text_response("400 Bad Request", "Expected {\"approved\":true|false}");
            }
        };
        let valid = pending
            .lock()
            .ok()
            .and_then(|mut guard| {
                let value = guard.take()?;
                (value.id == id && value.expires >= Instant::now()).then_some(value)
            })
            .is_some();
        if valid {
            let _ = decisions.send(MobileDecision {
                approval_id: id.to_owned(),
                approved,
            });
            return HttpResponse {
                status: "200 OK",
                content_type: "application/json",
                body: "{\"ok\":true}".into(),
            };
        } else {
            return text_response("404 Not Found", "Unknown approval");
        }
    }
    text_response("404 Not Found", "Not found")
}

fn text_response(status: &'static str, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type: "text/plain",
        body: body.to_owned(),
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let size = stream.read(&mut chunk)?;
        if size == 0 {
            break;
        }
        if request.len() + size > MAX_REQUEST_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "mobile request exceeds bound",
            ));
        }
        request.extend_from_slice(&chunk[..size]);
        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let body_start = header_end + 4;
            let headers = std::str::from_utf8(&request[..header_end]).unwrap_or_default();
            let content_length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            });
            if content_length.is_none_or(|length| request.len() >= body_start + length) {
                break;
            }
        }
    }
    Ok(request)
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let mut drain = [0_u8; 256];
    loop {
        match stream.read(&mut drain) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
}

const PWA_HTML: &str = r#"<!doctype html><meta name="viewport" content="width=device-width"><title>Agent Vesper Approval</title><style>body{font:18px system-ui;max-width:36rem;margin:3rem auto;padding:1rem;background:#0b1017;color:#d9e7f5}button{font-size:1.2rem;padding:1rem;margin:.5rem}</style><h1>Agent Vesper</h1><p id="state">Waiting for an approval…</p><button id="allow" disabled>Allow once</button><button id="deny" disabled>Deny</button><script>const pair=new URLSearchParams(location.search).get('pair');let current='';async function poll(){const r=await fetch('/pending?pair='+encodeURIComponent(pair));if(r.ok){const j=await r.json();current=j.approval||'';state.textContent=current?'Approval requested':'Waiting for an approval…';allow.disabled=deny.disabled=!current}setTimeout(poll,1000)}async function decide(approved){if(!current)return;await fetch('/approve/'+encodeURIComponent(current),{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({approved})});current='';allow.disabled=deny.disabled=true}allow.onclick=()=>decide(true);deny.onclick=()=>decide(false);poll()</script>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_bind_fails_closed_without_acknowledgement() {
        assert!(MobileServer::start("0.0.0.0:0", false, None).is_err());
    }

    #[test]
    fn loopback_pairing_does_not_claim_a_phone_scannable_qr() {
        let server = MobileServer::start("127.0.0.1:0", false, None).unwrap();
        assert!(server.pairing_qr().is_none());
    }

    #[test]
    fn acknowledged_public_bind_renders_a_pairing_qr() {
        let server = MobileServer::start("0.0.0.0:0", true, Some("http://192.0.2.1:8765")).unwrap();
        let qr = server.pairing_qr().expect("public pairing QR");
        assert!(!qr.trim().is_empty());
        assert!(
            server
                .pairing_url()
                .starts_with("http://192.0.2.1:8765/?pair=")
        );
    }

    #[test]
    fn loopback_server_accepts_a_local_connection() {
        let server = MobileServer::start("127.0.0.1:0", false, None).unwrap();
        let (origin, _) = server.pairing_url().split_once("/?pair=").unwrap();
        TcpStream::connect(origin.trim_start_matches("http://")).unwrap();
    }

    #[test]
    fn request_router_resolves_one_short_lived_decision() {
        let approval = "approval-one";
        let pending = Mutex::new(Some(PendingApproval {
            id: approval.into(),
            expires: Instant::now() + APPROVAL_TTL,
        }));
        let (decisions, received) = mpsc::channel();
        let listed = route_request(
            "GET /pending?pair=pair HTTP/1.1\r\nHost: localhost\r\n\r\n",
            "pair",
            &pending,
            &decisions,
        );
        assert_eq!(listed.status, "200 OK");
        assert!(listed.body.contains(approval));
        let response = route_request(
            &format!(
                "POST /approve/{approval} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 17\r\n\r\n{{\"approved\":true}}"
            ),
            "pair",
            &pending,
            &decisions,
        );
        assert_eq!(response.status, "200 OK");
        let decision = received.try_recv().expect("mobile decision");
        assert_eq!(decision.approval_id, approval);
        assert!(decision.approved);
    }

    #[test]
    fn malformed_decision_is_rejected_without_consuming_approval() {
        let approval = "approval-two";
        let pending = Mutex::new(Some(PendingApproval {
            id: approval.into(),
            expires: Instant::now() + APPROVAL_TTL,
        }));
        let (decisions, received) = mpsc::channel();
        let malformed = route_request(
            &format!(
                "POST /approve/{approval} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{{}}"
            ),
            "pair",
            &pending,
            &decisions,
        );
        assert_eq!(malformed.status, "400 Bad Request");
        assert!(pending.lock().unwrap().is_some());
        let valid = route_request(
            &format!(
                "POST /approve/{approval} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 18\r\n\r\n{{\"approved\":false}}"
            ),
            "pair",
            &pending,
            &decisions,
        );
        assert_eq!(valid.status, "200 OK");
        let decision = received.try_recv().expect("mobile decision");
        assert!(!decision.approved);
        assert!(pending.lock().unwrap().is_none());
    }
}
