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
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
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
    let Some(first) = request.lines().next() else {
        return;
    };
    let mut fields = first.split_whitespace();
    let method = fields.next().unwrap_or_default();
    let target = fields.next().unwrap_or_default();
    if method == "GET" && target.starts_with("/?pair=") {
        if target.trim_start_matches("/?pair=") == pair_token {
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", PWA_HTML);
        } else {
            respond(&mut stream, "404 Not Found", "text/plain", "Not found");
        }
        return;
    }
    if method == "GET" && target.starts_with("/pending?pair=") {
        if target.trim_start_matches("/pending?pair=") != pair_token {
            respond(&mut stream, "404 Not Found", "text/plain", "Not found");
            return;
        }
        let id = pending
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .filter(|approval| approval.expires >= Instant::now())
            .map(|approval| approval.id)
            .unwrap_or_default();
        respond(
            &mut stream,
            "200 OK",
            "application/json",
            &format!("{{\"approval\":\"{id}\"}}"),
        );
        return;
    }
    if method == "POST" && target.starts_with("/approve/") {
        let id = target.trim_start_matches("/approve/");
        let Some(body) = request.split("\r\n\r\n").nth(1) else {
            respond(&mut stream, "400 Bad Request", "text/plain", "Missing body");
            return;
        };
        let approved = match serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("approved").and_then(serde_json::Value::as_bool))
        {
            Some(value) => value,
            None => {
                respond(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain",
                    "Expected {\"approved\":true|false}",
                );
                return;
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
            respond(&mut stream, "200 OK", "application/json", "{\"ok\":true}");
        } else {
            respond(
                &mut stream,
                "404 Not Found",
                "text/plain",
                "Unknown approval",
            );
        }
        return;
    }
    respond(&mut stream, "404 Not Found", "text/plain", "Not found");
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
    fn loopback_server_resolves_one_short_lived_decision() {
        let server = MobileServer::start("127.0.0.1:0", false, None).unwrap();
        let pair_url = server.pairing_url().to_owned();
        let (origin, pair) = pair_url.split_once("/?pair=").unwrap();
        let approval = server.register_approval();
        let pending = http_request(
            origin,
            &format!("GET /pending?pair={pair} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        );
        assert!(pending.contains(&approval));
        let response = http_request(
            origin,
            &format!(
                "POST /approve/{approval} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 17\r\n\r\n{{\"approved\":true}}"
            ),
        );
        assert!(response.contains("200 OK"));
        let decision = (0..50)
            .find_map(|_| {
                let value = server.try_decision();
                if value.is_none() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                value
            })
            .expect("mobile decision");
        assert_eq!(decision.approval_id, approval);
        assert!(decision.approved);
    }

    #[test]
    fn malformed_decision_is_rejected_without_consuming_approval() {
        let server = MobileServer::start("127.0.0.1:0", false, None).unwrap();
        let (origin, _) = server.pairing_url().split_once("/?pair=").unwrap();
        let approval = server.register_approval();
        let malformed = http_request(
            origin,
            &format!(
                "POST /approve/{approval} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{{}}"
            ),
        );
        assert!(malformed.contains("400 Bad Request"));
        let valid = http_request(
            origin,
            &format!(
                "POST /approve/{approval} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 18\r\n\r\n{{\"approved\":false}}"
            ),
        );
        assert!(valid.contains("200 OK"));
        let decision = (0..50)
            .find_map(|_| {
                let value = server.try_decision();
                if value.is_none() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                value
            })
            .expect("mobile decision");
        assert!(!decision.approved);
    }

    fn http_request(origin: &str, request: &str) -> String {
        let address = origin.trim_start_matches("http://");
        for _ in 0..50 {
            if let Ok(mut stream) = TcpStream::connect(address) {
                stream.write_all(request.as_bytes()).unwrap();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut response = String::new();
                match stream.read_to_string(&mut response) {
                    Ok(_) => return response,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::ConnectionReset
                            && !response.is_empty() =>
                    {
                        return response;
                    }
                    Err(error) => panic!("mobile response read failed: {error}"),
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("mobile server did not accept connections");
    }
}
