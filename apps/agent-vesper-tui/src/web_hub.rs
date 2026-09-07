//! Native Settings → Web tools editor; no file editing required.
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph},
};
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
            ("Fetch / scrape / map / crawl", self.config.fetch_enabled),
            ("JavaScript rendering", self.config.render_enabled),
            (
                "Browser interaction (click/type/submit)",
                self.config.interact_enabled,
            ),
            ("Respect robots.txt", self.config.respect_robots),
        ]
        .into_iter()
        .map(|(label, value)| format!("{label}: {}", if value { "ON" } else { "OFF" }))
        .collect();
        rows.extend([
            "Set up / repair driver (included in installation)".into(),
            "Save settings".into(),
        ]);
        rows
    }
}

pub fn render(frame: &mut Frame<'_>, hub: &WebHub) {
    let area = frame.area();
    if area.width < 60 || area.height < 18 {
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("Web tools settings: resize to at least 60×18. Esc cancels; S saves."),
            area,
        );
        return;
    }
    let width = area.width.min(88);
    let height = area.height.min(20);
    let panel = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    let mut lines = vec![
        "↑/↓ select · Enter/Space toggle · S save · Esc cancel".into(),
        String::new(),
    ];
    lines.extend(
        hub.rows()
            .iter()
            .enumerate()
            .map(|(index, row)| format!("{} {row}", if index == hub.selected { "›" } else { " " })),
    );
    lines.push(String::new());
    lines.push(format!(
        "Driver: {}",
        hub.config
            .driver_image
            .as_deref()
            .unwrap_or("not configured — select Set up / repair driver")
    ));
    lines.push("Private-address blocking and sandbox isolation: always ON".into());
    lines.push(hub.notice.clone());
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Settings › Web tools (saved configuration) "),
            ),
        panel,
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
        assert_eq!(hub.rows().len(), 7);
    }

    #[test]
    fn panel_renders_controls_and_restart_notice() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| render(frame, &WebHub::new(WebScopeConfig::default())))
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
