#![forbid(unsafe_code)]
//! Focused markdown → ratatui [`Line`] renderer for the Agent Vesper TUI.
//!
//! The TUI re-parses the buffered assistant text end-to-end on every frame, so
//! streaming output degrades gracefully:
//! - An open inline marker (`**bold` with no closer) renders the marker
//!   literally so the user sees exactly what has been received.
//! - An unclosed fenced code block (```` ``` ```` with no terminator) renders
//!   the remainder of the buffer as a styled code block so partially-streamed
//!   code is shown in code style immediately.
//!
//! The parser supports the subset the rendering directive requires: **bold**,
//! *italics*, `inline code`, fenced code blocks, ordered and unordered lists
//! (with nesting indentation), and ATX headings (`#`). It never panics and
//! always returns owned [`Line<'static>`] values so they can be moved into a
//! [`ratatui::widgets::Paragraph`] without lifetime juggling. Underscore
//! emphasis is intentionally **not** supported so `snake_case` identifiers are
//! left intact.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Background color for fenced code blocks.
const CODE_BG: Color = Color::Rgb(30, 32, 40);
/// Foreground color for fenced code blocks.
const CODE_FG: Color = Color::Rgb(214, 220, 232);
/// Foreground color for inline `code`.
const INLINE_CODE_FG: Color = Color::Rgb(255, 176, 81);
/// Foreground color for list bullets / numbers and structural markers.
const MARKER_FG: Color = Color::Rgb(255, 176, 81);
/// Foreground color for ATX headings.
const HEADING_FG: Color = Color::Rgb(130, 170, 255);
/// Foreground color for the optional code-block language label.
const CODE_LABEL_FG: Color = Color::Rgb(120, 130, 145);
/// Maximum list nesting depth rendered with indentation (deeper stays at 8).
const MAX_LIST_DEPTH: usize = 8;

/// Parses a markdown document into styled, owned ratatui [`Line`]s.
///
/// Returns an empty vector for empty input. Never panics; safe to call on
/// every frame with partially-streamed text.
#[must_use]
pub fn render_markdown(input: &str) -> Vec<Line<'static>> {
    if input.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = input.split('\n').collect();
    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Fenced code block.
        if let Some((fence_char, fence_len, lang)) = parse_opening_fence(line) {
            let mut code: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() {
                if is_closing_fence(lines[i], fence_char, fence_len) {
                    i += 1;
                    break;
                }
                code.push(lines[i]);
                i += 1;
            }
            // If the loop exited because `i == lines.len()`, the fence ran to
            // EOF (streaming) — render what we collected as a code block.
            push_code_block(&mut out, lang.as_deref(), &code);
            continue;
        }
        // Unordered list item.
        if let Some((depth, marker, text)) = parse_unordered_item(line) {
            push_list_item(&mut out, depth, &marker, text);
            i += 1;
            continue;
        }
        // Ordered list item.
        if let Some((depth, marker, text)) = parse_ordered_item(line) {
            push_list_item(&mut out, depth, &marker, text);
            i += 1;
            continue;
        }
        // ATX heading.
        if let Some((level, text)) = parse_heading(line) {
            push_heading(&mut out, level, text);
            i += 1;
            continue;
        }
        // Blank line — preserve vertical spacing.
        if line.trim().is_empty() {
            out.push(Line::from(Span::raw(String::new())));
            i += 1;
            continue;
        }
        // Paragraph line — apply inline formatting.
        out.push(Line::from(parse_inline(line)));
        i += 1;
    }
    out
}

/// Renders a fenced code block: an optional language label, then one styled
/// line per source line.
fn push_code_block(out: &mut Vec<Line<'static>>, lang: Option<&str>, code: &[&str]) {
    let code_style = Style::default().bg(CODE_BG).fg(CODE_FG);
    if let Some(lang) = lang {
        let lang = lang.trim();
        if !lang.is_empty() {
            out.push(Line::from(Span::styled(
                format!("┌ {lang}"),
                Style::default().fg(CODE_LABEL_FG),
            )));
        }
    }
    if code.is_empty() {
        // Empty code block — emit one styled blank line so the block is visible.
        out.push(Line::from(Span::styled(String::new(), code_style)));
    } else {
        for raw in code {
            out.push(Line::from(Span::styled((*raw).to_string(), code_style)));
        }
    }
}

/// Renders a list item: indentation, a colored marker, then inline-formatted
/// text.
fn push_list_item(out: &mut Vec<Line<'static>>, depth: usize, marker: &str, text: &str) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let indent = "  ".repeat(depth.min(MAX_LIST_DEPTH));
    if !indent.is_empty() {
        spans.push(Span::raw(indent));
    }
    spans.push(Span::styled(
        format!("{marker} "),
        Style::default().fg(MARKER_FG),
    ));
    spans.extend(parse_inline(text));
    out.push(Line::from(spans));
}

/// Renders an ATX heading: a bold, colored `#` prefix and bolded text.
fn push_heading(out: &mut Vec<Line<'static>>, level: u8, text: &str) {
    let heading_style = Style::default().add_modifier(Modifier::BOLD).fg(HEADING_FG);
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(
        format!("{} ", "#".repeat(level as usize)),
        heading_style,
    ));
    for mut span in parse_inline(text) {
        span.style = heading_style.patch(span.style);
        spans.push(span);
    }
    out.push(Line::from(spans));
}

/// Inline parser: scans a single line and emits styled [`Span`]s.
///
/// Match order: `**bold**`, `` `code` ``, `~~strike~~`, `*italic*`. Open
/// markers without a closer are emitted literally so partial streaming output
/// stays readable.
fn parse_inline(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        // Bold: **text** (also __text__).
        if (c == '*' || c == '_') && i + 1 < n && chars[i + 1] == c {
            let marker = c;
            if let Some(end) = find_double(&chars, i + 2, marker, n) {
                flush_plain(&mut plain, &mut spans);
                let inner: String = chars[i + 2..end].iter().collect();
                style_inner(&mut spans, &inner, Modifier::BOLD);
                i = end + 2;
                continue;
            }
            plain.push(marker);
            plain.push(marker);
            i += 2;
            continue;
        }
        // Inline code: `code`.
        if c == '`' {
            if let Some(end) = find_single(&chars, i + 1, '`', n) {
                flush_plain(&mut plain, &mut spans);
                let inner: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(inner, Style::default().fg(INLINE_CODE_FG)));
                i = end + 1;
                continue;
            }
            plain.push('`');
            i += 1;
            continue;
        }
        // Strikethrough: ~~text~~.
        if c == '~' && i + 1 < n && chars[i + 1] == '~' {
            if let Some(end) = find_double(&chars, i + 2, '~', n) {
                flush_plain(&mut plain, &mut spans);
                let inner: String = chars[i + 2..end].iter().collect();
                style_inner(&mut spans, &inner, Modifier::CROSSED_OUT);
                i = end + 2;
                continue;
            }
            plain.push('~');
            plain.push('~');
            i += 2;
            continue;
        }
        // Italic: *text* (single asterisk; underscores intentionally not
        // supported to avoid mangling snake_case identifiers).
        if c == '*' {
            if let Some(end) = find_single(&chars, i + 1, '*', n) {
                flush_plain(&mut plain, &mut spans);
                let inner: String = chars[i + 1..end].iter().collect();
                style_inner(&mut spans, &inner, Modifier::ITALIC);
                i = end + 1;
                continue;
            }
            plain.push('*');
            i += 1;
            continue;
        }
        plain.push(c);
        i += 1;
    }
    flush_plain(&mut plain, &mut spans);
    if spans.is_empty() {
        // Keep a non-empty line so ratatui renders a blank row.
        spans.push(Span::raw(String::new()));
    }
    spans
}

/// Recursively parses `inner` for nested markers, then applies `modifier` to
/// every produced span. Used for bold / italic / strikethrough. Bold stays in
/// the active theme's body color: painting every emphasized phrase yellow
/// turns long agent reports into a noisy wall and defeats the document
/// hierarchy supplied by headings and inline code.
fn style_inner(spans: &mut Vec<Span<'static>>, inner: &str, modifier: Modifier) {
    let base = Style::default().add_modifier(modifier);
    for mut span in parse_inline(inner) {
        span.style = base.patch(span.style);
        spans.push(span);
    }
}

#[inline]
fn flush_plain(plain: &mut String, spans: &mut Vec<Span<'static>>) {
    if !plain.is_empty() {
        spans.push(Span::raw(std::mem::take(plain)));
    }
}

/// Finds the first index `j >= start` where `chars[j] == target &&
/// chars[j + 1] == target`. Returns `None` if no such pair exists.
fn find_double(chars: &[char], start: usize, target: char, n: usize) -> Option<usize> {
    let mut j = start;
    while j + 1 < n {
        if chars[j] == target && chars[j + 1] == target {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Finds the first index `j >= start` where `chars[j] == target`.
fn find_single(chars: &[char], start: usize, target: char, n: usize) -> Option<usize> {
    let mut j = start;
    while j < n {
        if chars[j] == target {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Detects an opening fenced code block (```` ``` ```` or `~~~`).
/// Returns `(fence_char, fence_length, Option<language>)`.
fn parse_opening_fence(line: &str) -> Option<(char, usize, Option<String>)> {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let fence_len = trimmed.chars().take_while(|&c| c == first).count();
    if fence_len < 3 {
        return None;
    }
    // Byte offset == char count because the fence prefix is ASCII.
    let info = trimmed[fence_len..].trim();
    let lang = if info.is_empty() {
        None
    } else {
        Some(info.split_whitespace().next().unwrap_or("").to_string())
    };
    Some((first, fence_len, lang))
}

/// Returns `true` if `line` closes a fence of `fence_char` opened with at
/// least `min_len` markers.
fn is_closing_fence(line: &str, fence_char: char, min_len: usize) -> bool {
    let trimmed = line.trim();
    let count = trimmed.chars().take_while(|&c| c == fence_char).count();
    if count < min_len {
        return false;
    }
    trimmed[count..].trim().is_empty()
}

/// Detects an unordered list item. Returns `(indent_depth, marker, text)`.
fn parse_unordered_item(line: &str) -> Option<(usize, String, &str)> {
    let indent_bytes = line.len() - line.trim_start().len();
    let rest = &line[indent_bytes..];
    for marker in &["- ", "* ", "+ "] {
        if let Some(text) = rest.strip_prefix(marker) {
            let depth = (indent_bytes / 2).min(MAX_LIST_DEPTH);
            return Some((depth, marker.trim().to_string(), text));
        }
    }
    None
}

/// Detects an ordered list item (`1. `, `2. `, …). Returns
/// `(indent_depth, marker, text)`.
fn parse_ordered_item(line: &str) -> Option<(usize, String, &str)> {
    let indent_bytes = line.len() - line.trim_start().len();
    let rest = &line[indent_bytes..];
    let dot = rest.find(". ")?;
    let (num, after) = rest.split_at(dot);
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let text = after.get(2..)?;
    let depth = (indent_bytes / 2).min(MAX_LIST_DEPTH);
    Some((depth, format!("{num}."), text))
}

/// Detects an ATX heading (`# ` … `###### `). Returns `(level, text)`.
fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if rest.is_empty() {
        return Some((level as u8, ""));
    }
    let rest = rest.strip_prefix(' ')?;
    let rest = rest.trim_end_matches('#');
    Some((level as u8, rest.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flatten_spans(lines: &[Line<'static>]) -> Vec<Span<'static>> {
        lines.iter().flat_map(|l| l.spans.clone()).collect()
    }

    fn styled_text(spans: &[Span<'static>]) -> String {
        spans
            .iter()
            .map(|s| s.content.clone().into_owned())
            .collect()
    }

    fn has_modifier(spans: &[Span<'static>], modifier: Modifier) -> bool {
        spans
            .iter()
            .any(|s| s.style.add_modifier.contains(modifier))
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        assert!(render_markdown("").is_empty());
    }

    #[test]
    fn bold_renders_with_bold_modifier() {
        let spans = flatten_spans(&render_markdown("**hi**"));
        assert!(has_modifier(&spans, Modifier::BOLD), "no BOLD in {spans:?}");
        assert_eq!(styled_text(&spans), "hi");
    }

    #[test]
    fn italic_renders_with_italic_modifier() {
        let spans = flatten_spans(&render_markdown("*hi*"));
        assert!(
            has_modifier(&spans, Modifier::ITALIC),
            "no ITALIC in {spans:?}"
        );
        assert_eq!(styled_text(&spans), "hi");
    }

    #[test]
    fn snake_case_is_not_mangled_as_italic() {
        // Underscore emphasis is unsupported: `my_var` stays literal, not italic.
        let spans = flatten_spans(&render_markdown("my_var_name"));
        assert!(!has_modifier(&spans, Modifier::ITALIC));
        assert_eq!(styled_text(&spans), "my_var_name");
    }

    #[test]
    fn inline_code_renders_with_distinct_color() {
        let spans = flatten_spans(&render_markdown("use `cargo` now"));
        let code = spans
            .iter()
            .find(|s| s.content == "cargo")
            .expect("code span present");
        assert_eq!(code.style.fg, Some(INLINE_CODE_FG));
    }

    #[test]
    fn unordered_list_indents_and_marks() {
        let lines = render_markdown("- item");
        assert_eq!(lines.len(), 1);
        let marker = lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains('-'))
            .expect("bullet marker");
        assert_eq!(marker.style.fg, Some(MARKER_FG));
    }

    #[test]
    fn ordered_list_keeps_number() {
        let lines = render_markdown("1. first\n2. second");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans.iter().any(|s| s.content.contains("1.")));
        assert!(lines[1].spans.iter().any(|s| s.content.contains("2.")));
    }

    #[test]
    fn nested_unordered_list_increases_depth() {
        let lines = render_markdown("- top\n  - nested");
        assert_eq!(lines.len(), 2);
        assert!(
            lines[1].spans.iter().any(|s| s.content == "  "),
            "nested item should carry a 2-space indent"
        );
    }

    #[test]
    fn code_block_renders_all_lines_with_code_style_and_label() {
        let lines = render_markdown("```rust\nfn main() {}\n```");
        let label = lines
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.content.contains("rust"))
            .expect("language label");
        assert_eq!(label.style.fg, Some(CODE_LABEL_FG));
        let code_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("fn main")))
            .expect("code line");
        let span = &code_line.spans[0];
        assert_eq!(span.style.bg, Some(CODE_BG));
        assert_eq!(span.style.fg, Some(CODE_FG));
    }

    #[test]
    fn heading_renders_bold_with_hash_prefix() {
        let spans = flatten_spans(&render_markdown("## Title"));
        assert!(spans.iter().any(|s| s.content.contains("##")));
        assert!(has_modifier(&spans, Modifier::BOLD));
    }

    #[test]
    fn partial_bold_marker_renders_literally() {
        // Streaming: open `**` with no closer must not swallow the rest.
        let spans = flatten_spans(&render_markdown("**unclosed"));
        assert!(
            styled_text(&spans).contains("**"),
            "open ** should render literally"
        );
        assert!(!has_modifier(&spans, Modifier::BOLD));
    }

    #[test]
    fn partial_inline_code_renders_literally() {
        let text = styled_text(&flatten_spans(&render_markdown("`unclosed")));
        assert!(text.contains('`'), "open backtick should render literally");
    }

    #[test]
    fn unclosed_fence_renders_remainder_as_code() {
        // Streaming: an open fence eats the rest of the buffer as a code block.
        let lines = render_markdown("```\nfn main() {}");
        let code = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("fn main")))
            .expect("code line present");
        assert_eq!(code.spans[0].style.bg, Some(CODE_BG));
    }

    #[test]
    fn strikethrough_renders_with_crossed_out() {
        let spans = flatten_spans(&render_markdown("~~done~~"));
        assert!(has_modifier(&spans, Modifier::CROSSED_OUT));
        assert_eq!(styled_text(&spans), "done");
    }

    #[test]
    fn nested_bold_italic_combines_modifiers() {
        // **bold *and italic* bold** → outer bold, inner bold+italic.
        let spans = flatten_spans(&render_markdown("**bold *and italic* bold**"));
        assert!(has_modifier(&spans, Modifier::BOLD));
        assert!(
            has_modifier(&spans, Modifier::ITALIC),
            "inner should still be italic inside bold"
        );
    }

    #[test]
    fn does_not_panic_on_arbitrary_input() {
        for input in [
            "",
            "**",
            "*",
            "`",
            "```",
            "~~",
            "1.",
            "- ",
            "#",
            "######",
            "#7 bad",
            "**a*b**c*d**",
            "中**文**字",
            "日本語`code`です",
            "\n\n\n",
            "   ",
            "******",
            "*`*_~",
            "- - -",
            "1. 2. 3.",
            "````\n``",
            "> not a quote",
        ] {
            // Must not panic; output is discarded.
            let out = render_markdown(input);
            // Every line must be non-panicking to construct (guaranteed by type).
            for line in &out {
                let _ = line.spans.len();
            }
        }
    }

    #[test]
    fn mixed_document_parses_all_blocks() {
        let doc = "# Heading\n\nSome **bold** and `code`.\n\n- a\n- b\n\n```\ncode\n```\n";
        let lines = render_markdown(doc);
        // Sanity: at least one heading, list, code, and bold span appear.
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains('#')))
        );
        assert!(has_modifier(&flatten_spans(&lines), Modifier::BOLD));
        assert!(
            flatten_spans(&lines)
                .iter()
                .any(|s| s.style.bg == Some(CODE_BG))
        );
    }
}
