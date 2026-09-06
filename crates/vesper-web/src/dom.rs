//! Lenient HTML parsing on `quick-xml` (PR-0 lesson: HTML is not XML).
//!
//! PR-0 validated three hard-won lessons, all applied here:
//!
//! 1. **`check_end_names` must be `false`.** HTML routinely leaves tags
//!    unclosed (`<li>`, `<meta>`, `<link>`); with the XML checker on,
//!    quick-xml aborts with `MismatchedEndTag` at the first HTML-shaped
//!    document and silently truncates everything after it. In the spike
//!    this produced head-swallowed-body trees and 64-byte "markdown".
//! 2. **Void elements never take children.** `<br>`, `<meta>`, `<img>`,
//!    `<hr>`, `<input>`, `<link>`, `<source>` … emitted as `Start` (they
//!    have no self-closing slash in HTML) must be attached to the parent
//!    immediately; pushing them on the stack would swallow their
//!    following siblings as phantom children.
//! 3. **Sibling implicit close.** A new `tr` closes an open `td`/`th`/`tr`,
//!    a new `li` closes an open `li`, a new `p` closes an open `p`, and so
//!    on (the HTML5 tag-omission set the PRD scope requires). Without it,
//!    table and list fixtures nest as a single column.

use std::collections::HashSet;

/// One DOM element node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Lowercased tag name (`"div"`, `"p"`, …).
    pub tag: String,
    /// Attributes, case-preserved keys in source order.
    pub attrs: Vec<(String, String)>,
    /// Child nodes in document order.
    pub children: Vec<Node>,
}

/// A DOM node: element, text, or a skipped placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// An element subtree.
    Element(Element),
    /// A text run (entities decoded, whitespace preserved).
    Text(String),
}

/// A parsed document; the root is a synthetic `#document` wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Document root element (tag `#document`).
    pub root: Element,
}

impl Element {
    /// First matching attribute by case-insensitive name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The element's own direct text runs concatenated (no descendants).
    pub fn direct_text(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            if let Node::Text(text) = child {
                out.push_str(text);
            }
        }
        out
    }

    /// All descendant text concatenated in document order.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            match child {
                Node::Text(text) => out.push_str(text),
                Node::Element(el) => out.push_str(&el.text()),
            }
        }
        out
    }

    /// Serialized inner length in bytes: own markup + text (beta's
    /// `encode_contents` proxy used by the density metrics).
    pub fn content_len(&self) -> usize {
        let mut bytes = 0;
        for child in &self.children {
            match child {
                Node::Text(text) => bytes += text.len(),
                Node::Element(el) => {
                    bytes += el.tag.len() + 2; // <tag
                    bytes += el.content_len();
                    bytes += el.tag.len() + 3; // </tag>
                }
            }
        }
        bytes
    }

    /// Total text length of direct-child `a` elements (beta's
    /// link-density numerator: only direct `a` children count).
    pub fn direct_child_link_text_len(&self) -> usize {
        let mut bytes = 0;
        for child in &self.children {
            if let Node::Element(el) = child
                && el.tag == "a"
            {
                bytes += el.text().len();
            }
        }
        bytes
    }
}

/// HTML void elements (no closing tag ever).
pub const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Maximum element nesting the parser preserves. Deeper opens attach as
/// flat children at the cap — the same graceful degradation browsers
/// apply (~512 in Chrome) — so adversarially deep markup cannot overflow
/// the recursive tree walks (PR-6 adversarial gate).
pub const MAX_PARSE_DEPTH: usize = 256;

/// The sibling implicit-close table (HTML5 tag omission, PRD scope).
fn implicit_close_for(tag: &str) -> &'static [&'static str] {
    match tag {
        "tr" => &["td", "th", "tr"],
        "td" | "th" => &["td", "th"],
        "li" => &["li"],
        "p" => &["p"],
        "option" => &["option"],
        "dt" | "dd" => &["dt", "dd"],
        _ => &[],
    }
}

/// Parse HTML bytes into a `Document`. Lenient: unknown entities decode
/// literally, mismatched closers pop to their opener (or are dropped as
/// strays), and the unclosed remainder is unwound at EOF.
///
/// Line endings are normalized first: the HTML5 spec treats CRLF and CR
/// as LF everywhere, so a CRLF checkout (Windows git autocrlf rewriting
/// fixture bytes) must not change any output byte. This is what keeps the
/// golden corpus byte-stable across platforms (`fixtures/AGENTS.md`).
pub fn parse(html: &str) -> Document {
    let html = if html.contains('\r') {
        html.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        html.to_string()
    };
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(&html);
    // PR-0 lesson 1: HTML is not XML; never abort on end-tag mismatch.
    reader.config_mut().check_end_names = false;
    reader.config_mut().trim_text(false);

    let mut stack: Vec<Element> = vec![Element {
        tag: "#document".to_string(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    let mut buffer = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buffer);
        match event {
            Ok(Event::Start(start)) => {
                let element = element_from(&start);
                // PR-0 lesson 2: void elements attach immediately and never
                // take children regardless of how they were serialized.
                if VOID.contains(&element.tag.as_str()) {
                    if let Some(top) = stack.last_mut() {
                        top.children.push(Node::Element(element));
                    }
                } else if stack.len() >= MAX_PARSE_DEPTH {
                    // PR-6 adversarial gate: hostile nesting beyond the
                    // browser-comparable cap attaches flat at the cap
                    // (browsers flatten ~512-deep trees rather than
                    // recursing). Without this, a 5,000-deep document
                    // overflows the recursive walks downstream.
                    if let Some(top) = stack.last_mut() {
                        top.children.push(Node::Element(element));
                    }
                } else {
                    // PR-0 lesson 3: implicitly close same-scope siblings
                    // before opening this element.
                    let closes = implicit_close_for(&element.tag);
                    if !closes.is_empty() {
                        while stack.len() > 1 {
                            let top_tag = stack.last().map(|el| el.tag.clone()).unwrap_or_default();
                            if closes.contains(&top_tag.as_str()) {
                                if let Some(closed) = stack.pop()
                                    && let Some(parent) = stack.last_mut()
                                {
                                    parent.children.push(Node::Element(closed));
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    stack.push(element);
                }
            }
            Ok(Event::Empty(empty)) => {
                let element = element_from(&empty);
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Element(element));
                }
            }
            Ok(Event::Text(text)) => {
                let raw = text
                    .decode()
                    .map(|cow| cow.into_owned())
                    .unwrap_or_default();
                // quick-xml's `decode` handles encoding, not entities;
                // HTML text content needs entity decoding too.
                let decoded = decode_entities(&raw);
                if !decoded.is_empty()
                    && let Some(top) = stack.last_mut()
                {
                    top.children.push(Node::Text(decoded));
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                // quick-xml 0.41 emits character/general entity references as
                // their own events between Text events; decode them here so
                // text content keeps its entities (PR-0 lesson: the default
                // `Ok(_) => {}` arm silently drops every entity).
                let raw = String::from_utf8_lossy(reference.as_ref()).into_owned();
                // The reference arrives WITHOUT its `&`/`;` delimiters;
                // re-wrap so the shared decoder resolves it.
                let wrapped = format!("&{raw};");
                let decoded = decode_entities(&wrapped);
                if !decoded.is_empty()
                    && let Some(top) = stack.last_mut()
                {
                    top.children.push(Node::Text(decoded));
                }
            }
            Ok(Event::End(end)) => {
                let closing = end.name().as_ref().to_vec();
                if let Some(element) = stack.pop() {
                    if element.tag.as_bytes() == closing.as_slice() {
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(Node::Element(element));
                        }
                    } else {
                        // Unclosed inner elements: close them in order, then
                        // close the matched opener if one is on the stack;
                        // otherwise treat the closer as a stray and restore.
                        let mut pool: Vec<Element> = vec![element];
                        while let Some(current) = stack.last() {
                            if current.tag.as_bytes() == closing.as_slice() {
                                break;
                            }
                            pool.push(stack.pop().expect("stack non-empty while matching closer"));
                        }
                        let matched = stack
                            .last()
                            .is_some_and(|el| el.tag.as_bytes() == closing.as_slice());
                        if matched {
                            // Re-nest the unclosed chain: pool holds
                            // [innermost … outermost]; pool[i]'s original
                            // parent is pool[i+1], and the outermost's
                            // parent was the matched opener. Fold from the
                            // innermost end so each element lands inside
                            // its original parent instead of flattening to
                            // siblings (or inverting the nesting).
                            let mut chain: std::collections::VecDeque<Element> =
                                pool.into_iter().rev().collect();
                            while chain.len() > 1 {
                                let child = chain.pop_back().expect("len > 1");
                                if let Some(parent) = chain.back_mut() {
                                    parent.children.push(Node::Element(child));
                                }
                            }
                            if let Some(outermost) = chain.pop_front()
                                && let Some(top) = stack.last_mut()
                            {
                                top.children.push(Node::Element(outermost));
                            }
                            let closed = stack.pop().expect("matched opener present");
                            if let Some(parent) = stack.last_mut() {
                                parent.children.push(Node::Element(closed));
                            }
                        } else {
                            for pooled in pool.into_iter().rev() {
                                stack.push(pooled);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                // Lenient recovery: a hard error ends the document here
                // rather than discarding everything already parsed.
                break;
            }
        }
        buffer.clear();
    }

    // Unwind anything left open at EOF.
    while stack.len() > 1 {
        if let Some(element) = stack.pop()
            && let Some(parent) = stack.last_mut()
        {
            parent.children.push(Node::Element(element));
        }
    }

    Document {
        root: stack.pop().unwrap_or(Element {
            tag: "#document".to_string(),
            attrs: Vec::new(),
            children: Vec::new(),
        }),
    }
}

/// Locate the `body` element (depth-first); the fallback is the root.
pub fn body(document: &Document) -> &Element {
    fn descend(element: &Element) -> Option<&Element> {
        if element.tag == "body" {
            return Some(element);
        }
        for child in &element.children {
            if let Node::Element(el) = child
                && let Some(found) = descend(el)
            {
                return Some(found);
            }
        }
        None
    }
    descend(&document.root).unwrap_or(&document.root)
}

/// Collect the distinct set of tags present in the document.
pub fn tag_set(document: &Document) -> HashSet<String> {
    let mut set = HashSet::new();
    fn walk(element: &Element, set: &mut HashSet<String>) {
        set.insert(element.tag.clone());
        for child in &element.children {
            if let Node::Element(el) = child {
                walk(el, set);
            }
        }
    }
    walk(&document.root, &mut set);
    set
}

fn numeric_entity_codepoint(entity: &str) -> Option<u32> {
    if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        entity.strip_prefix('#').and_then(|dec| dec.parse().ok())
    }
}

fn element_from(start: &quick_xml::events::BytesStart<'_>) -> Element {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_ascii_lowercase();
    let mut attrs = Vec::new();
    for attribute in start.attributes().with_checks(false) {
        let Ok(attribute) = attribute else {
            continue;
        };
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = decode_entities(&String::from_utf8_lossy(&attribute.value));
        attrs.push((key, value));
    }
    Element {
        tag,
        attrs,
        children: Vec::new(),
    }
}

/// Decode the handful of entities HTML emits in practice (the full
/// named-entity table is out of scope for this PR's pure-logic stage).
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        match rest.find(';') {
            Some(semi) if semi <= 10 => {
                let entity = &rest[1..semi];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    // U+00A0, not a plain space: it is meaningful spacing
                    // that must survive inline whitespace collapsing.
                    "nbsp" => Some('\u{a0}'),
                    "copy" => Some('©'),
                    "reg" => Some('®'),
                    "hellip" => Some('…'),
                    numeric => numeric_entity_codepoint(numeric).and_then(char::from_u32),
                };
                match decoded {
                    Some(ch) => {
                        out.push(ch);
                        rest = &rest[semi + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out
}

/// Find the first element with `tag` in document order (depth-first).
pub fn find_first<'a>(element: &'a Element, tag: &str) -> Option<&'a Element> {
    if element.tag == tag {
        return Some(element);
    }
    for child in &element.children {
        if let Node::Element(el) = child
            && let Some(found) = find_first(el, tag)
        {
            return Some(found);
        }
    }
    None
}

/// Total decoded text bytes of a document (measurement helper).
pub fn document_text_bytes(document: &Document) -> usize {
    document.root.text().len()
}

/// Concatenated decoded text of the whole document (strip-stage tests).
pub fn render_text(document: &Document) -> String {
    document.root.text()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_structure() {
        let document = parse("<div><p>hello <b>world</b></p></div>");
        let div = &document.root.children[0];
        let Element { tag, .. } = match &div {
            Node::Element(el) => el,
            Node::Text(_) => panic!("expected element"),
        };
        assert_eq!(tag, "div");
        let div_el = match div {
            Node::Element(el) => el,
            Node::Text(_) => panic!("expected element"),
        };
        let para = match &div_el.children[0] {
            Node::Element(el) => el,
            Node::Text(_) => panic!("expected element child"),
        };
        assert!(para.text().contains("hello world"));
    }

    #[test]
    fn void_elements_do_not_swallow_siblings() {
        let document = parse("<p>a<br>b</p>");
        let text = body_text(&document);
        assert!(text.contains('a') && text.contains('b'), "text: {text}");
        // `br` must not own `b` as a child text.
        let para = find_first(&document, "p");
        let void_count = para
            .children
            .iter()
            .filter(|child| matches!(child, Node::Element(el) if el.tag == "br"))
            .count();
        assert_eq!(void_count, 1, "br appears once, as a sibling");
    }

    #[test]
    fn check_end_names_is_off() {
        // The exact PR-0 killer: unclosed voids then a normal close.
        let document = parse(
            "<html><head><meta charset=\"utf-8\"><link rel=\"stylesheet\" href=\"x\"></head><body><p>t</p></body></html>",
        );
        let text = body_text(&document);
        assert!(text.contains('t'), "body text survived: {text}");
    }

    #[test]
    fn implicit_close_li() {
        let document = parse("<ul><li>one<li>two</ul>");
        let list = find_first(&document, "ul");
        let items = list
            .children
            .iter()
            .filter(|child| matches!(child, Node::Element(el) if el.tag == "li"))
            .count();
        assert_eq!(items, 2, "two sibling li elements");
    }

    #[test]
    fn implicit_close_table_cells() {
        let document = parse("<table><tr><td>a<td>b</table>");
        let row = find_first(&document, "tr");
        let cells = row
            .children
            .iter()
            .filter(|child| matches!(child, Node::Element(el) if el.tag == "td"))
            .count();
        assert_eq!(cells, 2, "two sibling td elements");
    }

    #[test]
    fn entities_decode() {
        let document = parse("<p>a &amp; b &lt;c&gt; &nbsp; &copy;</p>");
        let text = body_text(&document);
        assert!(
            text.contains("a & b"),
            "text chars: {:?}",
            text.chars().collect::<Vec<_>>()
        );
        assert!(text.contains("<c>"), "text: {text}");
        assert!(text.contains('©'), "text: {text}");
    }

    fn body_text(document: &Document) -> String {
        body(document).text()
    }

    fn find_first<'a>(document: &'a Document, tag: &str) -> &'a Element {
        fn walk<'a>(element: &'a Element, tag: &str) -> Option<&'a Element> {
            if element.tag == tag {
                return Some(element);
            }
            for child in &element.children {
                if let Node::Element(el) = child
                    && let Some(found) = walk(el, tag)
                {
                    return Some(found);
                }
            }
            None
        }
        walk(&document.root, tag).unwrap_or(&document.root)
    }
}

#[cfg(test)]
mod crlf_tests {
    use super::parse;

    /// Platform-checkout resilience (the Windows CI failure): CRLF and CR
    /// inputs must produce byte-identical trees to their LF form — the
    /// HTML5 input-stream normalization.
    #[test]
    fn crlf_and_cr_inputs_match_lf_outputs() {
        let lf = "<html><body><p>line one\nline two</p><pre>code\nblock</pre></body></html>";
        let crlf = lf.replace('\n', "\r\n");
        let cr = lf.replace('\n', "\r");
        let a = parse(lf);
        let b = parse(&crlf);
        let c = parse(&cr);
        assert_eq!(a.root.text(), b.root.text(), "CRLF must equal LF");
        assert_eq!(a.root.text(), c.root.text(), "CR must equal LF");
        assert_eq!(
            format!("{:?}", a.root.children),
            format!("{:?}", b.root.children),
            "trees must be structurally identical"
        );
    }
}
