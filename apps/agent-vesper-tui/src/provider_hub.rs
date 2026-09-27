//! Registry-driven Settings → Providers draft editor.
use ratatui::Frame;

pub struct ProviderHub {
    pub providers: Vec<String>,
    /// Provider that is already active. Navigation never changes this.
    pub current: String,
    /// Draft provider row, changed only by choosing a provider row.
    pub chosen: usize,
    /// Keyboard focus. Even rows are providers; the next row is that provider's
    /// authentication action. Save and Cancel follow the pairs.
    pub selected: usize,
    pub notice: String,
}

impl ProviderHub {
    pub fn new(providers: Vec<String>, current: String) -> Self {
        let chosen = providers.iter().position(|id| id == &current).unwrap_or(0);
        Self {
            providers,
            current,
            selected: chosen.saturating_mul(2),
            chosen,
            notice: "Choose a provider, then Save settings. Manage authentication is immediate and does not change the active provider."
                .into(),
        }
    }

    pub fn row_count(&self) -> usize {
        self.providers.len().saturating_mul(2) + 2
    }

    pub fn save_row(&self) -> usize {
        self.providers.len().saturating_mul(2)
    }

    pub fn cancel_row(&self) -> usize {
        self.save_row() + 1
    }

    pub fn is_provider_row(&self) -> bool {
        self.selected < self.providers.len().saturating_mul(2) && self.selected.is_multiple_of(2)
    }

    pub fn is_manage_row(&self) -> bool {
        self.selected < self.providers.len().saturating_mul(2) && !self.selected.is_multiple_of(2)
    }

    /// Provider owning the focused row. Provider rows and the action beneath
    /// them share one target. Save and Cancel have none.
    pub fn authentication_provider(&self) -> Option<&str> {
        if self.selected >= self.providers.len().saturating_mul(2) {
            return None;
        }
        self.providers.get(self.selected / 2).map(String::as_str)
    }

    pub fn choose(&mut self) {
        if self.is_provider_row() {
            self.chosen = self.selected / 2;
        }
    }

    pub fn choice(&self) -> Option<&str> {
        self.providers.get(self.chosen).map(String::as_str)
    }
}

pub fn render(frame: &mut Frame<'_>, hub: &ProviderHub, theme: &str) {
    let mut labels = Vec::with_capacity(hub.row_count());
    for (index, id) in hub.providers.iter().enumerate() {
        labels.push(format!(
            "[{}] {}{}",
            if index == hub.chosen { "x" } else { " " },
            id,
            if id == &hub.current { " (active)" } else { "" }
        ));
        labels.push(format!("Manage authentication · {id}"));
    }
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
        "↑↓ select · Enter/Space choose · M manage authentication · S save · Esc cancel",
        theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_does_not_change_the_draft_until_chosen() {
        let mut hub = ProviderHub::new(vec!["one".into(), "two".into()], "two".into());
        assert_eq!(hub.choice(), Some("two"));
        assert_eq!(hub.authentication_provider(), Some("two"));
        hub.selected = 0;
        assert_eq!(hub.choice(), Some("two"));
        hub.choose();
        assert_eq!(hub.choice(), Some("one"));
        assert_eq!(hub.current, "two");
        hub.selected = hub.save_row();
        hub.choose();
        assert_eq!(hub.choice(), Some("one"));
    }

    #[test]
    fn down_from_each_provider_reaches_its_own_authentication_action() {
        let providers = ["lmstudio", "openai", "xai", "zai"];
        for (index, id) in providers.iter().enumerate() {
            let mut hub = ProviderHub::new(
                providers
                    .iter()
                    .map(|provider| (*provider).to_owned())
                    .collect(),
                (*id).to_owned(),
            );
            assert!(hub.is_provider_row(), "{id}");
            assert_eq!(hub.authentication_provider(), Some(*id));
            hub.selected += 1;
            assert!(hub.is_manage_row(), "{id}");
            assert_eq!(hub.authentication_provider(), Some(*id));
            assert_eq!(hub.choice(), Some(*id));
            assert_eq!(hub.current, *id);
            if index + 1 < providers.len() {
                hub.selected += 1;
                assert!(hub.is_provider_row());
                assert_eq!(hub.authentication_provider(), Some(providers[index + 1]));
                assert_eq!(
                    hub.choice(),
                    Some(*id),
                    "crossing a provider retargeted the draft"
                );
            }
        }
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
                    "Manage authentication · real-adapter",
                    "Save settings",
                    "Cancel",
                ] {
                    assert!(text.contains(label), "missing {label}");
                }
            } else {
                assert!(text.contains("Resize"));
            }
        }
    }

    #[test]
    fn click_on_each_manage_row_resolves_to_that_provider() {
        use ratatui::layout::Rect;
        let providers = ["lmstudio", "openai", "xai", "zai"];
        for (width, height) in [(120, 40), (80, 24), (100, 30), (60, 18)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            let mut hub = ProviderHub::new(
                providers.iter().map(|id| (*id).to_owned()).collect(),
                "xai".into(),
            );
            // Match the screenshot: openai is the draft, focus is xAI's action.
            hub.chosen = 1;
            hub.selected = 5;
            terminal
                .draw(|frame| render(frame, &hub, "chatgpt-black"))
                .unwrap();
            let buffer = terminal.backend().buffer().clone();
            let mut rows: Vec<String> = Vec::new();
            for y in 0..height {
                let line: String = (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect();
                rows.push(line);
            }
            for (index, label_id) in providers.iter().enumerate() {
                let needle = format!("Manage authentication · {label_id}");
                let Some(y) = rows.iter().position(|line| line.contains(&needle)) else {
                    continue;
                };
                let x = rows[y].find(&needle).unwrap() as u16;
                let hit = crate::settings_menu::item_at(
                    Rect::new(0, 0, width, height),
                    hub.row_count(),
                    hub.selected,
                    x,
                    y as u16,
                );
                let provider = hit.and_then(|index| {
                    if index < providers.len() * 2 {
                        Some(providers[index / 2])
                    } else {
                        None
                    }
                });
                assert_eq!(
                    provider,
                    Some(*label_id),
                    "{width}x{height} click on {needle} at ({x},{y}) hit {hit:?} selected row {index}"
                );
            }
        }
    }
}
