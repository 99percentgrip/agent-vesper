//! Strip stage (VRO-14 PR-1, PRD Feature 1 §1.2): drop non-content
//! subtrees before any scoring, porting the strip semantics of web oracle
//! alpha's `removeUnwantedElements` intent plus beta's `excluded_tags`
//! set. Pure tree surgery — no I/O.

use crate::dom::{Document, Element, Node};

/// Tags whose entire subtree is non-content for LLM consumption.
/// Beta removes `nav`, `footer`, `header`, `aside`, `script`, `style`,
/// `form`, `iframe`, `noscript`; alpha additionally drops `template` and
/// decorative `svg` foliage.
pub const STRIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "iframe", "nav", "footer", "header", "aside",
    "form", "button", "select", "option",
];

/// True when an inline `style` attribute or `aria-hidden` marks the
/// element invisible (the pure-logic subset of visibility: no computed
/// styles here — the render pipeline supplies those when it exists).
pub fn element_hidden(el: &Element) -> bool {
    if let Some(style) = el.attr("style") {
        let collapsed: String = style.chars().filter(|c| !c.is_whitespace()).collect();
        let lowered = collapsed.to_ascii_lowercase();
        if lowered.contains("display:none") || lowered.contains("visibility:hidden") {
            return true;
        }
    }
    el.attr("aria-hidden")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// Strip the document in place: remove STRIP_TAGS subtrees and
/// inline-hidden elements, recursively.
pub fn strip(doc: &mut Document) {
    strip_element(&mut doc.root);
}

fn strip_element(el: &mut Element) {
    for child in &mut el.children {
        if matches!(child, Node::Element(frame) if frame.tag == "iframe" && !element_hidden(frame))
        {
            // Label the missing embedded document without echoing an untrusted
            // origin, srcdoc body, or arbitrary frame attributes.
            *child = Node::Text("[embedded frame]".into());
        }
    }
    el.children.retain(|child| match child {
        Node::Element(e) => !STRIP_TAGS.contains(&e.tag.as_str()) && !element_hidden(e),
        Node::Text(_) => true,
    });
    for child in &mut el.children {
        if let Node::Element(e) = child {
            strip_element(e);
        }
    }
}

/// Parse then strip in one step (the F1 pipeline's first two stages).
pub fn parse_and_strip(html: &str) -> Document {
    let mut doc = crate::dom::parse(html);
    strip(&mut doc);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_script_and_style_subtrees() {
        let doc = parse_and_strip(
            "<html><body><script>var x = 1;</script>\
             <p>keep</p><style>.a { color: red }</style></body></html>",
        );
        let text = crate::dom::render_text(&doc);
        assert!(text.contains("keep"));
        assert!(!text.contains("var x"));
        assert!(!text.contains("color: red"));
    }

    #[test]
    fn strip_removes_hidden_elements() {
        let doc = parse_and_strip(
            "<body><p style=\"display: none\">secret</p>\
             <p style=\"visibility:hidden\">also hidden</p>\
             <p aria-hidden=\"true\">aria hidden</p><p>visible</p></body>",
        );
        let text = crate::dom::render_text(&doc);
        assert!(text.contains("visible"));
        assert!(!text.contains("secret"), "text: {text}");
        assert!(!text.contains("also hidden"), "text: {text}");
        assert!(!text.contains("aria hidden"), "text: {text}");
    }

    #[test]
    fn strip_removes_nav_footer_aside_form_subtrees() {
        let doc = parse_and_strip(
            "<body><nav><a>menu</a></nav><aside>ad</aside>\
             <form><input><button>b</button></form>\
             <footer>(c) site</footer><main><p>content</p></main></body>",
        );
        let text = crate::dom::render_text(&doc);
        assert!(text.contains("content"));
        assert!(!text.contains("menu"));
        assert!(!text.contains("ad"));
        assert!(!text.contains("site"));
    }

    #[test]
    fn strip_keeps_deeper_content_inside_kept_wrappers() {
        let doc = parse_and_strip(
            "<body><div><section><article><p>deep content</p></article></section></div></body>",
        );
        assert!(crate::dom::render_text(&doc).contains("deep content"));
    }
}
