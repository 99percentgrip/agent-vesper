//! InteractableMap: the token-efficient, numbered tree the model sees
//! (VRO-14 PR-4, gamma's serializer port).
//!
//! Two guarantees, both unit-tested:
//!
//! 1. **Token efficiency.** Only interactive nodes carry index tags; text
//!    is capped; hidden/paint-occluded subtrees are skipped entirely —
//!    gamma's discipline of never paying tokens for what cannot be used.
//! 2. **Index stability.** Indices are assigned from the
//!    [`SelectorMapCache`], keyed by `(session_id, backend_node_id)`: a
//!    node that persists across DOM mutations keeps its number, new nodes
//!    take fresh numbers, and removed numbers are never reused within a
//!    session (gamma's cross-step stability contract, which keeps the
//!    model's references meaningful between steps).
//!
//! The output is markdown-ish: `- [17] <button> Submit` lines under
//! their nearest interactive ancestor context.

use crate::interactable::{is_interactive, redact_sensitive};
use crate::snapshot::{MaterializedDocument, MaterializedNode};
use std::collections::HashMap;

/// A `(session_id, backend_node_id)` pair — the stable identity of one
/// interactable element across steps.
pub type NodeKey = (String, i64);

/// Assigns stable per-session indices to interactable nodes.
///
/// Gamma keys its cached selector map the same way: the backend node id
/// is the browser's own stable identifier for a DOM node (it survives
/// attribute/text changes and only changes when the node is replaced),
/// and the session id scopes the mapping to one tab.
#[derive(Debug, Default, Clone)]
pub struct SelectorMapCache {
    session_id: String,
    /// `(session, backend_node_id)` -> assigned index.
    assigned: HashMap<NodeKey, usize>,
    /// Indexes retired by node removal; never reassigned in-session.
    retired: Vec<usize>,
    next_index: usize,
}

impl SelectorMapCache {
    /// A cache for one browser session/tab.
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            assigned: HashMap::new(),
            retired: Vec::new(),
            next_index: 1,
        }
    }

    /// The session this cache is scoped to.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The stable index for one node, assigning a fresh one when the node
    /// is new to this session. Indices are 1-based and monotonically
    /// increasing; a removed node's index is retired, never reused.
    pub fn index_for(&mut self, backend_node_id: i64) -> usize {
        let key: NodeKey = (self.session_id.clone(), backend_node_id);
        if let Some(existing) = self.assigned.get(&key) {
            return *existing;
        }
        let fresh = self.next_index;
        self.next_index += 1;
        self.assigned.insert(key, fresh);
        fresh
    }

    /// How many distinct nodes have ever been indexed this session.
    #[must_use]
    pub fn total_assigned(&self) -> usize {
        self.assigned.len()
    }

    /// Retire a node's index (its DOM node was removed). The index is not
    /// returned to the pool — model-visible references must never point
    /// at a different element later in the same session.
    pub fn retire(&mut self, backend_node_id: i64) {
        let key: NodeKey = (self.session_id.clone(), backend_node_id);
        if let Some(index) = self.assigned.remove(&key) {
            self.retired.push(index);
        }
    }

    /// Retired (never-reused) indexes, in retirement order.
    #[must_use]
    pub fn retired_indexes(&self) -> &[usize] {
        &self.retired
    }

    /// The stable index for a node without assigning (lookup only).
    #[must_use]
    pub fn peek(&self, backend_node_id: i64) -> Option<usize> {
        self.assigned
            .get(&(self.session_id.clone(), backend_node_id))
            .copied()
    }
}

/// One line of the serialized map (debugging and test assertions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLine {
    /// Stable interactive index (`[n]` in the output).
    pub index: usize,
    /// Depth in the rendered tree.
    pub depth: usize,
    /// Tag name, lowercased.
    pub tag: String,
    /// Redacted, capped visible label (text, aria-label, placeholder,
    /// value, or role fallback).
    pub label: String,
}

/// Serialize a materialized document into the numbered, token-efficient
/// markdown-ish tree. Interactive nodes get `[n]` tags from `cache`;
/// non-interactive text is included only when it gives the interactive
/// element its label; hidden subtrees are skipped.
pub fn serialize_interactable_map(
    doc: &MaterializedDocument,
    cache: &mut SelectorMapCache,
) -> String {
    let lines = collect_lines(doc, cache);
    let mut out = String::new();
    for line in &lines {
        let indent = "  ".repeat(line.depth);
        out.push_str(&format!(
            "{}- [{}] <{}> {}\n",
            indent, line.index, line.tag, line.label
        ));
    }
    out
}

/// Only the interactive lines (assertions; also the compact form hosts
/// may prefer when token budgets are tight).
pub fn interactable_lines(
    doc: &MaterializedDocument,
    cache: &mut SelectorMapCache,
) -> Vec<MapLine> {
    collect_lines(doc, cache)
}

fn collect_lines(doc: &MaterializedDocument, cache: &mut SelectorMapCache) -> Vec<MapLine> {
    let mut out = Vec::new();
    // The flat snapshot lists every node with a parent index; the roots
    // (negative parents, the document shells) open the walk. Depth derives
    // from the ancestor chain, not recursion over owned children.
    for (position, node) in doc.nodes.iter().enumerate() {
        if node.parent_index.is_none() {
            walk(position, 0, doc, cache, &mut out);
        }
    }
    out
}

fn walk(
    id: usize,
    depth: usize,
    doc: &MaterializedDocument,
    cache: &mut SelectorMapCache,
    out: &mut Vec<MapLine>,
) {
    let Some(node) = doc.nodes.get(id) else {
        return;
    };
    if !node_visible(node) {
        return; // hidden subtree: zero tokens
    }
    if is_interactive(node) {
        // Only nodes with a backend id can carry a stable session index.
        if let Some(backend_id) = node.backend_node_id {
            let index = cache.index_for(backend_id);
            out.push(MapLine {
                index,
                depth,
                tag: node.tag_name.clone(),
                label: label_for(node),
            });
        }
        // Children of an interactive node rarely add value; gamma caps
        // them too. Do not recurse.
        return;
    }
    for (position, child) in doc.nodes.iter().enumerate() {
        if child.parent_index == Some(id) {
            walk(position, depth + 1, doc, cache, out);
        }
    }
}

fn node_visible(node: &MaterializedNode) -> bool {
    let display = node.style("display").unwrap_or("");
    if display.eq_ignore_ascii_case("none") {
        return false;
    }
    let visibility = node.style("visibility").unwrap_or("");
    if visibility.eq_ignore_ascii_case("hidden") {
        return false;
    }
    if let Some(opacity) = node.style("opacity").and_then(|o| o.parse::<f32>().ok())
        && opacity <= 0.0
    {
        return false;
    }
    true
}

/// Cap labels at this many characters (token efficiency).
pub const LABEL_CAP: usize = 60;

/// Build the display label for one interactive node, redacted and capped.
/// Priority mirrors gamma's hint order: visible text, aria-label,
/// placeholder, value (redacted), title, then role/name fallbacks.
pub fn label_for(node: &MaterializedNode) -> String {
    let mut label = redact_sensitive(node).unwrap_or_default();
    if label.len() > LABEL_CAP {
        // Cut on a char boundary.
        let mut end = LABEL_CAP;
        while end > 0 && !node_is_char_boundary(&label, end) {
            end -= 1;
        }
        label.truncate(end);
        label.push('…');
    }
    label
}

fn node_is_char_boundary(s: &str, index: usize) -> bool {
    s.is_char_boundary(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::tests_support::build_document;

    fn doc_with_button_and_link() -> MaterializedDocument {
        // <button style="cursor:pointer">Save</button>
        // <a href="x">Next page</a>
        build_document(&[
            (1, "button", "Save", &[("cursor", "pointer")], vec![2]),
            (2, "#text", "Save", &[][..], vec![]),
            (3, "a", "Next page", &[("cursor", "pointer")], vec![4]),
            (4, "#text", "Next page", &[][..], vec![]),
        ])
    }

    #[test]
    fn indices_are_stable_across_rebuilds() {
        let doc = doc_with_button_and_link();
        let mut cache = SelectorMapCache::new("sess-1");

        let first = interactable_lines(&doc, &mut cache);
        assert_eq!(first.len(), 2);
        let (b1, l1) = (first[0].index, first[1].index);

        // Same document serialized again: identical indices.
        let second = interactable_lines(&doc, &mut cache);
        assert_eq!(second[0].index, b1);
        assert_eq!(second[1].index, l1);

        // A *reordered* document (link first): indices follow nodes, not
        // positions — the core stability contract.
        let reordered = build_document(&[
            (3, "a", "Next page", &[("cursor", "pointer")], vec![4]),
            (4, "#text", "Next page", &[], vec![]),
            (1, "button", "Save", &[("cursor", "pointer")], vec![2]),
            (2, "#text", "Save", &[], vec![]),
        ]);
        let third = interactable_lines(&reordered, &mut cache);
        assert_eq!(third[0].index, l1, "link keeps its number");
        assert_eq!(third[1].index, b1, "button keeps its number");
    }

    #[test]
    fn fresh_nodes_take_fresh_indices_retired_never_reused() {
        let doc = doc_with_button_and_link();
        let mut cache = SelectorMapCache::new("sess-1");
        let _ = interactable_lines(&doc, &mut cache);

        // The button's node is replaced (new backend id), the link stays.
        let mutated = build_document(&[
            (3, "a", "Next page", &[("cursor", "pointer")], vec![4]),
            (4, "#text", "Next page", &[], vec![]),
            (9, "button", "Save", &[("cursor", "pointer")], vec![10]),
            (10, "#text", "Save", &[], vec![]),
        ]);
        cache.retire(101);
        let lines = interactable_lines(&mutated, &mut cache);
        // Link kept index 2 (assigned second), new button got 3 — not the
        // retired 1.
        assert_eq!(lines[0].index, 2);
        assert_eq!(lines[1].index, 3);
        assert_eq!(cache.retired_indexes(), &[1]);
    }

    #[test]
    fn sessions_are_independent() {
        let doc = doc_with_button_and_link();
        let mut s1 = SelectorMapCache::new("sess-1");
        let mut s2 = SelectorMapCache::new("sess-2");
        let a = interactable_lines(&doc, &mut s1);
        let b = interactable_lines(&doc, &mut s2);
        // Both sessions number independently from 1.
        assert_eq!(a[0].index, 1);
        assert_eq!(b[0].index, 1);
        assert_ne!(s1.total_assigned(), 0);
    }

    #[test]
    fn hidden_subtrees_cost_zero_tokens() {
        let doc = build_document(&[
            (1, "div", "", &[("display", "none")], vec![2]),
            (2, "button", "Invisible", &[("cursor", "pointer")], vec![]),
            (3, "button", "Visible", &[("cursor", "pointer")], vec![]),
        ]);
        let mut cache = SelectorMapCache::new("s");
        let lines = interactable_lines(&doc, &mut cache);
        assert_eq!(lines.len(), 1, "only the visible button: {lines:?}");
        assert_eq!(lines[0].label, "Visible");
    }

    #[test]
    fn output_is_numbered_markdown_ish() {
        let doc = doc_with_button_and_link();
        let mut cache = SelectorMapCache::new("s");
        let text = serialize_interactable_map(&doc, &mut cache);
        assert!(text.contains("- [1] <button> Save"), "map: {text}");
        assert!(text.contains("- [2] <a> Next page"), "map: {text}");
    }

    #[test]
    fn labels_are_capped() {
        let long = "x".repeat(200);
        let doc = build_document(&[
            (1, "button", &long, &[("cursor", "pointer")], vec![2]),
            (2, "#text", &long, &[], vec![]),
        ]);
        let mut cache = SelectorMapCache::new("s");
        let lines = interactable_lines(&doc, &mut cache);
        assert!(lines[0].label.chars().count() <= LABEL_CAP + 1);
    }
}
