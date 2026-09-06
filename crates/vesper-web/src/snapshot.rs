//! CDP DOMSnapshot serde types (VRO-14 PR-4, gamma port).
//!
//! Wire-faithful deserialization of `DOMSnapshot.captureSnapshot` results:
//! the flattened per-document node arrays, the layout tree, and the
//! computed-style mapping. All Action-Engine perception (visibility,
//! clickability, bounds) derives from these fields, so these types are the
//! single source of truth for how the engine sees a page.
//!
//! The computed-style extraction exposes exactly the styles the pinned
//! gamma source names in its `REQUIRED_COMPUTED_STYLES` table **plus the
//! two overflow axes it lists separately** — 10 entries total (the PRD
//! said "11"; the pinned table is authoritative and the count is
//! documented here honestly):
//! display, visibility, opacity, overflow, overflow-x, overflow-y,
//! cursor, pointer-events, position, background-color. Requesting extra
//! styles from the browser costs snapshot time on heavy pages, so the
//! list is intentionally minimal.

use serde::Deserialize;

/// CDP sparse string/integer columns. The array form reads early recorded
/// fixtures; live CDP sends parallel `index` and `value` arrays.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SparseValues {
    /// Live sparse column.
    Sparse { index: Vec<usize>, value: Vec<i64> },
    /// Legacy dense fixture column.
    Dense(Vec<i64>),
}
impl SparseValues {
    fn get(&self, row: usize) -> Option<i64> {
        match self {
            Self::Sparse { index, value } => index
                .iter()
                .position(|i| *i == row)
                .and_then(|i| value.get(i))
                .copied(),
            Self::Dense(values) => values.get(row).copied(),
        }
    }
}

/// CDP sparse boolean column (presence means true).
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct SparseBoolean {
    /// Rows whose value is true.
    pub index: Vec<usize>,
}

/// The exact computed styles the Action Engine consumes (pinned gamma
/// table). Order matters: `computedStyles` arrays index into this list.
pub const REQUIRED_COMPUTED_STYLES: [&str; 10] = [
    "display",
    "visibility",
    "opacity",
    "overflow",
    "overflow-x",
    "overflow-y",
    "cursor",
    "pointer-events",
    "position",
    "background-color",
];

/// The whole `captureSnapshot` result: one entry per document (the main
/// frame plus any same-process iframes).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CaptureSnapshotResult {
    /// Flattened documents in the snapshot.
    pub documents: Vec<DocumentSnapshot>,
    /// The string table every index in the snapshot refers to.
    pub strings: Vec<String>,
}

/// One document's flattened tree (`documents[i]`).
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct DocumentSnapshot {
    /// The document's nodes.
    #[serde(default)]
    pub nodes: NodeTreeSnapshot,
    /// The layout tree (bounds, paints, styles) for the same nodes.
    #[serde(default)]
    pub layout: LayoutTreeSnapshot,
    /// The document's URL, as an index into `strings` (CDP `documentURL`).
    #[serde(default, rename = "documentURL")]
    pub document_url: Option<i64>,
    /// Base URL for resolving relative references (`strings` index).
    #[serde(default, rename = "baseURL")]
    pub base_url: Option<i64>,
}

/// `documents[i].nodes` — parallel arrays; index `j` describes node `j`.
///
/// Every field is optional because the browser omits empty arrays; a
/// missing array means "no node has this property", which the accessor
/// helpers translate to `None` per node.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct NodeTreeSnapshot {
    /// Parent index per node (root's parent is omitted).
    #[serde(default, rename = "parentIndex")]
    pub parent_index: Option<Vec<i64>>,
    /// Node type per node (1 = element, 3 = text, …).
    #[serde(default, rename = "nodeType")]
    pub node_type: Option<Vec<i64>>,
    /// Node name (lowercase tag for elements) as `strings` indices.
    #[serde(default, rename = "nodeName")]
    pub node_name: Option<Vec<i64>>,
    /// Node value (text content) as `strings` indices.
    #[serde(default, rename = "nodeValue")]
    pub node_value: Option<Vec<i64>>,
    /// Text nodes' full text, `strings`-indexed (CDP `textValue`).
    #[serde(default, rename = "textValue")]
    pub text_value: Option<SparseValues>,
    /// Live input value, string-table indexed; redacted at materialization.
    #[serde(default, rename = "inputValue")]
    pub input_value: Option<SparseValues>,
    /// Listener-aware browser clickability signal.
    #[serde(default, rename = "isClickable")]
    pub is_clickable: Option<SparseBoolean>,
    /// Flattened attribute pairs: `[name_idx, value_idx, …]` per node.
    #[serde(default)]
    pub attributes: Option<Vec<Vec<i64>>>,
    /// Per-node shadow-root types, `strings`-indexed.
    #[serde(default, rename = "shadowRootType")]
    pub shadow_root_type: Option<SparseValues>,
    /// Link from an iframe owner to its document in this capture.
    #[serde(default, rename = "contentDocumentIndex")]
    pub content_document_index: Option<SparseValues>,
    /// The CDP backend node id per node — the stable identity the
    /// selector-map cache keys on across snapshots.
    #[serde(default, rename = "backendNodeId")]
    pub backend_node_id: Option<Vec<i64>>,
}

/// `documents[i].layout` — parallel arrays aligned to layout objects.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct LayoutTreeSnapshot {
    /// Layout-object → node-index mapping (CDP `nodeIndex`).
    #[serde(default, rename = "nodeIndex")]
    pub node_index: Option<Vec<i64>>,
    /// Computed styles per layout object, `strings`-indexed.
    #[serde(default)]
    pub styles: Option<Vec<Vec<i64>>>,
    /// Absolute page-absolute bounds per layout object, in CSS pixels:
    /// `[x, y, width, height]` quads flattened.
    #[serde(default)]
    pub bounds: Option<Vec<Vec<f64>>>,
    /// Paint order per layout object (occlusion filtering input; CDP
    /// `paintOrders`).
    #[serde(default, rename = "paintOrders")]
    pub paint_orders: Option<Vec<i64>>,
    /// Whether each layout object is visible/in-layout.
    #[serde(default, rename = "isInLayout")]
    pub is_in_layout: Option<Vec<bool>>,
}

/// A materialized, index-resolved view of one node with everything the
/// heuristics need — the deserialized arrays are never walked directly by
/// the heuristics layer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MaterializedNode {
    /// Position within `documents[i].nodes` (per-document node index).
    pub node_index: usize,
    /// Parent's node index within the same document.
    pub parent_index: Option<usize>,
    /// CDP node type (1 = element, 3 = text).
    pub node_type: i64,
    /// Lowercased tag name for elements (empty for text nodes).
    pub tag_name: String,
    /// Whether a JS click listener was observed for this node (gamma's
    /// CDP `DOMDebugger` signal; carried on the snapshot for the
    /// heuristics layer).
    pub has_js_click_listener: bool,
    /// Node value (text content), resolved from the string table.
    pub node_value: Option<String>,
    /// Attributes as (name, value) pairs (names lowercased).
    pub attributes: Vec<(String, String)>,
    /// Backend node id — stable across mutations within a session.
    pub backend_node_id: Option<i64>,
    /// Computed styles resolved from the style table (name → value).
    pub computed_styles: std::collections::BTreeMap<String, String>,
    /// Paint-order hint (higher paints later / on top).
    pub paint_order: Option<i64>,
    /// Bounds `[x, y, width, height]` in CSS pixels when laid out.
    pub bounds: Option<[f64; 4]>,
    /// Current input value (from `DOM.resolveNode`/snapshot value attrs).
    pub input_value: Option<String>,
    /// Current checked state for checkbox/radio inputs.
    pub input_checked: Option<bool>,
    /// Owned children (tree form; populated for tree-form consumers).
    pub children: Vec<MaterializedNode>,
}

/// Back-compat alias (the flat form and the tree form are one type).
pub type SnapshotNode = MaterializedNode;

impl MaterializedNode {
    /// Attribute lookup (name matched case-insensitively).
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Style lookup (name matched case-insensitively).
    #[must_use]
    pub fn style(&self, name: &str) -> Option<&str> {
        self.computed_styles.get(name).map(String::as_str)
    }

    /// Whether the node is an element (CDP type 1).
    #[must_use]
    pub fn is_element(&self) -> bool {
        self.node_type == 1
    }
}

/// A document's nodes fully materialized with parent links resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MaterializedDocument {
    /// Materialized nodes in node-index order (the flat form is
    /// authoritative: `node_index` equals the position in this list).
    pub nodes: Vec<MaterializedNode>,
    /// The document's URL, resolved from the string table.
    pub url: Option<String>,
    /// Root node indexes (nodes whose parent is None) into `nodes`.
    pub roots: Vec<usize>,
    /// Owned root nodes with owned children (tree form), aligned with
    /// `roots`.
    pub root_nodes: Vec<MaterializedNode>,
}

impl CaptureSnapshotResult {
    /// Materialize document `doc_idx`: resolve every string index, join
    /// the layout arrays to nodes, and resolve parent links. Returns an
    /// empty document when the index is out of range.
    #[must_use]
    pub fn materialize(&self, doc_idx: usize) -> MaterializedDocument {
        let mut out = MaterializedDocument::default();
        let Some(document) = self.documents.get(doc_idx) else {
            return out;
        };
        let str_at = |idx: Option<i64>| -> Option<String> {
            idx.and_then(|i| self.strings.get(usize::try_from(i).ok()?).cloned())
        };
        out.url = str_at(document.document_url);

        let node_count = document.nodes.node_type.as_ref().map_or(0, Vec::len);
        let empty_i64: Vec<i64> = Vec::new();
        let node_names = document.nodes.node_name.as_ref().unwrap_or(&empty_i64);
        let node_types = document.nodes.node_type.as_ref().unwrap_or(&empty_i64);
        let parents = document.nodes.parent_index.as_ref().unwrap_or(&empty_i64);
        let values = document.nodes.node_value.as_ref().unwrap_or(&empty_i64);
        let backend_ids = document
            .nodes
            .backend_node_id
            .as_ref()
            .unwrap_or(&empty_i64);

        // Join layout rows to node indices once.
        let layout_nodes = document.layout.node_index.as_ref().unwrap_or(&empty_i64);
        let layout_styles = document.layout.styles.as_ref();
        let layout_bounds = document.layout.bounds.as_ref();
        let layout_paints = document.layout.paint_orders.as_ref();
        let layout_rows: std::collections::HashMap<_, _> = layout_nodes
            .iter()
            .enumerate()
            .map(|(row, node)| (*node, row))
            .collect();
        let clickable: std::collections::HashSet<_> = document
            .nodes
            .is_clickable
            .as_ref()
            .map(|column| column.index.iter().copied().collect())
            .unwrap_or_default();

        for j in 0..node_count {
            let layout_row = layout_rows.get(&(j as i64)).copied();
            let style_map = layout_row.and_then(|row| {
                let styles = layout_styles?.get(row)?;
                let mut map = std::collections::BTreeMap::new();
                for (slot, style_index) in styles.iter().enumerate() {
                    let Some(name) = REQUIRED_COMPUTED_STYLES.get(slot) else {
                        break;
                    };
                    let value = str_at(Some(*style_index)).unwrap_or_default();
                    map.insert(name.to_string(), value);
                }
                Some(map)
            });
            let bounds = layout_row.and_then(|row| {
                let quad = layout_bounds?.get(row)?;
                if quad.len() < 4 {
                    return None;
                }
                Some([quad[0], quad[1], quad[2], quad[3]])
            });
            let paint_order = layout_row.and_then(|row| layout_paints?.get(row).copied());

            let mut attributes = document
                .nodes
                .attributes
                .as_ref()
                .and_then(|per_node| per_node.get(j))
                .map(|flat| {
                    flat.chunks(2)
                        .filter_map(|pair| {
                            let name = str_at(pair.first().copied())?;
                            let value = str_at(pair.get(1).copied()).unwrap_or_default();
                            Some((name.to_ascii_lowercase(), value))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let attr = |key: &str| {
                attributes
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.as_str())
            };
            let sensitive =
                crate::interactable::is_sensitive_value(attr("type"), attr("autocomplete"));
            let input_value = str_at(
                document
                    .nodes
                    .input_value
                    .as_ref()
                    .and_then(|column| column.get(j)),
            )
            .map(|value| {
                if sensitive {
                    crate::interactable::redact(value.chars().count())
                } else {
                    value
                }
            });
            if sensitive {
                for (key, value) in &mut attributes {
                    if key == "value" {
                        *value = crate::interactable::redact(value.chars().count());
                    }
                }
            }
            if let Some(value) = &input_value {
                attributes.retain(|(key, _)| key != "value");
                attributes.push(("value".into(), value.clone()));
            }
            if let Some(kind) = str_at(
                document
                    .nodes
                    .shadow_root_type
                    .as_ref()
                    .and_then(|column| column.get(j)),
            ) {
                attributes.push(("data-vesper-shadow-root".into(), kind));
            }
            if let Some(index) = document
                .nodes
                .content_document_index
                .as_ref()
                .and_then(|column| column.get(j))
            {
                attributes.push(("data-vesper-frame-document".into(), index.to_string()));
            }

            out.nodes.push(MaterializedNode {
                node_index: j,
                parent_index: parents
                    .get(j)
                    .and_then(|p| usize::try_from(*p).ok().filter(|p| *p < j)),
                node_type: node_types.get(j).copied().unwrap_or(0),
                tag_name: str_at(node_names.get(j).copied())
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
                has_js_click_listener: clickable.contains(&j),
                node_value: str_at(values.get(j).copied()),
                attributes,
                backend_node_id: backend_ids.get(j).copied(),
                computed_styles: style_map.unwrap_or_default(),
                paint_order,
                bounds,
                input_value,
                input_checked: None,
                children: Vec::new(),
            });
        }

        // Assemble the owned tree (cloned) from `parent_index` while the
        // flat list stays authoritative: walks use parent links over
        // `nodes`, and `roots`/`root_nodes` expose the tree form.
        let mut by_parent: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        let mut root_indexes: Vec<usize> = Vec::new();
        for node in &out.nodes {
            match node.parent_index {
                Some(parent) if parent != node.node_index => {
                    by_parent.entry(parent).or_default().push(node.node_index);
                }
                _ => root_indexes.push(node.node_index),
            }
        }
        fn clone_subtree(
            nodes: &[MaterializedNode],
            root: usize,
            kids_of: &std::collections::HashMap<usize, Vec<usize>>,
            depth: usize,
        ) -> MaterializedNode {
            let mut node = nodes[root].clone();
            if depth >= 256 {
                return node;
            }
            let child_list = kids_of.get(&root).cloned().unwrap_or_default();
            node.children = child_list
                .into_iter()
                .filter(|&child_index| nodes.get(child_index).is_some())
                .map(|child_index| clone_subtree(nodes, child_index, kids_of, depth + 1))
                .collect();
            node
        }
        let mut assembled: Vec<MaterializedNode> = Vec::new();
        for &root_index in &root_indexes {
            if out.nodes.get(root_index).is_some() {
                assembled.push(clone_subtree(&out.nodes, root_index, &by_parent, 0));
            }
        }
        out.roots = root_indexes;
        out.root_nodes = assembled;
        out
    }
}

/// Test-only helpers shared across modules (builds documents from concise
/// tuples without JSON).
///
/// `(identity, tag, text, styles, child_ids)` → one document. The first
/// field is the node's **identity** (used for `backend_node_id`, so
/// stability tests can reorder rows and keep identity), `child_ids`
/// reference other rows by their identity. `node_index` is the position,
/// matching `materialize`'s contract.
/// Test/builder support: compiled for both unit tests (`cfg(test)`) and
/// integration tests (which build the crate as a dependency, where
/// `cfg(test)` is off). Marked `#[doc(hidden)]` so it never appears in
/// public documentation; it is not part of the crate's stable surface.
#[doc(hidden)]
pub mod tests_support {
    use super::{MaterializedDocument, MaterializedNode};

    /// One builder row: (identity, tag, text, styles, child_identities).
    pub type Row<'a> = (
        usize,
        &'a str,
        &'a str,
        &'a [(&'a str, &'a str)],
        Vec<usize>,
    );

    #[must_use]
    pub fn build_document(rows: &[Row<'_>]) -> MaterializedDocument {
        let mut doc = MaterializedDocument::default();
        let mut listed_as_child: Vec<usize> = Vec::new();
        for row in rows {
            listed_as_child.extend(row.4.iter().copied());
        }
        for (position, (identity, tag, text, styles, _kids)) in rows.iter().enumerate() {
            let mut computed = std::collections::BTreeMap::new();
            for (name, value) in *styles {
                computed.insert((*name).to_string(), (*value).to_string());
            }
            let is_text = *tag == "#text";
            let mut attributes: Vec<(String, String)> = Vec::new();
            if !is_text && !text.is_empty() {
                attributes.push(("value".to_string(), (*text).to_string()));
            }
            // Non-CSS "styles" are HTML attributes the tests mean to set:
            // type/autocomplete drive the sensitive-value gate, href makes
            // anchors interactive, placeholder/title feed labels.
            for (name, value) in *styles {
                if matches!(*name, "type" | "autocomplete" | "placeholder" | "title") {
                    attributes.push(((*name).to_string(), (*value).to_string()));
                }
            }
            if *tag == "a" {
                attributes.push(("href".to_string(), "#".to_string()));
            }
            doc.nodes.push(MaterializedNode {
                node_index: position,
                parent_index: None,
                node_type: if is_text { 3 } else { 1 },
                tag_name: if is_text {
                    String::new()
                } else {
                    (*tag).to_string()
                },
                has_js_click_listener: false,
                node_value: Some((*text).to_string()),
                attributes,
                backend_node_id: Some(100 + *identity as i64),
                computed_styles: computed,
                paint_order: None,
                bounds: None,
                input_value: None,
                input_checked: None,
                children: Vec::new(),
            });
        }
        // Map identity → position, then set parent links + roots by
        // identity so reorderings keep node identity stable.
        let id_to_pos: std::collections::HashMap<usize, usize> = rows
            .iter()
            .enumerate()
            .map(|(position, (identity, ..))| (*identity, position))
            .collect();
        for (position, (identity, ..)) in rows.iter().enumerate() {
            let kids: Vec<usize> = rows[position]
                .4
                .iter()
                .filter_map(|child_id| id_to_pos.get(child_id).copied())
                .collect();
            for child_pos in kids {
                if let Some(child) = doc.nodes.get_mut(child_pos) {
                    child.parent_index = Some(position);
                }
            }
            let _ = identity;
        }
        doc.roots = rows
            .iter()
            .enumerate()
            .filter(|(_, (identity, ..))| !listed_as_child.contains(identity))
            .map(|(position, _)| position)
            .collect();
        // Tree form: clone per root.
        let mut by_parent: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();
        for node in &doc.nodes {
            if let Some(parent) = node.parent_index {
                by_parent.entry(parent).or_default().push(node.node_index);
            }
        }
        fn clone_subtree(
            nodes: &[MaterializedNode],
            root: usize,
            kids_of: &std::collections::HashMap<usize, Vec<usize>>,
        ) -> MaterializedNode {
            let mut node = nodes[root].clone();
            let child_list = kids_of.get(&root).cloned().unwrap_or_default();
            node.children = child_list
                .into_iter()
                .filter(|&child_index| nodes.get(child_index).is_some())
                .map(|child_index| clone_subtree(nodes, child_index, kids_of))
                .collect();
            node
        }
        doc.root_nodes = doc
            .roots
            .iter()
            .filter_map(|&root| {
                doc.nodes
                    .get(root)
                    .map(|_| clone_subtree(&doc.nodes, root, &by_parent))
            })
            .collect();
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_sparse_values_are_masked_and_click_listeners_survive() {
        let capture: CaptureSnapshotResult = serde_json::from_value(serde_json::json!({"strings":["INPUT","type","password","secret-canary"],"documents":[{"nodes":{"nodeType":[1],"nodeName":[0],"backendNodeId":[17],"attributes":[[1,2]],"inputValue":{"index":[0],"value":[3]},"shadowRootType":{"index":[],"value":[]},"isClickable":{"index":[0]}}}]})).unwrap();
        let doc = capture.materialize(0);
        assert!(doc.nodes[0].has_js_click_listener);
        assert!(!format!("{doc:?}").contains("secret-canary"));
        assert_eq!(doc.nodes[0].input_value.as_deref(), Some("••••••••"));
    }

    /// A minimal capture with the fields the heuristics use. Style rows
    /// are authored against REQUIRED_COMPUTED_STYLES slots and this exact
    /// strings table (see the cursor test): slot0 display, slot1
    /// visibility, … slot6 cursor.
    fn recorded() -> CaptureSnapshotResult {
        serde_json::from_str(
            r#"{
              "documents": [
                {
                  "documentURL": 0,
                  "nodes": {
                    "parentIndex": [-1, 0, 1, 1],
                    "nodeType":    [9, 1, 1, 3],
                    "nodeName":    [1, 2, 3, 4],
                    "nodeValue":   [-1, -1, -1, 5],
                    "backendNodeId": [1, 2, 3, 4],
                    "attributes": [[], [], [5, 6, 7, 8], []]
                  },
                  "layout": {
                    "nodeIndex": [1, 2],
                    "styles": [[9, 10, 11, 12, 13, 14, 15, 16, 17, 18], [9, 19, 11, 12, 13, 14, 15, 16, 17, 18]],
                    "bounds": [[0, 0, 800, 600], [10, 10, 100, 30]],
                    "paintOrders": [0, 1]
                  }
                }
              ],
              "strings": [
                "https://example.invalid/", "html", "body", "button", "Submit",
                "class", "cta", "data-testid", "go", "block", "visible", "1",
                "auto", "auto", "auto", "pointer", "auto", "static", "rgba(0,0,0,0)",
                "hidden"
              ]
            }"#,
        )
        .expect("recorded snapshot parses")
    }

    #[test]
    fn required_styles_match_pinned_table() {
        assert_eq!(REQUIRED_COMPUTED_STYLES.len(), 10);
        assert_eq!(REQUIRED_COMPUTED_STYLES[0], "display");
        assert_eq!(REQUIRED_COMPUTED_STYLES[6], "cursor");
        assert_eq!(REQUIRED_COMPUTED_STYLES[7], "pointer-events");
        assert_eq!(REQUIRED_COMPUTED_STYLES[9], "background-color");
    }

    #[test]
    fn deserializes_recorded_capture() {
        let result = recorded();
        assert_eq!(result.documents.len(), 1);
        assert_eq!(result.strings.len(), 20);
        let nodes = result.documents[0].nodes.node_type.as_ref().unwrap();
        assert_eq!(nodes.len(), 4);
        let backend = result.documents[0].nodes.backend_node_id.as_ref().unwrap();
        assert_eq!(backend.len(), 4);
    }

    #[test]
    fn materialize_resolves_strings_styles_and_bounds() {
        let result = recorded();
        let doc = result.materialize(0);
        assert_eq!(doc.url.as_deref(), Some("https://example.invalid/"));
        assert_eq!(doc.nodes.len(), 4);

        let button = &doc.nodes[2];
        assert_eq!(button.tag_name, "button");
        assert_eq!(button.attr("class"), Some("cta"));
        assert_eq!(button.attr("data-testid"), Some("go"));
        assert_eq!(button.backend_node_id, Some(3));
        assert_eq!(button.style("cursor"), Some("pointer"));
        assert_eq!(button.style("display"), Some("block"));
        assert_eq!(button.bounds, Some([10.0, 10.0, 100.0, 30.0]));
        assert_eq!(button.paint_order, Some(1));

        // Row 1 (node 2) carries the `hidden` visibility override; the
        // body's row keeps `visible`.
        let body = &doc.nodes[1];
        assert_eq!(body.style("visibility"), Some("visible"));
        assert_eq!(button.style("visibility"), Some("hidden"));

        // Tree form: roots + owned children.
        assert_eq!(doc.roots, vec![0]);
        assert_eq!(doc.root_nodes.len(), 1);
        assert_eq!(doc.root_nodes[0].tag_name, "html");
        assert_eq!(doc.root_nodes[0].children.len(), 1);
    }

    #[test]
    fn materialize_out_of_range_document_is_empty() {
        let result = recorded();
        assert!(result.materialize(7).nodes.is_empty());
    }

    #[test]
    fn materialize_survives_missing_optional_arrays() {
        // A capture with no layout and no attributes at all.
        let minimal: CaptureSnapshotResult = serde_json::from_str(
            r##"{"documents": [{"nodes": {"nodeType": [9, 3],
               "nodeName": [1, 2], "nodeValue": [-1, 3]}}], "strings": ["", "#text", "hi"]}"##,
        )
        .expect("minimal capture parses");
        let doc = minimal.materialize(0);
        assert_eq!(doc.nodes.len(), 2);
        assert!(doc.nodes[0].computed_styles.is_empty());
        assert!(doc.nodes[1].bounds.is_none());
    }
}
