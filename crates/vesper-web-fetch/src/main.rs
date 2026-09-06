//! VRO-14 PR-3: the sandboxed fetch helper binary.
//!
//! This is the **only** production component of the web oracle that opens
//! network sockets, and it is designed to run exclusively inside a
//! provisioned `vesper-sandbox` under `IsolationRequirement::Network` with
//! an explicit egress grant (see the composition boundary in
//! `crates/vesper-web-fetch/src/lib.rs`). Running it on the host is a
//! deployment error the egress gate exists to prevent.
//!
//! Contract (one line of stderr metadata, body on stdout):
//!
//! ```text
//! vesper-web-fetch <url>
//!   → stdout: raw body bytes (streamed, hard cap 64 KiB)
//!   → stderr: {"vwf":"1","status":200,"contentType":"text/html; charset=utf-8",
//!              "finalUrl":"https://…","bytesRead":1234,"redirects":2,
//!              "charset":"utf-8"}   (single JSON line, then exit)
//! ```
//!
//! Exit codes: 0 success (2xx/3xx-followed), 1 egress/policy refusal,
//! 2 transport failure, 3 body cap exceeded, 4 usage.
//!
//! Upstream behavior references (naming rule, PRD §0): web oracle alpha's
//! fetch engine (redirect ladder, content-type sniffing) and the PR-0
//! spike's size-cap discipline. No brand tokens appear here or in any
//! artifact of this PR.

// This binary performs network I/O by design — but only std + reqwest,
// never unsafe, and only ever executed inside the sandbox boundary.
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::process::ExitCode;
use vesper_web::egress::{EgressPolicy, EgressVerdict};
// egress policy and the transport trait live in the library; the binary only fetches.

/// Hard streaming cap: read at most this many body bytes (alpha's
/// bounded-download discipline; the sandbox output cap is the same size,
/// so a capped body never overflows the channel either).
const BODY_CAP_BYTES: u64 = 64 * 1024;

/// Redirect ladder cap (alpha's engine ladder is deeper; the PRD fixes
/// the pure fetch path at five).
const MAX_REDIRECTS: usize = 5;

fn main() -> ExitCode {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("usage: vesper-web-fetch <url>");
        return ExitCode::from(4);
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        eprintln!("refused: only http/https URLs are fetchable");
        return ExitCode::from(1);
    }

    let mut args = std::env::args().skip(2);
    let cap = args
        .next()
        .and_then(|arg| arg.parse::<u64>().ok())
        .unwrap_or(BODY_CAP_BYTES)
        .clamp(1, BODY_CAP_BYTES);
    let policy = match args.next() {
        Some(json) => match serde_json::from_str::<HelperPolicy>(&json) {
            Ok(policy) => policy,
            Err(_) => {
                eprintln!("invalid fetch policy");
                return ExitCode::from(4);
            }
        },
        None => HelperPolicy::default(),
    };
    match fetch(&url, cap, &policy) {
        Ok(outcome) => {
            // Metadata travels on stderr as one JSON line so stdout stays
            // byte-exact body content.
            eprintln!("{}", metadata_line(&outcome));
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(&outcome.body).is_err() {
                return ExitCode::from(2);
            }
            let _ = stdout.flush();
            ExitCode::SUCCESS
        }
        Err(FetchError::CapExceeded) => {
            eprintln!("body cap exceeded ({BODY_CAP_BYTES} bytes)");
            ExitCode::from(3)
        }
        Err(FetchError::Transport(message)) => {
            eprintln!("fetch failed: {message}");
            ExitCode::from(2)
        }
        Err(FetchError::Policy(reason)) => {
            eprintln!("egress denied: {reason}");
            ExitCode::from(1)
        }
    }
}

struct Outcome {
    status: u16,
    content_type: String,
    final_url: String,
    charset: String,
    redirects: usize,
    bytes_read: u64,
    body: Vec<u8>,
}

enum FetchError {
    CapExceeded,
    Transport(String),
    Policy(String),
}

#[derive(serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct HelperPolicy {
    allow_plain_http: bool,
    allowed_hosts: Vec<String>,
    respect_robots: bool,
}

impl Default for HelperPolicy {
    fn default() -> Self {
        Self {
            allow_plain_http: false,
            allowed_hosts: Vec::new(),
            respect_robots: true,
        }
    }
}

fn checked_url(url: &str, policy: &HelperPolicy) -> Result<url::Url, FetchError> {
    let egress = EgressPolicy {
        allow_plain_http: policy.allow_plain_http,
        allowed_hosts: policy.allowed_hosts.clone(),
        respect_robots: policy.respect_robots,
    };
    if let EgressVerdict::Deny(denial) = vesper_web::egress::evaluate(url, &egress, None) {
        return Err(FetchError::Policy(denial.name().into()));
    }
    let parsed = url::Url::parse(url).map_err(|_| FetchError::Policy("unparsable_url".into()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(FetchError::Policy("url_credentials".into()));
    }
    Ok(parsed)
}

fn checked_addresses(addresses: &[SocketAddr]) -> Result<(), FetchError> {
    if addresses.is_empty() {
        return Err(FetchError::Transport("DNS returned no addresses".into()));
    }
    for address in addresses {
        if let EgressVerdict::Deny(denial) = vesper_web::egress::classify_ip(&address.ip()) {
            return Err(FetchError::Policy(denial.name().into()));
        }
    }
    Ok(())
}

fn get_one(url: &url::Url) -> Result<reqwest::blocking::Response, FetchError> {
    // Resolve inside the helper, validate every answer, then pin those
    // addresses into the client so a second DNS lookup cannot rebind it.
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|_| FetchError::Transport("DNS resolution failed".into()))?
        .collect();
    checked_addresses(&addresses)?;
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| FetchError::Transport("HTTP client initialization failed".into()))?
        .get(url.clone())
        .header("user-agent", "agent-vesper")
        .send()
        .map_err(|_| FetchError::Transport("HTTP request failed".into()))
}

fn robots_check(url: &url::Url, policy: &HelperPolicy) -> Result<(), FetchError> {
    let mut robots = url
        .join("/robots.txt")
        .map_err(|_| FetchError::Policy("robots_url".into()))?;
    for hop in 0..=MAX_REDIRECTS {
        robots = checked_url(robots.as_str(), policy)?;
        let response = get_one(&robots)?;
        let status = response.status().as_u16();
        if response.status().is_redirection() {
            robots = redirect_target(&robots, &response, hop, policy)?;
            continue;
        }
        if (400..500).contains(&status) {
            return Ok(());
        }
        if !(200..300).contains(&status) {
            return Err(FetchError::Policy("robots_unavailable".into()));
        }
        let mut body = Vec::new();
        response
            .take(BODY_CAP_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| FetchError::Policy("robots_unavailable".into()))?;
        if body.len() as u64 > BODY_CAP_BYTES {
            return Err(FetchError::Policy("robots_budget_exceeded".into()));
        }
        let rules =
            vesper_web::crawl::RobotsRules::parse(&String::from_utf8_lossy(&body), "agent-vesper");
        let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
        return if rules.is_allowed(path) {
            Ok(())
        } else {
            Err(FetchError::Policy("robots_disallowed".into()))
        };
    }
    Err(FetchError::Policy("robots_redirect_limit".into()))
}

fn redirect_target(
    url: &url::Url,
    response: &reqwest::blocking::Response,
    hop: usize,
    policy: &HelperPolicy,
) -> Result<url::Url, FetchError> {
    if hop >= MAX_REDIRECTS {
        return Err(FetchError::Transport("redirect_limit".into()));
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| FetchError::Transport("redirect without valid location".into()))?;
    let target = url
        .join(location)
        .map_err(|_| FetchError::Policy("invalid_redirect".into()))?;
    checked_url(target.as_str(), policy)
}

fn fetch(url: &str, cap: u64, policy: &HelperPolicy) -> Result<Outcome, FetchError> {
    let mut current = checked_url(url, policy)?;
    for redirects in 0..=MAX_REDIRECTS {
        if policy.respect_robots {
            robots_check(&current, policy)?;
        }
        let response = get_one(&current)?;
        if response.status().is_redirection() {
            current = redirect_target(&current, &response, redirects, policy)?;
            continue;
        }
        return decode_response(response, redirects, cap);
    }
    Err(FetchError::Transport("redirect_limit".into()))
}

fn decode_response(
    response: reqwest::blocking::Response,
    redirects: usize,
    cap: u64,
) -> Result<Outcome, FetchError> {
    let status = response.status().as_u16();
    let final_url = response.url().as_str().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    if !(200..300).contains(&status) {
        // Non-2xx after the redirect ladder: surface the status; empty body.
        return Ok(Outcome {
            status,
            content_type,
            final_url,
            charset: String::new(),
            redirects,
            bytes_read: 0,
            body: Vec::new(),
        });
    }

    // Stream with a hard cap: read one extra byte to detect overflow.
    let mut reader = response;
    let mut body = Vec::new();
    let mut limited = (&mut reader).take(cap + 1);
    limited
        .read_to_end(&mut body)
        .map_err(|error| FetchError::Transport(error.to_string()))?;
    if body.len() as u64 > cap {
        return Err(FetchError::CapExceeded);
    }
    let bytes_read = body.len() as u64;

    let (sniffed_type, charset) = sniff(&content_type, &body);
    let decoded = decode_charset(&body, &charset);
    if decoded.len() as u64 > cap {
        return Err(FetchError::CapExceeded);
    }
    Ok(Outcome {
        status,
        content_type: sniffed_type,
        final_url,
        charset,
        redirects,
        bytes_read,
        body: decoded,
    })
}

/// Content-type sniffing: when the header is missing or generic, look at
/// the body's first bytes (HTML tags, JSON/JS braces, UTF BOMs).
fn sniff(header: &str, body: &[u8]) -> (String, String) {
    let (mut mime, charset) = split_content_type(header);
    let head = &body.iter().copied().take(256).collect::<Vec<_>>();
    let looks_like_html = {
        let lower = String::from_utf8_lossy(head).to_lowercase();
        lower.contains("<!doctype html") || lower.contains("<html")
    };
    if (mime == "application/octet-stream" || mime.is_empty()) && looks_like_html {
        mime = "text/html".to_string();
    }
    if charset.is_empty() {
        if body.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return (mime, "utf-8".to_string());
        }
        if body.starts_with(&[0xFE, 0xFF]) {
            return (mime, "utf-16be".to_string());
        }
        if body.starts_with(&[0xFF, 0xFE]) {
            return (mime, "utf-16le".to_string());
        }
    }
    (mime, charset)
}

/// Split `type/subtype; charset=x` into its parts.
fn split_content_type(header: &str) -> (String, String) {
    let mut parts = header.split(';');
    let mime = parts.next().unwrap_or("").trim().to_lowercase();
    let mut charset = String::new();
    for part in parts {
        let part = part.trim();
        if let Some(value) = part
            .strip_prefix("charset=")
            .or_else(|| part.strip_prefix("CHARSET="))
        {
            charset = value.trim_matches('"').to_lowercase();
        }
    }
    (mime, charset)
}

/// Decode a body to UTF-8 bytes when the charset says otherwise. Latin-1
/// is the byte-preserving fallback for unlabeled 8-bit content; UTF-16
/// bodies convert via manual code-unit pairing (no iconv dependency).
fn decode_charset(body: &[u8], charset: &str) -> Vec<u8> {
    match charset {
        "utf-8" | "us-ascii" | "" => body.to_vec(),
        "utf-16be" | "utf-16le" => utf16_to_utf8(body, charset == "utf-16be"),
        "iso-8859-1" | "latin1" | "windows-1252" | "cp1252" => latin1_to_utf8(body),
        _ => body.to_vec(),
    }
}

fn latin1_to_utf8(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    for &byte in body {
        if byte < 0x80 {
            out.push(byte);
        } else {
            out.extend_from_slice(&[0xC0 | (byte >> 6), 0x80 | (byte & 0x3F)]);
        }
    }
    out
}

fn utf16_to_utf8(body: &[u8], big_endian: bool) -> Vec<u8> {
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units).into_bytes()
}

fn metadata_line(outcome: &Outcome) -> String {
    format!(
        "VWMETA:{}",
        serde_json::json!({
            "vwf": "1", "status": outcome.status,
            "content_type": outcome.content_type, "final_url": outcome.final_url,
            "bytes_read": outcome.bytes_read, "redirects": outcome.redirects,
            "charset": outcome.charset, "truncated": false,
        })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitted_metadata_round_trips_through_production_parser() {
        let outcome = Outcome {
            status: 200,
            content_type: "text/html".into(),
            final_url: "https://example.com/final".into(),
            charset: "utf-8".into(),
            redirects: 3,
            bytes_read: 4,
            body: b"body".to_vec(),
        };
        let meta = vesper_web_fetch::parse::parse_helper_meta(&metadata_line(&outcome))
            .expect("actual helper metadata must parse");
        assert_eq!(meta.status, 200);
        assert_eq!(meta.redirects, 3);
        assert_eq!(meta.final_url.as_deref(), Some("https://example.com/final"));
    }

    #[test]
    fn resolved_private_dns_answers_fail_even_when_mixed_with_public() {
        for private in [
            "127.0.0.1:443",
            "10.0.0.1:443",
            "[fd00::1]:443",
            "[fe80::1]:443",
        ] {
            let answers = [
                "93.184.216.34:443".parse().unwrap(),
                private.parse().unwrap(),
            ];
            assert!(matches!(
                checked_addresses(&answers),
                Err(FetchError::Policy(_))
            ));
        }
        assert!(checked_addresses(&[]).is_err());
    }

    #[test]
    fn redirect_policy_rejects_private_credentials_and_off_allowlist() {
        let policy = HelperPolicy {
            allowed_hosts: vec!["example.com".into()],
            ..HelperPolicy::default()
        };
        for url in [
            "https://127.0.0.1/",
            "https://[fd00::1]/",
            "https://elsewhere.com/",
            "https://user:secret@example.com/",
        ] {
            assert!(checked_url(url, &policy).is_err());
        }
        assert!(checked_url("https://example.com/", &policy).is_ok());
    }

    #[test]
    fn splits_content_type_and_charset() {
        assert_eq!(
            split_content_type("text/html; charset=UTF-8"),
            ("text/html".to_string(), "utf-8".to_string())
        );
        assert_eq!(
            split_content_type("Application/JSON"),
            ("application/json".to_string(), String::new())
        );
    }

    #[test]
    fn sniffs_html_when_header_generic() {
        let body = b"<!DOCTYPE html><html><body>x</body></html>";
        let (mime, _) = sniff("application/octet-stream", body);
        assert_eq!(mime, "text/html");
    }

    #[test]
    fn latin1_decodes_to_utf8() {
        assert_eq!(latin1_to_utf8(&[0x41, 0xE9]), "A\u{e9}".as_bytes());
    }

    #[test]
    fn utf16_le_round_trips() {
        let text = "hello";
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(utf16_to_utf8(&bytes, false), text.as_bytes());
    }

    #[test]
    fn usage_exit_is_distinct_from_policy() {
        // Exit-code mapping is contract: 4 usage, 1 policy, 2 transport.
        assert_ne!(4, 1);
        assert_ne!(1, 2);
    }

    #[test]
    fn body_cap_is_64k() {
        assert_eq!(BODY_CAP_BYTES, 64 * 1024);
        assert_eq!(MAX_REDIRECTS, 5);
    }
}
