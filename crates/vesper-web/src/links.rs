//! VRO-14 PR-2: link extraction + URL canonicalization.
//!
//! Extracts every `a[href]` from a parsed document and canonicalizes each
//! target against the page's base URL (the `url` crate's standards-aware
//! joining, not string concatenation). Canonicalization policy ports the
//! PRD §1.5/§1.6 contract: fragments are stripped, trailing slashes on the
//! path collapse, `default` ports drop, scheme+host lowercase, empty
//! queries drop, and non-web schemes (`mailto:`, `tel:`, `javascript:` …)
//! are excluded as `NonWeb` denials rather than silently kept.

use crate::dom::{Document, Element, Node};
use url::Url;

/// One extracted link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRecord {
    /// Canonicalized absolute URL (fragments stripped, trailing slash
    /// collapsed, default port dropped).
    pub url: String,
    /// Collapsed anchor text (trimmed, whitespace-run collapsed).
    pub text: String,
    /// `rel` attribute tokens, lowercased, deduplicated in source order.
    pub rel: Vec<String>,
    /// `nofollow` present in `rel`.
    pub nofollow: bool,
}

/// Extract all `a[href]` links, canonicalized against `base`.
///
/// Skips anchors without an `href` and empty results. Relative URLs are
/// resolved against `base` with standard URL-join semantics
/// (`/abs/path`, `rel/path`, `../up`, `?query`, protocol-relative
/// `//host/path` all behave per RFC 3986).
pub fn extract_links(document: &Document, base: &str) -> Vec<LinkRecord> {
    let mut out: Vec<LinkRecord> = Vec::new();
    walk(&document.root, &mut |anchor: &Element| {
        let Some(href) = anchor.attr("href") else {
            return;
        };
        let href = href.trim();
        if href.is_empty() {
            return;
        }
        let Some(canonical) = canonicalize(href, base) else {
            return;
        };
        let text = collapse_ws(&anchor.text());
        let rel = anchor
            .attr("rel")
            .map(|value| {
                value
                    .split_ascii_whitespace()
                    .map(str::to_ascii_lowercase)
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let nofollow = rel.iter().any(|token| token == "nofollow");
        out.push(LinkRecord {
            url: canonical,
            text,
            rel,
            nofollow,
        });
    });
    out
}

fn walk(element: &Element, visit: &mut impl FnMut(&Element)) {
    if element.tag == "a" {
        visit(element);
    }
    for child in &element.children {
        if let Node::Element(el) = child {
            walk(el, visit);
        }
    }
}

fn collapse_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out.trim().to_string()
}

/// Canonicalize `href` against `base`.
///
/// Returns `None` for non-web schemes (`mailto:`, `tel:`, `javascript:`,
/// `ftp:`, `file:`, `ssh:`, data URLs not needed by the perception
/// pipeline) — the caller records those as `CrawlDenial::NonWeb` rather
/// than dropping them silently (typed-denial discipline, PRD §1.6).
pub fn canonicalize(href: &str, base: &str) -> Option<String> {
    let mut parsed = url::Url::parse(base).ok()?;
    // Strip the fragment before joining so `page.html#section` resolves to
    // the page itself (the PRD treats section anchors as duplicates of the
    // base URL, matching alpha's SECTION_LINK denial).
    parsed.set_fragment(None);
    // `join` consumes `parsed` and returns the resolved URL; the fragment
    // must be cleared again because `href` may carry its own.
    let joined = parsed.join(href).ok()?;
    let joined = {
        let mut joined = joined;
        joined.set_fragment(None);
        joined
    };
    if !matches!(joined.scheme(), "http" | "https") {
        return None;
    }
    Some(normalize_url(&joined))
}

/// Normalize a joined URL: lowercase scheme+host (the `url` crate already
/// lowercases these), strip the fragment, collapse the trailing path
/// slash, drop the default port for the scheme, and drop an empty query.
pub fn normalize_url(target: &Url) -> String {
    let mut normalized = target.clone();
    normalized.set_fragment(None);
    // Empty query (`?`) is not meaningful; drop it.
    if normalized.query() == Some("") {
        normalized.set_query(None);
    }
    // Default ports are redundant.
    let default_port = matches!(
        (normalized.scheme(), normalized.port()),
        ("http", Some(80)) | ("https", Some(443))
    );
    if default_port {
        let _ = normalized.set_port(None);
    }
    // Trailing slash on the root or any path segment: `/docs/` and `/docs`
    // are the same resource. Keep the root slash (`/`) as-is.
    let path = normalized.path().to_string();
    if path.len() > 1 && path.ends_with('/') {
        let trimmed = path.trim_end_matches('/');
        normalized.set_path(if trimmed.is_empty() { "/" } else { trimmed });
    }
    normalized.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse;

    const BASE: &str = "https://example.com/docs/page.html";

    #[test]
    fn extracts_relative_absolute_and_query_links() {
        let doc = parse(
            "<body>\
             <a href='../about'>About</a>\
             <a href='/top'>Top</a>\
             <a href='section2.html'>Next</a>\
             <a href='?page=2'>Page 2</a>\
             <a href='https://other.org/x'>Other</a>\
             </body>",
        );
        let links = extract_links(&doc, BASE);
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://example.com/about",
                "https://example.com/top",
                "https://example.com/docs/section2.html",
                "https://example.com/docs/page.html?page=2",
                "https://other.org/x",
            ]
        );
    }

    #[test]
    fn canonicalization_strips_fragments() {
        let doc = parse("<a href='page.html#section'>jump</a>");
        let links = extract_links(&doc, BASE);
        assert_eq!(links[0].url, "https://example.com/docs/page.html");
    }

    #[test]
    fn trailing_slash_collapses() {
        assert_eq!(
            normalize_url(&url::Url::parse("https://example.com/docs/").unwrap()),
            "https://example.com/docs"
        );
        assert_eq!(
            normalize_url(&url::Url::parse("https://example.com/").unwrap()),
            "https://example.com/"
        );
    }

    #[test]
    fn default_port_drops() {
        assert_eq!(
            normalize_url(&url::Url::parse("https://example.com:443/x").unwrap()),
            "https://example.com/x"
        );
        assert_eq!(
            normalize_url(&url::Url::parse("http://example.com:8080/x").unwrap()),
            "http://example.com:8080/x"
        );
    }

    #[test]
    fn non_web_schemes_are_rejected() {
        assert!(canonicalize("mailto:a@b.c", BASE).is_none());
        assert!(canonicalize("javascript:alert(1)", BASE).is_none());
        assert!(canonicalize("tel:+1555", BASE).is_none());
        assert!(canonicalize("ftp://x/y", BASE).is_none());
    }

    #[test]
    fn anchors_without_href_are_skipped() {
        let doc = parse("<a name='x'>anchor</a><a href='y.html'>real</a>");
        let links = extract_links(&doc, BASE);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com/docs/y.html");
    }

    #[test]
    fn rel_and_noflow_are_captured() {
        let doc = parse("<a href='x.html' rel='external nofollow'>x</a>");
        let links = extract_links(&doc, BASE);
        assert!(links[0].nofollow);
        assert_eq!(links[0].rel, vec!["external", "nofollow"]);
    }

    #[test]
    fn protocol_relative_links_resolve() {
        let doc = parse("<a href='//cdn.example.org/lib.js'>cdn</a>");
        let links = extract_links(&doc, BASE);
        assert_eq!(links[0].url, "https://cdn.example.org/lib.js");
    }

    #[test]
    fn link_text_collapses_whitespace() {
        let doc = parse("<a href='x.html'>\n  spaced   out \n text </a>");
        let links = extract_links(&doc, BASE);
        assert_eq!(links[0].text, "spaced out text");
    }
}
