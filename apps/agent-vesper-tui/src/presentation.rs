//! Pure terminal syntax and cell-aware wrapping shared by reports and activity.
use crate::ui::ThemePalette;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[derive(Clone, Copy)]
pub(crate) struct Ink {
    pub label: Color,
    pub keyword: Color,
    pub string: Color,
    pub number: Color,
    pub operator: Color,
    pub running: Color,
    pub success: Color,
    pub failure: Color,
}
impl Ink {
    pub fn for_palette(p: ThemePalette) -> Self {
        let light = matches!(p.background, Color::Rgb(r,g,b) if r > 180 && g > 180 && b > 180);
        if light {
            Self {
                label: Color::Rgb(0, 105, 150),
                keyword: Color::Rgb(113, 55, 160),
                string: Color::Rgb(45, 112, 50),
                number: Color::Rgb(165, 74, 15),
                operator: Color::Rgb(0, 112, 115),
                running: Color::Rgb(180, 85, 0),
                success: Color::Rgb(20, 120, 55),
                failure: Color::Rgb(190, 40, 45),
            }
        } else {
            Self {
                label: Color::Rgb(40, 185, 235),
                keyword: Color::Rgb(181, 150, 235),
                string: Color::Rgb(155, 205, 135),
                number: Color::Rgb(235, 160, 105),
                operator: Color::Rgb(95, 210, 200),
                running: Color::Rgb(245, 165, 65),
                success: Color::Rgb(70, 205, 100),
                failure: Color::Rgb(245, 85, 95),
            }
        }
    }
}

/// Bounded lexical coloring only; source text is never executed or rewritten.
pub(crate) fn syntax(text: &str, language: &str, p: ThemePalette) -> Line<'static> {
    let ink = Ink::for_palette(p);
    let shell = matches!(language, "sh" | "bash" | "shell" | "console");
    let python = matches!(language, "py" | "python");
    let known = shell
        || python
        || matches!(
            language,
            "rs" | "rust"
                | "js"
                | "javascript"
                | "ts"
                | "typescript"
                | "json"
                | "toml"
                | "yaml"
                | "yml"
                | "c"
                | "cpp"
        );
    if !known {
        return Line::raw(text.to_owned());
    }
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut command = shell;
    while i < chars.len() {
        let start = i;
        let c = chars[i];
        let color;
        if ((shell || python || matches!(language, "toml" | "yaml" | "yml")) && c == '#')
            || (!shell && !python && c == '/' && chars.get(i + 1) == Some(&'/'))
        {
            i = chars.len();
            color = p.muted;
        } else if c == '\'' || c == '"' || (c == '`' && !shell) {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i = (i + 2).min(chars.len());
                    continue;
                }
                if chars[i] == c {
                    i += 1;
                    break;
                }
                i += 1;
            }
            color = ink.string;
        } else if c.is_ascii_digit() {
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '.' | '_'))
            {
                i += 1;
            }
            color = ink.number;
        } else if c.is_alphabetic() || c == '_' || (shell && matches!(c, '-' | '/')) {
            i += 1;
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || (shell && matches!(chars[i], '-' | '.' | '/' | ':')))
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let keyword = matches!(
                word.as_str(),
                "fn" | "pub"
                    | "let"
                    | "mut"
                    | "use"
                    | "impl"
                    | "struct"
                    | "enum"
                    | "async"
                    | "await"
                    | "if"
                    | "else"
                    | "for"
                    | "while"
                    | "match"
                    | "return"
                    | "const"
                    | "static"
                    | "mod"
                    | "in"
                    | "self"
                    | "Self"
                    | "true"
                    | "false"
                    | "None"
                    | "Some"
                    | "def"
                    | "class"
                    | "import"
                    | "from"
                    | "as"
                    | "with"
                    | "try"
                    | "except"
                    | "raise"
                    | "True"
                    | "False"
                    | "function"
                    | "export"
                    | "null"
            );
            color = if command {
                command = false;
                ink.label
            } else if keyword || word.starts_with('-') {
                ink.keyword
            } else if chars.get(i) == Some(&'(') || chars.get(i) == Some(&'!') {
                ink.label
            } else {
                p.text
            };
        } else {
            i += 1;
            color = if c.is_whitespace() {
                p.text
            } else {
                ink.operator
            };
            if shell && matches!(c, '|' | ';' | '&' | '\n') {
                command = true;
            }
        }
        spans.push(Span::styled(
            chars[start..i].iter().collect::<String>(),
            Style::default().fg(color),
        ));
    }
    Line::from(spans)
}

/// Wrap styled graphemes once, preserving Unicode cell widths and hanging indents.
/// The caller must render these physical rows without a second Paragraph wrap.
pub(crate) fn wrap(line: Line<'static>, width: usize, hanging: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let hanging = hanging.min(width.saturating_sub(2));
    let style = line.style;
    let mut cells: Vec<(String, Style, usize)> = Vec::new();
    for span in &line.spans {
        for g in span.styled_graphemes(style) {
            let text = if g.symbol == "\t" { "    " } else { g.symbol };
            if text.chars().any(|c| c.is_control()) {
                continue;
            }
            cells.push((text.to_owned(), g.style, Span::raw(text).width()));
        }
    }
    if cells.is_empty() {
        return vec![Line::raw("").style(style)];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let indent = if out.is_empty() { 0 } else { hanging };
        let mut used = indent;
        let mut end = start;
        let mut space = None;
        while end < cells.len() && used + cells[end].2 <= width {
            used += cells[end].2;
            if cells[end].0 == " " && end > start {
                space = Some(end + 1);
            }
            end += 1;
        }
        if end == start {
            // A two-cell glyph cannot fit in a one-cell terminal. Emit a
            // visible replacement rather than overrun the neighboring column.
            out.push(Line::from(Span::styled("�", cells[start].1)).style(style));
            start += 1;
            continue;
        }
        if end < cells.len()
            && let Some(boundary) = space
        {
            end = boundary;
        }
        let mut spans = Vec::new();
        if indent > 0 {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        for (text, s, _) in &cells[start..end] {
            if let Some(last) = spans.last_mut()
                && last.style == *s
            {
                last.content.to_mut().push_str(text);
            } else {
                spans.push(Span::styled(text.clone(), *s));
            }
        }
        out.push(Line::from(spans).style(style));
        start = end;
    }
    out
}

pub(crate) fn status_dot(state: &str, frame: u64, p: ThemePalette) -> Span<'static> {
    let ink = Ink::for_palette(p);
    let color = match state {
        "ok" => ink.success,
        "failed" => ink.failure,
        "running" => ink.running,
        _ => p.muted,
    };
    let mut style = Style::default().fg(color);
    if state == "running" && (frame / 5) % 2 == 1 {
        style = style.add_modifier(Modifier::DIM);
    }
    Span::styled("● ", style)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_colors_and_blink_are_driven_by_result_state() {
        for theme in [
            "chatgpt-black",
            "chatgpt-white",
            "nord",
            "dracula",
            "light",
            "ansi",
        ] {
            let p = crate::ui::theme_palette(theme);
            let ink = Ink::for_palette(p);
            assert_eq!(status_dot("running", 0, p).style.fg, Some(ink.running));
            assert_ne!(
                status_dot("running", 0, p).style,
                status_dot("running", 5, p).style
            );
            for (state, color) in [("ok", ink.success), ("failed", ink.failure)] {
                assert_eq!(status_dot(state, 0, p).style.fg, Some(color));
                assert_eq!(status_dot(state, 0, p), status_dot(state, 5, p));
            }
            assert_ne!(status_dot("interrupted", 0, p).style.fg, Some(ink.success));
        }
    }
    #[test]
    fn syntax_preserves_source_and_separates_commands_strings_flags_and_operators() {
        let p = crate::ui::theme_palette("nord");
        for (lang, source) in [
            ("sh", "cargo test --offline > /tmp/test.log 2>&1"),
            ("python", "print('hello', 42) # comment"),
            ("rust", "pub fn main() { println!(\"hello\"); }"),
        ] {
            let line = syntax(source, lang, p);
            assert_eq!(
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>(),
                source
            );
            let colors: std::collections::HashSet<_> = line
                .spans
                .iter()
                .map(|s| format!("{:?}", s.style.fg))
                .collect();
            assert!(colors.len() >= 4, "{lang}: {colors:?}");
        }
    }
    #[test]
    fn wraps_unicode_cells_without_losing_graphemes_or_styles() {
        let source = "  界面 café e\u{301} 👩‍💻 alpha beta";
        let line = Line::from(Span::styled(source, Style::default().fg(Color::Cyan)));
        for width in [8, 12, 24, 80] {
            let lines = wrap(line.clone(), width, 2);
            assert!(lines.iter().all(|l| l.width() <= width));
            let reconstructed = lines
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let text = l
                        .spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>();
                    if i == 0 { text } else { text[2..].to_owned() }
                })
                .collect::<String>();
            assert_eq!(reconstructed, source);
        }
    }
}
