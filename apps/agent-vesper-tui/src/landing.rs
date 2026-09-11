//! Terminal-only welcome screen; actions reuse existing host controls.
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

pub const RELEASES_URL: &str = "https://github.com/99percentgrip/agent-vesper/releases/latest";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingAction {
    Code,
    Settings,
    CheckUpdates,
    Quit,
}

#[derive(Default)]
pub struct LandingState {
    pub selected: usize,
    pub checking: bool,
    pub notice: String,
}

impl LandingState {
    pub fn navigate(&mut self, forward: bool) {
        self.selected = (self.selected + if forward { 1 } else { 2 }) % 3;
    }

    pub fn activate(&self) -> LandingAction {
        match self.selected {
            1 => LandingAction::Settings,
            2 => LandingAction::CheckUpdates,
            _ => LandingAction::Code,
        }
    }
}

/// Each row uses one fixed canvas and mirrored cells, never independent centering.
fn mascot(large: bool, compact: bool) -> Vec<String> {
    let halves: Vec<(&str, char)> = if large {
        vec![
            ("", '✦'),
            ("                   ╱", ' '),
            ("              ────╱ ", '✧'),
            ("        ▄▄▄▄▄      ╲", ' '),
            ("     ▄▀▀     ╲╭─────", '┴'),
            ("    █   ╱╲    │     ", ' '),
            ("    █  ╱  ╲   │  ●  ", ' '),
            ("    █  ╲   ╲  │     ", ' '),
            ("     ▀▄ ╲   ╲ ╰╮  ╰─", '─'),
            ("       ▀▀▄▄  ╲ ╰╮   ", '✦'),
            ("           ▀▀ ╲ ╰╮  ", ' '),
            ("               ╲ ╰╮ ", ' '),
            ("                  ╰─", '─'),
        ]
    } else if compact {
        vec![
            ("", '✦'),
            ("      ╭─────", '┴'),
            ("   ▐▌ │  ●  ", ' '),
            ("    ╲ │   ╰─", '─'),
            ("      ╰╮    ", '✧'),
            ("       ╰────", '─'),
        ]
    } else {
        return Vec::new();
    };
    let half_width = if large { 20 } else { 12 };
    halves
        .into_iter()
        .map(|(left, center)| {
            let left = format!("{left:<half_width$}");
            let right: String = left.chars().rev().map(mirror_cell).collect();
            format!("{left}{center}{right}")
        })
        .collect()
}

fn mirror_cell(cell: char) -> char {
    match cell {
        '╱' => '╲',
        '╲' => '╱',
        '╭' => '╮',
        '╮' => '╭',
        '╰' => '╯',
        '╯' => '╰',
        '▐' => '▌',
        '▌' => '▐',
        other => other,
    }
}

/// Shared geometry for painting and mouse hit-testing, including compact terminals.
pub fn menu_area(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(52);
    let height = 5.min(area.height);
    let desired_y = if area.height >= 32 {
        21
    } else if area.height >= 22 {
        13
    } else {
        5
    };
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + desired_y.min(area.height.saturating_sub(height + 3)),
        width,
        height,
    )
}

pub fn action_at(area: Rect, x: u16, y: u16) -> Option<usize> {
    let menu = menu_area(area);
    (x > menu.x
        && x < menu.right().saturating_sub(1)
        && y > menu.y
        && y < menu.bottom().saturating_sub(1))
    .then(|| usize::from(y - menu.y - 1))
    .filter(|index| *index < 3)
}

pub fn render(
    frame: &mut Frame<'_>,
    state: &LandingState,
    provider: &str,
    model: &str,
    version: &str,
    tick: u64,
    theme: &str,
) {
    let area = frame.area();
    let palette = crate::ui::theme_palette(theme);
    let background = palette.background;
    let accent = palette.accent;
    let dim = palette.muted;
    let text = palette.text;
    frame.render_widget(
        Block::default().style(Style::default().bg(background).fg(text)),
        area,
    );
    if area.width < 30 || area.height < 14 {
        frame.render_widget(Paragraph::new("VESPER\nEnter: code  S: settings\nU: updates  Esc: quit\nResize for the full menu.").style(Style::default().fg(accent)), area);
        return;
    }
    let large = area.height >= 32 && area.width >= 58;
    let art = mascot(large, area.height >= 22);
    let art_height = art.len() as u16;
    let top = if large { 2 } else { 1 };
    frame.render_widget(
        Paragraph::new(art.into_iter().map(Line::from).collect::<Vec<_>>())
            .style(Style::default().fg(accent)),
        Rect::new(
            area.x + area.width.saturating_sub(if large { 41 } else { 25 }) / 2,
            area.y + top,
            area.width.min(if large { 41 } else { 25 }),
            art_height,
        ),
    );
    let title_y = area.y + top + art_height;
    frame.render_widget(
        Paragraph::new("A G E N T   V E S P E R")
            .alignment(Alignment::Center)
            .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
        Rect::new(area.x, title_y, area.width, 1),
    );
    let model = if model.is_empty() {
        "model unavailable"
    } else {
        model
    };
    frame.render_widget(
        Paragraph::new(format!("{provider} · {model} · v{version}"))
            .alignment(Alignment::Center)
            .style(Style::default().fg(text)),
        Rect::new(area.x, title_y + 1, area.width, 1),
    );
    let menu = menu_area(area);
    let labels = ["Start coding", "Settings", "Check for updates"];
    let lines: Vec<Line<'_>> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let chosen = index == state.selected;
            Line::styled(
                format!(
                    "{:<width$}",
                    format!("  {}  {label}", if chosen { "›" } else { " " }),
                    width = usize::from(menu.width.saturating_sub(2))
                ),
                if chosen {
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
    let notice = if state.checking {
        let mut bar = vec!['░'; 16];
        for offset in 0..4 {
            bar[((tick as usize) + offset) % 16] = '█';
        }
        format!("{} Checking GitHub…", bar.into_iter().collect::<String>())
    } else if state.notice.is_empty() {
        "Ready when you are.".into()
    } else {
        state.notice.clone()
    };
    frame.render_widget(
        Paragraph::new(notice)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center)
            .style(Style::default().fg(accent)),
        Rect::new(
            area.x,
            menu.bottom(),
            area.width,
            area.bottom().saturating_sub(menu.bottom() + 1).min(3),
        ),
    );
    frame.render_widget(
        Paragraph::new(if area.width < 50 {
            "↑↓ Enter · R releases · Esc"
        } else {
            "↑↓ select · Enter open · R releases · Esc quit"
        })
        .alignment(Alignment::Center)
        .style(Style::default().fg(dim)),
        Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
    );
}

/// Only stable numeric release versions are compared; unexpected tags fail honestly.
pub fn release_notice(current: &str, latest: &str) -> Result<String, &'static str> {
    fn version(value: &str) -> Option<[u64; 3]> {
        let parts: Vec<_> = value.trim_start_matches('v').split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some([
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ])
    }
    let installed =
        version(current).ok_or("Cannot compare this development version; press R for releases.")?;
    let released = version(latest).ok_or("Unrecognized release version; press R for releases.")?;
    Ok(match released.cmp(&installed) {
        std::cmp::Ordering::Greater => {
            format!("{latest} available · R: release notes and downloads")
        }
        std::cmp::Ordering::Equal => format!("Up to date · v{current}"),
        std::cmp::Ordering::Less => format!("Installed v{current} is newer than release {latest}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn selected_theme_colors_cover_canvas_mascot_and_menu() {
        for theme in [
            "chatgpt-black",
            "chatgpt-white",
            "light",
            "dracula",
            "nord",
            "ansi",
        ] {
            let mut terminal = Terminal::new(TestBackend::new(100, 38)).unwrap();
            terminal
                .draw(|frame| {
                    render(
                        frame,
                        &LandingState::default(),
                        "test",
                        "model",
                        "1.0.0",
                        0,
                        theme,
                    )
                })
                .unwrap();
            let palette = crate::ui::theme_palette(theme);
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].bg, palette.background, "{theme}");
            assert_eq!(buffer[(49, 2)].fg, palette.accent, "mascot: {theme}");
            let menu = menu_area(Rect::new(0, 0, 100, 38));
            assert_eq!(
                buffer[(menu.x + 3, menu.y + 1)].bg,
                palette.selection,
                "selection: {theme}"
            );
        }
    }

    #[test]
    fn mascot_has_fixed_width_and_a_shared_mirror_axis() {
        for (large, width) in [(true, 41), (false, 25)] {
            for row in mascot(large, true) {
                let cells: Vec<_> = row.chars().collect();
                assert_eq!(cells.len(), width, "{row}");
                assert_eq!(Line::from(row.clone()).width(), width);
                for x in 0..width / 2 {
                    assert_eq!(cells[x], mirror_cell(cells[width - 1 - x]), "{row}");
                }
            }
        }
    }

    #[test]
    fn menu_navigation_wraps_and_actions_are_distinct() {
        let mut state = LandingState::default();
        assert_eq!(state.activate(), LandingAction::Code);
        state.navigate(false);
        assert_eq!(state.activate(), LandingAction::CheckUpdates);
        state.navigate(false);
        assert_eq!(state.activate(), LandingAction::Settings);
        state.navigate(true);
        state.navigate(true);
        assert_eq!(state.activate(), LandingAction::Code);
    }

    #[test]
    fn responsive_menu_and_hit_testing_match_visible_actions() {
        for (width, height) in [(120, 40), (80, 24), (40, 16), (30, 14)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render(
                        frame,
                        &LandingState::default(),
                        "test-provider",
                        "test-model",
                        "1.2.3",
                        0,
                        "chatgpt-black",
                    )
                })
                .unwrap();
            let area = Rect::new(0, 0, width, height);
            let menu = menu_area(area);
            let buffer = terminal.backend().buffer();
            for (index, label) in ["Start coding", "Settings", "Check for updates"]
                .iter()
                .enumerate()
            {
                let y = menu.y + 1 + index as u16;
                let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                assert!(row.contains(label), "{width}x{height}: {row}");
                assert_eq!(action_at(area, menu.x + 2, y), Some(index));
            }
            assert_eq!(action_at(area, menu.x, menu.y), None);
        }
    }

    #[test]
    fn compact_update_errors_keep_the_retry_instruction_visible() {
        let mut terminal = Terminal::new(TestBackend::new(40, 16)).unwrap();
        let state = LandingState {
            notice: "Update check unavailable (network or timeout). Press U to retry.".into(),
            ..Default::default()
        };
        terminal
            .draw(|frame| render(frame, &state, "test", "model", "1.0.0", 0, "chatgpt-black"))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..16)
            .map(|y| (0..40).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        assert!(rows.iter().any(|row| row.contains("Press U to retry.")));
        assert!(rows.last().unwrap().contains("Esc"));
    }

    #[test]
    fn tiny_and_empty_terminals_do_not_panic() {
        for (width, height) in [(0, 0), (1, 1), (20, 8), (100, 2)] {
            Terminal::new(TestBackend::new(width, height))
                .unwrap()
                .draw(|frame| {
                    render(
                        frame,
                        &LandingState::default(),
                        "test",
                        "",
                        "1.0.0",
                        0,
                        "chatgpt-black",
                    )
                })
                .unwrap();
        }
    }

    #[test]
    fn release_comparison_is_numeric_and_never_labels_unknown_as_current() {
        assert!(
            release_notice("0.9.9", "v0.10.0")
                .unwrap()
                .contains("available")
        );
        assert!(
            release_notice("0.21.7", "v0.21.7")
                .unwrap()
                .contains("Up to date")
        );
        assert!(
            release_notice("0.22.0", "v0.21.7")
                .unwrap()
                .contains("newer")
        );
        for value in ["oops", "v1.0.0-rc1", "1.2", "1.2.3.4", "\u{1b}[31m"] {
            assert!(release_notice("1.2.3", value).is_err());
        }
    }
}
