//! Terminal renderer abstraction (Stage 11b).
//!
//! The approved Stage 11b architecture defines a [`TerminalRenderer`] trait
//! that decouples the TUI event loop from any specific terminal backend. The
//! default implementation uses [`ratatui`] + [`crossterm`]; the trait exists
//! so the loop and the unit-tested modules (Plan Mode, command registry,
//! superpowers adapter) can be exercised under a stub renderer without
//! touching a real terminal.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::plan_mode::{PlanPhase, PlanState};
use crate::superpowers::{ProviderSuperpowerSurface, SuperpowerOverrides};

/// Pure view model the renderer consumes every frame.
#[derive(Debug, Clone, Default)]
pub struct ViewModel {
    /// Plan Mode state for the status panel.
    pub plan: PlanState,
    /// Active provider superpower surface.
    pub superpowers: Option<ProviderSuperpowerSurface>,
    /// Per-turn superpower overrides applied via `/effort`, `/thinking`,
    /// `/model`. Surfaced so the driver sees the active layer at a glance.
    pub overrides: SuperpowerOverrides,
    /// Transcript lines shown in the main panel (most recent at the bottom).
    pub transcript: Vec<String>,
    /// Current input buffer (for echo).
    pub input: String,
    /// One-line status / error / notice.
    pub status: Option<String>,
    /// Slash-command palette entries matching the current input.
    pub command_menu: Vec<(String, String)>,
    /// Highlighted command-palette entry.
    pub command_menu_selected: usize,
    /// Whether an agent turn is currently running.
    pub agent_running: bool,
}

/// Abstraction over a terminal backend.
///
/// Production code uses [`RatatuiRenderer`]; tests inject a [`StubRenderer`]
/// that records the most recent view model instead of painting pixels.
pub trait TerminalRenderer {
    /// Renders one frame from the view model.
    fn render(&mut self, model: &ViewModel);
}

/// Renders a [`ViewModel`] into the active [`Frame`] using the ratatui/crossterm
/// backend. Used by the production binary; the [`TerminalRenderer`] trait is
/// exercised by [`StubRenderer`] in unit tests.
pub fn render_to_frame(frame: &mut Frame<'_>, model: &ViewModel) {
    let area = frame.area();
    let menu_height = if model.command_menu.is_empty() {
        0
    } else {
        (model.command_menu.len() as u16 + 2).min(12)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(8),    // conversation + sidebar
            Constraint::Length(menu_height),
            Constraint::Length(1), // command hint
            Constraint::Length(1), // activity
            Constraint::Length(3), // composer
            Constraint::Length(1), // footer
        ])
        .split(area);

    // Header — a compact, persistent identity/status line like the oracle's
    // Textual header. The old renderer spent the entire top three rows on a
    // banner and left no room for the conversation/sidebar composition.
    let phase = model.plan.phase();
    let phase_style = banner_style_for_phase(phase);
    let model_name = superpower_value_for(model, "model").unwrap_or_else(|| "provider".into());
    let state = if model.agent_running {
        "RUNNING"
    } else {
        "READY"
    };
    let header = Line::from(vec![
        Span::styled(
            " Agent Vesper ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("• {model_name} "), Style::default().fg(Color::Cyan)),
        Span::styled(format!("• {} ", phase.label()), phase_style),
        Span::styled(format!("• {state}"), Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(header), chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(34)])
        .split(chunks[1]);

    // Conversation — prepended with Plan Mode context (pending questions
    // while PLANNING, the plan body while REVIEW), then the live transcript.
    let transcript_lines = transcript_lines_for(model);
    let transcript_items: Vec<ListItem> = if transcript_lines.is_empty() {
        vec![ListItem::new(Line::from(vec![
            Span::styled(
                "Agent Vesper",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ready. Type a prompt, or type "),
            Span::styled(
                "/",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to browse commands."),
        ]))]
    } else {
        transcript_lines.into_iter().map(ListItem::new).collect()
    };
    frame.render_widget(
        List::new(transcript_items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Conversation "),
        ),
        body[0],
    );

    render_sidebar(frame, body[1], model);

    if !model.command_menu.is_empty() {
        let menu_items = model
            .command_menu
            .iter()
            .take(10)
            .map(|(command, description)| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        command.clone(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(description.clone(), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(menu_items)
                .block(Block::default().borders(Borders::ALL).title(" Commands "))
                .highlight_style(Style::default().bg(Color::Rgb(17, 49, 75)).fg(Color::White))
                .highlight_symbol("▸ "),
            chunks[2],
        );
        // ratatui's stateless List does not own a selected index. Emphasize
        // the selected row explicitly so Tab/↑/↓ remains visible.
        let selected = model.command_menu_selected.min(9);
        let selected_y = chunks[2].y.saturating_add(1 + selected as u16);
        if selected_y < chunks[2].bottom().saturating_sub(1) {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("▸ {}", model.command_menu[selected].0),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ))),
                ratatui::layout::Rect {
                    x: chunks[2].x.saturating_add(1),
                    y: selected_y,
                    width: chunks[2].width.saturating_sub(2),
                    height: 1,
                },
            );
        }
    }

    let hint = if model.command_menu.is_empty() {
        "↑↓ history  •  Enter send  •  Ctrl-C quit"
    } else {
        "↑↓ navigate  •  Tab complete  •  Enter run  •  Esc close"
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );

    let activity = if model.agent_running {
        "● Working — waiting for the provider/agent loop"
    } else {
        model.status.as_deref().unwrap_or("○ Ready")
    };
    frame.render_widget(
        Paragraph::new(activity).style(Style::default().fg(if model.agent_running {
            Color::Yellow
        } else {
            Color::Gray
        })),
        chunks[4],
    );

    // Composer. Keep a visible insertion point: the old renderer hid the
    // terminal cursor, which made the input look like a static mockup.
    let input_value = if model.input.is_empty() {
        "> Type a prompt or / for commands".to_string()
    } else {
        format!("> {}", model.input)
    };
    frame.render_widget(
        Paragraph::new(input_value)
            .block(Block::default().borders(Borders::ALL).title(" Composer ")),
        chunks[5],
    );
    let cursor_x = chunks[5]
        .x
        .saturating_add(1 + 2 + model.input.chars().count() as u16)
        .min(chunks[5].right().saturating_sub(1));
    frame.set_cursor_position(Position {
        x: cursor_x,
        y: chunks[5].y.saturating_add(1),
    });
    frame.render_widget(
        Paragraph::new("Type / for commands  •  Ctrl-C quit")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[6],
    );
}

fn render_sidebar(frame: &mut Frame<'_>, area: ratatui::layout::Rect, model: &ViewModel) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(4),
            Constraint::Length(6),
        ])
        .split(area);

    let model_name = superpower_value_for(model, "model").unwrap_or_else(|| "provider".into());
    let thinking = superpower_value_for(model, "thinking").unwrap_or_else(|| "enabled".into());
    let session = vec![
        Line::from(Span::styled(
            "Session",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Model       {model_name}")),
        Line::from(format!("Thinking    {thinking}")),
        Line::from(format!("Phase       {}", model.plan.phase().label())),
        Line::from(format!("Transcript  {} lines", model.transcript.len())),
    ];
    frame.render_widget(
        Paragraph::new(session).block(Block::default().borders(Borders::ALL).title(" Session ")),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(superpower_lines_for(model))
            .block(Block::default().borders(Borders::ALL).title(" Controls ")),
        chunks[1],
    );

    let plan = match model.plan.phase() {
        PlanPhase::Normal => "No active plan".to_string(),
        PlanPhase::Planning => format!(
            "Planning\n{} question(s) pending",
            model.plan.pending_questions().len()
        ),
        PlanPhase::Review => "Plan ready\n/approve to execute\n/cancel to abort".into(),
        PlanPhase::Executing => "Executing approved plan".into(),
    };
    frame.render_widget(
        Paragraph::new(plan).block(Block::default().borders(Borders::ALL).title(" Plan ")),
        chunks[2],
    );
}

fn superpower_value_for(model: &ViewModel, alias: &str) -> Option<String> {
    let descriptor = model
        .superpowers
        .as_ref()?
        .descriptors()
        .iter()
        .find(|descriptor| {
            descriptor
                .command_alias
                .as_ref()
                .is_some_and(|value| value.as_str() == alias)
        })?;
    model
        .overrides
        .get(descriptor.id.as_str(), Some(&descriptor.default_value))
        .map(|value| format_superpower_value(&value))
}

/// Builds the transcript lines for the main panel: Plan Mode context first
/// (pending questions during PLANNING, the plan body during REVIEW), then the
/// accumulated transcript.
fn transcript_lines_for(model: &ViewModel) -> Vec<String> {
    let mut lines = Vec::new();
    match model.plan.phase() {
        PlanPhase::Planning => {
            for (index, question) in model.plan.pending_questions().iter().enumerate() {
                lines.push(format!("❓ Q{}: {}", index + 1, question.text.as_str()));
            }
        }
        PlanPhase::Review => {
            if let Some(body) = model.plan.plan() {
                lines.push(format!("📋 Plan under review:\n{}", body.as_str()));
            }
        }
        PlanPhase::Normal | PlanPhase::Executing => {}
    }
    lines.extend(model.transcript.iter().cloned());
    lines
}

fn banner_style_for_phase(phase: PlanPhase) -> Style {
    let color = match phase {
        PlanPhase::Normal => Color::Blue,
        PlanPhase::Planning => Color::Yellow,
        PlanPhase::Review => Color::Magenta,
        PlanPhase::Executing => Color::Green,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn superpower_lines_for(model: &ViewModel) -> Vec<Line<'_>> {
    let Some(surface) = model.superpowers.as_ref() else {
        return vec![Line::from(Span::styled(
            "No provider superpowers advertised.",
            Style::default().fg(Color::DarkGray),
        ))];
    };
    let mut lines = Vec::new();
    for descriptor in surface.descriptors() {
        let alias = descriptor
            .command_alias
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or("<no-alias>");
        let display = descriptor.display_name.as_str();
        // Annotate with the active override (or the advertised default) so
        // the driver sees the live superpower layer, not just the menu.
        let active = model
            .overrides
            .get(descriptor.id.as_str(), Some(&descriptor.default_value));
        let suffix = match active {
            Some(value) => format!(" = {}", format_superpower_value(&value)),
            None => String::new(),
        };
        let style = if model.overrides.get(descriptor.id.as_str(), None).is_some() {
            // Override is live — emphasize it.
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("/{alias} — {display}{suffix}"),
            style,
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Provider advertised no superpowers.",
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}

/// Renders a superpower value as a short, terminal-safe string.
fn format_superpower_value(value: &vesper_provider::SuperpowerValue) -> String {
    use vesper_provider::SuperpowerValue;
    match value {
        SuperpowerValue::Choice { value } => value.as_str().to_string(),
        SuperpowerValue::Flag { value } => value.to_string(),
        SuperpowerValue::Number { value } => value.to_string(),
    }
}

/// In-memory renderer that records the most recent view model. Used by unit
/// tests so they never touch a real terminal.
#[derive(Debug, Default, Clone)]
pub struct StubRenderer {
    /// Most recent view model passed to [`TerminalRenderer::render`].
    pub last_model: Option<ViewModel>,
    /// Number of frames rendered so far.
    pub frames: usize,
}

impl StubRenderer {
    /// Creates an empty stub.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl TerminalRenderer for StubRenderer {
    fn render(&mut self, model: &ViewModel) {
        self.last_model = Some(model.clone());
        self.frames += 1;
    }
}

#[cfg(test)]
mod tests {
    //! Stub renderer records the most recent model verbatim.

    use super::*;
    use crate::plan_mode::PlanState;
    use vesper_domain::ProviderId;
    use vesper_provider::SuperpowerDescriptor;

    #[test]
    fn stub_records_model_and_increments_frame_counter() {
        let mut renderer = StubRenderer::new();
        let model = ViewModel {
            plan: PlanState::default(),
            superpowers: None,
            overrides: SuperpowerOverrides::default(),
            transcript: vec!["hello".into()],
            input: "/plan ship it".into(),
            status: Some("ok".into()),
            ..ViewModel::default()
        };
        renderer.render(&model);
        let last = renderer.last_model.expect("model recorded");
        assert_eq!(last.transcript, vec!["hello".to_string()]);
        assert_eq!(last.input, "/plan ship it");
        assert_eq!(renderer.frames, 1);
    }

    #[test]
    fn banner_style_matches_phase() {
        assert_eq!(
            banner_style_for_phase(PlanPhase::Normal).fg,
            Some(ratatui::style::Color::Blue)
        );
        assert_eq!(
            banner_style_for_phase(PlanPhase::Planning).fg,
            Some(ratatui::style::Color::Yellow)
        );
        assert_eq!(
            banner_style_for_phase(PlanPhase::Review).fg,
            Some(ratatui::style::Color::Magenta)
        );
        assert_eq!(
            banner_style_for_phase(PlanPhase::Executing).fg,
            Some(ratatui::style::Color::Green)
        );
    }

    #[test]
    fn superpower_lines_handles_missing_surface() {
        let model = ViewModel {
            plan: PlanState::default(),
            superpowers: None,
            overrides: SuperpowerOverrides::default(),
            transcript: Vec::new(),
            input: String::new(),
            status: None,
            ..ViewModel::default()
        };
        let lines = superpower_lines_for(&model);
        assert_eq!(lines.len(), 1);
    }

    // Compile-time guard so the test module exercises the trait name.
    fn _assert_renderer_object_safe(_: &dyn TerminalRenderer) {}

    #[test]
    fn renderer_trait_is_object_safe_via_dummy_use() {
        // The function above is the actual proof; this test exists so clippy
        // does not flag `_assert_renderer_object_safe` as dead code.
        let renderer: StubRenderer = StubRenderer::new();
        let model = ViewModel {
            plan: PlanState::default(),
            superpowers: Some(ProviderSuperpowerSurface::new(
                ProviderId::new("x").unwrap(),
                Vec::<SuperpowerDescriptor>::new(),
            )),
            overrides: SuperpowerOverrides::default(),
            transcript: Vec::new(),
            input: String::new(),
            status: None,
            ..ViewModel::default()
        };
        let _: &dyn TerminalRenderer = &renderer;
        let mut dynamic: Box<dyn TerminalRenderer> = Box::new(renderer);
        dynamic.render(&model);
    }
}
