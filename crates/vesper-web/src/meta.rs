//! Page metadata extraction (VRO-14 PR-2).
//!
//! Extracts the title, meta description, canonical URL, and OpenGraph
//! tags from a parsed [`Document`]. Pure DOM walks; the upstream shape
//! mirrors web oracle alpha's `extractMetadata` intent (title,
//! description, canonical, og:title/og:description/og:url/og:image/
//! og:site_name/og:type/og:locale), minus its favicon/proxy plumbing.

use crate::dom::{Document, Element, Node, body, find_first};

/// OpenGraph tags captured by the extractor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenGraph {
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
    pub og_type: Option<String>,
    pub locale: Option<String>,
}

/// Page-level metadata extracted from the DOM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub canonical: Option<String>,
    pub keywords: Option<String>,
    pub lang: Option<String>,
    pub open_graph: OpenGraph,
}

/// Extract [`PageMetadata`] from a parsed document.
pub fn extract_metadata(document: &Document) -> PageMetadata {
    let mut meta = PageMetadata {
        lang: find_first(&document.root, "html")
            .and_then(|html| html.attr("lang").map(str::to_string)),
        ..PageMetadata::default()
    };
    for element in iter_all(&document.root) {
        match element.tag.as_str() {
            "title" => {
                let text = element.text();
                let trimmed = text.trim();
                if meta.title.is_none() && !trimmed.is_empty() {
                    meta.title = Some(trimmed.to_string());
                }
            }
            "meta" => apply_meta(element, &mut meta),
            "link"
                if element
                    .attr("rel")
                    .is_some_and(|rel| rel.eq_ignore_ascii_case("canonical")) =>
            {
                let href = element.attr("href").unwrap_or("").trim();
                if !href.is_empty() && meta.canonical.is_none() {
                    meta.canonical = Some(href.to_string());
                }
            }
            _ => {}
        }
    }
    meta
}

fn apply_meta(element: &Element, meta: &mut PageMetadata) {
    let name = element.attr("name").map(str::to_ascii_lowercase);
    let property = element.attr("property").map(str::to_ascii_lowercase);
    let content = || {
        element
            .attr("content")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match name.as_deref() {
        Some("description") => {
            if meta.description.is_none() {
                meta.description = content();
            }
            return;
        }
        Some("keywords") => {
            if meta.keywords.is_none() {
                meta.keywords = content();
            }
            return;
        }
        _ => {}
    }
    let og = &mut meta.open_graph;
    let content = content();
    let slot = match property.as_deref() {
        Some("og:title") => Some(&mut og.title),
        Some("og:description") => Some(&mut og.description),
        Some("og:url") => Some(&mut og.url),
        Some("og:image") => Some(&mut og.image),
        Some("og:site_name") => Some(&mut og.site_name),
        Some("og:type") => Some(&mut og.og_type),
        Some("og:locale") => Some(&mut og.locale),
        _ => None,
    };
    if let (Some(slot), Some(value)) = (slot, content)
        && slot.is_none()
    {
        *slot = Some(value);
    }
}

/// Depth-first iteration over every element in the tree (head included).
pub(crate) fn iter_all(root: &Element) -> impl Iterator<Item = &Element> {
    let mut stack = vec![root];
    std::iter::from_fn(move || {
        let element = stack.pop()?;
        for child in &element.children {
            if let Node::Element(child_el) = child {
                stack.push(child_el);
            }
        }
        Some(element)
    })
}

/// Body text excerpt (title fallback uses the first heading, mirroring
/// alpha's title-from-h1 fallback when `<title>` is absent).
pub fn fallback_title(document: &Document) -> Option<String> {
    iter_all(body(document))
        .find(|element| {
            matches!(
                element.tag.as_str(),
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            )
        })
        .map(|heading| heading.text().trim().to_string())
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse;

    #[test]
    fn extracts_title_description_canonical_and_og() {
        let html = "<html lang=\"en\"><head>\
            <title>Settings Overview</title>\
            <meta name=\"description\" content=\"Manage workspace preferences.\">\
            <meta name=\"keywords\" content=\"settings, preferences\">\
            <link rel=\"canonical\" href=\"https://example.com/settings\">\
            <meta property=\"og:title\" content=\"Settings\">\
            <meta property=\"og:image\" content=\"https://example.com/og.png\">\
            </head><body><h1>Settings</h1></body></html>";
        let meta = extract_metadata(&parse(html));
        assert_eq!(meta.title.as_deref(), Some("Settings Overview"));
        assert_eq!(
            meta.description.as_deref(),
            Some("Manage workspace preferences.")
        );
        assert_eq!(
            meta.canonical.as_deref(),
            Some("https://example.com/settings")
        );
        assert_eq!(meta.lang.as_deref(), Some("en"));
        assert_eq!(meta.open_graph.title.as_deref(), Some("Settings"));
        assert_eq!(
            meta.open_graph.image.as_deref(),
            Some("https://example.com/og.png")
        );
    }

    #[test]
    fn missing_metadata_yields_defaults() {
        let meta = extract_metadata(&parse("<p>no head at all</p>"));
        assert_eq!(meta, PageMetadata::default());
        assert!(fallback_title(&parse("<p>x</p>")).is_none());
    }

    #[test]
    fn fallback_title_uses_first_heading() {
        let html = "<body><h2>Deep Settings</h2><p>body</p></body>";
        assert_eq!(
            fallback_title(&parse(html)).as_deref(),
            Some("Deep Settings")
        );
    }

    #[test]
    fn empty_content_is_not_stored() {
        let html = "<meta property=\"og:title\" content=\"   \">";
        let meta = extract_metadata(&parse(html));
        assert!(meta.open_graph.title.is_none());
    }
}
