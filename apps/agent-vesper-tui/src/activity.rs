//! Chronological projection of host-owned tool start/result records.
use crate::{
    presentation::{self, Ink},
    ui::ThemePalette,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

pub(crate) fn project(entries: &[String], running: bool, expanded: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let entry = entries[index].trim();
        let Some(action) = entry.strip_prefix('⏺') else {
            if entry.starts_with("http") || entry.contains("VesperLens") {
                out.push(entries[index].clone());
            }
            index += 1;
            continue;
        };
        let action = action.trim();
        let name = action.split([' ', '·']).next().unwrap_or_default();
        let detail = action
            .strip_prefix(name)
            .unwrap_or_default()
            .trim()
            .trim_start_matches('·')
            .trim();
        let (label, lang) = if name.contains("command") || name.contains("shell") {
            ("Ran", "sh")
        } else if name.contains("read") {
            ("Read", "")
        } else if name.contains("search") || name.contains("grep") {
            ("Search", "")
        } else if name.contains("list") {
            ("Explored", "")
        } else if name.contains("write") || name.contains("edit") || name.contains("patch") {
            ("Edited", "")
        } else {
            (name, "")
        };
        let mut end = index + 1;
        // ReAct may emit its bare executing marker after the detailed action.
        // They describe one call, not a second unfinished operation.
        if !detail.is_empty() && end < entries.len() && entries[end].trim() == format!("⏺ {name}")
        {
            end += 1;
        }
        while end < entries.len() && !entries[end].trim().starts_with('⏺') {
            end += 1;
        }
        let result = entries[index + 1..end]
            .iter()
            .find_map(|s| s.trim().strip_prefix('⎿'))
            .map(str::trim);
        let state = match result {
            Some(s) if s.starts_with('✗') => "failed",
            Some(_) => "ok",
            None if running && end == entries.len() => "running",
            None => "interrupted",
        };
        out.push(format!("tool:{state}\t{label}\t{lang}\t{detail}"));
        if let Some(result) = result {
            let result = result.trim_start_matches(['✓', '✗']).trim();
            let result = result
                .strip_prefix(name)
                .unwrap_or(result)
                .trim()
                .trim_start_matches('·')
                .trim();
            let lines = result.lines().collect::<Vec<_>>();
            let limit = if expanded { 80 } else { 2 };
            for line in lines.iter().take(limit) {
                out.push(format!("tool-output:{line}"));
            }
            if result.is_empty() {
                out.push("tool-output:(no output)".into());
            }
            if lines.len() > limit {
                out.push(format!(
                    "tool-output:… +{} lines · Ctrl+T to view transcript",
                    lines.len() - limit
                ));
            }
        } else if state == "interrupted" {
            out.push("tool-output:Interrupted · no completion result received".into());
        }
        out.extend(
            entries[index + 1..end]
                .iter()
                .filter(|s| s.starts_with("http") || s.contains("VesperLens"))
                .cloned(),
        );
        index = end;
    }
    out
}

pub(crate) fn render(raw: &str, width: usize, frame: u64, p: ThemePalette) -> Vec<Line<'static>> {
    if let Some(text) = raw.strip_prefix("tool-output:") {
        let mut line = presentation::syntax(text, "sh", p);
        line.spans
            .insert(0, Span::styled("  └ ", Style::default().fg(p.muted)));
        return presentation::wrap(line, width, 4);
    }
    let mut fields = raw.trim_start_matches("tool:").splitn(4, '\t');
    let state = fields.next().unwrap_or("interrupted");
    let label = fields.next().unwrap_or("Tool");
    let lang = fields.next().unwrap_or("");
    let detail = fields.next().unwrap_or("");
    let mut spans = vec![
        presentation::status_dot(state, frame, p),
        Span::styled(
            format!("{label} "),
            Style::default()
                .fg(Ink::for_palette(p).label)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    spans.extend(presentation::syntax(detail, lang, p).spans);
    presentation::wrap(Line::from(spans), width, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chronological_results_stay_paired_and_pending_does_not_become_success() {
        let entries = vec![
            "⏺ run_command · cargo test".into(),
            "  ⎿ ✗ run_command · compile failed".into(),
            "⏺ read_file · src/main.rs".into(),
            "  ⎿ ✓ read_file · 42 lines".into(),
            "⏺ run_command · cargo check".into(),
        ];
        let running = project(&entries, true, false);
        let headers: Vec<_> = running.iter().filter(|s| s.starts_with("tool:")).collect();
        assert!(headers[0].starts_with("tool:failed\tRan"));
        assert!(headers[1].starts_with("tool:ok\tRead"));
        assert!(headers[2].starts_with("tool:running\tRan"));
        assert!(
            project(&entries, false, false)
                .iter()
                .any(|s| s.starts_with("tool:interrupted"))
        );
    }
    #[test]
    fn react_duplicate_start_folding_and_links_preserve_one_call() {
        let entries = vec![
            "⏺ run_command · cargo test".into(),
            "⏺ run_command".into(),
            "  ⎿ line1\nline2\nline3\nline4".into(),
            "http://127.0.0.1/review".into(),
        ];
        let compact = project(&entries, true, false);
        assert_eq!(compact.iter().filter(|s| s.starts_with("tool:")).count(), 1);
        assert!(compact.iter().any(|s| s.contains("+2 lines")));
        assert!(compact.iter().any(|s| s == "http://127.0.0.1/review"));
        let expanded = project(&entries, true, true);
        assert!(expanded.iter().any(|s| s == "tool-output:line4"));
    }
}
