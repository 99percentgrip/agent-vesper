//! VRO-14 PR-0 spike: minimal quick-xml DOM (lenient HTML parsing).
//!
//! The directive named html5ever; the offline cache holds only quick-xml
//! 0.41, so this spike parses with quick-xml's lenient reader and the
//! verdict documents the substitution (logic portability, not parser
//! equivalence, is what PR-0 must prove).

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Text(String),
    Element(Element),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn has_class(&self, needle: &str) -> bool {
        match self.attr("class") {
            Some(c) => c.split_whitespace().any(|x| x.eq_ignore_ascii_case(needle)),
            None => false,
        }
    }

    /// Concatenated descendant text.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) => s.push_str(t),
                Node::Element(e) => s.push_str(&e.text()),
            }
        }
        s
    }

    /// Approximation of beta's `encode_contents().len()` (inner HTML
    /// length) — enough for the density ratio.
    pub fn content_len(&self) -> usize {
        let mut n = 0;
        for c in &self.children {
            match c {
                Node::Text(t) => n += t.len(),
                Node::Element(e) => {
                    n += e.tag.len() + 2;
                    n += e.content_len();
                    n += e.tag.len() + 3;
                }
            }
        }
        n
    }

    /// Sum of direct-child `a` element text lengths (beta measures only
    /// direct children links).
    pub fn direct_child_link_text_len(&self) -> usize {
        let mut n = 0;
        for c in &self.children {
            if let Node::Element(e) = c {
                if e.tag == "a" {
                    n += e.text().len();
                }
            }
        }
        n
    }
}

pub struct Document {
    pub root: Element,
}

impl Document {
    pub fn body(&self) -> Option<&Element> {
        self.root.find_first("body")
    }
}

impl Element {
    pub fn find_first(&self, tag: &str) -> Option<&Element> {
        if self.tag == tag {
            return Some(self);
        }
        for c in &self.children {
            if let Node::Element(e) = c {
                if let Some(found) = e.find_first(tag) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub fn find_all<'a>(&'a self, tag: &str, out: &mut Vec<&'a Element>) {
        if self.tag == tag {
            out.push(self);
        }
        for c in &self.children {
            if let Node::Element(e) = c {
                e.find_all(tag, out);
            }
        }
    }
}

fn element_hidden(el: &Element) -> bool {
    if let Some(s) = el.attr("style") {
        let s = s.to_ascii_lowercase().replace(' ', "");
        if s.contains("display:none") || s.contains("visibility:hidden") {
            return true;
        }
    }
    el.attr("aria-hidden").map(|v| v == "true").unwrap_or(false)
}

/// Strip stage (alpha's removeUnwantedElements intent + beta's
/// excluded_tags): drop noise subtrees in-place before any scoring.
pub fn strip(doc: &mut Document) {
    strip_element(&mut doc.root);
}

fn strip_element(el: &mut Element) {
    el.children.retain(|c| match c {
        Node::Element(e) => !STRIP_TAGS.contains(&e.tag.as_str()) && !element_hidden(e),
        Node::Text(_) => true,
    });
    for c in &mut el.children {
        if let Node::Element(e) = c {
            strip_element(e);
        }
    }
}

pub const STRIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "iframe", "nav",
    "footer", "header", "aside", "form", "button", "select", "option",
];

/// Void elements (no close tag in HTML).
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link",
    "meta", "param", "source", "track", "wbr",
];

pub fn parse(html: &str) -> Document {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(html);
    // HTML is not XML: unclosed <link>/<meta> etc. must not abort parsing
    // (quick-xml otherwise errors with MismatchedEndTag and the lenient
    // handler would truncate the document at </head>).
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = false;
    let mut stack: Vec<Element> = vec![Element {
        tag: "#document".into(),
        attrs: vec![],
        children: vec![],
    }];
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let el = element_from(&e);
                if VOID.contains(&el.tag.as_str()) {
                    // Void element: attach immediately; never takes children.
                    if let Some(top) = stack.last_mut() {
                        top.children.push(Node::Element(el));
                    }
                    buf.clear();
                    continue;
                }
                // Implicit close rules (HTML5 tag omission, minimal set):
                // a new tr closes an open td/th/tr; a new td/th closes an
                // open td/th; a new li closes an open li; a new p closes an
                // open p; option closes option; dt/dd close dt/dd.
                let closes: &[&str] = match el.tag.as_str() {
                    "tr" => &["td", "th", "tr"],
                    "td" | "th" => &["td", "th"],
                    "li" => &["li"],
                    "p" => &["p"],
                    "option" => &["option"],
                    "dt" | "dd" => &["dt", "dd"],
                    _ => &[],
                };
                if !closes.is_empty() {
                    while stack.len() > 1 {
                        let top_tag = stack.last().unwrap().tag.clone();
                        if closes.contains(&top_tag.as_str()) {
                            if let Some(closed) = stack.pop() {
                                if let Some(parent) = stack.last_mut() {
                                    parent.children.push(Node::Element(closed));
                                }
                            }
                        } else {
                            break;
                        }
                    }
                }
                stack.push(el);
            }
            Ok(Event::Empty(e)) => {
                let el = element_from(&e);
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Element(el));
                }
            }
            Ok(Event::Text(t)) => {
                let txt = t
                    .decode()
                    .map(|c| c.into_owned())
                    .unwrap_or_default();
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Text(txt));
                }
            }
            Ok(Event::CData(t)) => {
                let txt = String::from_utf8_lossy(t.as_ref()).into_owned();
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::Text(txt));
                }
            }
            Ok(Event::End(e)) => {
                let closing = e.name().as_ref().to_vec();
                if let Some(el) = stack.pop() {
                    if el.tag.as_bytes() != closing {
                        // Mismatched close (unclosed inner elements): pop
                        // the unmatched openers in order, attach each as a
                        // child of the NEXT outer element (preserving
                        // sibling order), then close the matched element.
                        let mut pool: Vec<Element> = vec![el];
                        while let Some(cur) = stack.last() {
                            if cur.tag.as_bytes() == closing {
                                break;
                            }
                            pool.push(stack.pop().unwrap());
                        }
                        if stack.last().map(|c| c.tag.as_bytes() == closing).unwrap_or(false) {
                            // pool is [innermost..outermost]; attach each
                            // popped element as a child of the next outer.
                            for p in pool.into_iter() {
                                if let Some(top) = stack.last_mut() {
                                    top.children.push(Node::Element(p));
                                }
                            }
                            // Close the matched element itself.
                            let matched = stack.pop().unwrap();
                            if let Some(top) = stack.last_mut() {
                                top.children.push(Node::Element(matched));
                            }
                        } else {
                            // No opener matched: stray close; restore stack.
                            for p in pool.into_iter().rev() {
                                stack.push(p);
                            }
                        }
                    } else {
                        if VOID.contains(&el.tag.as_str()) {
                            // void element closed immediately: leave as
                            // child of parent (already appended? no—voids
                            // are pushed via Empty only; here just attach).
                        }
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(Node::Element(el));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                // Lenient recovery: stop parsing at the first hard error.
                break;
            }
        }
        buf.clear();
    }
    // Unwind any unclosed elements.
    while stack.len() > 1 {
        if let Some(el) = stack.pop() {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Node::Element(el));
            }
        }
    }
    Document {
        root: stack.pop().unwrap_or(Element {
            tag: "#document".into(),
            attrs: vec![],
            children: vec![],
        }),
    }
}

fn element_from(e: &quick_xml::events::BytesStart<'_>) -> Element {
    let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for a in e.attributes().with_checks(true) {
        match a {
            Ok(attr) => {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let val = String::from_utf8_lossy(&attr.value).into_owned();
                attrs.push((key, val));
            }
            Err(_) => continue,
        }
    }
    Element {
        tag,
        attrs,
        children: vec![],
    }
}

/// Textual byte size of the document's stripped tree (used as the
/// "strip-in" column: bytes of text the converter could consume).
pub fn document_text_bytes(doc: &Document) -> usize {
    doc.root.text().len()
}

pub fn parse_and_strip(html: &str) -> Document {
    let mut doc = parse(html);
    strip(&mut doc);
    doc
}

pub fn body_or_root(doc: &Document) -> &Element {
    doc.body().unwrap_or(&doc.root)
}

pub fn tag_set(doc: &Document) -> HashSet<String> {
    let mut set = HashSet::new();
    collect_tags(&doc.root, &mut set);
    set
}

fn collect_tags(el: &Element, set: &mut HashSet<String>) {
    set.insert(el.tag.clone());
    for c in &el.children {
        if let Node::Element(e) = c {
            collect_tags(e, set);
        }
    }
}

pub fn render_text(doc: &Document) -> String {
    body_or_root(doc).text()
}

// ---------------------------------------------------------------------------
// Arena surface (used by the density filter for detach-during-walk).
// ---------------------------------------------------------------------------

pub type NodeId = usize;

#[derive(Debug, Clone)]
pub enum ArenaNode {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
}

impl ArenaNode {
    pub fn is_element(&self) -> bool {
        matches!(self, ArenaNode::Element { .. })
    }

    pub fn tag(&self) -> &str {
        match self {
            ArenaNode::Element { tag, .. } => tag,
            ArenaNode::Text(_) => "",
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        match self {
            ArenaNode::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str()),
            ArenaNode::Text(_) => None,
        }
    }

    pub fn hidden_inline(&self) -> bool {
        if let Some(s) = self.attr("style") {
            let s = s.to_ascii_lowercase().replace(' ', "");
            if s.contains("display:none") || s.contains("visibility:hidden") {
                return true;
            }
        }
        self.attr("aria-hidden").map(|v| v == "true").unwrap_or(false)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Dom {
    pub nodes: Vec<ArenaNode>,
    pub parent: Vec<Option<NodeId>>,
    pub children: Vec<Vec<NodeId>>,
}

impl Dom {
    pub fn from_document(doc: &Document) -> Self {
        let mut dom = Dom::default();
        dom.nodes.push(ArenaNode::Element {
            tag: "#document".to_string(),
            attrs: vec![],
        });
        dom.parent.push(None);
        dom.children.push(vec![]);
        dom.append_tree(0, &doc.root);
        dom
    }

    fn append_tree(&mut self, parent: NodeId, el: &Element) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(ArenaNode::Element {
            tag: el.tag.clone(),
            attrs: el.attrs.clone(),
        });
        self.parent.push(Some(parent));
        self.children.push(vec![]);
        self.children[parent].push(id);
        for c in &el.children {
            match c {
                Node::Text(t) => {
                    let tid = self.nodes.len();
                    self.nodes.push(ArenaNode::Text(t.clone()));
                    self.parent.push(Some(id));
                    self.children.push(vec![]);
                    self.children[id].push(tid);
                }
                Node::Element(e) => {
                    self.append_tree(id, e);
                }
            }
        }
        id
    }

    pub fn root(&self) -> NodeId {
        0
    }

    pub fn get(&self, id: NodeId) -> Option<&ArenaNode> {
        self.nodes.get(id)
    }

    pub fn children_of(&self, id: NodeId) -> &[NodeId] {
        &self.children[id]
    }

    pub fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.parent[id] {
            self.children[p].retain(|c| *c != id);
            self.parent[id] = None;
        }
    }

    /// Post-order over surviving nodes.
    pub fn post_order(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.walk_post(self.root(), &mut out);
        out
    }

    fn walk_post(&self, id: NodeId, out: &mut Vec<NodeId>) {
        for &k in &self.children[id] {
            self.walk_post(k, out);
        }
        out.push(id);
    }

    pub fn tag_of(&self, id: NodeId) -> &str {
        self.get(id).map(|n| n.tag()).unwrap_or("")
    }

    pub fn attr_of(&self, id: NodeId, name: &str) -> Option<&str> {
        self.get(id).and_then(|n| n.attr(name))
    }

    /// Subtree concatenated text (elements only).
    pub fn text_of(&self, id: NodeId) -> String {
        match self.get(id) {
            Some(ArenaNode::Text(t)) => t.clone(),
            Some(ArenaNode::Element { .. }) => {
                let mut s = String::new();
                self.subtree_text(id, &mut s);
                s
            }
            None => String::new(),
        }
    }

    fn subtree_text(&self, id: NodeId, out: &mut String) {
        for &c in &self.children[id] {
            match self.get(c) {
                Some(ArenaNode::Text(t)) => out.push_str(t),
                Some(ArenaNode::Element { .. }) => self.subtree_text(c, out),
                None => {}
            }
        }
    }

    pub fn text_len(&self, id: NodeId) -> usize {
        self.text_of(id).len()
    }

    /// Inner-HTML length approximation (see Element::content_len).
    pub fn tag_len(&self, id: NodeId) -> usize {
        match self.get(id) {
            Some(ArenaNode::Element { tag, .. }) => {
                let mut n = tag.len() + 2;
                for &c in &self.children[id] {
                    match self.get(c) {
                        Some(ArenaNode::Text(t)) => n += t.len(),
                        Some(ArenaNode::Element { .. }) => n += self.tag_len(c),
                        None => {}
                    }
                }
                n += tag.len() + 3;
                n
            }
            _ => 0,
        }
    }

    pub fn link_text_len(&self, id: NodeId) -> usize {
        let mut n = 0;
        for &c in &self.children[id] {
            if self.tag_of(c) == "a" {
                n += self.text_len(c);
            }
        }
        n
    }

    pub fn root_text(&self) -> String {
        self.text_of(self.root())
    }
}

/// Materialize surviving body children as tree elements (for the converter
/// — mirrors beta's `filter_content` returning HTML blocks; here we skip
/// the serialize/parse round trip).
pub fn surviving_elements(dom: &Dom) -> Vec<Element> {
    // Descend to <body>: the arena root is #document -> html -> head+body.
    // Walk down single-child wrappers (html) until we find body.
    let mut start = dom.root();
    'outer: loop {
        for &c in dom.children_of(start) {
            if dom.tag_of(c) == "body" {
                start = c;
                break 'outer;
            }
        }
        // No body at this level: descend into the first element child
        // while it is the only element child (the html wrapper).
        let elem_kids: Vec<usize> = dom
            .children_of(start)
            .iter()
            .copied()
            .filter(|&id| dom.get(id).map(|n| n.is_element()).unwrap_or(false))
            .collect();
        if elem_kids.len() == 1 && dom.tag_of(start) == "#document" {
            start = elem_kids[0];
        } else {
            break;
        }
    }
    let mut out = Vec::new();
    for &c in dom.children_of(start) {
        if dom.get(c).map(|n| n.is_element()).unwrap_or(false) {
            out.push(materialize_element(c, dom));
        }
    }
    out
}

fn materialize_element(id: NodeId, dom: &Dom) -> Element {
    let tag = dom.tag_of(id).to_string();
    let attrs = match dom.get(id) {
        Some(ArenaNode::Element { attrs, .. }) => attrs.clone(),
        _ => vec![],
    };
    let mut el = Element {
        tag,
        attrs,
        children: vec![],
    };
    for &c in dom.children_of(id) {
        match dom.get(c) {
            Some(ArenaNode::Text(t)) => el.children.push(Node::Text(t.clone())),
            Some(ArenaNode::Element { .. }) => {
                el.children.push(Node::Element(materialize_element(c, dom)));
            }
            None => {}
        }
    }
    el
}

/// Debug helper: recursive tree dump (used by src/bin/tree.rs).
pub fn dump_tree(el: &Element, depth: usize, out: &mut String) {
    let _ = std::fmt::write(
        out,
        format_args!(
            "{}<{}> text_len={} kids={}\n",
            "  ".repeat(depth),
            el.tag,
            el.text().len(),
            el.children.len()
        ),
    );
    for c in &el.children {
        if let Node::Element(e) = c {
            dump_tree(e, depth + 1, out);
        }
    }
}
