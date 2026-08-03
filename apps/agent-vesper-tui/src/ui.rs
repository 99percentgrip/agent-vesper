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
    widgets::{
        Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

use crate::dispatch::{PanelVisibility, SessionControls, TaskItem, TerminalPreferences};
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
    /// Typed live controls governing real agent turns.
    pub controls: SessionControls,
    /// Native dashboard panel visibility.
    pub panels: PanelVisibility,
    /// Latest model-authored TODO plan.
    pub task_plan: Vec<TaskItem>,
    /// Bounded live provider/tool activity.
    pub activity: Vec<String>,
    /// Provider-visible reasoning streamed during the current turn.
    pub reasoning: String,
    /// Assistant response accumulated during the current turn.
    pub live_response: String,
    /// Last structured turn-completion report.
    pub last_report: Vec<String>,
    /// Active F4 working-tree view, when the panel is open.
    pub working_tree_title: Option<String>,
    /// Bounded output from the selected real Git/files/GitHub query.
    pub working_tree_lines: Vec<String>,
    /// Theme, accessibility, mouse, and sound preferences.
    pub preferences: TerminalPreferences,
    /// Manual conversation scroll offset in **rendered lines from the top**.
    /// `None` = auto-follow (stick to bottom, the default); `Some(n)` = the
    /// user pressed PageUp/Home and is reading history at offset `n`. The
    /// renderer mirrors this into a `ScrollbarState` so the visual scrollbar
    /// reflects the same position the `Paragraph::scroll` call uses.
    pub conversation_manual_scroll: Option<u16>,
}

/// Abstraction over a terminal backend.
///
/// Production code uses [`RatatuiRenderer`]; tests inject a [`StubRenderer`]
/// that records the most recent view model instead of painting pixels.
pub trait TerminalRenderer {
    /// Renders one frame from the view model.
    fn render(&mut self, model: &ViewModel);
}

/// Clickable footer segments shared by rendering and mouse hit-testing.
pub const FOOTER_ACTIONS: &[(&str, &str)] = &[
    ("^f Search", "open_search"),
    ("^x Quit", "quit_agent"),
    ("^c Cancel turn", "cancel_turn"),
    ("^l Clear view", "clear_transcript"),
    ("F1 Help", "show_help"),
    ("F2 Reasoning", "toggle_thinking"),
    ("F3 Settings", "settings"),
    ("F4 Working tree", "toggle_working_tree"),
    ("F5 Push to talk", "toggle_voice"),
    ("F6 History", "open_history"),
    ("^y Copy response", "copy_last_response"),
    ("^p Palette", "open_palette"),
];

#[must_use]
pub fn command_menu_height(area_height: u16, item_count: usize) -> u16 {
    if item_count == 0 {
        0
    } else {
        ((item_count.saturating_mul(2) + 2) as u16)
            .min(area_height.saturating_mul(2) / 3)
            .max(4)
    }
}

/// Renders a [`ViewModel`] into the active [`Frame`] using the ratatui/crossterm
/// backend. Used by the production binary; the [`TerminalRenderer`] trait is
/// exercised by [`StubRenderer`] in unit tests.
pub fn render_to_frame(frame: &mut Frame<'_>, model: &ViewModel) {
    if model.preferences.screen_reader {
        render_screen_reader(frame, model);
        return;
    }
    let area = frame.area();
    let theme = theme_style(&model.preferences.theme);
    frame.render_widget(Block::default().style(theme), area);
    let menu_height = command_menu_height(area.height, model.command_menu.len());
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

    let show_sidebar = model.panels.sidebar && chunks[1].width >= 110;
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if show_sidebar {
            vec![Constraint::Min(50), Constraint::Length(40)]
        } else {
            vec![Constraint::Percentage(100), Constraint::Length(0)]
        })
        .split(chunks[1]);

    let working_tree_height = if model.working_tree_title.is_some() {
        10
    } else {
        0
    };
    let reasoning_height = if model.panels.reasoning { 10 } else { 0 };
    let activity_height = 0;
    let conversation_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(reasoning_height),
            Constraint::Length(activity_height),
            Constraint::Length(working_tree_height),
        ])
        .split(body[0]);

    // Conversation — prepended with Plan Mode context (pending questions
    // while PLANNING, the plan body while REVIEW), then the live transcript.
    let transcript_lines = transcript_lines_for(model);
    let transcript_area = conversation_chunks[0];
    let inner_width = usize::from(transcript_area.width.saturating_sub(2));
    // Render the transcript to markdown Lines once, then estimate the
    // wrapped-line count from the *rendered* output. Estimating from the raw
    // strings would over-count (markdown collapses fenced code markers) and
    // over-scroll the first line out of view.
    let (transcript, wrapped_lines) = if transcript_lines.is_empty() {
        let ready = Line::from(vec![
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
        ]);
        (ratatui::text::Text::from(ready), 1)
    } else {
        // Parse the joined transcript as one markdown document so multi-line
        // constructs (code fences, multi-line lists) render correctly, and so
        // streaming partial syntax degrades gracefully.
        let joined = transcript_lines.join("\n");
        let rendered = crate::markdown::render_markdown(&joined);
        let estimate = estimated_wrapped_lines(&rendered, inner_width);
        (ratatui::text::Text::from(rendered), estimate)
    };
    let paragraph = Paragraph::new(transcript).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(59, 160, 255)))
            .title(" Conversation "),
    );
    let visible_lines = usize::from(transcript_area.height.saturating_sub(2));
    let max_scroll = wrapped_lines
        .saturating_sub(visible_lines)
        .min(u16::MAX as usize) as u16;
    // `None` (auto-follow) sticks to the bottom; `Some(n)` honors the user's
    // manual scroll position (clamped to the valid range so a resize or new
    // content cannot push the offset past the last rendered line).
    let effective_scroll = model
        .conversation_manual_scroll
        .map(|manual| manual.min(max_scroll))
        .unwrap_or(max_scroll);
    frame.render_widget(paragraph.scroll((effective_scroll, 0)), transcript_area);

    // Vertical scrollbar on the right edge of the Conversation block. The
    // state mirrors the same `effective_scroll` used by the paragraph so the
    // thumb position is always truthful, even when the user is in manual
    // scroll mode (PageUp/Home). We render against the *inner* area (inside
    // the borders) so the scrollbar sits next to the text rather than over
    // the right border.
    let mut scrollbar_state = ScrollbarState::new(wrapped_lines.min(u16::MAX as usize))
        .position(usize::from(effective_scroll))
        .viewport_content_length(visible_lines);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::default().fg(Color::Rgb(120, 140, 160)))
            .track_style(Style::default().fg(Color::Rgb(60, 70, 85))),
        transcript_area,
        &mut scrollbar_state,
    );

    if reasoning_height > 0 {
        let reasoning_lines: Vec<Line<'static>> = if model.reasoning.is_empty() {
            vec![Line::from("Waiting for provider-visible reasoning…")]
        } else {
            crate::markdown::render_markdown(&model.reasoning)
        };
        frame.render_widget(
            Paragraph::new(reasoning_lines)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Color::Rgb(159, 122, 234)))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(159, 122, 234)))
                        .title(" Reasoning "),
                ),
            conversation_chunks[1],
        );
    }
    if activity_height > 0 {
        let activity = model
            .activity
            .iter()
            .rev()
            .take(6)
            .rev()
            .map(|line| ListItem::new(line.as_str()))
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(activity)
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL).title(format!(
                    " Live activity • {} event(s) ",
                    model.activity.len()
                ))),
            conversation_chunks[2],
        );
    }
    if working_tree_height > 0 {
        frame.render_widget(
            Paragraph::new(model.working_tree_lines.join("\n"))
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(format!(
                    " Working tree • {} • F4 cycles ",
                    model.working_tree_title.as_deref().unwrap_or_default()
                ))),
            conversation_chunks[3],
        );
    }

    if show_sidebar {
        render_sidebar(frame, body[1], model);
    }

    if !model.command_menu.is_empty() {
        let selected = model
            .command_menu_selected
            .min(model.command_menu.len().saturating_sub(1));
        let menu_items = model
            .command_menu
            .iter()
            .enumerate()
            .map(|(index, (command, description))| {
                let is_selected = index == selected;
                let row_style = if is_selected {
                    Style::default().bg(Color::Rgb(17, 49, 75))
                } else {
                    Style::default()
                };
                ListItem::new(vec![
                    Line::from(Span::styled(
                        if is_selected {
                            format!("▸ {command}")
                        } else {
                            format!("  {command}")
                        },
                        row_style
                            .fg(if is_selected {
                                Color::White
                            } else {
                                Color::Cyan
                            })
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        format!("  {description}"),
                        row_style.fg(if is_selected {
                            Color::White
                        } else {
                            Color::Gray
                        }),
                    )),
                ])
                .style(row_style)
            })
            .collect::<Vec<_>>();
        let mut menu_state = ListState::default().with_selected(Some(selected));
        frame.render_stateful_widget(
            List::new(menu_items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(59, 160, 255)))
                    .style(Style::default().bg(Color::Rgb(48, 59, 80)))
                    .title(format!(
                        " 🔎 Commands {}/{} — click a command or type to filter ",
                        selected + 1,
                        model.command_menu.len()
                    )),
            ),
            chunks[2],
            &mut menu_state,
        );
    }

    let hint = if model.command_menu.is_empty() {
        "↑↓ history  •  Enter send  •  Ctrl-C cancel  •  Ctrl-X quit"
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
    let composer_title = if model.preferences.vim {
        format!(
            " Composer · VIM {} ",
            model.preferences.vim_mode.to_uppercase()
        )
    } else {
        " Composer ".into()
    };
    frame.render_widget(
        Paragraph::new(input_value).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(59, 160, 255)))
                .title(composer_title),
        ),
        chunks[5],
    );
    let cursor_x = chunks[5]
        .x
        .saturating_add(
            1 + 2
                + model.input[..model.preferences.composer_cursor.min(model.input.len())]
                    .chars()
                    .count() as u16,
        )
        .min(chunks[5].right().saturating_sub(1));
    frame.set_cursor_position(Position {
        x: cursor_x,
        y: chunks[5].y.saturating_add(1),
    });
    let footer = FOOTER_ACTIONS
        .iter()
        .map(|(label, _)| *label)
        .collect::<Vec<_>>()
        .join("  ");
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::Rgb(91, 155, 213))),
        chunks[6],
    );
}

fn theme_style(theme: &str) -> Style {
    match theme {
        "light" => Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(245, 245, 245)),
        "dracula" => Style::default()
            .fg(Color::Rgb(248, 248, 242))
            .bg(Color::Rgb(40, 42, 54)),
        "nord" => Style::default()
            .fg(Color::Rgb(216, 222, 233))
            .bg(Color::Rgb(46, 52, 64)),
        "ansi" => Style::default().fg(Color::White).bg(Color::Black),
        _ => Style::default()
            .fg(Color::Rgb(226, 232, 240))
            .bg(Color::Rgb(7, 11, 18)),
    }
}

/// Estimates how many display rows `lines` will occupy after ratatui wraps
/// them to `width` columns. Operates on the *rendered* markdown [`Line`]s so
/// the estimate reflects collapsed fenced-code markers, list indentation, and
/// heading prefixes rather than the raw source strings.
fn estimated_wrapped_lines(lines: &[Line<'_>], width: usize) -> usize {
    let width = width.max(1);
    lines
        .iter()
        .map(|line| {
            let chars: usize = line
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum();
            chars.max(1).div_ceil(width)
        })
        .sum::<usize>()
        .max(1)
}

fn render_screen_reader(frame: &mut Frame<'_>, model: &ViewModel) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(area);
    let state = if model.agent_running {
        "WORKING"
    } else {
        "READY"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "Agent Vesper. Model {}. Mode {:?}. Permission {:?}. {state}.",
            superpower_value_for(model, "model").unwrap_or_else(|| "provider".into()),
            model.controls.operating_mode,
            model.controls.permission_mode,
        )),
        chunks[0],
    );
    let mut transcript = transcript_lines_for(model);
    if !model.activity.is_empty() {
        transcript.push(format!("Activity: {}", model.activity.join("; ")));
    }
    frame.render_widget(
        Paragraph::new(transcript.join("\n")).wrap(Wrap { trim: false }),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(model.status.as_deref().unwrap_or("Ready")),
        chunks[2],
    );
    frame.render_widget(Paragraph::new(format!("> {}", model.input)), chunks[3]);
    frame.set_cursor_position(Position {
        x: chunks[3]
            .x
            .saturating_add(
                2 + model.input[..model.preferences.composer_cursor.min(model.input.len())]
                    .chars()
                    .count() as u16,
            )
            .min(chunks[3].right().saturating_sub(1)),
        y: chunks[3].y,
    });
}

fn render_sidebar(frame: &mut Frame<'_>, area: ratatui::layout::Rect, model: &ViewModel) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Length(10),
            Constraint::Min(8),
            Constraint::Length(8),
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
        Line::from(format!("Permission  {:?}", model.controls.permission_mode)),
        Line::from(format!("Mode        {:?}", model.controls.operating_mode)),
        Line::from(format!("Generation  {}", model.controls.generation_profile)),
        Line::from(format!("API plan    {}", model.controls.endpoint_plan)),
        Line::from(format!("Phase       {}", model.plan.phase().label())),
        Line::from(format!("Transcript  {} lines", model.transcript.len())),
    ];
    frame.render_widget(
        Paragraph::new(session).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(236, 178, 46)))
                .title(" Session "),
        ),
        chunks[0],
    );

    let activity = if model.activity.is_empty() {
        vec![ListItem::new("Waiting for tool activity…")]
    } else {
        model
            .activity
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|line| ListItem::new(line.as_str()))
            .collect()
    };
    frame.render_widget(
        List::new(activity).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(236, 178, 46)))
                .title(" Activity "),
        ),
        chunks[1],
    );

    let tasks = if !model.panels.tasks {
        vec![ListItem::new("TODO panel hidden (/tasks)")]
    } else if model.task_plan.is_empty() {
        vec![ListItem::new("No model-authored tasks yet")]
    } else {
        model
            .task_plan
            .iter()
            .map(|task| {
                let (symbol, color) = match task.status.as_str() {
                    "completed" => ("✓", Color::Green),
                    "in_progress" => ("●", Color::Yellow),
                    _ => ("○", Color::DarkGray),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{symbol} "), Style::default().fg(color)),
                    Span::raw(task.content.as_str()),
                ]))
            })
            .collect()
    };
    let completed = model
        .task_plan
        .iter()
        .filter(|task| task.status == "completed")
        .count();
    frame.render_widget(
        List::new(tasks).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" TODO {completed}/{} ", model.task_plan.len())),
        ),
        chunks[2],
    );

    if model.last_report.is_empty() {
        let ratio = if model.task_plan.is_empty() {
            0.0
        } else {
            completed as f64 / model.task_plan.len() as f64
        };
        frame.render_widget(
            Gauge::default()
                .block(Block::default().borders(Borders::ALL).title(" Run "))
                .gauge_style(Style::default().fg(Color::Cyan))
                .ratio(ratio)
                .label(if model.agent_running {
                    "WORKING"
                } else {
                    "READY"
                }),
            chunks[3],
        );
    } else {
        frame.render_widget(
            Paragraph::new(model.last_report.join("\n"))
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(" Run report ")),
            chunks[3],
        );
    }
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
    if model.agent_running && !model.live_response.is_empty() {
        lines.push(format!("assistant (streaming): {}", model.live_response));
    }
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
    fn superpower_value_handles_missing_surface() {
        let model = ViewModel {
            plan: PlanState::default(),
            superpowers: None,
            overrides: SuperpowerOverrides::default(),
            transcript: Vec::new(),
            input: String::new(),
            status: None,
            ..ViewModel::default()
        };
        assert_eq!(superpower_value_for(&model, "model"), None);
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

    #[test]
    fn render_to_frame_renders_markdown_in_conversation_without_panicking() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let model = ViewModel {
            transcript: vec![
                "assistant: Here is **bold** and `code`".to_string(),
                "- item one".to_string(),
                "- item two".to_string(),
                "```rust\nfn main() {}\n```".to_string(),
            ],
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("draw must not panic with markdown transcript");

        // The styled text must have made it to the buffer (markdown removed
        // the literal `**` / `` ` `` markers from visible plain text).
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("bold"), "bold text should be visible");
        assert!(content.contains("item one"), "list item should be visible");
        assert!(content.contains("fn main"), "code block should be visible");
    }

    #[test]
    fn render_to_frame_survives_zero_width_resize() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let model = ViewModel {
            transcript: vec!["assistant: **bold** `code`".to_string()],
            ..ViewModel::default()
        };
        // A degenerate 1x1 area stresses the wrap path; must not panic.
        let backend = TestBackend::new(1, 1);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("degenerate resize must not panic");
    }

    #[test]
    fn render_to_frame_renders_reasoning_markdown() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let model = ViewModel {
            panels: PanelVisibility {
                reasoning: true,
                ..PanelVisibility::default()
            },
            reasoning: "Thinking about **this** step.".to_string(),
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("reasoning markdown must not panic");
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            content.contains("this"),
            "reasoning bold text should be visible"
        );
    }

    #[test]
    fn render_to_frame_renders_vertical_scrollbar_when_transcript_overflows() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // A transcript taller than the viewport must render the new Scrollbar
        // widget without panicking, and the renderer must honor an explicit
        // `conversation_manual_scroll` (PageUp/Home) without going past the
        // valid range.
        let long_lines: Vec<String> = (0..200)
            .map(|i| format!("assistant: line number {i} of a long transcript"))
            .collect();
        let model = ViewModel {
            transcript: long_lines,
            conversation_manual_scroll: Some(40),
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("scrollbar render must not panic when transcript overflows");

        // Manual scroll position 40 should be honored: line 40 should be near
        // the top of the viewport (not the bottom of the transcript).
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            content.contains("line number 40"),
            "manual scroll offset 40 must position that line in the viewport"
        );
        assert!(
            !content.contains("line number 199"),
            "the bottom of the transcript must NOT be visible under manual scroll 40"
        );
    }

    #[test]
    fn render_to_frame_auto_follows_when_manual_scroll_is_none() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // `None` (auto-follow) must keep the bottom of the transcript visible
        // even when the transcript is much taller than the viewport.
        let long_lines: Vec<String> = (0..200)
            .map(|i| format!("user: turn {i} of many"))
            .collect();
        let model = ViewModel {
            transcript: long_lines,
            conversation_manual_scroll: None,
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("auto-follow render must not panic");
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            content.contains("turn 199"),
            "auto-follow (None) must show the bottom of the transcript"
        );
    }
}
