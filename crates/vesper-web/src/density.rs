//! VRO-14 PR-1: heuristic content pruning — a faithful port of web oracle
//! beta's `PruningContentFilter` composite score with its exact default
//! weights, dynamic-threshold modifiers, negative class/id tokens, preserve
//! overrides, and the `min_word_threshold` guaranteed-removal sentinel.
//!
//! Pipeline position: runs AFTER [`crate::dom::strip`] (beta removes its
//! excluded tags outright before scoring; the verified finding from PR-0).

use crate::arena::{ArenaNode, Dom, NodeId};
use std::collections::HashSet;

/// Beta's exact metric weights (`content_filter_strategy.py`):
/// text_density 0.4, link_density 0.2, tag_weight 0.2, class_id_weight 0.1,
/// text_length 0.1. Each metric divides by the running total weight, so the
/// composite is a weighted mean.
pub const W_TEXT_DENSITY: f32 = 0.4;
pub const W_LINK_DENSITY: f32 = 0.2;
pub const W_TAG_WEIGHT: f32 = 0.2;
pub const W_CLASS_ID_WEIGHT: f32 = 0.1;
pub const W_TEXT_LENGTH: f32 = 0.1;

/// Beta's fixed default threshold.
pub const DEFAULT_THRESHOLD: f32 = 0.48;

/// Threshold regime (beta's `threshold_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdType {
    /// Fixed comparison against `threshold`.
    Fixed,
    /// Tag-importance/text/link adjusted threshold (beta's dynamic mode).
    Dynamic,
}

/// Beta's negative-pattern class/id tokens, matched case-insensitively as
/// substrings of the class list / id (their regex
/// `nav|footer|header|sidebar|ads|comment|promo|advert|social|share`).
const NEGATIVE_TOKENS: &[&str] = &[
    "nav", "footer", "header", "sidebar", "ads", "comment", "promo", "advert", "social", "share",
];

/// Heuristic pruning filter (beta's `PruningContentFilter` port).
#[derive(Debug, Clone)]
pub struct TextDensityFilter {
    /// Comparison threshold (fixed mode; base for dynamic mode).
    pub threshold: f32,
    /// Fixed or dynamic comparison regime.
    pub threshold_type: ThresholdType,
    /// Minimum subtree word count; below it the node scores −1.0
    /// (beta's guaranteed-removal sentinel).
    pub min_word_threshold: Option<usize>,
    /// CSS class names whose subtrees are always preserved.
    pub preserve_classes: HashSet<String>,
    /// Tag names whose subtrees are always preserved.
    pub preserve_tags: HashSet<String>,
}

impl TextDensityFilter {
    /// Beta's default configuration: fixed threshold 0.48, no word floor,
    /// no preserve overrides.
    #[must_use]
    pub fn fixed_default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            threshold_type: ThresholdType::Fixed,
            min_word_threshold: None,
            preserve_classes: HashSet::new(),
            preserve_tags: HashSet::new(),
        }
    }

    /// Beta's dynamic-threshold configuration.
    #[must_use]
    pub fn dynamic_default() -> Self {
        Self {
            threshold_type: ThresholdType::Dynamic,
            ..Self::fixed_default()
        }
    }

    /// Builder: set the word floor.
    #[must_use]
    pub fn with_min_word_threshold(mut self, min_words: usize) -> Self {
        self.min_word_threshold = Some(min_words);
        self
    }

    /// Builder: preserve by class names.
    #[must_use]
    pub fn with_preserve_classes<I: IntoIterator<Item = String>>(mut self, classes: I) -> Self {
        self.preserve_classes = classes.into_iter().collect();
        self
    }

    /// Builder: preserve by tag names.
    #[must_use]
    pub fn with_preserve_tags<I: IntoIterator<Item = String>>(mut self, tags: I) -> Self {
        self.preserve_tags = tags.into_iter().map(|t| t.to_ascii_lowercase()).collect();
        self
    }

    fn is_preserved(&self, node: &ArenaNode) -> bool {
        if !node.is_element() {
            return true;
        }
        if self.preserve_tags.contains(node.tag()) {
            return true;
        }
        if !self.preserve_classes.is_empty()
            && let Some(cls) = node.attr("class")
        {
            for token in cls.split_ascii_whitespace() {
                if self.preserve_classes.contains(token) {
                    return true;
                }
            }
        }
        false
    }

    /// Composite score for one node (beta's `_compute_composite_score`).
    ///
    /// The per-node measurements mirror beta exactly:
    /// - `text_len`  = length of the subtree's concatenated text
    /// - `tag_len`   = length of the subtree's serialized markup (content)
    /// - `link_text_len` = sum of direct-child `<a>` element text
    /// - word floor  = `text.count(" ") + 1` (beta's own word estimate)
    #[must_use]
    pub fn score_node(&self, dom: &Dom, id: NodeId) -> f32 {
        let Some(node) = dom.get(id) else { return 0.0 };
        if !node.is_element() {
            return 0.0;
        }

        let text_len = dom.text_len(id) as f32;
        let tag_len = dom.tag_len(id).max(1) as f32;
        let link_text_len = dom.link_text_len(id) as f32;

        if let Some(min_words) = self.min_word_threshold {
            // Beta's own word-count estimate (spaces + 1), which rounds up:
            // "a b c" → 3 words.
            let words = dom.text_of(id).matches(' ').count() + 1;
            if words < min_words {
                return -1.0;
            }
        }

        let mut score = 0.0_f32;
        let mut total_w = 0.0_f32;

        // text_density = text_len / tag_len
        score += W_TEXT_DENSITY * (text_len / tag_len);
        total_w += W_TEXT_DENSITY;

        // link_density metric = 1 − (link_text_len / text_len)
        let link_ratio = if text_len > 0.0 {
            link_text_len / text_len
        } else {
            0.0
        };
        score += W_LINK_DENSITY * (1.0 - link_ratio);
        total_w += W_LINK_DENSITY;

        // tag_weight = beta's tag_weights lookup (default 0.5)
        score += W_TAG_WEIGHT * tag_weight(node.tag());
        total_w += W_TAG_WEIGHT;

        // class_id_weight, clamped at zero (beta: max(0, …))
        score += W_CLASS_ID_WEIGHT * class_id_weight(node).max(0.0);
        total_w += W_CLASS_ID_WEIGHT;

        // text_length = ln(text_len + 1)
        score += W_TEXT_LENGTH * (text_len + 1.0).ln();
        total_w += W_TEXT_LENGTH;

        if total_w > 0.0 { score / total_w } else { 0.0 }
    }

    /// Bottom-up prune: children first, then the node itself (beta's
    /// `_prune_tree`, which returns early on preserved nodes — the whole
    /// preserved subtree stays untouched). Detached subtrees vanish from
    /// `surviving_elements`.
    pub fn filter(&self, dom: &mut Dom) {
        let root = dom.root();
        // Pre-pass (top-down): register every preserved subtree root; the
        // bottom-up prune below skips any node with a preserved ancestor —
        // beta's `_prune_tree` returns early on preserved nodes, leaving
        // the entire subtree untouched. Registering ancestors up front is
        // required because post-order visits children before parents.
        let order = dom.pre_order();
        let mut preserved: Vec<bool> = vec![false; dom.len()];
        for &id in &order {
            if id == root {
                continue;
            }
            let is_preserved_node = dom
                .get(id)
                .is_some_and(|node| node.is_element() && self.is_preserved(node));
            if is_preserved_node {
                preserved[id] = true;
            } else if let Some(parent) = dom.parent_of(id) {
                preserved[id] = preserved[parent];
            }
        }
        for id in dom.post_order() {
            if id == root || preserved.get(id).copied().unwrap_or(false) {
                continue;
            }
            let Some(node) = dom.get(id) else { continue };
            if !node.is_element() {
                continue;
            }
            let score = self.score_node(dom, id);
            let remove = match self.threshold_type {
                ThresholdType::Fixed => score < self.threshold,
                ThresholdType::Dynamic => {
                    let mut t = self.threshold;
                    let importance = tag_importance(node.tag());
                    let text_len = dom.text_len(id) as f32;
                    let tag_len = dom.tag_len(id).max(1) as f32;
                    let link_len = dom.link_text_len(id) as f32;
                    if importance > 1.0 {
                        t *= 0.8;
                    }
                    if text_len / tag_len > 0.4 {
                        t *= 0.9;
                    }
                    if text_len > 0.0 && link_len / text_len > 0.6 {
                        t *= 1.2;
                    }
                    score < t
                }
            };
            if remove {
                dom.detach(id);
            }
        }
    }
}

/// Beta's `tag_weights` table (structure tags score above 1.0; wrappers
/// below; unknown default 0.5).
fn tag_weight(tag: &str) -> f32 {
    match tag {
        "article" => 1.5,
        "main" => 1.4,
        "section" | "p" | "h3" => 1.0,
        "h1" => 1.2,
        "h2" => 1.1,
        "h4" => 0.9,
        "h5" => 0.8,
        "h6" => 0.7,
        "div" | "li" | "ul" | "ol" | "td" | "th" | "tr" => 0.5,
        "span" => 0.3,
        _ => 0.5,
    }
}

/// Beta's `tag_importance` table (dynamic threshold modifiers).
fn tag_importance(tag: &str) -> f32 {
    match tag {
        "article" => 1.5,
        "main" => 1.4,
        "section" => 1.3,
        "p" => 1.2,
        "h1" => 1.4,
        "h2" => 1.3,
        "h3" => 1.2,
        "div" => 0.7,
        "span" => 0.6,
        _ => 0.7,
    }
}

/// Beta's `_compute_class_id_weight`: −0.5 per negative token match on the
/// class list, −0.5 on the id (case-insensitive substring).
fn class_id_weight(node: &ArenaNode) -> f32 {
    let mut score = 0.0_f32;
    let is_negative = |value: &str| -> bool {
        let lowered = value.to_ascii_lowercase();
        NEGATIVE_TOKENS.iter().any(|t| lowered.contains(t))
    };
    if let Some(classes) = node.attr("class")
        && is_negative(classes)
    {
        score -= 0.5;
    }
    if let Some(id) = node.attr("id")
        && is_negative(id)
    {
        score -= 0.5;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::Dom;
    use crate::strip::parse_and_strip;

    fn arena(html: &str) -> Dom {
        Dom::from_document(&parse_and_strip(html))
    }

    #[test]
    fn weights_match_beta_defaults() {
        assert!((W_TEXT_DENSITY - 0.4).abs() < f32::EPSILON);
        assert!((W_LINK_DENSITY - 0.2).abs() < f32::EPSILON);
        assert!((W_TAG_WEIGHT - 0.2).abs() < f32::EPSILON);
        assert!((W_CLASS_ID_WEIGHT - 0.1).abs() < f32::EPSILON);
        assert!((W_TEXT_LENGTH - 0.1).abs() < f32::EPSILON);
        assert!((DEFAULT_THRESHOLD - 0.48).abs() < f32::EPSILON);
    }

    #[test]
    fn link_farm_scores_below_paragraph() {
        let dom = arena(
            "<div><a href=\"/a\">alpha link text</a> <a href=\"/b\">beta link text</a></div>\
             <p>Real article content lives here with enough words to score well.</p>",
        );
        let f = TextDensityFilter::fixed_default();
        let mut farm: Option<NodeId> = None;
        let mut para: Option<NodeId> = None;
        let mut work: Vec<NodeId> = dom.children_of(dom.root()).to_vec();
        while let Some(id) = work.pop() {
            match dom.tag_of(id) {
                "div" => farm = Some(id),
                "p" => para = Some(id),
                _ => {}
            }
            work.extend(dom.children_of(id).iter().copied());
        }
        let (Some(farm), Some(para)) = (farm, para) else {
            panic!("nodes missing");
        };
        assert!(
            f.score_node(&dom, para) > f.score_node(&dom, farm),
            "paragraph must outscore the link farm"
        );
    }

    #[test]
    fn negative_class_tokens_lower_scores() {
        let dom = arena("<div class=\"footer-links\">a b c d</div><p>x y z w more words here</p>");
        let f = TextDensityFilter::fixed_default();
        let mut footer: Option<NodeId> = None;
        let mut para: Option<NodeId> = None;
        let mut work: Vec<NodeId> = dom.children_of(dom.root()).to_vec();
        while let Some(id) = work.pop() {
            match dom.tag_of(id) {
                "div" => footer = Some(id),
                "p" => para = Some(id),
                _ => {}
            }
            work.extend(dom.children_of(id).iter().copied());
        }
        let (Some(footer), Some(para)) = (footer, para) else {
            panic!("nodes missing");
        };
        assert!(
            f.score_node(&dom, para) > f.score_node(&dom, footer),
            "negative tokens must lower the footer score"
        );
    }

    #[test]
    fn min_word_threshold_guarantees_removal() {
        let mut dom = arena("<div>short</div><p>one two three four five</p>");
        let f = TextDensityFilter::fixed_default().with_min_word_threshold(3);
        f.filter(&mut dom);
        assert!(!dom.root_text().contains("short"));
    }

    #[test]
    fn preserve_classes_short_circuit() {
        let mut dom = arena("<div class=\"sidebar\">keep me despite low density</div>");
        let f = TextDensityFilter::fixed_default().with_preserve_classes(["sidebar".to_string()]);
        f.filter(&mut dom);
        assert!(dom.root_text().contains("keep me"));
    }

    #[test]
    fn preserve_tags_short_circuit() {
        let mut dom = arena("<table><tr><td>x</td></tr></table><p>content words here</p>");
        let f = TextDensityFilter::fixed_default().with_preserve_tags(["table".to_string()]);
        f.filter(&mut dom);
        assert!(dom.root_text().contains('x'));
    }

    #[test]
    fn fixed_threshold_keeps_content_paragraphs() {
        // Verified against the pinned beta source: a healthy paragraph
        // scores ≈ 1.17 under the composite; fixed 0.48 keeps it.
        let mut dom = arena(
            "<p>Agentic automation needs a hard boundary between the agent process and the host machine it runs on.</p>",
        );
        TextDensityFilter::fixed_default().filter(&mut dom);
        assert!(dom.root_text().contains("Agentic"));
    }

    #[test]
    fn word_floor_uses_beta_space_counting() {
        // "a b c" → 3 words by spaces+1; threshold 4 removes it.
        let mut dom = arena("<p>a b c</p><p>one two three four</p>");
        let f = TextDensityFilter::fixed_default().with_min_word_threshold(4);
        f.filter(&mut dom);
        let text = dom.root_text();
        assert!(!text.contains("a b c"), "3-word node must go: {text}");
        assert!(text.contains("one two three four"));
    }
}
