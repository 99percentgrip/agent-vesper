//! Registry-driven Settings → Providers draft editor.
use ratatui::Frame;

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

pub fn render(frame: &mut Frame<'_>, hub: &ProviderHub, theme: &str) {
    let mut labels: Vec<String> = hub
        .providers
        .iter()
        .enumerate()
        .map(|(index, id)| {
            format!(
                "[{}] {}{}",
                if index == hub.chosen { "x" } else { " " },
                id,
                if id == &hub.current { " (active)" } else { "" }
            )
        })
        .collect();
    labels.push("Save settings".into());
    labels.push("Cancel".into());
    crate::settings_menu::render_menu(
        frame,
        &labels,
        hub.selected,
        "Settings › Providers",
        &format!(
            "Active: {} · Next launch: {}\n{}",
            hub.current,
            hub.choice().unwrap_or("none registered"),
            hub.notice
        ),
        "↑↓ select · Enter/Space choose · S save · Esc cancel",
        theme,
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
            terminal
                .draw(|frame| render(frame, &hub, "chatgpt-black"))
                .unwrap();
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
                assert!(text.contains("Resize"));
            }
        }
    }
}
