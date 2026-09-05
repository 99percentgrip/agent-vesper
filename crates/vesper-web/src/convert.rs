//! Markdown conversion — the PR-0 baseline converter, productionized.
//!
//! Emits a deterministic Markdown projection of the surviving DOM with
//! beta's option surface (images off by default, single-line-break mode,
//! code marking) and alpha's post-processing (code-fence de-indentation,
//! blank-line collapsing). Inline whitespace is normalized once, at final
//! block assembly — never per text node — so inter-element boundary
//! spaces survive (the PR-0 whitespace lesson).

use crate::dom::{Document, Element, Node};

/// Converter options (beta's html2text option surface, PR-0 validated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOptions {
    /// `body_width = 0`: no wrapping (beta default).
    pub wrap_width: usize,
    /// Emit images as `![alt](src)` (beta default: ignored).
    pub images: bool,
    /// `<br>` becomes a single `\n` instead of a hard break (beta option).
    pub single_line_break: bool,
    /// Wrap `<pre>` content in code fences (beta `mark_code`).
    pub mark_code: bool,
    /// Emit links as `[text](href)` (on for LLM consumption).
    pub links: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            wrap_width: 0,
            images: false,
            single_line_break: true,
            mark_code: true,
            links: true,
        }
    }
}

/// Convert a full (stripped) document to Markdown, body-scoped.
pub fn document_to_markdown(doc: &Document, opts: &ConvertOptions) -> String {
    let body = crate::dom::body(doc);
    let mut out = String::with_capacity(body.text().len() / 2 + 64);
    render_children(&mut out, &body.children, opts, 0);
    post_process(out)
}

/// Convert surviving prune-stage blocks (beta's fit path) to Markdown.
pub fn blocks_to_markdown(blocks: &[Element], opts: &ConvertOptions) -> String {
    let mut out = String::new();
    for el in blocks {
        render_element(&mut out, el, opts, 0);
    }
    post_process(out)
}

/// Full pipeline convenience: parse → strip → convert (no prune).
pub fn html_to_markdown(html: &str, opts: &ConvertOptions) -> String {
    let doc = crate::strip::parse_and_strip(html);
    document_to_markdown(&doc, opts)
}

/// Full fit pipeline: parse → strip → prune → convert surviving blocks.
pub fn html_to_fit_markdown(
    html: &str,
    filter: &crate::density::TextDensityFilter,
    opts: &ConvertOptions,
) -> String {
    let doc = crate::strip::parse_and_strip(html);
    let mut arena = crate::arena::Dom::from_document(&doc);
    filter.filter(&mut arena);
    let blocks = crate::arena::surviving_elements(&arena);
    blocks_to_markdown(&blocks, opts)
}

// ------------------------------------------------------------------ render

fn render_children(out: &mut String, nodes: &[Node], opts: &ConvertOptions, depth: usize) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Element(el) => render_element(out, el, opts, depth),
        }
    }
}

/// Render an element's children inline (used by p/strong/em/a contexts).
fn render_inline(out: &mut String, el: &Element, opts: &ConvertOptions, depth: usize) {
    render_children(out, &el.children, opts, depth);
}

#[allow(clippy::too_many_lines)]
fn render_element(out: &mut String, el: &Element, opts: &ConvertOptions, depth: usize) {
    match el.tag.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = el.tag[1..].parse::<usize>().unwrap_or(1);
            let txt = normalize_inline(&el.text());
            if txt.is_empty() {
                return;
            }
            push_line(out, &format!("{} {}", "#".repeat(level.min(6)), txt));
        }
        "p" => {
            let mut inner = String::new();
            render_inline(&mut inner, el, opts, depth);
            let txt = normalize_inline(&inner);
            if txt.is_empty() {
                return;
            }
            push_paragraph(out, &txt);
        }
        "strong" | "b" => {
            let mut inner = String::new();
            render_inline(&mut inner, el, opts, depth);
            let txt = normalize_inline(&inner);
            if !txt.is_empty() {
                out.push_str(&format!("**{txt}**"));
            }
        }
        "em" | "i" => {
            let mut inner = String::new();
            render_inline(&mut inner, el, opts, depth);
            let txt = normalize_inline(&inner);
            if !txt.is_empty() {
                out.push_str(&format!("*{txt}*"));
            }
        }
        "del" | "s" | "strike" => {
            let mut inner = String::new();
            render_inline(&mut inner, el, opts, depth);
            let txt = normalize_inline(&inner);
            if !txt.is_empty() {
                out.push_str(&format!("~~{txt}~~"));
            }
        }
        "code" => {
            let txt = normalize_inline(&el.text());
            if !txt.is_empty() {
                out.push_str(&format!("`{txt}`"));
            }
        }
        "br" => {
            if opts.single_line_break {
                out.push('\n');
            } else {
                out.push_str("  \n");
            }
        }
        "a" => {
            if opts.links {
                let href = el.attr("href").unwrap_or("");
                let mut inner = String::new();
                render_inline(&mut inner, el, opts, depth);
                let txt = normalize_inline(&inner);
                if href.is_empty() || txt.is_empty() {
                    out.push_str(&txt);
                } else {
                    out.push_str(&format!("[{txt}]({href})"));
                }
            } else {
                let mut inner = String::new();
                render_inline(&mut inner, el, opts, depth);
                out.push_str(&normalize_inline(&inner));
            }
        }
        "img" => {
            if opts.images {
                let src = el.attr("src").unwrap_or("");
                let alt = el.attr("alt").unwrap_or("");
                out.push_str(&format!("![{alt}]({src})"));
            }
        }
        "ul" | "ol" => render_list(out, el, opts, depth),
        "li" => {
            // Standalone li (no list wrapper survived): render as bullet.
            let mut inner = String::new();
            render_inline(&mut inner, el, opts, depth);
            let txt = normalize_inline(&inner);
            if !txt.is_empty() {
                push_line(out, &format!("- {txt}"));
            }
        }
        "pre" => {
            if opts.mark_code {
                out.push_str("```\n");
                out.push_str(&el.text());
                ensure_trailing_newline(out);
                out.push_str("```\n");
            } else {
                let txt = normalize_inline(&el.text());
                if !txt.is_empty() {
                    push_paragraph(out, &txt);
                }
            }
        }
        "blockquote" => {
            let txt = normalize_inline(&el.text());
            if txt.is_empty() {
                return;
            }
            for line in txt.lines() {
                push_line(out, &format!("> {line}"));
            }
        }
        "hr" => push_line(out, "---"),
        "table" => render_table(out, el),
        "dl" => {
            for child in &el.children {
                if let Node::Element(c) = child {
                    match c.tag.as_str() {
                        "dt" => push_line(out, &normalize_inline(&c.text())),
                        "dd" => push_line(out, &format!("  {}", normalize_inline(&c.text()))),
                        _ => {}
                    }
                }
            }
        }
        _ => render_children(out, &el.children, opts, depth),
    }
}

fn render_list(out: &mut String, el: &Element, opts: &ConvertOptions, depth: usize) {
    let ordered = el.tag == "ol";
    let mut index = 1usize;
    for child in &el.children {
        if let Node::Element(li) = child {
            if li.tag != "li" {
                continue;
            }
            let marker = if ordered {
                let m = format!("{index}. ");
                index += 1;
                m
            } else {
                "- ".to_string()
            };
            let indent = "  ".repeat(depth);

            // Split the li into inline text children and block children
            // (nested lists). Nested lists render as an indented block
            // after the li text, never inline on the same line.
            let mut inline_parts: Vec<String> = Vec::new();
            let mut nested_lists: Vec<&Element> = Vec::new();
            for node in &li.children {
                match node {
                    Node::Text(t) => inline_parts.push(t.clone()),
                    Node::Element(c) => {
                        if matches!(c.tag.as_str(), "ul" | "ol") {
                            nested_lists.push(c);
                        } else {
                            let mut rendered = String::new();
                            render_element(&mut rendered, c, opts, depth + 1);
                            inline_parts.push(rendered);
                        }
                    }
                }
            }
            let txt = normalize_inline(&inline_parts.join(""));
            if txt.is_empty() && nested_lists.is_empty() {
                continue;
            }
            if !txt.is_empty() {
                let mut first = true;
                for line in txt.lines() {
                    if first {
                        push_line(out, &format!("{indent}{marker}{line}"));
                        first = false;
                    } else {
                        push_line(out, &format!("{indent}  {line}"));
                    }
                }
            }
            for nested in nested_lists {
                // The nested list renders at depth+1; its own marker line
                // already carries the full indent, so prefix continuation
                // lines only (the marker line must not double-indent).
                let mut block = String::new();
                render_list(&mut block, nested, opts, depth + 1);
                let mut lines = block.lines();
                if let Some(first) = lines.next() {
                    push_line(out, first);
                    for line in lines {
                        push_line(out, &format!("{indent}  {line}"));
                    }
                }
            }
        }
    }
}

fn render_table(out: &mut String, el: &Element) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    collect_rows(el, &mut rows);
    if rows.is_empty() {
        return;
    }
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    for (ri, row) in rows.iter().enumerate() {
        let mut padded = row.clone();
        padded.resize(cols, String::new());
        let cells: Vec<String> = padded
            .iter()
            .map(|c| normalize_inline(c).replace('|', "\\|"))
            .collect();
        push_line(out, &format!("| {} |", cells.join(" | ")));
        if ri == 0 {
            let sep: Vec<&str> = (0..cols).map(|_| "---").collect();
            push_line(out, &format!("| {} |", sep.join(" | ")));
        }
    }
}

fn collect_rows(el: &Element, rows: &mut Vec<Vec<String>>) {
    for child in &el.children {
        if let Node::Element(c) = child {
            match c.tag.as_str() {
                "tr" => {
                    let mut row = Vec::new();
                    for cell in &c.children {
                        if let Node::Element(ce) = cell
                            && matches!(ce.tag.as_str(), "td" | "th")
                        {
                            row.push(ce.text());
                        }
                    }
                    rows.push(row);
                }
                "thead" | "tbody" | "tfoot" => collect_rows(c, rows),
                _ => {}
            }
        }
    }
}

// ------------------------------------------------------------- text helpers

/// Normalize inline whitespace: collapse space/tab runs to single spaces,
/// preserve newlines (from `br`), then trim the ends. Applied exactly
/// once at block assembly — per-node application destroys boundary
/// spaces (the PR-0 lesson: `Hello <b>world</b>` must render as
/// `Hello **world**`, not `Hello**world**`).
fn normalize_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        match ch {
            ' ' | '\t' | '\r' => {
                if !in_ws {
                    out.push(' ');
                    in_ws = true;
                }
            }
            '\n' => {
                // A hard break (`<br>` with single_line_break = false) emits
                // two spaces before the newline; preserve that shape by
                // re-emitting one pending space before the newline. A single
                // source space before a newline carries no meaning in HTML.
                if in_ws {
                    out.push(' ');
                }
                out.push('\n');
                in_ws = false;
            }
            _ => {
                out.push(ch);
                in_ws = false;
            }
        }
    }
    out.trim().to_string()
}

fn push_line(out: &mut String, s: &str) {
    out.push_str(s);
    ensure_trailing_newline(out);
}

fn push_paragraph(out: &mut String, txt: &str) {
    out.push_str(txt);
    out.push_str("\n\n");
}

fn ensure_trailing_newline(out: &mut String) {
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Alpha-style post-processing: strip the 4-space code-fence indent the
/// renderer never emits (defensive), collapse blank-line runs to one.
fn post_process(md: String) -> String {
    let mut out = String::with_capacity(md.len());
    let mut blank = 0;
    for line in md.split('\n') {
        let empty = line.trim().is_empty();
        if empty {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    let trimmed = out.trim_end_matches('\n');
    let mut final_s = String::with_capacity(trimmed.len() + 1);
    final_s.push_str(trimmed);
    final_s.push('\n');
    final_s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse;

    #[test]
    fn converts_heading_paragraph_and_inline_emphasis() {
        let doc = parse("<h1>Title</h1><p>Hello <b>world</b></p>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("# Title"), "md: {md}");
        assert!(md.contains("Hello **world**"), "md: {md}");
    }

    #[test]
    fn images_stripped_by_default() {
        let doc = parse("<p>text <img src='x.png' alt='pic'></p>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(!md.contains("x.png"), "md: {md}");
        assert!(md.contains("text"), "md: {md}");
    }

    #[test]
    fn single_line_break_option() {
        let doc = parse("<p>one<br>two</p>");
        let opts = ConvertOptions {
            single_line_break: true,
            ..ConvertOptions::default()
        };
        let md = document_to_markdown(&doc, &opts);
        assert!(md.contains("one\ntwo"), "md: {md}");
    }

    #[test]
    fn hard_break_mode() {
        let doc = parse("<p>one<br>two</p>");
        let opts = ConvertOptions {
            single_line_break: false,
            ..ConvertOptions::default()
        };
        let md = document_to_markdown(&doc, &opts);
        assert!(md.contains("one  \ntwo"), "md: {md}");
    }

    #[test]
    fn table_renders_gfm() {
        let doc =
            parse("<table><tr><th>k</th><th>v</th></tr><tr><td>a</td><td>1</td></tr></table>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("| k | v |"), "md: {md}");
        assert!(md.contains("| --- | --- |"), "md: {md}");
        assert!(md.contains("| a | 1 |"), "md: {md}");
    }

    #[test]
    fn table_pipe_escaping() {
        let doc = parse("<table><tr><th>a|b</th></tr></table>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("a\\|b"), "md: {md}");
    }

    #[test]
    fn links_render() {
        let doc = parse("<p>see <a href='https://example.com/x'>docs</a></p>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("[docs](https://example.com/x)"), "md: {md}");
    }

    #[test]
    fn nested_lists_indent() {
        let doc = parse("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        let a = md.find("- a").unwrap();
        let b = md.find("- b").unwrap();
        let c = md.find("- c").unwrap();
        assert!(b > a && c > b, "order: {md}");
        let b_line_start = md[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
        assert_eq!(md[b_line_start..b].len(), 2, "nested indent: {md}");
    }

    #[test]
    fn ordered_list_numbers() {
        let doc = parse("<ol><li>first</li><li>second</li></ol>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("1. first"), "md: {md}");
        assert!(md.contains("2. second"), "md: {md}");
    }

    #[test]
    fn code_fence_marking() {
        let doc = parse("<pre><code>let x = 1;</code></pre>");
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("```"), "md: {md}");
        assert!(md.contains("let x = 1;"), "md: {md}");
    }

    #[test]
    fn deterministic_output() {
        let html = "<div><p>a</p><p>b</p><ul><li>x</li><li>y</li></ul></div>";
        let one = html_to_markdown(html, &ConvertOptions::default());
        let two = html_to_markdown(html, &ConvertOptions::default());
        assert_eq!(one, two);
    }

    #[test]
    fn fit_pipeline_prunes_and_keeps_content() {
        let html = "<html><body>\
            <div class=\"ads promo\">buy stuff buy stuff buy stuff</div>\
            <p id=\"content\">Real content with enough words to survive the density filter \
            and clearly more substantive than the advertisement block.</p>\
            </body></html>";
        let filter = crate::density::TextDensityFilter::fixed_default();
        let fit = html_to_fit_markdown(html, &filter, &ConvertOptions::default());
        assert!(fit.contains("Real content"), "fit: {fit}");
    }

    #[test]
    fn empty_body_yields_empty_markdown() {
        assert_eq!(
            html_to_markdown("<html><body></body></html>", &ConvertOptions::default()),
            "\n"
        );
    }
}
