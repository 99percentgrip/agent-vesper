//! Native Settings → Web tools editor; no file editing required.
use ratatui::Frame;
use vesper_harness::web_settings::WebScopeConfig;

/// Draft settings: cancelling never persists a toggle.
pub struct WebHub {
    pub config: WebScopeConfig,
    pub selected: usize,
    pub notice: String,
}

impl WebHub {
    pub fn new(config: WebScopeConfig) -> Self {
        Self {
            config,
            selected: 0,
            notice: "Changes apply after restarting the host. Network approval still applies."
                .into(),
        }
    }
    pub fn toggle(&mut self) {
        let flag = match self.selected {
            0 => &mut self.config.enabled,
            1 => &mut self.config.fetch_enabled,
            2 => &mut self.config.render_enabled,
            3 => &mut self.config.interact_enabled,
            4 => &mut self.config.respect_robots,
            _ => return,
        };
        *flag = !*flag;
    }
    pub fn rows(&self) -> Vec<String> {
        let mut rows: Vec<_> = [
            ("Web tools", self.config.enabled),
            ("Fetch, scrape, map, crawl", self.config.fetch_enabled),
            ("JavaScript rendering", self.config.render_enabled),
            ("Browser interaction", self.config.interact_enabled),
            ("Respect robots.txt", self.config.respect_robots),
        ]
        .into_iter()
        .map(|(label, value)| format!("{label}: {}", if value { "ON" } else { "OFF" }))
        .collect();
        rows.extend([
            "Set up / repair driver".into(),
            "Save settings".into(),
            "Cancel".into(),
        ]);
        rows
    }
}

pub fn render(frame: &mut Frame<'_>, hub: &WebHub, theme: &str) {
    let driver = if hub.config.driver_image.is_some() {
        "Driver configured"
    } else {
        "Driver not configured"
    };
    let detail = format!("{driver} · {}", hub.notice);
    crate::settings_menu::render_menu(
        frame,
        &hub.rows(),
        hub.selected,
        "Settings · Web tools",
        &detail,
        "↑↓ select · Enter toggle · S save · Esc cancel",
        theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_control_toggles_independently_and_round_trips() {
        let mut hub = WebHub::new(WebScopeConfig::default());
        for index in 0..5 {
            hub.selected = index;
            let before = hub.config.clone();
            hub.toggle();
            assert_ne!(hub.config, before);
            hub.toggle();
            assert_eq!(hub.config, before);
        }
        assert_eq!(hub.rows().len(), 8);
    }

    #[test]
    fn web_editor_shares_menu_geometry_colors_and_cancel_row() {
        for theme in [
            "chatgpt-black",
            "chatgpt-white",
            "dracula",
            "nord",
            "light",
            "ansi",
        ] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
            let hub = WebHub::new(WebScopeConfig::default());
            terminal.draw(|frame| render(frame, &hub, theme)).unwrap();
            let palette = crate::ui::theme_palette(theme);
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].bg, palette.background);
            let viewport = ratatui::layout::Rect::new(0, 0, 80, 24);
            let menu = crate::settings_menu::area(viewport, hub.rows().len());
            assert_eq!(buffer[(menu.x + 3, menu.y + 1)].bg, palette.selection);
            assert_eq!(
                crate::settings_menu::item_at(viewport, 8, 0, menu.x + 2, menu.y + 8),
                Some(7)
            );
            let row: String = (0..80).map(|x| buffer[(x, menu.y + 8)].symbol()).collect();
            assert!(row.contains("Cancel"));
        }
    }

    #[test]
    fn panel_renders_controls_and_restart_notice() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &WebHub::new(WebScopeConfig::default()),
                    "chatgpt-black",
                )
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for label in [
            "Web tools",
            "JavaScript rendering",
            "Browser interaction",
            "Set up / repair driver",
            "Save settings",
            "restarting",
        ] {
            assert!(text.contains(label), "missing {label}");
        }
    }
}
