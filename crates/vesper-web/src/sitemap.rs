//! Bounded sitemap discovery primitives; all fetching/decompression is owned
//! by the sandbox transport. External entities are never resolved.
use quick_xml::{Reader, events::Event};

/// Maximum documents traversed during one sitemap discovery operation.
pub const SITEMAP_LIMIT: usize = 25;
/// Maximum links retained from all sitemaps by the composition layer.
pub const URL_LIMIT: usize = 10_000;

/// One parsed sitemap or sitemap index.
#[derive(Debug, PartialEq, Eq)]
pub struct Sitemap {
    /// `true` means locations reference child sitemap documents.
    pub is_index: bool,
    /// Absolute, normalized http(s) locations in source order.
    pub locations: Vec<String>,
}

/// Extract Sitemap directives independently of user-agent rule groups.
pub fn robots_sitemaps(robots: &str, base: &str) -> Vec<String> {
    robots
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split('#').next()?.split_once(':')?;
            if !key.trim().eq_ignore_ascii_case("sitemap") {
                return None;
            }
            crate::links::canonicalize(value.trim(), base)
        })
        .take(SITEMAP_LIMIT)
        .collect()
}

/// Strict XML parsing with byte, nesting and URL bounds.
pub fn parse(xml: &str, base: &str) -> Result<Sitemap, &'static str> {
    if xml.len() > 512 * 1024 {
        return Err("sitemap_budget_exceeded");
    }
    let mut reader = Reader::from_str(xml);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut out = Sitemap {
        is_index: false,
        locations: Vec::new(),
    };
    let mut root_seen = false;
    let mut location = String::new();
    loop {
        match reader.read_event().map_err(|_| "invalid_sitemap_xml")? {
            Event::Empty(tag) if stack.is_empty() => {
                let name = tag.local_name();
                if root_seen || !matches!(name.as_ref(), b"sitemapindex" | b"urlset") {
                    return Err("invalid_sitemap_root");
                }
                root_seen = true;
                out.is_index = name.as_ref() == b"sitemapindex";
            }
            Event::Start(tag) => {
                let name = tag.local_name().as_ref().to_vec();
                if stack.is_empty() {
                    if root_seen || !matches!(name.as_slice(), b"sitemapindex" | b"urlset") {
                        return Err("invalid_sitemap_root");
                    }
                    root_seen = true;
                    out.is_index = name == b"sitemapindex";
                }
                if stack.len() >= 8 {
                    return Err("sitemap_depth_exceeded");
                }
                if name == b"loc" {
                    location.clear();
                }
                stack.push(name);
            }
            Event::Text(text) if stack.last().is_some_and(|name| name == b"loc") => {
                location.push_str(&text.decode().map_err(|_| "invalid_sitemap_text")?);
            }
            Event::GeneralRef(reference) if stack.last().is_some_and(|name| name == b"loc") => {
                let name = reference.decode().map_err(|_| "invalid_sitemap_entity")?;
                let encoded = format!("&{name};");
                location.push_str(
                    &quick_xml::escape::unescape(&encoded).map_err(|_| "invalid_sitemap_entity")?,
                );
            }
            Event::CData(text) if stack.last().is_some_and(|name| name == b"loc") => {
                location.push_str(&text.decode().map_err(|_| "invalid_sitemap_text")?);
            }
            Event::End(_) if stack.pop().is_some_and(|name| name == b"loc") => {
                let expected = if out.is_index {
                    b"sitemap".as_slice()
                } else {
                    b"url".as_slice()
                };
                if stack.last().is_some_and(|name| name == expected)
                    && let Some(url) = crate::links::canonicalize(location.trim(), base)
                {
                    out.locations.push(url);
                    if out.locations.len() >= URL_LIMIT {
                        return Err("sitemap_url_limit");
                    }
                }
            }
            Event::DocType(_) => return Err("sitemap_doctype_disallowed"),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || !stack.is_empty() {
        return Err("invalid_sitemap_xml");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_namespaced_index_and_entities() {
        let map = parse("<s:sitemapindex xmlns:s='urn:test'><s:sitemap><s:loc>https://example.com/map.xml?a=1&amp;b=2</s:loc></s:sitemap></s:sitemapindex>", "https://example.com").unwrap();
        assert!(map.is_index);
        assert_eq!(map.locations, ["https://example.com/map.xml?a=1&b=2"]);
    }
    #[test]
    fn directives_are_group_independent_and_bounded() {
        assert_eq!(
            robots_sitemaps(
                "User-agent: *\nSitemap: https://example.com/a.xml\nsitemap: /b.xml # comment",
                "https://example.com"
            ),
            ["https://example.com/a.xml", "https://example.com/b.xml"]
        );
    }
    #[test]
    fn malformed_external_entities_and_oversize_fail_closed() {
        for xml in [
            "<html/>",
            "<urlset><url>",
            "<!DOCTYPE x SYSTEM 'file:///secret'><urlset/>",
        ] {
            assert!(parse(xml, "https://example.com").is_err());
        }
        assert!(parse(&"x".repeat(512 * 1024 + 1), "https://example.com").is_err());
    }
}
