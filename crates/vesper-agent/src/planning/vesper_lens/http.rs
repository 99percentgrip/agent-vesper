//! Pure HTTP/1.1 request parser and response builders (ADR 0017).
//!
//! Everything in this module is a pure function operating on byte slices —
//! no network, no async, no `tokio`. This is what makes the parser
//! unit-testable without a real `TcpListener` (PRD §4: "mock the TCP byte
//! streams"). The async server in [`super::server`] is a thin wrapper that
//! reads bytes off a `TcpStream` and feeds them here.

use std::collections::HashMap;

/// A successfully parsed HTTP/1.1 request.
///
/// Headers are stored in a `HashMap` keyed by lowercased header name. The
/// parser is lenient about header value case (per RFC 9110 §5.5) but
/// strict about the request line and the `\r\n` line terminators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    /// HTTP method (`GET`, `POST`, ...), uppercased.
    pub method: String,
    /// Request target (path + optional query), e.g. `/feedback`.
    pub path: String,
    /// All headers, lowercased names, original-cased values.
    pub headers: HashMap<String, String>,
    /// Decoded body bytes. Empty for typical GETs.
    pub body: Vec<u8>,
}

impl ParsedRequest {
    /// Returns the value of `Content-Length`, parsed as `usize`, if present
    /// and well-formed.
    pub fn content_length(&self) -> Option<usize> {
        self.headers
            .get("content-length")
            .and_then(|v| v.trim().parse::<usize>().ok())
    }
}

/// Errors raised by [`try_parse_request`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The request line is missing or malformed (e.g. not three
    /// space-separated tokens, or unsupported HTTP version).
    #[error("malformed request line: {0}")]
    MalformedRequestLine(String),
    /// A header line lacks a `:` separator.
    #[error("malformed header line: {0}")]
    MalformedHeader(String),
    /// A header value contained a NUL byte or other non-UTF-8 sequence.
    #[error("header line was not valid UTF-8: {0}")]
    NonUtf8Header(String),
    /// The declared `Content-Length` was not a non-negative integer.
    #[error("invalid content-length: {0}")]
    InvalidContentLength(String),
}

/// Try to parse a complete HTTP/1.1 request from `buf`.
///
/// Returns:
/// - `Ok(Some(request))` if `buf` contains a complete, well-formed request
///   (header block terminator `\r\n\r\n` present AND, when a body is
///   declared, all body bytes present).
/// - `Ok(None)` if `buf` does not yet contain a complete request — the
///   caller should read more bytes and retry. This is the "needs more data"
///   path that lets the streaming reader avoid hanging.
/// - `Err(...)` if `buf` contains something that is not HTTP/1.1.
pub fn try_parse_request(buf: &[u8]) -> Result<Option<ParsedRequest>, ParseError> {
    // Locate the end of the header block.
    let Some(header_end) = find_subsequence(buf, b"\r\n\r\n") else {
        return Ok(None);
    };
    let header_block = &buf[..header_end];
    let header_str = std::str::from_utf8(header_block)
        .map_err(|_| ParseError::NonUtf8Header("<header block>".into()))?;
    let mut lines = header_str.split("\r\n");

    let request_line = lines
        .next()
        .ok_or_else(|| ParseError::MalformedRequestLine("empty header block".into()))?;
    let mut rl_parts = request_line.splitn(3, ' ');
    let method = rl_parts
        .next()
        .ok_or_else(|| ParseError::MalformedRequestLine(request_line.into()))?;
    let path = rl_parts
        .next()
        .ok_or_else(|| ParseError::MalformedRequestLine(request_line.into()))?;
    let version = rl_parts
        .next()
        .ok_or_else(|| ParseError::MalformedRequestLine(request_line.into()))?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(ParseError::MalformedRequestLine(format!(
            "unsupported HTTP version: {version}"
        )));
    }
    // Per RFC 9110 method names are uppercase tokens; we accept and normalize.
    let method = method.to_ascii_uppercase();

    let mut headers: HashMap<String, String> = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| ParseError::MalformedHeader(line.into()))?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name.is_empty() {
            return Err(ParseError::MalformedHeader(line.into()));
        }
        // Validate against NUL/control bytes in values (defensive — the
        // browser will not send these, but a fuzzer might).
        if value.bytes().any(|b| b == 0) {
            return Err(ParseError::NonUtf8Header(name));
        }
        headers.insert(name, value);
    }

    // Body handling: if Content-Length is declared, the body starts at
    // header_end + 4 and must have at least that many bytes.
    let body_start = header_end + 4;
    let body = if let Some(cl_str) = headers.get("content-length") {
        let cl: usize = cl_str
            .trim()
            .parse()
            .map_err(|_| ParseError::InvalidContentLength(cl_str.clone()))?;
        if buf.len() < body_start + cl {
            // Header block complete but body still streaming in.
            return Ok(None);
        }
        buf[body_start..body_start + cl].to_vec()
    } else {
        Vec::new()
    };

    Ok(Some(ParsedRequest {
        method,
        path: path.to_string(),
        headers,
        body,
    }))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Build a minimal HTTP/1.1 response with a UTF-8 text/html body.
///
/// Used by the GET handler to serve the injected artifact.
pub fn build_html_response(html: &str) -> Vec<u8> {
    build_response("200 OK", "text/html; charset=utf-8", html.as_bytes())
}

/// Build a minimal HTTP/1.1 JSON response. Used for the POST /feedback
/// acknowledgement.
pub fn build_json_response(status: u16, json_body: &str) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        410 => "Gone",
        413 => "Content Too Large",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let status_line = format!("{status} {reason}");
    build_response(&status_line, "application/json", json_body.as_bytes())
}

pub(crate) fn build_response(status_and_reason: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    build_response_with_headers(status_and_reason, content_type, body, &[])
}

pub(crate) fn build_response_with_headers(
    status_and_reason: &str,
    content_type: &str,
    body: &[u8],
    extra_headers: &[(&str, &str)],
) -> Vec<u8> {
    // Cap arbitrary content-type with charset where it makes sense.
    let ct = if content_type.starts_with("text/") || content_type.starts_with("application/") {
        if content_type.contains("charset") {
            content_type.to_string()
        } else {
            format!("{content_type}; charset=utf-8")
        }
    } else {
        content_type.to_string()
    };
    let mut head = format!(
        "HTTP/1.1 {status_and_reason}\r\n\
         Content-Type: {ct}\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n",
        len = body.len()
    );
    for (name, value) in extra_headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut out = Vec::with_capacity(head.len() + body.len());
    out.extend_from_slice(head.as_bytes());
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_root() -> &'static [u8] {
        b"GET / HTTP/1.1\r\nHost: 127.0.0.1:54321\r\nUser-Agent: test\r\n\r\n"
    }

    #[test]
    fn parses_simple_get() {
        let req = try_parse_request(get_root()).unwrap().unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/");
        assert_eq!(req.headers.get("host").unwrap(), "127.0.0.1:54321");
        assert_eq!(req.headers.get("user-agent").unwrap(), "test");
        assert!(req.body.is_empty());
    }

    #[test]
    fn returns_none_when_header_block_incomplete() {
        let partial = b"GET / HTTP/1.1\r\nHost: x\r\n";
        assert!(try_parse_request(partial).unwrap().is_none());
    }

    #[test]
    fn returns_none_when_body_still_streaming() {
        let head = b"POST /feedback HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc";
        assert!(try_parse_request(head).unwrap().is_none());
    }

    #[test]
    fn parses_post_with_body_exact_length() {
        let body = r#"{"action":"approve"}"#;
        let full = format!(
            "POST /feedback HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let req = try_parse_request(full.as_bytes()).unwrap().unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/feedback");
        assert_eq!(req.content_length(), Some(body.len()));
        assert_eq!(req.body, body.as_bytes());
    }

    #[test]
    fn parses_post_with_zero_body() {
        let full = b"POST /feedback HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        let req = try_parse_request(full).unwrap().unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.body.len(), 0);
    }

    #[test]
    fn normalizes_header_names_to_lowercase() {
        let raw = b"GET / HTTP/1.1\r\nCONTENT-TYPE: Text/HTML\r\nX-Custom: v\r\n\r\n";
        let req = try_parse_request(raw).unwrap().unwrap();
        assert!(req.headers.contains_key("content-type"));
        assert!(req.headers.contains_key("x-custom"));
        // Value case preserved.
        assert_eq!(req.headers.get("content-type").unwrap(), "Text/HTML");
    }

    #[test]
    fn uppercases_method() {
        let raw = b"get / HTTP/1.1\r\nHost: x\r\n\r\n";
        let req = try_parse_request(raw).unwrap().unwrap();
        assert_eq!(req.method, "GET");
    }

    #[test]
    fn rejects_bad_http_version() {
        let raw = b"GET / HTTP/2.0\r\nHost: x\r\n\r\n";
        let err = try_parse_request(raw).unwrap_err();
        assert!(matches!(err, ParseError::MalformedRequestLine(_)));
    }

    #[test]
    fn rejects_malformed_header_line_without_colon() {
        let raw = b"GET / HTTP/1.1\r\nBadHeaderLine\r\nHost: x\r\n\r\n";
        assert!(matches!(
            try_parse_request(raw).unwrap_err(),
            ParseError::MalformedHeader(_)
        ));
    }

    #[test]
    fn rejects_non_numeric_content_length() {
        let raw = b"POST /x HTTP/1.1\r\nContent-Length: abc\r\n\r\n";
        assert!(matches!(
            try_parse_request(raw).unwrap_err(),
            ParseError::InvalidContentLength(_)
        ));
    }

    #[test]
    fn build_html_response_is_valid_http11() {
        let body = "<html>hi</html>";
        let bytes = build_html_response(body);
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(s.contains(&format!("Content-Length: {}\r\n", body.len())));
        assert!(s.contains("Connection: close\r\n"));
        assert!(s.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(s.ends_with(body));
    }

    #[test]
    fn build_json_response_has_correct_status() {
        let bytes = build_json_response(200, "{\"ok\":true}");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Type: application/json; charset=utf-8\r\n"));
        assert!(s.ends_with("{\"ok\":true}"));
    }

    #[test]
    fn empty_buffer_returns_none_not_error() {
        // Important: an empty buffer must not be treated as a malformed
        // request — the caller is just at the start of a read.
        assert!(try_parse_request(b"").unwrap().is_none());
    }

    #[test]
    fn handles_extra_whitespace_in_header_values() {
        let raw = b"GET / HTTP/1.1\r\nHost:    spaced.example   \r\n\r\n";
        let req = try_parse_request(raw).unwrap().unwrap();
        assert_eq!(req.headers.get("host").unwrap(), "spaced.example");
    }
}
