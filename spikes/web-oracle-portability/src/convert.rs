//! VRO-14 PR-0 spike: minimal self-contained HTML → Markdown converter.
//!
//! Objective: prove a converter can be written against the stripped/pruned
//! DOM with the options the PRD needs (image stripping, single-line-break
//! paragraphs, code fences, GFM tables, link citation mode). This replaces
//! the unreachable external converter crates in the offline sandbox
//! (see VERDICT.md) and pins deterministic output.

use crate::dom::{Document, Element, Node};

/// Beta's option surface (markdown_generation_strategy defaults), adapted:
/// body_width 0 (no wrapping), single_line_break, mark_code, images off.
pub struct ConvertOptions {
    pub body_width: usize,          // 0 = no wrapping
    pub single_line_break: bool,    // paragraphs join with one newline
    pub mark_code: bool,            // emit fenced code blocks
    pub images: bool,               // false = strip images entirely
    pub links: bool,                // false = plain text (citation mode drops inline links)
    pub heading_style_atx: bool,    // ATX headings (#)
    pub list_bullets: &'static str, // "-*+" any; fixed "-" for determinism
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            body_width: 0,
            single_line_break: true,
            mark_code: true,
            images: false,
            links: true,
            heading_style_atx: true,
            list_bullets: "-",
        }
    }
}

/// Convert a full stripped Document to markdown.
pub fn document_to_markdown(doc: &Document, opts: &ConvertOptions) -> String {
    let body = doc.body().unwrap_or(&doc.root);
    let mut out = String::with_capacity(body.text().len() / 2 + 64);
    render_children(&mut out, &body.children, opts, 0);
    post_process(out)
}

/// Convert surviving filter blocks (beta's fit path) to markdown.
pub fn blocks_to_markdown(blocks: &[Element], opts: &ConvertOptions) -> String {
    let mut out = String::new();
    for el in blocks {
        render_element(&mut out, el, opts, 0);
    }
    post_process(out)
}

/// Render an element's children as inline content (no block breaks):
/// used by p/strong/em/code/br contexts.
fn render_inline_children(out: &mut String, el: &Element, opts: &ConvertOptions, depth: usize) {
    render_children(out, &el.children, opts, depth);
}

fn render_children(out: &mut String, nodes: &[Node], opts: &ConvertOptions, depth: usize) {
    for n in nodes {
        match n {
            // Raw text: boundary spaces must survive until the owning block
            // collapses whitespace once (per-node collapse+trim ate "Hello "
            // before "<b>world</b>" could join it — fixed by deferring).
            Node::Text(t) => out.push_str(t),
            Node::Element(el) => render_element(out, el, opts, depth),
        }
    }
}

fn render_element(out: &mut String, el: &Element, opts: &ConvertOptions, depth: usize) {
    match el.tag.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = el.tag[1..].parse::<usize>().unwrap_or(1);
            let txt = el.text();
            if txt.trim().is_empty() {
                return;
            }
            push_line(out, &format!("{} {}", "#".repeat(level), collapse_ws(&txt)));
        }
        "p" => {
            let mut inner = String::new();
            render_inline_children(&mut inner, el, opts, 0);
            let txt = collapse_ws_keep_breaks(&inner);
            if txt.is_empty() {
                return;
            }
            push_paragraph(out, &txt, opts);
        }
        "strong" | "b" => {
            let mut inner = String::new();
            render_inline_children(&mut inner, el, opts, 0);
            let txt = collapse_ws(&inner);
            if !txt.is_empty() {
                out.push_str(&format!("**{txt}**"));
            }
        }
        "em" | "i" => {
            let mut inner = String::new();
            render_inline_children(&mut inner, el, opts, depth);
            let txt = collapse_ws(&inner);
            if !txt.is_empty() {
                out.push_str(&format!("*{txt}*"));
            }
        }
        "code" => {
            let txt = collapse_ws(&el.text());
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
        "table" => render_table(out, el, opts),
        "a" => {
            if opts.links {
                let href = el.attr("href").unwrap_or("");
                let txt = collapse_ws(&el.text());
                if href.is_empty() || txt.is_empty() {
                    out.push_str(&txt);
                } else {
                    out.push_str(&format!("[{txt}]({href})"));
                }
            } else {
                out.push_str(&collapse_ws(&el.text()));
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
            let txt = collapse_ws(&el.text());
            if !txt.is_empty() {
                push_line(out, &format!("{} {}", opts.list_bullets, txt));
            }
        }
        "pre" => {
            if opts.mark_code {
                out.push_str("```\n");
                out.push_str(&el.text());
                ensure_trailing_newline(out);
                out.push_str("```\n");
            } else {
                let txt = collapse_ws(&el.text());
                if !txt.is_empty() {
                    push_paragraph(out, &txt, opts);
                }
            }
        }
        "div" | "section" | "article" | "main" | "span" | "figure" | "figcaption"
        | "details" | "summary" | "address" | "time" | "small" => {
            render_children(out, &el.children, opts, depth);
        }
        "dl" => {
            for child in &el.children {
                if let Node::Element(c) = child {
                    if c.tag == "dt" {
                        push_line(out, &collapse_ws(&c.text()));
                    } else if c.tag == "dd" {
                        push_line(out, &format!("  {}", collapse_ws(&c.text())));
                    }
                }
            }
        }
        _ => {
            // Unknown/other tags: render children transparently (lenient).
            render_children(out, &el.children, opts, depth);
        }
    }
}

fn render_list(out: &mut String, el: &Element, opts: &ConvertOptions, depth: usize) {
    let ordered = el.tag == "ol";
    let mut idx = 1usize;
    for child in &el.children {
        if let Node::Element(li) = child {
            if li.tag != "li" {
                continue;
            }
            let marker = if ordered {
                let m = format!("{idx}.");
                idx += 1;
                m
            } else {
                opts.list_bullets.to_string()
            };
            // Nested lists inside li: render text first, then recurse.
            let mut li_text = String::new();
            let mut nested: Vec<&Element> = Vec::new();
            for c in &li.children {
                match c {
                    Node::Text(t) => li_text.push_str(&collapse_ws(t)),
                    Node::Element(ce) => {
                        if matches!(ce.tag.as_str(), "ul" | "ol") {
                            nested.push(ce);
                        } else {
                            let mut sub = String::new();
                            render_element(&mut sub, ce, opts, depth + 1);
                            li_text.push_str(&sub);
                        }
                    }
                }
            }
            let indent = "  ".repeat(depth.max(1) - 1);
            push_line(out, &format!("{indent}{marker} {}", collapse_ws(&li_text)));
            for n in nested {
                render_list(out, n, opts, depth + 1);
            }
        }
    }
}

fn render_table(out: &mut String, el: &Element, opts: &ConvertOptions) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    collect_rows(el, &mut rows);
    if rows.is_empty() {
        return;
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    // Header = first row (GFM).
    let header = &rows[0];
    let mut line = String::from("| ");
    for c in 0..cols {
        let cell = header.get(c).map(String::as_str).unwrap_or("");
        line.push_str(&cell.replace('|', "\\|"));
        line.push_str(" | ");
    }
    push_line(out, line.trim_end());
    let mut sep = String::from("|");
    for _ in 0..cols {
        sep.push_str(" --- |");
    }
    push_line(out, &sep);
    for row in rows.iter().skip(1) {
        let mut line = String::from("| ");
        for c in 0..cols {
            let cell = row.get(c).map(String::as_str).unwrap_or("");
            line.push_str(&cell.replace('|', "\\|"));
            line.push_str(" | ");
        }
        push_line(out, line.trim_end());
    }
    let _ = opts;
}

fn collect_rows(el: &Element, rows: &mut Vec<Vec<String>>) {
    for child in &el.children {
        if let Node::Element(c) = child {
            match c.tag.as_str() {
                "tr" => {
                    let mut cells = Vec::new();
                    for cc in &c.children {
                        if let Node::Element(cell) = cc {
                            if matches!(cell.tag.as_str(), "td" | "th") {
                                cells.push(collapse_ws(&cell.text()));
                            }
                        }
                    }
                    if !cells.is_empty() {
                        rows.push(cells);
                    }
                }
                "thead" | "tbody" | "tfoot" => collect_rows(c, rows),
                _ => {}
            }
        }
    }
}

fn push_line(out: &mut String, s: &str) {
    out.push_str(s);
    ensure_trailing_newline(out);
}

fn push_paragraph(out: &mut String, txt: &str, opts: &ConvertOptions) {
    out.push_str(txt);
    out.push_str(if opts.single_line_break { "\n" } else { "\n\n" });
}

fn ensure_trailing_newline(out: &mut String) {
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Collapse horizontal whitespace runs but PRESERVE newlines emitted by
/// `br` handling — used at paragraph assembly time.
fn collapse_ws_keep_breaks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch == '\n' {
            out.push('\n');
            in_ws = false;
        } else if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out.trim_matches(|c: char| c != '\n' && c.is_whitespace())
        .to_string()
}

/// Collapse internal whitespace runs (not newlines inside pre) for inline text.
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out.trim().to_string()
}

/// Alpha-style post-processing: de-indent code fences, collapse blank runs.
fn post_process(md: String) -> String {
    let mut out = String::with_capacity(md.len());
    let mut blank = 0;
    for line in md.split('\n') {
        let t = line.strip_prefix("    ").unwrap_or(line);
        let empty = t.trim().is_empty();
        if empty {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(t);
        out.push('\n');
    }
    out.trim_end_matches('\n').to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse;

    #[test]
    fn converts_basic_paragraph_and_heading() {
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
    fn table_renders_gfm() {
        let doc = parse(
            "<table><tr><th>k</th><th>v</th></tr><tr><td>a</td><td>1</td></tr></table>",
        );
        let md = document_to_markdown(&doc, &ConvertOptions::default());
        assert!(md.contains("| k | v |"), "md: {md}");
        assert!(md.contains("| --- | --- |"), "md: {md}");
        assert!(md.contains("| a | 1 |"), "md: {md}");
    }

    #[test]
    fn deterministic_output() {
        let html = "<div><p>a</p><ul><li>x</li><li>y</li></ul></div>";
        let a = document_to_markdown(&parse(html), &ConvertOptions::default());
        let b = document_to_markdown(&parse(html), &ConvertOptions::default());
        assert_eq!(a, b);
    }
}
