//! Centered settings presentation over the existing registry-driven command choices.
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

pub fn retry_models(event: &crossterm::event::Event, viewport: Rect) -> bool {
    use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            matches!(key.code, KeyCode::Enter | KeyCode::Char('r'))
        }
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
            item_at(viewport, 1, 0, mouse.column, mouse.row) == Some(0)
        }
        _ => false,
    }
}

pub fn render_model_unavailable(frame: &mut Frame<'_>, notice: &str, theme: &str) {
    render_menu(
        frame,
        &["Retry model list".into()],
        0,
        "Settings · model",
        notice,
        "Enter retry · Esc back",
        theme,
    );
}

pub fn area(viewport: Rect, count: usize) -> Rect {
    let width = viewport.width.saturating_sub(4).min(60);
    let height = (count.min(14) as u16 + 2).min(viewport.height.saturating_sub(8));
    Rect::new(
        viewport.x + viewport.width.saturating_sub(width) / 2,
        viewport.y + viewport.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub fn offset(menu: Rect, selected: usize) -> usize {
    selected.saturating_sub(usize::from(menu.height.saturating_sub(2)).saturating_sub(1))
}

pub fn item_at(viewport: Rect, count: usize, selected: usize, x: u16, y: u16) -> Option<usize> {
    if viewport.width < 30 || viewport.height < 12 {
        return None;
    }
    let menu = area(viewport, count);
    if x <= menu.x
        || x >= menu.right().saturating_sub(1)
        || y <= menu.y
        || y >= menu.bottom().saturating_sub(1)
    {
        return None;
    }
    let index = offset(menu, selected) + usize::from(y - menu.y - 1);
    (index < count).then_some(index)
}

pub fn render(
    frame: &mut Frame<'_>,
    choices: &[(String, String)],
    selected: usize,
    input: &str,
    notice: Option<&str>,
    theme: &str,
) {
    let root = input.trim().starts_with("/settings");
    let title = if root {
        "Settings".to_owned()
    } else {
        format!(
            "Settings · {}",
            input
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('/')
        )
    };
    let labels: Vec<String> = choices
        .iter()
        .map(|(command, description)| {
            if root {
                description
                    .split('·')
                    .next()
                    .unwrap_or(description)
                    .trim()
                    .to_owned()
            } else {
                command
                    .split_once(' ')
                    .map(|(_, value)| value)
                    .unwrap_or(command)
                    .to_owned()
            }
        })
        .collect();
    let detail = notice.unwrap_or_else(|| {
        choices
            .get(selected)
            .map(|(_, text)| text.as_str())
            .unwrap_or("No matching settings")
    });
    render_menu(
        frame,
        &labels,
        selected,
        &title,
        detail,
        "↑↓ select · Enter choose · Esc back",
        theme,
    );
}

/// Shared geometry and theme for Settings categories, values, and native editors.
pub fn render_menu(
    frame: &mut Frame<'_>,
    labels: &[String],
    selected: usize,
    title: &str,
    detail: &str,
    footer: &str,
    theme: &str,
) {
    let viewport = frame.area();
    let palette = crate::ui::theme_palette(theme);
    let accent = palette.accent;
    let dim = palette.muted;
    let text = palette.text;
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.background).fg(text)),
        viewport,
    );
    if viewport.width < 30 || viewport.height < 12 {
        frame.render_widget(
            Paragraph::new("Settings\nResize to 30×12 or larger.\nEsc: back")
                .style(Style::default().fg(accent)),
            viewport,
        );
        return;
    }
    let menu = area(viewport, labels.len());
    frame.render_widget(
        Paragraph::new(title)
            .alignment(Alignment::Center)
            .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
        Rect::new(viewport.x, menu.y.saturating_sub(2), viewport.width, 1),
    );
    let selected = selected.min(labels.len().saturating_sub(1));
    let lines: Vec<Line<'_>> = labels
        .iter()
        .enumerate()
        .skip(offset(menu, selected))
        .take(usize::from(menu.height.saturating_sub(2)))
        .map(|(index, label)| {
            let row = format!("  {}  {label}", if index == selected { "›" } else { " " });
            Line::styled(
                format!(
                    "{row:<width$}",
                    width = usize::from(menu.width.saturating_sub(2))
                ),
                if index == selected {
                    Style::default()
                        .fg(palette.selected_text)
                        .bg(palette.selection)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(text)
                },
            )
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette.border)),
        ),
        menu,
    );
    let detail_y = menu.bottom() + u16::from(viewport.height >= 16);
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center)
            .style(Style::default().fg(dim)),
        Rect::new(
            menu.x,
            detail_y,
            menu.width,
            viewport.bottom().saturating_sub(detail_y + 1),
        ),
    );
    frame.render_widget(
        Paragraph::new(footer)
            .alignment(Alignment::Center)
            .style(Style::default().fg(accent)),
        Rect::new(
            viewport.x,
            viewport.bottom().saturating_sub(1),
            viewport.width,
            1,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn providers_and_swarm_use_the_selected_theme_for_canvas_and_selection() {
        for theme in [
            "chatgpt-black",
            "chatgpt-white",
            "dracula",
            "nord",
            "light",
            "ansi",
        ] {
            let palette = crate::ui::theme_palette(theme);
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            let hub =
                crate::provider_hub::ProviderHub::new(vec!["fixture".into()], "fixture".into());
            terminal
                .draw(|f| crate::provider_hub::render(f, &hub, theme))
                .unwrap();
            let menu = area(Rect::new(0, 0, 80, 24), 3);
            assert_eq!(terminal.backend().buffer()[(0, 0)].bg, palette.background);
            assert_eq!(
                terminal.backend().buffer()[(menu.x + 1, menu.y + 1)].bg,
                palette.selection
            );
            #[cfg(feature = "swarm")]
            {
                let root = tempfile::tempdir().unwrap();
                let hub = crate::swarm_hub::SwarmHub::new(
                    vesper_harness::swarm_settings::SwarmSettingsDraft::open(root.path()).unwrap(),
                );
                terminal
                    .draw(|f| crate::swarm_hub::render(f, &hub, theme))
                    .unwrap();
                let menu = area(Rect::new(0, 0, 80, 24), 8);
                assert_eq!(terminal.backend().buffer()[(0, 0)].bg, palette.background);
                assert_eq!(
                    terminal.backend().buffer()[(menu.x + 1, menu.y + 1)].bg,
                    palette.selection
                );
            }
        }
    }
    #[test]
    fn unavailable_models_show_reason_and_operable_retry_instead_of_empty_input() {
        use crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };
        for (w, h) in [(120, 40), (60, 20), (30, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| render_model_unavailable(f, "Discovery failed (HTTP 403)", "ansi"))
                .unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(rendered.contains("Retry model list"));
            assert!(
                rendered
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .contains("HTTP 403")
            );
            assert!(rendered.contains("Esc back"));
            assert!(!rendered.contains("Type the command"));
            let viewport = Rect::new(0, 0, w, h);
            for code in [KeyCode::Enter, KeyCode::Char('r')] {
                assert!(retry_models(
                    &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
                    viewport
                ));
            }
            assert!(!retry_models(
                &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                viewport
            ));
            let menu = area(viewport, 1);
            let mut mouse = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: menu.x + 2,
                row: menu.y + 1,
                modifiers: KeyModifiers::NONE,
            };
            assert!(retry_models(&Event::Mouse(mouse), viewport));
            mouse.row = menu.y;
            assert!(!retry_models(&Event::Mouse(mouse), viewport));
        }
    }
    #[test]
    fn scrolling_and_mouse_geometry_match_without_chat_chrome() {
        let choices: Vec<_> = (0..25)
            .map(|i| (format!("/model model-{i}"), "Primary model".into()))
            .collect();
        for (w, h) in [(120, 40), (40, 16), (30, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| render(f, &choices, 24, "/model ", None, "chatgpt-black"))
                .unwrap();
            let viewport = Rect::new(0, 0, w, h);
            let menu = area(viewport, choices.len());
            assert_eq!(
                item_at(viewport, 25, 24, menu.x + 2, menu.bottom() - 2),
                Some(24)
            );
            let rendered: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(rendered.contains("model-24"));
            assert!(!rendered.contains("Conversation"));
            assert!(!rendered.contains("/model"));
        }
    }
}
