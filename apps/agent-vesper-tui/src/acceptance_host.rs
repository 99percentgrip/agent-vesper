//! Native Save/Cancel acceptance controls. No model can toggle these controls.
use super::*;
use vesper_harness::acceptance_settings::AcceptanceSettings;

pub async fn settings(
    terminal: &mut Terminal<Backend>,
    root: &std::path::Path,
    initial: Option<AcceptanceSettings>,
    theme: &str,
) -> Result<Option<AcceptanceSettings>, String> {
    let mut draft = match initial {
        Some(settings) => settings,
        None => AcceptanceSettings::load(root)?,
    };
    let mut selected = 0usize;
    let mut notice =
        "Vesper recognizes and remembers the PRD automatically. An explicit path is optional."
            .to_string();
    loop {
        terminal
            .draw(|frame| render(frame, &draft, selected, &notice, theme))
            .map_err(|e| e.to_string())?;
        let Event::Key(key) = event::read().map_err(|e| e.to_string())? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => selected = (selected + 1) % 4,
            KeyCode::Enter if selected == 3 => return Ok(None),
            KeyCode::Enter if selected == 2 => match draft.validate(root) {
                Ok(()) => return Ok(Some(draft)),
                Err(error) => notice = error,
            },
            KeyCode::Enter | KeyCode::Char(' ') if selected == 0 => draft.enabled = !draft.enabled,
            KeyCode::Backspace if selected == 1 => {
                draft.prd.pop();
            }
            KeyCode::Char(c) if selected == 1 && draft.prd.len() < 2048 => draft.prd.push(c),
            _ => {}
        }
    }
}

fn render(
    frame: &mut ratatui::Frame<'_>,
    draft: &AcceptanceSettings,
    selected: usize,
    notice: &str,
    theme: &str,
) {
    let rows = [
        format!(
            "Enforced completion: {}",
            if draft.enabled { "ON" } else { "OFF" }
        ),
        format!(
            "PRD: {}",
            if draft.prd.is_empty() {
                "automatic"
            } else {
                &draft.prd
            }
        ),
        "Save".into(),
        "Cancel".into(),
    ];
    agent_vesper_tui::settings_menu::render_menu(
        frame,
        &rows,
        selected,
        "Implementation acceptance",
        &format!(
            "{notice}\nTests and independent review are required. Restart requires fresh evidence."
        ),
        "↑↓ select · Enter toggle/save · type PRD path · Esc cancel",
        theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acceptance_uses_every_active_theme() {
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
            let menu =
                agent_vesper_tui::settings_menu::area(ratatui::layout::Rect::new(0, 0, 80, 24), 4);
            terminal
                .draw(|f| {
                    agent_vesper_tui::settings_menu::render_menu(
                        f,
                        &["one".into(), "two".into(), "three".into(), "four".into()],
                        0,
                        "Settings",
                        "",
                        "",
                        theme,
                    )
                })
                .unwrap();
            let background = terminal.backend().buffer()[(0, 0)].bg;
            let selection = terminal.backend().buffer()[(menu.x + 1, menu.y + 1)].bg;
            terminal
                .draw(|f| render(f, &AcceptanceSettings::default(), 0, "", theme))
                .unwrap();
            assert_eq!(terminal.backend().buffer()[(0, 0)].bg, background);
            assert_eq!(
                terminal.backend().buffer()[(menu.x + 1, menu.y + 1)].bg,
                selection
            );
        }
    }
    #[test]
    fn acceptance_native_settings_show_scope_and_save_cancel() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &AcceptanceSettings {
                        enabled: true,
                        prd: "PRD.md".into(),
                    },
                    2,
                    "",
                    "chatgpt-black",
                )
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        for expected in [
            "Implementation acceptance",
            "ON",
            "PRD.md",
            "Save",
            "Cancel",
            "fresh evidence",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
    }
}
