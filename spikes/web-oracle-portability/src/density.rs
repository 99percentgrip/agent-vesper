//! VRO-14 PR-0 spike: heuristic density measurement + retention sentinels.

use crate::dom::{ArenaNode, Dom, NodeId};

/// Beta's exact metric weights (content_filter_strategy.py):
/// text_density 0.4, link_density 0.2, tag_weight 0.2,
/// class_id_weight 0.1, text_length 0.1 — weighted mean over enabled
/// metrics (each metric adds weight to the running total).
pub const W_TEXT_DENSITY: f32 = 0.4;
pub const W_LINK_DENSITY: f32 = 0.2;
pub const W_TAG_WEIGHT: f32 = 0.2;
pub const W_CLASS_ID_WEIGHT: f32 = 0.1;
pub const W_TEXT_LENGTH: f32 = 0.1;

pub struct TextDensityFilter {
    pub threshold: f32,
    pub threshold_type: ThresholdType,
    pub min_word_threshold: Option<usize>,
    pub preserve_classes: Vec<String>,
    pub preserve_tags: Vec<String>,
}

pub enum ThresholdType {
    Fixed,
    Dynamic,
}

impl TextDensityFilter {
    pub fn fixed_default() -> Self {
        TextDensityFilter {
            threshold: 0.48,
            threshold_type: ThresholdType::Fixed,
            min_word_threshold: None,
            preserve_classes: vec![],
            preserve_tags: vec![],
        }
    }

    fn is_preserved(&self, node: &ArenaNode) -> bool {
        if self
            .preserve_tags
            .iter()
            .any(|t| node.tag().eq_ignore_ascii_case(t))
        {
            return true;
        }
        if !self.preserve_classes.is_empty() {
            if let Some(cls) = node.attr("class") {
                let parts: Vec<&str> = cls.split_whitespace().collect();
                if self
                    .preserve_classes
                    .iter()
                    .any(|c| parts.iter().any(|p| p.eq_ignore_ascii_case(c)))
                {
                    return true;
                }
            }
        }
        false
    }

    /// Composite score for one element node (beta's
    /// `_compute_composite_score`).
    pub fn score_node(&self, dom: &Dom, id: NodeId) -> f32 {
        let Some(node) = dom.get(id) else { return 0.0 };
        if !node.is_element() {
            return 0.0;
        }

        let text_len = dom.text_len(id) as f32;
        let tag_len = dom.tag_len(id).max(1) as f32;
        let link_text_len = dom.link_text_len(id) as f32;

        if let Some(min_words) = self.min_word_threshold {
            let words = dom.text_of(id).split_whitespace().count();
            if words < min_words {
                return -1.0; // guaranteed removal
            }
        }

        let mut score = 0.0f32;
        let mut total_w = 0.0f32;

        score += W_TEXT_DENSITY * (text_len / tag_len);
        total_w += W_TEXT_DENSITY;

        let ld = 1.0 - (if text_len > 0.0 {
            link_text_len / text_len
        } else {
            0.0
        });
        score += W_LINK_DENSITY * ld;
        total_w += W_LINK_DENSITY;

        score += W_TAG_WEIGHT * tag_weight(node.tag());
        total_w += W_TAG_WEIGHT;

        score += W_CLASS_ID_WEIGHT * class_id_weight(node).max(0.0);
        total_w += W_CLASS_ID_WEIGHT;

        score += W_TEXT_LENGTH * (text_len + 1.0).ln();
        total_w += W_TEXT_LENGTH;

        score / total_w
    }

    /// Bottom-up prune: children first, then the node itself.
    pub fn filter(&self, dom: &mut Dom) {
        // Beta removes excluded tags inside filter_content before pruning:
        // port the same pre-pass so the filter alone reproduces the
        // documented behavior (nav/footer/header/aside/...).
        let excluded: &[&str] = &[
            "nav", "footer", "header", "aside", "script", "style", "form",
            "iframe", "noscript",
        ];
        for id in dom.post_order() {
            if id == dom.root() {
                continue;
            }
            if let Some(node) = dom.get(id) {
                if node.is_element() && excluded.contains(&node.tag()) {
                    dom.detach(id);
                }
            }
        }
        let root = dom.root();
        for id in dom.post_order() {
            if id == root {
                continue;
            }
            let Some(node) = dom.get(id) else { continue };
            if !node.is_element() || self.is_preserved(node) {
                continue;
            }
            let score = self.score_node(dom, id);
            let remove = match self.threshold_type {
                ThresholdType::Fixed => score < self.threshold,
                ThresholdType::Dynamic => {
                    let mut t = self.threshold;
                    let ti = tag_importance(node.tag());
                    let text_len = dom.text_len(id) as f32;
                    let tag_len = dom.tag_len(id).max(1) as f32;
                    let link_text_len = dom.link_text_len(id) as f32;
                    if ti > 1.0 {
                        t *= 0.8;
                    }
                    if text_len / tag_len > 0.4 {
                        t *= 0.9;
                    }
                    if text_len > 0.0 && link_text_len / text_len > 0.6 {
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

fn tag_weight(tag: &str) -> f32 {
    match tag {
        "article" => 1.5,
        "main" => 1.4,
        "section" => 1.0,
        "p" => 1.0,
        "h1" => 1.2,
        "h2" => 1.1,
        "h3" => 1.0,
        "h4" => 0.9,
        "h5" => 0.8,
        "h6" => 0.7,
        "div" => 0.5,
        "span" => 0.3,
        "li" => 0.5,
        "ul" => 0.5,
        "ol" => 0.5,
        "td" | "th" | "tr" => 0.5,
        _ => 0.5,
    }
}

fn tag_importance(tag: &str) -> f32 {
    match tag {
        "article" => 1.5,
        "main" => 1.4,
        "section" => 1.3,
        "p" => 1.2,
        "h1" => 1.4,
        "h2" => 1.3,
        "h3" => 1.2,
        "div" => 0.5,
        _ => 0.7,
    }
}

/// Beta's negative-pattern intent (`nav|footer|header|sidebar|ads|comment|
/// promo|advert|social|share`, case-insensitive) without a regex dep.
const NEGATIVE_TOKENS: &[&str] = &[
    "nav", "footer", "header", "sidebar", "ads", "comment", "promo",
    "advert", "social", "share",
];

fn class_id_weight(node: &ArenaNode) -> f32 {
    let mut score = 0.0f32;
    let hay = |v: &str| -> bool {
        let v = v.to_ascii_lowercase();
        NEGATIVE_TOKENS.iter().any(|t| v.contains(t))
    };
    if let Some(c) = node.attr("class") {
        if hay(c) {
            score -= 0.5;
        }
    }
    if let Some(i) = node.attr("id") {
        if hay(i) {
            score -= 0.5;
        }
    }
    score
}

/// Visibility pruning of hidden subtrees (inline styles in the spike).
pub fn prune_hidden(dom: &mut Dom) {
    let mut work: Vec<NodeId> = dom.children_of(dom.root()).to_vec();
    while let Some(id) = work.pop() {
        let hidden = dom
            .get(id)
            .map(|n| n.is_element() && n.hidden_inline())
            .unwrap_or(false);
        if hidden {
            dom.detach(id);
            continue;
        }
        work.extend(dom.children_of(id).iter().copied());
    }
}

/// Does the surviving text contain the sentinel a reader would call the
/// page's substance?
pub fn retention(sentinel: &str, dom: &Dom) -> bool {
    dom.root_text().contains(sentinel)
}

pub struct Measurement {
    pub name: String,
    pub html_bytes: usize,
    pub strip_bytes: usize,
    pub full_md_bytes: usize,
    pub fit_md_bytes: usize,
    pub sentinels_total: usize,
    sentinels_kept: usize,
    fit_sentinels_kept: usize,
}

impl Measurement {
    pub fn md_density(&self) -> f32 {
        ratio(self.fit_md_bytes, self.full_md_bytes)
    }
    pub fn raw_density(&self) -> f32 {
        ratio(self.fit_md_bytes, self.html_bytes)
    }
    pub fn strip_ratio(&self) -> f32 {
        ratio(self.strip_bytes, self.html_bytes)
    }
}

fn ratio(a: usize, b: usize) -> f32 {
    if b == 0 {
        0.0
    } else {
        a as f32 / b as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse_and_strip;

    #[test]
    fn link_farm_scores_lower_than_paragraph() {
        let doc = parse_and_strip(
            "<html><body><div class=\"nav promo\">Menu Menu Menu Menu</div>\
             <p id=\"content\">Real content here with several words</p></body></html>",
        );
        let dom0 = Dom::from_document(&doc);
        let f = TextDensityFilter::fixed_default();
        let mut nav = None;
        let mut para = None;
        let mut work: Vec<NodeId> = dom0.children_of(dom0.root()).to_vec();
        while let Some(id) = work.pop() {
            let (t, cls) = match dom0.get(id) {
                Some(n) if n.is_element() => (n.tag().to_string(), n.attr("class").map(|s| s.to_string())),
                _ => continue,
            };
            if t == "p" {
                para = Some(id);
            }
            if cls.as_deref().unwrap_or("").contains("nav") {
                nav = Some(id);
            }
            work.extend(dom0.children_of(id).iter().copied());
        }
        let (Some(nav), Some(para)) = (nav, para) else { panic!("nodes missing") };
        let nav_score = f.score_node(&dom0, nav);
        let para_score = f.score_node(&dom0, para);
        assert!(
            para_score > nav_score,
            "para {para_score} must beat nav {nav_score}"
        );
    }

    #[test]
    fn filter_keeps_content_and_honors_min_word_threshold() {
        // Faithful-port finding: a short link-less <div class="nav promo">
        // scores ABOVE fixed-0.48 under beta's exact composite (verified
        // against the pinned upstream source: text_density ~0.7-1.0 for
        // plain short text dominates). Beta itself keeps such nodes under
        // the fixed threshold; operators prune them with
        // min_word_threshold or the dynamic threshold. This test pins that
        // verified behavior instead of an invented expectation.
        let doc = parse_and_strip(
            "<html><body><div class=\"nav promo\">Menu Menu Menu Menu</div>\
             <p id=\"content\">Real content here with several words</p></body></html>",
        );
        let mut dom = Dom::from_document(&doc);
        let f = TextDensityFilter::fixed_default();
        f.filter(&mut dom);
        let text = dom.root_text();
        assert!(text.contains("Real content"), "kept: {text}");

        // With min_word_threshold=5 the short nav div (4 words) is
        // guaranteed-removed (beta's -1.0 sentinel) while the longer
        // paragraph survives. Beta counts words as count(" ")+1, so
        // "Menu Menu Menu Menu" is 4 words.
        let doc2 = parse_and_strip(
            "<html><body><div class=\"nav promo\">Menu Menu Menu Menu</div>\
             <p id=\"content\">Real content here with several words</p></body></html>",
        );
        let mut dom2 = Dom::from_document(&doc2);
        let f2 = TextDensityFilter {
            min_word_threshold: Some(5),
            ..TextDensityFilter::fixed_default()
        };
        f2.filter(&mut dom2);
        let text2 = dom2.root_text();
        assert!(!text2.contains("Menu Menu"), "pruned: {text2}");
        assert!(text2.contains("Real content"), "kept: {text2}");
    }

    #[test]
    fn min_word_threshold_guarantees_removal() {
        let doc = parse_and_strip("<div>short</div><p>one two three four five</p>");
        let mut dom = Dom::from_document(&doc);
        let f = TextDensityFilter {
            min_word_threshold: Some(3),
            ..TextDensityFilter::fixed_default()
        };
        f.filter(&mut dom);
        assert!(!dom.root_text().contains("short"));
    }

    #[test]
    fn preserve_classes_short_circuit() {
        let doc = parse_and_strip("<div class=\"sidebar\">keep me despite low density</div>");
        let mut dom = Dom::from_document(&doc);
        let f = TextDensityFilter {
            preserve_classes: vec!["sidebar".into()],
            ..TextDensityFilter::fixed_default()
        };
        f.filter(&mut dom);
        assert!(dom.root_text().contains("keep me"));
    }

    #[test]
    fn negative_class_tokens_lower_scores() {
        let doc = parse_and_strip("<div class=\"footer-links\">a b c d</div><p>x y z w</p>");
        let dom = Dom::from_document(&doc);
        let f = TextDensityFilter::fixed_default();
        let mut footer = None;
        let mut para = None;
        let mut work: Vec<NodeId> = dom.children_of(dom.root()).to_vec();
        while let Some(id) = work.pop() {
            let (t, cls) = match dom.get(id) {
                Some(n) if n.is_element() => (n.tag().to_string(), n.attr("class").map(|s| s.to_string())),
                _ => continue,
            };
            if t == "p" {
                para = Some(id);
            }
            if cls.as_deref().unwrap_or("").contains("footer") {
                footer = Some(id);
            }
            work.extend(dom.children_of(id).iter().copied());
        }
        let (Some(footer), Some(para)) = (footer, para) else { panic!() };
        assert!(f.score_node(&dom, para) > f.score_node(&dom, footer));
    }
}

/// Objective-1 driver: measure byte-in/byte-out density over every fixture.
///
/// For each fixture: parse -> strip -> (a) full markdown from the stripped
/// tree, (b) fit markdown from the density-pruned tree; count sentinel
/// retention. Prints one table row per fixture plus totals.
pub fn run(dir: &str) -> anyhow::Result<()> {
    let dir = std::path::Path::new(dir);
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "html").unwrap_or(false))
        .collect();
    entries.sort();
    if entries.is_empty() {
        anyhow::bail!("no .html fixtures under {}", dir.display());
    }

    println!("== Objective 1: density measurements ({} fixtures) ==", entries.len());
    println!(
        "{:<22} {:>9} {:>9} {:>9} {:>9} {:>7} {:>7}",
        "fixture", "html-in", "strip-in", "full-md", "fit-md", "fit/ful", "sentils"
    );

    let mut totals = Totals::default();
    for path in &entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let html = std::fs::read_to_string(path)?;
        let mut doc = crate::dom::parse(&html);
        let html_bytes = html.len();

        // strip stage (shared by full and fit paths)
        crate::dom::strip(&mut doc);
        // strip stage (shared by full and fit paths): the stripped tree's
        // text-plus-markup size approximates beta's "html in" baseline.
        let strip_bytes = doc.root.content_len();

        // (a) full markdown: convert the whole stripped tree
        let opts = crate::convert::ConvertOptions::default();
        let full_md = crate::convert::document_to_markdown(&doc, &opts);

        // (b) fit markdown: density-prune a fresh copy, then convert
        let mut doc2 = crate::dom::parse(&html);
        crate::dom::strip(&mut doc2);
        let mut arena = crate::dom::Dom::from_document(&doc2);
        prune_hidden(&mut arena);
        let f = TextDensityFilter::fixed_default();
        f.filter(&mut arena);
        let blocks = crate::dom::surviving_elements(&arena);
        let fit_md = crate::convert::blocks_to_markdown(&blocks, &opts);

        let mut m = Measurement {
            name: name.clone(),
            html_bytes,
            strip_bytes,
            full_md_bytes: full_md.len(),
            fit_md_bytes: fit_md.len(),
            sentinels_total: 0,
            sentinels_kept: 0,
            fit_sentinels_kept: 0,
        };
        // Sentinel retention: content-bearing phrases planted in each
        // fixture, checked against full text and fit text.
        let sentinels = sentinel_list(&name);
        for s in &sentinels {
            m.sentinels_total += 1;
            if full_md.contains(s.as_str()) {
                m.sentinels_kept += 1;
            }
            if fit_md.contains(s.as_str()) {
                m.fit_sentinels_kept += 1;
            }
        }
        println!(
            "{:<22} {:>9} {:>9} {:>9} {:>9} {:>6.1}% {:>3}/{:<3}",
            m.name,
            m.html_bytes,
            m.strip_bytes,
            m.full_md_bytes,
            m.fit_md_bytes,
            m.md_density() * 100.0,
            m.fit_sentinels_kept,
            m.sentinels_total
        );
        totals.add(&m);
    }
    totals.print(entries.len());
    Ok(())
}

#[derive(Default)]
struct Totals {
    html: usize,
    strip: usize,
    full: usize,
    fit: usize,
    sent_total: usize,
    sent_fit: usize,
    sent_full: usize,
    n: usize,
}

impl Totals {
    fn add(&mut self, m: &Measurement) {
        self.html += m.html_bytes;
        self.strip += m.strip_bytes;
        self.full += m.full_md_bytes;
        self.fit += m.fit_md_bytes;
        n_add(&mut self.n, 1);
        self.sent_total += m.sentinels_total;
        self.sent_fit += m.fit_sentinels_kept;
        self.sent_full += m.sentinels_kept;
    }
    fn print(&self, fixtures: usize) {
        let pct = |a: usize, b: usize| -> f32 {
            if b == 0 { 0.0 } else { a as f32 / b as f32 * 100.0 }
        };
        println!("---- totals over {fixtures} fixtures ----");
        println!("html-in   {:>9}", self.html);
        println!("strip-in  {:>9}", self.strip);
        println!("full-md   {:>9}", self.full);
        println!("fit-md    {:>9} ({:.1}% of full-md)", self.fit, pct(self.fit, self.full));
        println!("sentinels fit kept  {}/{} ({:.1}%)", self.sent_fit, self.sent_total, pct(self.sent_fit, self.sent_total));
        println!("sentinels full kept {}/{} ({:.1}%)", self.sent_full, self.sent_total, pct(self.sent_full, self.sent_total));
    }
}

fn n_add(n: &mut usize, v: usize) {
    *n += v;
}

/// Content sentinels: sentences that actually exist in the fixtures
/// (verified by grep; see VERDICT.md). Each is body substance a reader
/// would consider the page's payload.
fn sentinel_list(name: &str) -> Vec<String> {
    match name {
        "f01-article.html" => vec![
            "Agentic browser automation needs a hard boundary",
            "Anonymous pipes exist only as file descriptors",
            "The child reads commands on FD 3 and writes events on FD 4",
        ],
        "f02-docs.html" => vec![
            "The policy file is TOML",
            "Denial outranks approval which outranks allowance",
            "Denial is absolute",
        ],
        "f03-landing.html" => vec![
            "Widgets are fast because they are compiled ahead of time",
            "Every widget runs in a locked-down sandbox",
            "Structured logs and bounded traces leave the runtime",
        ],
        "f04-spa-shell.html" => vec![
            "This application requires JavaScript",
        ],
        "f05-forum.html" => vec![
            "Has anyone wired the debugging channel to anonymous pipes",
            "Spawn the child with the pipe flags",
            "Linking the engineering writeup from the platform team",
        ],
        "f06-table-heavy.html" => vec![
            "Latency percentiles across the three deployment targets",
        ],
        "f07-api-docs.html" => vec![
            "Opens a session",
            "Navigates to a URL",
            "Clicks an indexed element",
        ],
        "f08-wiki.html" => vec![
            "Pipes are created",
            "Anonymous pipes exist only as file descriptors",
        ],
        "f09-nav-heavy.html" => vec![
            "Preferences are grouped into profiles",
            "Sessions expire after 30 days of inactivity",
            "Themes are stored per profile",
            "Email digests are sent weekly",
        ],
        "f10-listicle.html" => vec![
            "None of these required new technology",
        ],
        "f11-edge.html" => vec![
            "Plain div text before any structure",
            "Entities:",
            "Unicode: héllo wörld ünïcode",
        ],
        "f12-e-commerce.html" => vec![
            "In stock, ships in 2 days",
            "Machined from a single billet",
            "verified purchase",
        ],
        _ => vec![],
    }
    .into_iter()
    .map(String::from)
    .collect()
}
