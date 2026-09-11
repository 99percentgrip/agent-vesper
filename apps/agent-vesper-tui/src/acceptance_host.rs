//! Native Save/Cancel acceptance controls. No model can toggle these controls.
use super::*;
use vesper_harness::acceptance_settings::AcceptanceSettings;

pub async fn settings(
    terminal: &mut Terminal<Backend>,
    root: &std::path::Path,
    initial: Option<AcceptanceSettings>,
) -> Result<Option<AcceptanceSettings>, String> {
    let mut draft = match initial {
        Some(settings) => settings,
        None => AcceptanceSettings::load(root)?,
    };
    let mut selected = 0usize;
    let mut notice = "Select the PRD that defines this implementation's full scope.".to_string();
    loop {
        terminal
            .draw(|frame| render(frame, &draft, selected, &notice))
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
) {
    use ratatui::{
        layout::Rect,
        widgets::{Block, Borders, Clear, Paragraph, Wrap},
    };
    let area = frame.area();
    let width = area.width.min(90);
    let height = area.height.min(15);
    let panel = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    let rows = [
        format!(
            "Enforced completion: {}",
            if draft.enabled { "ON" } else { "OFF" }
        ),
        format!("PRD path: {}", draft.prd),
        "Save".into(),
        "Cancel".into(),
    ];
    let mut text = "↑/↓ select · Enter toggle/save · type PRD path · Esc cancel\n\n".to_string();
    for (i, row) in rows.iter().enumerate() {
        text.push_str(&format!(
            "{} {row}\n",
            if i == selected { "›" } else { " " }
        ));
    }
    text.push_str(&format!(
        "\n{notice}\nTests and independent review are required. Restart requires fresh evidence."
    ));
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Settings › Implementation acceptance "),
        ),
        panel,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
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
