//! VWMETA parsing: the helper's single-line JSON status report on stderr.
//!
//! Contract: the LAST line of the helper's stderr that starts with
//! `VWMETA:` is a JSON object with the keys `status` (integer),
//! `content_type` (optional string), `charset` (optional string),
//! `final_url` (optional string), `redirects` (integer), `truncated`
//! (boolean). Unknown keys are ignored (forward compatibility).

use serde::Deserialize;

/// Parsed helper metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HelperMeta {
    pub status: u16,
    pub content_type: Option<String>,
    pub charset: Option<String>,
    pub final_url: Option<String>,
    pub redirects: u32,
    pub truncated: bool,
}

/// The marker prefix the helper writes before its JSON status line.
pub const VWMETA_PREFIX: &str = "VWMETA:";

/// Parse the metadata line from the helper's stderr.
///
/// Returns `None` when no `VWMETA:` line is present (a helper crash or
/// misuse of the binary yields no metadata; the caller maps that to a
/// typed fetch failure).
pub fn parse_helper_meta(stderr: &str) -> Option<HelperMeta> {
    let mut meta: Option<HelperMeta> = None;
    for line in stderr.lines() {
        let Some(json_part) = line.strip_prefix(VWMETA_PREFIX) else {
            continue;
        };
        if let Some(parsed) = parse_one(json_part) {
            meta = Some(parsed);
        }
    }
    meta
}

/// Deserialize one candidate JSON payload (tolerant of unknown keys).
fn parse_one(json_part: &str) -> Option<HelperMeta> {
    #[derive(Deserialize)]
    struct Raw {
        status: Option<u16>,
        content_type: Option<String>,
        charset: Option<String>,
        final_url: Option<String>,
        redirects: Option<u32>,
        truncated: Option<bool>,
    }
    let raw: Raw = serde_json::from_str(json_part.trim()).ok()?;
    // Charset: prefer the explicit key, else extract from the
    // content_type parameter (`text/html; charset=utf-8`) — the helper
    // may report either form.
    let charset = raw
        .charset
        .or_else(|| raw.content_type.as_deref().and_then(split_charset));
    Some(HelperMeta {
        status: raw.status.unwrap_or(0),
        content_type: raw.content_type,
        charset,
        final_url: raw.final_url,
        redirects: raw.redirects.unwrap_or(0),
        truncated: raw.truncated.unwrap_or(false),
    })
}

/// Extract a `charset=` parameter from a content-type header value.
fn split_charset(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|parameter| {
        let parameter = parameter.trim();
        let value = parameter.strip_prefix("charset=")?;
        let value = value.trim().trim_matches('"');
        if value.is_empty() {
            None
        } else {
            Some(value.to_ascii_lowercase())
        }
    })
}

/// Whether a status code counts as success for body purposes.
#[must_use]
pub fn status_is_ok(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Extract the MIME type (before any `;` parameter) from a Content-Type.
#[must_use]
pub fn mime_of(content_type: Option<&str>) -> Option<String> {
    let ct = content_type?;
    let mime = ct.split(';').next().unwrap_or("").trim();
    if mime.is_empty() {
        None
    } else {
        Some(mime.to_ascii_lowercase())
    }
}

/// Build a [`FetchResponse`] from the helper's parsed metadata and body.
pub fn response_from_parts(
    meta: HelperMeta,
    fallback_url: &str,
    body: String,
    truncated: bool,
) -> Result<vesper_web::transport::FetchResponse, String> {
    if !status_is_ok(meta.status) {
        return Err(format!("HTTP status {}", meta.status));
    }
    Ok(vesper_web::transport::FetchResponse {
        status: meta.status,
        url: meta.final_url.unwrap_or_else(|| fallback_url.to_string()),
        body,
        content_type: meta
            .content_type
            .unwrap_or_else(|| "application/octet-stream".to_string()),
        truncated: truncated || meta.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_meta_line() {
        let stderr = "noise\nVWMETA:{\"status\":200,\"content_type\":\"text/html; charset=utf-8\",\"final_url\":\"https://x.invalid/a\",\"redirects\":1,\"truncated\":false}\nmore noise";
        let meta = parse_helper_meta(stderr).expect("meta present");
        assert_eq!(meta.status, 200);
        assert_eq!(meta.charset.as_deref(), Some("utf-8"));
        assert_eq!(meta.redirects, 1);
        assert!(!meta.truncated);
    }

    #[test]
    fn last_meta_line_wins() {
        let stderr = "VWMETA:{\"status\":301}\nVWMETA:{\"status\":200}";
        let meta = parse_helper_meta(stderr).expect("meta present");
        assert_eq!(meta.status, 200);
    }

    #[test]
    fn no_meta_line_is_none() {
        assert!(parse_helper_meta("plain crash output").is_none());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let stderr = "VWMETA:{\"status\":200,\"future_field\":42}";
        let meta = parse_helper_meta(stderr).expect("meta present");
        assert_eq!(meta.status, 200);
    }

    #[test]
    fn status_bounds_and_mime() {
        assert!(status_is_ok(200));
        assert!(status_is_ok(299));
        assert!(!status_is_ok(300));
        assert!(!status_is_ok(199));
        assert_eq!(
            mime_of(Some("Text/HTML; charset=latin1")).as_deref(),
            Some("text/html")
        );
        assert!(mime_of(Some("  ")).is_none());
        assert!(mime_of(None).is_none());
    }
}
