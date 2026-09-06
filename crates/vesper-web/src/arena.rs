//! Arena-backed DOM projection used by the prune stage.
//!
//! The tree `Document` is ideal for conversion but pruning needs cheap
//! subtree detach during post-order walks, so the prune stage projects the
//! stripped tree into this arena (`Dom`) first. Metrics (text length,
//! markup length, link-text length) are precomputed per node at
//! projection time — the filter never walks the tree to score.

use crate::dom::{Document, Element, Node};

pub type NodeId = usize;

pub enum ArenaNode {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        text_len: usize,
        tag_len: usize,
        link_text_len: usize,
    },
    Text(String),
}

pub struct Dom {
    nodes: Vec<ArenaNode>,
    parent: Vec<Option<NodeId>>,
    children: Vec<Vec<NodeId>>,
}

impl Dom {
    /// Project a stripped `Document` into the arena.
    pub fn from_document(document: &Document) -> Self {
        let mut dom = Dom {
            nodes: Vec::new(),
            parent: Vec::new(),
            children: Vec::new(),
        };
        let root = dom.push_root();
        dom.append_element(Some(root), &document.root);
        dom
    }

    fn push_root(&mut self) -> NodeId {
        self.nodes.push(ArenaNode::Element {
            tag: "#document".to_string(),
            attrs: Vec::new(),
            text_len: 0,
            tag_len: 0,
            link_text_len: 0,
        });
        self.parent.push(None);
        self.children.push(Vec::new());
        self.nodes.len() - 1
    }

    fn append_element(&mut self, parent: Option<NodeId>, el: &Element) -> NodeId {
        let id = self.nodes.len();
        // PR-6 perf: metrics are accumulated bottom-up from the children
        // once they exist — one post-order pass over the whole projection
        // is O(N) total. Computing `el.text()`/`el.content_len()` per node
        // (each O(subtree)) made deep chains O(N²): the 100k-node
        // adversarial fixture spent ~830 ms in the projection and blew the
        // 150 ms prune+convert gate.
        self.nodes.push(ArenaNode::Element {
            tag: el.tag.clone(),
            attrs: el.attrs.clone(),
            text_len: 0,
            tag_len: 0,
            link_text_len: 0,
        });
        self.parent.push(parent);
        self.children.push(Vec::new());
        if let Some(p) = parent {
            self.children[p].push(id);
        }
        for child in &el.children {
            match child {
                Node::Text(t) => {
                    let tid = self.nodes.len();
                    self.nodes.push(ArenaNode::Text(t.clone()));
                    self.parent.push(Some(id));
                    self.children.push(Vec::new());
                    self.children[id].push(tid);
                }
                Node::Element(e) => {
                    self.append_element(Some(id), e);
                }
            }
        }
        // Bottom-up metric fill from the now-complete children.
        let mut text_len = 0usize;
        let mut tag_len = 0usize;
        let mut link_text_len = 0usize;
        for &child in &self.children[id] {
            match self.get(child) {
                Some(ArenaNode::Text(t)) => {
                    text_len += t.len();
                    tag_len += t.len();
                }
                Some(ArenaNode::Element {
                    tag,
                    text_len: child_text,
                    tag_len: child_tag,
                    ..
                }) => {
                    text_len += child_text;
                    tag_len += tag.len() + 2 + child_tag + tag.len() + 3;
                    if tag == "a" {
                        link_text_len += child_text;
                    }
                }
                None => {}
            }
        }
        if let Some(ArenaNode::Element {
            text_len: slot_text,
            tag_len: slot_tag,
            link_text_len: slot_link,
            ..
        }) = self.nodes.get_mut(id)
        {
            *slot_text = text_len;
            *slot_tag = tag_len.max(1);
            *slot_link = link_text_len;
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

    /// Detach a node from its parent (pruned nodes stay allocated; their
    /// parent link is severed so no traversal reaches them again).
    pub fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.parent[id] {
            self.children[p].retain(|c| *c != id);
            self.parent[id] = None;
        }
    }

    /// Post-order traversal of the surviving tree (children before
    /// parents), the order beta's bottom-up pruning requires.
    pub fn post_order(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.walk_post(self.root(), &mut out);
        out
    }

    /// Top-down traversal (parents before children).
    pub fn pre_order(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.walk_pre(self.root(), &mut out);
        out
    }

    fn walk_pre(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        for &child in &self.children[id] {
            self.walk_pre(child, out);
        }
    }

    /// Number of arena nodes (index bound for per-node tables).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Clippy contract companion to [`Dom::len`].
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[expect(dead_code)]
    fn unused(&self) {}

    fn walk_post(&self, id: NodeId, out: &mut Vec<NodeId>) {
        for &child in &self.children[id] {
            self.walk_post(child, out);
        }
        out.push(id);
    }

    // ---- precomputed metric accessors (set at projection time) ----

    pub fn tag_of(&self, id: NodeId) -> &str {
        match self.get(id) {
            Some(ArenaNode::Element { tag, .. }) => tag,
            _ => "",
        }
    }

    pub fn attr_of(&self, id: NodeId, name: &str) -> Option<&str> {
        match self.get(id) {
            Some(ArenaNode::Element { attrs, .. }) => attrs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn text_len(&self, id: NodeId) -> usize {
        match self.get(id) {
            Some(ArenaNode::Element { text_len, .. }) => *text_len,
            _ => 0,
        }
    }

    pub fn tag_len(&self, id: NodeId) -> usize {
        match self.get(id) {
            Some(ArenaNode::Element { tag_len, .. }) => *tag_len,
            _ => 0,
        }
    }

    pub fn link_text_len(&self, id: NodeId) -> usize {
        match self.get(id) {
            Some(ArenaNode::Element { link_text_len, .. }) => *link_text_len,
            _ => 0,
        }
    }

    pub fn text_of(&self, id: NodeId) -> String {
        match self.get(id) {
            Some(ArenaNode::Element { .. }) => {
                let mut out = String::new();
                self.subtree_text(id, &mut out);
                out
            }
            Some(ArenaNode::Text(t)) => t.clone(),
            None => String::new(),
        }
    }

    fn subtree_text(&self, id: NodeId, out: &mut String) {
        for &child in &self.children[id] {
            match self.get(child) {
                Some(ArenaNode::Text(t)) => out.push_str(t),
                Some(ArenaNode::Element { .. }) => self.subtree_text(child, out),
                None => {}
            }
        }
    }

    /// Concatenated text of the surviving tree (sentinel checks).
    /// True when `id` lies anywhere inside `anc`'s surviving subtree.
    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.parent.get(id).copied().flatten()
    }

    pub fn is_descendant_of(&self, id: NodeId, anc: NodeId) -> bool {
        let mut current = self.parent_of(id);
        while let Some(cur) = current {
            if cur == anc {
                return true;
            }
            current = self.parent_of(cur);
        }
        false
    }

    pub fn root_text(&self) -> String {
        let mut out = String::new();
        self.subtree_text(self.root(), &mut out);
        out
    }
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

    /// Inline-hidden detection (style/aria only; computed styles need the
    /// renderer, which is a later PR's port).
    pub fn hidden_inline(&self) -> bool {
        let style = self
            .attr("style")
            .map(|s| s.to_ascii_lowercase().replace(' ', ""))
            .unwrap_or_default();
        if style.contains("display:none") || style.contains("visibility:hidden") {
            return true;
        }
        self.attr("aria-hidden").is_some_and(|v| v == "true")
    }
}

/// Materialize the surviving body-level blocks back into tree `Element`s
/// for the converter (beta returns pruned HTML blocks; we hand the
/// converter pruned tree nodes to skip a serialize/parse round trip).
pub fn surviving_elements(dom: &Dom) -> Vec<Element> {
    let mut out = Vec::new();
    // Start at `body` when present (skip head/title), else the root. The
    // well-formed tree nests body under html, so search recursively for
    // the body element rather than among the root's direct children only
    // (fragments without html/body fall back to the root itself).
    fn find_body(dom: &Dom, id: NodeId) -> Option<NodeId> {
        if dom.tag_of(id) == "body" {
            return Some(id);
        }
        for &child in dom.children_of(id) {
            if dom.get(child).is_some_and(ArenaNode::is_element)
                && let Some(found) = find_body(dom, child)
            {
                return Some(found);
            }
        }
        None
    }
    let start = find_body(dom, dom.root()).unwrap_or_else(|| dom.root());
    let ids: Vec<NodeId> = dom
        .children_of(start)
        .to_vec()
        .into_iter()
        .filter(|&id| dom.get(id).is_some_and(ArenaNode::is_element))
        .collect();
    for id in ids {
        out.push(materialize(id, dom));
    }
    out
}

fn materialize(id: NodeId, dom: &Dom) -> Element {
    let (tag, attrs) = match dom.get(id) {
        Some(ArenaNode::Element { tag, attrs, .. }) => (tag.clone(), attrs.clone()),
        _ => (String::new(), Vec::new()),
    };
    let mut el = Element {
        tag,
        attrs,
        children: Vec::new(),
    };
    for &child in dom.children_of(id) {
        match dom.get(child) {
            Some(ArenaNode::Text(t)) => el.children.push(Node::Text(t.clone())),
            Some(ArenaNode::Element { .. }) => {
                el.children.push(Node::Element(materialize(child, dom)));
            }
            None => {}
        }
    }
    el
}
