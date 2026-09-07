//! Registry-driven Settings → Providers draft editor.
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct ProviderHub {
    pub providers: Vec<String>,
    pub current: String,
    pub chosen: usize,
    pub selected: usize,
    pub notice: String,
}

impl ProviderHub {
    pub fn new(providers: Vec<String>, current: String) -> Self {
        let chosen = providers.iter().position(|id| id == &current).unwrap_or(0);
        Self {
            providers,
            current,
            chosen,
            selected: chosen,
            notice: "Changes apply after restarting the TUI. Esc leaves your provider unchanged."
                .into(),
        }
    }

    pub fn choose(&mut self) {
        if self.selected < self.providers.len() {
            self.chosen = self.selected;
        }
    }

    pub fn choice(&self) -> Option<&str> {
        self.providers.get(self.chosen).map(String::as_str)
    }
}

pub fn render(frame: &mut Frame<'_>, hub: &ProviderHub) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    if area.width < 60 || area.height < 18 {
        frame.render_widget(
            Paragraph::new("Provider settings: resize to at least 60×18. Esc cancels; S saves."),
            area,
        );
        return;
    }
    let width = area.width.min(88);
    let height = area.height.min(20);
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let mut lines = vec![
        "↑/↓ select · Enter/Space choose · S save · Esc cancel".into(),
        String::new(),
    ];
    // Keep a bounded viewport even if more real adapters are registered later.
    let capacity = usize::from(height.saturating_sub(11)).max(1);
    let start = hub
        .selected
        .saturating_sub(capacity - 1)
        .min(hub.providers.len());
    for (index, id) in hub.providers.iter().enumerate().skip(start).take(capacity) {
        lines.push(format!(
            "{} [{}] {}{}",
            if index == hub.selected { "›" } else { " " },
            if index == hub.chosen { "x" } else { " " },
            id,
            if id == &hub.current { " (active)" } else { "" }
        ));
    }
    lines.push(format!(
        "{} Save settings",
        if hub.selected == hub.providers.len() {
            "›"
        } else {
            " "
        }
    ));
    lines.push(String::new());
    lines.push(format!("Active provider: {}", hub.current));
    lines.push(format!(
        "Selected for next launch: {}",
        hub.choice().unwrap_or("none registered")
    ));
    lines.push(hub.notice.clone());
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Settings › Providers (saved configuration) "),
            ),
        panel,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_does_not_change_the_draft_until_chosen() {
        let mut hub = ProviderHub::new(vec!["one".into(), "two".into()], "two".into());
        hub.selected = 0;
        assert_eq!(hub.choice(), Some("two"));
        hub.choose();
        assert_eq!(hub.choice(), Some("one"));
        assert_eq!(hub.current, "two");
        hub.selected = 2;
        hub.choose();
        assert_eq!(hub.choice(), Some("one"));
    }
    #[test]
    fn provider_panel_matches_settings_style_and_handles_small_terminals() {
        for (width, height) in [(100, 24), (60, 18), (30, 8)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            let hub = ProviderHub::new(vec!["real-adapter".into()], "real-adapter".into());
            terminal.draw(|frame| render(frame, &hub)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            if width >= 60 {
                for label in [
                    "Settings › Providers",
                    "[x] real-adapter (active)",
                    "Save settings",
                    "restarting",
                ] {
                    assert!(text.contains(label), "missing {label}");
                }
            } else {
                assert!(text.contains("resize"));
            }
        }
    }
}
