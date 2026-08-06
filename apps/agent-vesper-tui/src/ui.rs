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
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

use crate::dispatch::{PanelVisibility, SessionControls, TaskItem, TerminalPreferences};
use crate::plan_mode::{PlanPhase, PlanState};
use crate::superpowers::{ProviderSuperpowerSurface, SuperpowerOverrides};

/// Which action button the tool-permission modal highlights.
///
/// Defaults to `Allow` (the safe, conservative pick — the user must move focus
/// to `Deny` deliberately). Mirrored into the [`ViewModel`] every frame and
/// mutated by Tab/arrow-key input while the modal is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionChoice {
    /// Red-highlighted `[ Deny ]` button (reject the one-time approval).
    Deny,
    /// Green-highlighted `[ Allow once ]` button (default focus).
    #[default]
    Allow,
}

impl PermissionChoice {
    /// Toggles between `Deny` and `Allow` (Tab / Left / Right).
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Deny => Self::Allow,
            Self::Allow => Self::Deny,
        }
    }
}

/// Host-visible tool-permission modal state. Surfaced through
/// [`ViewModel::pending_permission`] when the agent loop has emitted an
/// unresolved one-time approval request. The renderer paints a centered
/// `Clear` + bordered dialog over the conversation; the binary's event loop
/// owns the focus pointer and submits the user's decision back through
/// [`vesper_agent::PermissionRequest::approve`] / `reject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionModal {
    /// Tool awaiting approval (e.g. `run_shell_command`).
    pub tool: String,
    /// Pretty-printed JSON arguments the model supplied.
    pub arguments: String,
    /// Human-readable static-gate reason.
    pub reason: String,
    /// Currently focused action button.
    pub focus: PermissionChoice,
}

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
    /// Manual conversation scroll expressed as **lines up from the bottom**
    /// (so the input handler can mutate it without knowing `max_scroll`).
    /// `None` = auto-follow (stick to bottom, the default); `Some(n)` = the
    /// user pressed PageUp/Home and is reading history `n` lines above the
    /// newest line. The renderer mirrors this into a `ScrollbarState` whose
    /// `position = max_scroll.saturating_sub(n)` — the same value passed to
    /// `Paragraph::scroll` — so the thumb position is always truthful.
    pub conversation_manual_scroll: Option<u16>,
    /// Manual reasoning-panel scroll expressed as **lines up from the
    /// bottom**, mirroring [`Self::conversation_manual_scroll`]. `None` =
    /// auto-follow the latest thinking; `Some(n)` = `n` lines above the
    /// newest reasoning line.
    pub reasoning_manual_scroll: Option<u16>,
    /// Which scrollable panel receives PageUp/PageDown/Home/End events.
    /// `false` (default) = Conversation panel; `true` = Reasoning panel.
    /// Toggled by the user pressing Tab when the composer is empty (so Tab
    /// inside a non-empty prompt still composes a tab character).
    pub reasoning_panel_focused: bool,
    /// Pending tool-permission modal. `Some` whenever the agent loop has
    /// emitted a one-time approval request that has not been resolved; the
    /// renderer overlays a centered `Clear` + bordered dialog over the
    /// conversation. The binary's event loop intercepts Tab/arrow/Enter/Esc
    /// while this is set so the user can only choose `Deny` or `Allow once`.
    pub pending_permission: Option<PermissionModal>,
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
        // Render each top-level transcript line through markdown separately
        // so role banners can apply a distinct background style across the
        // full pane width. A `user:` prefix marks a user turn (full-width
        // dark-blue banner); every other role (assistant, streaming, plan
        // context, errors) renders against the default background. Joining
        // all entries into one document (the previous approach) collapses
        // markdown constructs across role boundaries and loses the per-turn
        // segmentation the reference layout shows.
        //
        // `inner_width` is passed in so user banner lines can be padded to
        // the next multiple of the inner width — ratatui 0.30 does NOT fill
        // trailing empty cells with the line's bg, so explicit padding is
        // required for the banner to span the full pane width.
        let rendered = render_transcript_with_role_banners(&transcript_lines, inner_width);
        let estimate = estimated_wrapped_lines(&rendered, inner_width);
        (ratatui::text::Text::from(rendered), estimate)
    };
    let paragraph = Paragraph::new(transcript).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CHROME_BORDER))
            .title(" Conversation "),
    );
    let visible_lines = usize::from(transcript_area.height.saturating_sub(2));
    let max_scroll = wrapped_lines
        .saturating_sub(visible_lines)
        .min(u16::MAX as usize) as u16;
    // `manual_scroll` is expressed as **lines up from the bottom**, so the
    // input handler can mutate it without knowing `max_scroll` (which only
    // the renderer can compute from the wrapped markdown line count).
    // `None` (auto-follow) sticks to the bottom; `Some(n)` scrolls `n` lines
    // up from the bottom, clamped to the valid range so a resize or new
    // content cannot overshoot the top of the transcript.
    let manual = model
        .conversation_manual_scroll
        .unwrap_or(0)
        .min(max_scroll);
    let effective_scroll = max_scroll.saturating_sub(manual);
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
        let reasoning_area = conversation_chunks[1];
        let title = if model.reasoning_panel_focused {
            " Reasoning (focused — Tab to switch, PgUp/PgDn/Home/End scroll) "
        } else {
            " Reasoning "
        };
        let paragraph = Paragraph::new(reasoning_lines.clone())
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Rgb(159, 122, 234)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(if model.reasoning_panel_focused {
                        Style::default().fg(Color::Rgb(159, 122, 234))
                    } else {
                        Style::default().fg(CHROME_BORDER)
                    })
                    .title(title),
            );
        let visible_lines = usize::from(reasoning_area.height.saturating_sub(2));
        let inner_width = usize::from(reasoning_area.width.saturating_sub(2));
        let wrapped_lines = estimated_wrapped_lines(&reasoning_lines, inner_width);
        let max_scroll = wrapped_lines
            .saturating_sub(visible_lines)
            .min(u16::MAX as usize) as u16;
        let manual = model.reasoning_manual_scroll.unwrap_or(0).min(max_scroll);
        let effective_scroll = max_scroll.saturating_sub(manual);
        frame.render_widget(paragraph.scroll((effective_scroll, 0)), reasoning_area);

        // Vertical scrollbar mirroring the Conversation panel's pattern.
        let mut scrollbar_state = ScrollbarState::new(wrapped_lines.min(u16::MAX as usize))
            .position(usize::from(effective_scroll))
            .viewport_content_length(visible_lines);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(Color::Rgb(159, 122, 234)))
                .track_style(Style::default().fg(Color::Rgb(60, 50, 85))),
            reasoning_area,
            &mut scrollbar_state,
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
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(CHROME_BORDER))
                        .title(format!(
                            " Live activity • {} event(s) ",
                            model.activity.len()
                        )),
                ),
            conversation_chunks[2],
        );
    }
    if working_tree_height > 0 {
        frame.render_widget(
            Paragraph::new(model.working_tree_lines.join("\n"))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(CHROME_BORDER))
                        .title(format!(
                            " Working tree • {} • F4 cycles ",
                            model.working_tree_title.as_deref().unwrap_or_default()
                        )),
                ),
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
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(CHROME_BORDER))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CHROME_BORDER))
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

    // Overlay the tool-permission modal LAST so it paints over every other
    // panel. `Clear` resets the underlying cells first so the dialog reads as
    // a true pop-up rather than a tinted in-place block.
    render_permission_modal(frame, model);
}

/// Renders the interactive tool-permission modal centered over the screen.
/// Called from [`render_to_frame`] only when
/// [`ViewModel::pending_permission`] is set.
///
/// The modal uses [`Clear`] to reset the underlying cells, then paints a
/// bordered dialog with the tool name, JSON arguments, and two action
/// buttons (`[ Deny ]` red, `[ Allow once ]` green). The currently focused
/// button is highlighted; Tab / Left / Right toggles focus; Enter submits.
///
/// Dimensions scale with the terminal: width is ~3/4 of the pane clamped to
/// `[40, 90]`, height is ~2/5 of the pane clamped to `[9, 22]`. A degenerate
/// 1×1 terminal still renders without panicking because every layout
/// constraint has a `Length` floor.
fn render_permission_modal(frame: &mut Frame<'_>, model: &ViewModel) {
    let Some(modal) = model.pending_permission.as_ref() else {
        return;
    };
    let screen = frame.area();

    // Width / height scale smoothly with the terminal but never collapse
    // below the modal's structural minimums. The clamps guard against
    // degenerate resize windows (the directive: "the modal dimensions scale
    // smoothly and do not panic on compact terminal windows").
    let width = (screen.width * 3 / 4).clamp(40, 90).min(screen.width);
    let height = (screen.height * 2 / 5).clamp(9, 22).min(screen.height);
    let x = screen.x + (screen.width.saturating_sub(width)) / 2;
    let y = screen.y + (screen.height.saturating_sub(height)) / 2;
    let modal_area = Rect {
        x,
        y,
        width,
        height,
    };

    // Erase the underlying cells first so the dialog reads as a true pop-up.
    frame.render_widget(Clear, modal_area);

    // Two stacked regions: content body (flexible) + button row (fixed).
    let body_height = height.saturating_sub(4);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(body_height.max(3)),
            Constraint::Length(3),
        ])
        .split(modal_area);

    // Body: reason → blank → tool name → blank → "Arguments:" → wrapped args.
    let mut body_lines: Vec<Line<'static>> = Vec::new();
    if !modal.reason.is_empty() {
        body_lines.push(Line::from(Span::styled(
            modal.reason.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    body_lines.push(Line::raw(""));
    body_lines.push(Line::from(vec![
        Span::styled("Tool:  ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(modal.tool.clone()),
    ]));
    body_lines.push(Line::raw(""));
    body_lines.push(Line::from(Span::styled(
        "Arguments:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    // Wrap the JSON arguments into the inner width so a long shell command
    // stays readable and never overflows the bordered dialog.
    let inner_width = usize::from(width.saturating_sub(2)).max(1);
    for wrapped in wrap_text_simple(&modal.arguments, inner_width) {
        body_lines.push(Line::from(Span::styled(
            wrapped,
            Style::default().fg(Color::Rgb(159, 214, 255)),
        )));
    }

    frame.render_widget(
        Paragraph::new(body_lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(220, 80, 80)))
                .title(Line::from(vec![Span::styled(
                    " Tool permission required ",
                    Style::default()
                        .fg(Color::Rgb(255, 230, 120))
                        .add_modifier(Modifier::BOLD),
                )])),
        ),
        chunks[0],
    );

    // Action buttons. Focused button gets a saturated background; unfocused
    // button dims. Tab / Left / Right toggles focus; Enter submits the
    // focused choice.
    let deny_focused = matches!(modal.focus, PermissionChoice::Deny);
    let allow_focused = matches!(modal.focus, PermissionChoice::Allow);
    let deny_style = if deny_focused {
        Style::default()
            .bg(Color::Rgb(180, 30, 30))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let allow_style = if allow_focused {
        Style::default()
            .bg(Color::Rgb(20, 150, 60))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let button_row = Line::from(vec![
        Span::raw("   "),
        Span::styled("[ Deny ]", deny_style),
        Span::raw("       "),
        Span::styled("[ Allow once ]", allow_style),
        Span::raw("   "),
    ]);
    frame.render_widget(
        Paragraph::new(button_row).alignment(Alignment::Center),
        chunks[1],
    );
}

/// Greedy word-boundary wrapper for modal body text. Used for the JSON
/// arguments preview so a long shell command stays readable inside the
/// bordered dialog without overflowing. Not a general-purpose wrapper —
/// only the tool-permission modal uses it.
fn wrap_text_simple(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for source_line in text.split('\n') {
        if source_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in source_line.split_whitespace() {
            let sep = if current.is_empty() { "" } else { " " };
            let candidate_len =
                current.chars().count() + sep.chars().count() + word.chars().count();
            if candidate_len > width && !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                current.push_str(sep);
                current.push_str(word);
            }
        }
        if !current.is_empty() || out.last().is_some_and(|s| s.is_empty()) {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
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

/// Background style applied to user-turn banner lines. The reference Native
/// Subtle slate color used for the rounded application chrome (composer,
/// conversation, sidebar panels). Tying the whole UI to one border color
/// gives the polished, cohesive look the Native GLM reference layout shows.
const CHROME_BORDER: Color = Color::Rgb(60, 70, 85);

/// Background color for the user chat bubble (oracle: `#12314b`). The user
/// bubble is right-shifted via an 8-cell left indent so it reads as the
/// "outgoing" message in the iMessage-style asymmetric layout.
const USER_BUBBLE_BG: Color = Color::Rgb(18, 49, 75);

/// Background color for the agent chat bubble (oracle: `#171d26`). The
/// agent bubble is left-shifted via an 8-cell right indent so it reads as
/// the "incoming" message on the left side of the pane.
const AGENT_BUBBLE_BG: Color = Color::Rgb(23, 29, 38);

/// Left indent (empty cells with no bubble bg) for the user bubble. Matches
/// the oracle's `margin: 1 1 0 8` (left=8) so the user message sits on the
/// RIGHT side of the pane like an outgoing chat bubble.
const USER_BUBBLE_INDENT_LEFT: usize = 8;
/// Right indent for the user bubble (oracle: margin right=1).
const USER_BUBBLE_INDENT_RIGHT: usize = 1;
/// Left indent for the agent bubble (oracle: margin left=1).
const AGENT_BUBBLE_INDENT_LEFT: usize = 1;
/// Right indent for the agent bubble. Matches the oracle's
/// `margin: 1 8 0 1` (right=8) so the agent message sits on the LEFT side
/// of the pane like an incoming chat bubble.
const AGENT_BUBBLE_INDENT_RIGHT: usize = 8;

/// Wraps a markdown-rendered [`Line`] in an asymmetric chat bubble.
///
/// `indent_left` and `indent_right` are empty-cell margins on each side
/// (rendered with default background, NO bubble bg). The bubble's content
/// spans `inner_width - indent_left - indent_right` cells with `bubble_bg`
/// as the background. Each content span retains its original foreground /
/// modifier (bold color, code highlight, etc.) — the bubble bg layers
/// *underneath* via `.patch()`. Trailing cells inside the bubble are padded
/// with bg-styled spaces so the bubble fills its full width and reads as a
/// solid block rather than just highlighting the typed characters.
fn wrap_in_bubble(
    mut line: Line<'static>,
    indent_left: usize,
    indent_right: usize,
    bubble_bg: Color,
    inner_width: usize,
) -> Line<'static> {
    let bubble_width = inner_width
        .saturating_sub(indent_left + indent_right)
        .max(1);
    let bubble_style = Style::default().bg(bubble_bg);
    let mut new_spans: Vec<Span<'static>> = Vec::new();
    if indent_left > 0 {
        new_spans.push(Span::raw(" ".repeat(indent_left)));
    }
    let mut content_width: usize = 0;
    for span in line.spans.drain(..) {
        content_width += span.content.chars().count();
        new_spans.push(Span::styled(span.content, span.style.patch(bubble_style)));
    }
    if content_width < bubble_width {
        let pad = " ".repeat(bubble_width - content_width);
        new_spans.push(Span::styled(pad, bubble_style));
    }
    if indent_right > 0 {
        new_spans.push(Span::raw(" ".repeat(indent_right)));
    }
    Line::from(new_spans)
}

/// Renders each transcript entry through markdown, then wraps user and
/// assistant turns in **asymmetric chat bubbles** matching the frozen
/// Python oracle's iMessage-style layout.
///
/// **Oracle design (frozen `glm_acp/tui.py:2199-2201`, pinned bf4d428):**
/// ```text
/// .user-message  { margin: 1 1 0 8; padding: 1; background: #12314b; }
/// .agent-message { margin: 1 8 0 1; padding: 1; background: #171d26; }
/// ```
/// - **User bubble**: 8-cell left indent → message sits on the RIGHT side,
///   dark-blue bg `Rgb(18, 49, 75)`.
/// - **Agent bubble**: 8-cell right indent → message sits on the LEFT side,
///   dark-gray bg `Rgb(23, 29, 38)`.
/// - The **asymmetry alone separates turns** — no horizontal dividers
///   needed (the oracle uses none).
///
/// System messages, plan-mode context, and errors render without a bubble
/// (matching the oracle's `.system-message { color: $text-muted; }`).
fn render_transcript_with_role_banners(
    transcript_lines: &[String],
    inner_width: usize,
) -> Vec<Line<'static>> {
    let inner_width = inner_width.max(1);
    let mut rendered: Vec<Line<'static>> = Vec::new();
    for (idx, raw) in transcript_lines.iter().enumerate() {
        let is_user_turn = raw.starts_with("user:");
        let is_assistant_turn = raw.starts_with("assistant");

        // One blank line between turns for vertical pacing. The asymmetric
        // bubble indents (user right, agent left) provide the visual turn
        // separation — no horizontal `─` dividers (the oracle uses none).
        if idx > 0 {
            rendered.push(Line::raw(""));
        }

        // Strip the role prefix from the content so the bubble shows ONLY
        // the message text. The asymmetric indent already communicates the
        // role (user on the right, agent on the left) — showing "user:" or
        // "assistant:" inside the bubble would be redundant and ugly.
        // For user turns we prepend a bold "You" label matching the oracle
        // (`Static(f"{label}\n{text}")`). Agent turns render just the
        // markdown content with no label (matching the oracle's bare
        // `SelectableStatic` updated with `RichMarkdown(text)`).
        let (content, prepend_you_label) = if is_user_turn {
            let text = raw
                .strip_prefix("user: ")
                .or_else(|| raw.strip_prefix("user:"))
                .unwrap_or(raw);
            (text, true)
        } else if is_assistant_turn {
            // Handles both "assistant: ..." and "assistant (streaming): ..."
            let text = raw.find(": ").map(|i| &raw[i + 2..]).unwrap_or(raw);
            (text, false)
        } else {
            (raw.as_str(), false)
        };

        let mut lines = crate::markdown::render_markdown(content);
        if prepend_you_label {
            // Bold blue "You" label as the first line of the user bubble,
            // matching the oracle's `Static(f"{label}\n{text}")`.
            lines.insert(
                0,
                Line::from(Span::styled(
                    "You",
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .fg(Color::Rgb(130, 170, 255)),
                )),
            );
        }

        if is_user_turn {
            // User bubble: right-shifted (8-cell left indent), dark-blue bg.
            for line in lines {
                rendered.push(wrap_in_bubble(
                    line,
                    USER_BUBBLE_INDENT_LEFT,
                    USER_BUBBLE_INDENT_RIGHT,
                    USER_BUBBLE_BG,
                    inner_width,
                ));
            }
        } else if is_assistant_turn {
            // Agent bubble: left-shifted (8-cell right indent), dark-gray bg.
            for line in lines {
                rendered.push(wrap_in_bubble(
                    line,
                    AGENT_BUBBLE_INDENT_LEFT,
                    AGENT_BUBBLE_INDENT_RIGHT,
                    AGENT_BUBBLE_BG,
                    inner_width,
                ));
            }
        } else {
            // System / plan context / errors: no bubble, default styling.
            rendered.extend(lines);
        }
    }
    rendered
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CHROME_BORDER))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CHROME_BORDER))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CHROME_BORDER))
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
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(CHROME_BORDER))
                        .title(" Run "),
                )
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
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(CHROME_BORDER))
                        .title(" Run report "),
                ),
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
            // Disable the reasoning panel so the conversation panel gets the
            // full body height — the bubble + blank-line layout needs more
            // vertical room than the old flat layout did.
            panels: PanelVisibility {
                reasoning: false,
                ..PanelVisibility::default()
            },
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
            // 50 lines up from the bottom: the bottom-most line must NOT be
            // visible (we scrolled past it), but a line near the bottom of
            // the visible window should be present.
            conversation_manual_scroll: Some(50),
            // Disable the reasoning panel so the conversation panel gets the
            // full body height.
            panels: PanelVisibility {
                reasoning: false,
                ..PanelVisibility::default()
            },
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("scrollbar render must not panic when transcript overflows");

        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            !content.contains("line number 199"),
            "scrolling 50 lines up from the bottom must NOT show the newest line"
        );
        // The scrollbar renders without panic (proven by reaching this point).
        // With the new bubble + blank-line layout, each entry takes ~2
        // rendered lines (blank separator + bubble content), so 200 entries
        // produce ~400 rendered lines. Scroll 50 from bottom on a ~15-row
        // viewport shows entries roughly in the 165-185 range. Assert that
        // SOME mid-to-late line is visible to confirm the scroll math works.
        let any_visible = (165..=185).any(|i| content.contains(&format!("line number {i}")));
        assert!(
            any_visible,
            "manual scroll 50-from-bottom should keep some mid-to-late lines visible (checked 165-185)"
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

    #[test]
    fn render_to_frame_clamps_manual_scroll_to_max_scroll() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // Home sets `manual_scroll = Some(u16::MAX)` (the "as far up as
        // possible" sentinel). The renderer must clamp it to `max_scroll`,
        // positioning the TOP of the transcript in the viewport (line 0
        // visible, line 199 NOT visible). This guards against the original
        // bug where the input handler subtracted from u16::MAX and produced
        // a value that overflowed back to the bottom.
        let long_lines: Vec<String> = (0..200).map(|i| format!("assistant: line {i}")).collect();
        let model = ViewModel {
            transcript: long_lines,
            conversation_manual_scroll: Some(u16::MAX),
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("Home sentinel render must not panic");
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            content.contains("line 0"),
            "Home (Some(u16::MAX)) must clamp to max_scroll and show the top"
        );
        assert!(
            !content.contains("line 199"),
            "Home must NOT show the bottom of the transcript"
        );
    }

    #[test]
    fn user_turns_render_as_right_shifted_chat_bubble() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // The oracle (frozen glm_acp/tui.py:2199) uses iMessage-style
        // asymmetric chat bubbles:
        //   .user-message  { margin: 1 1 0 8; background: #12314b; }
        //   .agent-message { margin: 1 8 0 1; background: #171d26; }
        // User messages are right-shifted (8-cell left indent) with a
        // dark-blue bubble bg. The "user:" prefix is stripped and replaced
        // by a bold "You" label on the first line. This test verifies the
        // bubble math: indent on the left, fill on the right, correct bg
        // color, and no redundant "user:" prefix text.
        let model = ViewModel {
            transcript: vec![
                "assistant: hello there".to_string(),
                "user: please help".to_string(),
            ],
            ..ViewModel::default()
        };

        let width_u16 = 40u16;
        let height_u16 = 20u16;
        let backend = TestBackend::new(width_u16, height_u16);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("bubble render must not panic");

        let buffer = terminal.backend().buffer();
        let user_bubble_bg = Color::Rgb(18, 49, 75); // USER_BUBBLE_BG
        let width = usize::from(width_u16);
        let height = usize::from(height_u16);

        let content: String = buffer.content.iter().map(|c| c.symbol()).collect();

        // The "user:" prefix must be STRIPPED — not visible inside the bubble.
        assert!(
            !content.contains("user: please"),
            "the raw 'user:' prefix must be stripped from the bubble content"
        );

        // The "You" label must be present (oracle: Static(f\"{label}\\n{text}\")).
        assert!(
            content.contains("You"),
            "the bold 'You' label must appear as the first line of the user bubble"
        );

        // The message text must be present inside the bubble.
        assert!(
            content.contains("please"),
            "the message text 'please' must be visible inside the user bubble"
        );

        // Find a row carrying the user bubble bg (the "please help" content row).
        // Skip the "You" label row (which also has the bg) and find the text row.
        let mut bubble_row: Option<usize> = None;
        for y in 0..height {
            for x in 0..width {
                let cell = &buffer.content[y * width + x];
                if cell.symbol() == "p" && cell.bg == user_bubble_bg {
                    bubble_row = Some(y);
                    break;
                }
            }
            if bubble_row.is_some() {
                break;
            }
        }
        let bubble_row =
            bubble_row.expect("a row with 'p' carrying the user bubble bg must be visible");

        // The leftmost cells (x < 9 = 1 border + 8 indent) must NOT carry
        // the bubble bg — this proves the user bubble is RIGHT-SHIFTED,
        // not full-width.
        for x in 1..9 {
            let cell = &buffer.content[bubble_row * width + x];
            assert_ne!(
                cell.bg, user_bubble_bg,
                "cell at x={x} (left indent zone) must NOT carry the bubble bg — \
                 the user bubble is right-shifted, not full-width"
            );
        }

        // The rightmost bubble-bg cell must reach close to the right edge
        // (x >= 36), proving the bubble fills its allocated width via
        // trailing-space padding. The bubble ends at inner_width - 1 (right
        // indent = 1), so rightmost bg is at x ~ 37-38.
        let mut rightmost_bubble_x: Option<usize> = None;
        for x in (0..width).rev() {
            if buffer.content[bubble_row * width + x].bg == user_bubble_bg {
                rightmost_bubble_x = Some(x);
                break;
            }
        }
        let rightmost_bubble_x = rightmost_bubble_x
            .expect("at least one bubble-bg cell must exist on the user content row");
        assert!(
            rightmost_bubble_x >= 36,
            "rightmost bubble-bg cell is at x={rightmost_bubble_x}; must be >= 36 \
             to prove the bubble fills its allocated width to the right edge"
        );
    }

    #[test]
    fn render_permission_modal_overlays_centered_dialog_without_panicking() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let model = ViewModel {
            transcript: vec!["assistant: working…".to_string()],
            pending_permission: Some(PermissionModal {
                tool: "run_shell_command".to_string(),
                arguments: r#"{"command":"ls -la"}"#.to_string(),
                reason: "Shell tool requires one-time approval".to_string(),
                focus: PermissionChoice::Allow,
            }),
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("modal render must not panic");

        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            content.contains("Tool permission required"),
            "modal title must be visible"
        );
        assert!(
            content.contains("run_shell_command"),
            "modal must show the tool name"
        );
        assert!(
            content.contains("Deny") && content.contains("Allow once"),
            "both action buttons must be visible"
        );
    }

    #[test]
    fn render_permission_modal_survives_compact_terminal_resize() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // A degenerate 1x1 / 4x4 area stresses the modal layout math; the
        // clamps (width ≥ 9, height ≥ 9, both `.min(screen)`) must keep the
        // Rect inside the screen and prevent any panic from `Layout::split`.
        for (w, h) in [(1u16, 1u16), (4, 4), (10, 10), (40, 12), (200, 60)] {
            let model = ViewModel {
                pending_permission: Some(PermissionModal {
                    tool: "x".to_string(),
                    arguments: "{}".to_string(),
                    reason: "r".to_string(),
                    focus: PermissionChoice::Deny,
                }),
                ..ViewModel::default()
            };
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).expect("test backend");
            terminal
                .draw(|f| render_to_frame(f, &model))
                .unwrap_or_else(|e| panic!("modal must not panic at {w}x{h}: {e}"));
        }
    }

    #[test]
    fn permission_choice_toggle_round_trips() {
        // Tab / Left / Right toggle focus. Default is Allow (conservative);
        // a single toggle moves to Deny; a second returns to Allow.
        assert_eq!(PermissionChoice::Allow.toggle(), PermissionChoice::Deny);
        assert_eq!(PermissionChoice::Deny.toggle(), PermissionChoice::Allow);
        assert_eq!(
            PermissionChoice::default(),
            PermissionChoice::Allow,
            "default focus must be Allow so the user must deliberately move to Deny"
        );
    }

    #[test]
    fn wrap_text_simple_respects_width_without_dropping_words() {
        let wrapped = wrap_text_simple("alpha beta gamma delta", 10);
        // Each output line must fit within the requested width.
        for line in &wrapped {
            assert!(
                line.chars().count() <= 10,
                "line `{line}` exceeds the requested width"
            );
        }
        // No word may be dropped.
        let joined = wrapped.join(" ");
        for word in ["alpha", "beta", "gamma", "delta"] {
            assert!(joined.contains(word), "word `{word}` was dropped");
        }
        assert!(
            !wrapped.is_empty(),
            "wrapper must produce at least one line"
        );
    }
}
