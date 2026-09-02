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
    layout::{Alignment, Constraint, Direction, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
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

/// VRO-8 (PRD §8.1) — diagnostic projection surfaced at the top of the
/// Reasoning Panel. Computed by the binary from the active
/// [`vesper_domain::TaskProfile`] + effective
/// [`vesper_domain::ReasoningMode`] + [`vesper_domain::ReasoningBudget`].
/// `None` on [`ViewModel`] when VRO is disabled or no turn has run yet.
///
/// The fields are pre-formatted **labels** (snake/kebab-case strings) rather
/// than typed enums so the renderer never has to import domain strategy /
/// risk enums (keeping `ui.rs` decoupled from VRO internals). All numeric
/// budget fields are the PRD §10.4 wire types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReasoningDiagnostics {
    /// Snake_case strategy label (e.g. `"bounded_tree_search"`).
    pub strategy: String,
    /// Kebab-case mode label (e.g. `"auto"`, `"deep"`).
    pub mode: String,
    /// `true` when the user has forced this mode via `/reasoning set mode=…`
    /// (vs the profiler auto-selecting it). Surfaced as `*(override)*` in
    /// the panel header.
    pub override_active: bool,
    /// Lowercase risk label (`"low"` / `"medium"` / `"high"`).
    pub risk: String,
    /// Prominent risk-escalation flag — only `true` when the TaskProfiler
    /// escalated the task to `RiskLevel::High`. The renderer surfaces a
    /// **⚠ RISK ESCALATION** warning next to the strategy line.
    pub risk_escalation: bool,
    /// Maximum search-tree depth for `BoundedTreeSearch` (PRD §10.4).
    pub max_search_depth: u16,
    /// Maximum parallel candidate/search branches (PRD §10.4).
    pub max_parallel_branches: u16,
    /// Maximum provider model calls across the turn (PRD §10.4).
    pub max_model_calls: u32,
    /// Maximum verification→repair cycles (PRD §10.4).
    pub max_repairs: u16,
    /// VRO-10 (PRD §8.2 "Status Surface") — the active phase label. The
    /// orchestrator's phases (Understanding / Inspecting context / Building
    /// plan / Exploring alternatives / Running tools / Validating result /
    /// Repairing failed checks / Finalizing answer) are streamed through
    /// this field so the Reasoning Panel renders **`Phase:` <label>** as
    /// the turn progresses, rather than just the static strategy header.
    /// Empty string hides the phase line (the default for `Direct` turns
    /// where no orchestration phase applies).
    pub phase: String,
}

impl ReasoningDiagnostics {
    /// Renders the diagnostic header as a single markdown line. The
    /// renderer prepends this (followed by a horizontal rule) to the
    /// streamed reasoning text so the strategy decision is visible at the
    /// top of the panel.
    ///
    /// Pure: takes `&self`, returns a `String`. Tested in
    /// [`super::tests::reasoning_diagnostics_header_includes_strategy_and_budget`]
    /// and the `risk_escalation` variants.
    ///
    /// VRO-10 §8.2: when `phase` is non-empty the header prepends a
    /// **`Phase:` `<label>`** segment so the panel surfaces the live
    /// orchestrator phase rather than only the static strategy.
    #[must_use]
    pub fn render_header(&self) -> String {
        let mut out = String::new();
        // VRO-10 §8.2: the live phase comes first so the driver sees the
        // orchestrator's current activity at a glance.
        if !self.phase.is_empty() {
            out.push_str(&format!("**Phase:** `{}`", self.phase));
            out.push_str(" | ");
        }
        out.push_str(&format!("**Strategy:** `{}`", self.strategy));
        out.push_str(&format!(" | **Mode:** `{}`", self.mode));
        if self.override_active {
            out.push_str(" *(override)*");
        }
        out.push_str(&format!(" | **Risk:** `{}`", self.risk));
        if self.risk_escalation {
            out.push_str(" **⚠ RISK ESCALATION**");
        }
        out.push_str(&format!(
            " | Depth: {} | Branches: {} | Models: {} | Repairs: {}",
            self.max_search_depth,
            self.max_parallel_branches,
            self.max_model_calls,
            self.max_repairs
        ));
        out
    }

    /// Renders the diagnostics as ONE compact inline label for the
    /// VRO-11.5 thinking block that streams inside the Conversation panel
    /// (`🧠 Thinking · strategy · mode · risk`). This replaces the deleted
    /// bottom Reasoning panel's full markdown header: inline thinking needs
    /// a single quiet line, not a budget table. Budget numbers stay
    /// available through [`Self::render_header`] for hosts that want them.
    #[must_use]
    pub fn render_inline_header(&self) -> String {
        let mut out = String::from("🧠 Thinking");
        if !self.phase.is_empty() {
            out.push_str(&format!(" · {}", self.phase));
        }
        out.push_str(&format!(" · {} · {}", self.strategy, self.mode));
        if self.override_active {
            out.push_str(" (override)");
        }
        out.push_str(&format!(" · risk: {}", self.risk));
        if self.risk_escalation {
            out.push_str(" · ⚠ RISK ESCALATION");
        }
        out
    }
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
    /// Pending image attachment labels rendered before the editable text.
    /// The binary owns the image bytes; the renderer receives labels only.
    pub composer_attachments: Vec<String>,
    /// One-line status / error / notice.
    pub status: Option<String>,
    /// Slash-command palette entries matching the current input.
    pub command_menu: Vec<(String, String)>,
    /// Highlighted command-palette entry.
    pub command_menu_selected: usize,
    /// Whether an agent turn is currently running.
    pub agent_running: bool,
    /// Number of follow-up prompts retained in the native FIFO.
    pub queued_prompt_count: usize,
    /// Typed live controls governing real agent turns.
    pub controls: SessionControls,
    /// Native dashboard panel visibility.
    pub panels: PanelVisibility,
    /// Latest model-authored TODO plan.
    pub task_plan: Vec<TaskItem>,
    /// Bounded live provider/tool activity.
    pub activity: Vec<String>,
    /// VRO-11.4: inline tool telemetry rendered DIRECTLY in the main
    /// Conversation panel. Populated from the direct path's
    /// ToolStarted/ToolFinished events and the ReAct trajectory stream.
    pub live_trajectory: Vec<String>,
    /// Whether the conversation canvas is showing the full tool transcript.
    /// The default chat view keeps this telemetry collapsed into one summary.
    pub show_tool_details: bool,
    /// Provider-visible reasoning streamed during the current turn.
    pub reasoning: String,
    /// VRO-8 (PRD §8.1) — diagnostic projection rendered as a header at the
    /// top of the Reasoning Panel. `None` (the default) hides the header.
    /// The binary computes this from the active TaskProfile + effective
    /// ReasoningMode + ReasoningBudget before each VRO turn.
    pub reasoning_diagnostics: Option<ReasoningDiagnostics>,
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
///
/// Rendering and `handle_mouse_click` both walk this const in order with
/// computed x-offsets, so the two views cannot drift — but neither may
/// filter the list independently (that WOULD desync click targets).
/// `toggle_chat_only` (F11) stays always-visible because it is also the
/// restore affordance from chat-only mode.
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
    ("Ctrl+T Activity", "toggle_tool_details"),
    ("F6 Sessions", "open_history"),
    ("^y Copy response", "copy_last_response"),
    ("F11 Chat only", "toggle_chat_only"),
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
    let palette = theme_palette(&model.preferences.theme);
    frame.render_widget(Block::default().style(palette.base()), area);
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
        Span::styled(
            format!("• {model_name} "),
            Style::default().fg(palette.accent),
        ),
        Span::styled(format!("• {} ", phase.label()), phase_style),
        Span::styled(format!("• {state}"), Style::default().fg(palette.muted)),
    ]);
    frame.render_widget(
        Paragraph::new(header).style(Style::default().fg(palette.text).bg(palette.surface)),
        chunks[0],
    );

    let show_sidebar = model.panels.sidebar_visible() && chunks[1].width >= 110;
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
    // VRO-11.5: the bottom Reasoning panel and the Activity strip are gone —
    // the stream of provider thinking renders INLINE in the Conversation
    // panel (see `transcript_lines_for`) and the Conversation column takes
    // the full body height, matching Claude Code / Codex single-column
    // agent CLIs.
    let conversation_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(working_tree_height)])
        .split(body[0]);

    // Conversation — prepended with Plan Mode context (pending questions
    // while PLANNING, the plan body while REVIEW), then the live transcript.
    let transcript_lines = transcript_lines_for(model);
    let transcript_area = conversation_chunks[0];
    let transcript_content = transcript_area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let inner_width = usize::from(transcript_content.width);
    // Render the transcript to markdown Lines once, then estimate the
    // wrapped-line count from the *rendered* output. Estimating from the raw
    // strings would over-count (markdown collapses fenced code markers) and
    // over-scroll the first line out of view.
    let (transcript, wrapped_lines) = if transcript_lines.is_empty() {
        let ready = Line::from(vec![
            Span::styled(
                "Agent Vesper",
                Style::default()
                    .fg(palette.accent)
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
        // Render each top-level entry separately so markdown cannot bleed
        // across role/tool boundaries and user prompts can receive the quiet
        // terminal-native `›` marker.
        let rendered = render_transcript_lines_themed(&transcript_lines, inner_width, palette);
        let estimate = estimated_wrapped_lines(&rendered, inner_width);
        (ratatui::text::Text::from(rendered), estimate)
    };
    let paragraph = Paragraph::new(transcript).wrap(Wrap { trim: false });
    let visible_lines = usize::from(transcript_content.height);
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
    frame.render_widget(paragraph.scroll((effective_scroll, 0)), transcript_content);

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
            .thumb_style(Style::default().fg(palette.muted))
            .track_style(Style::default().fg(palette.border)),
        transcript_content,
        &mut scrollbar_state,
    );

    if working_tree_height > 0 {
        frame.render_widget(
            Paragraph::new(model.working_tree_lines.join("\n"))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(palette.border))
                        .title(format!(
                            " Working tree • {} • F4 cycles ",
                            model.working_tree_title.as_deref().unwrap_or_default()
                        )),
                ),
            conversation_chunks[1],
        );
    }

    if show_sidebar {
        render_sidebar(frame, body[1], model, palette);
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
                    Style::default()
                        .fg(palette.selected_text)
                        .bg(palette.selection)
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
                                palette.selected_text
                            } else {
                                palette.accent
                            })
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        format!("  {description}"),
                        row_style.fg(if is_selected {
                            palette.selected_text
                        } else {
                            palette.muted
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
                    .border_style(Style::default().fg(palette.border))
                    .style(Style::default().fg(palette.text).bg(palette.raised))
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
        Paragraph::new(hint).style(Style::default().fg(palette.muted)),
        chunks[3],
    );

    let activity = if model.agent_running {
        let notice = model.status.as_deref().unwrap_or("Working");
        if model.queued_prompt_count > 0 {
            format!(
                "● {notice} · {} queued · Enter steers · Tab queues",
                model.queued_prompt_count
            )
        } else {
            format!("● {notice} · Enter steers · Tab queues")
        }
    } else {
        model.status.clone().unwrap_or_else(|| "○ Ready".into())
    };
    frame.render_widget(
        Paragraph::new(activity).style(Style::default().fg(if model.agent_running {
            palette.warning
        } else {
            palette.muted
        })),
        chunks[4],
    );

    // Composer. Keep a visible insertion point: the old renderer hid the
    // terminal cursor, which made the input look like a static mockup.
    let mut composer_spans = vec![Span::styled(
        "› ",
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    )];
    for attachment in &model.composer_attachments {
        composer_spans.push(Span::styled(
            format!("{attachment} "),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if model.input.is_empty() && model.composer_attachments.is_empty() {
        composer_spans.push(Span::styled(
            "Type a prompt or / for commands",
            Style::default().fg(palette.muted),
        ));
    } else {
        composer_spans.push(Span::raw(model.input.clone()));
    }
    let composer_title = if model.preferences.vim {
        format!(
            " Composer · VIM {} ",
            model.preferences.vim_mode.to_uppercase()
        )
    } else {
        " Message ".into()
    };
    frame.render_widget(
        Paragraph::new(Line::from(composer_spans)).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(palette.border))
                .title(composer_title),
        ),
        chunks[5],
    );
    let cursor_x = chunks[5]
        .x
        .saturating_add(
            2 + model
                .composer_attachments
                .iter()
                .map(|label| label.chars().count() as u16 + 1)
                .sum::<u16>()
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
        Paragraph::new(footer).style(Style::default().fg(palette.accent)),
        chunks[6],
    );

    // Overlay the tool-permission modal LAST so it paints over every other
    // panel. `Clear` resets the underlying cells first so the dialog reads as
    // a true pop-up rather than a tinted in-place block.
    render_permission_modal(frame, model, palette);
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
fn render_permission_modal(frame: &mut Frame<'_>, model: &ViewModel, palette: ThemePalette) {
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
            Style::default().fg(palette.warning),
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
            Style::default().fg(palette.accent),
        )));
    }

    frame.render_widget(
        Paragraph::new(body_lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(220, 80, 80)))
                .style(Style::default().fg(palette.text).bg(palette.raised))
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

#[derive(Clone, Copy)]
struct ThemePalette {
    background: Color,
    surface: Color,
    raised: Color,
    text: Color,
    muted: Color,
    accent: Color,
    border: Color,
    selection: Color,
    selected_text: Color,
    warning: Color,
}

impl ThemePalette {
    fn base(self) -> Style {
        Style::default().fg(self.text).bg(self.background)
    }
}

fn theme_palette(theme: &str) -> ThemePalette {
    match theme {
        "chatgpt-white" => ThemePalette {
            background: Color::Rgb(255, 255, 255),
            surface: Color::Rgb(247, 247, 248),
            raised: Color::Rgb(238, 238, 240),
            text: Color::Rgb(32, 33, 35),
            muted: Color::Rgb(102, 102, 110),
            accent: Color::Rgb(16, 163, 127),
            border: Color::Rgb(210, 210, 215),
            selection: Color::Rgb(208, 238, 230),
            selected_text: Color::Rgb(20, 60, 50),
            warning: Color::Rgb(170, 105, 0),
        },
        "light" => ThemePalette {
            background: Color::Rgb(245, 245, 245),
            surface: Color::Rgb(232, 232, 232),
            raised: Color::Rgb(220, 220, 220),
            text: Color::Black,
            muted: Color::DarkGray,
            accent: Color::Rgb(0, 105, 85),
            border: Color::Rgb(170, 170, 170),
            selection: Color::Rgb(190, 220, 215),
            selected_text: Color::Black,
            warning: Color::Rgb(150, 90, 0),
        },
        "dracula" => ThemePalette {
            background: Color::Rgb(40, 42, 54),
            surface: Color::Rgb(33, 34, 44),
            raised: Color::Rgb(68, 71, 90),
            text: Color::Rgb(248, 248, 242),
            muted: Color::Rgb(98, 114, 164),
            accent: Color::Rgb(80, 250, 123),
            border: Color::Rgb(98, 114, 164),
            selection: Color::Rgb(68, 71, 90),
            selected_text: Color::Rgb(248, 248, 242),
            warning: Color::Rgb(241, 250, 140),
        },
        "nord" => ThemePalette {
            background: Color::Rgb(46, 52, 64),
            surface: Color::Rgb(59, 66, 82),
            raised: Color::Rgb(67, 76, 94),
            text: Color::Rgb(216, 222, 233),
            muted: Color::Rgb(129, 161, 193),
            accent: Color::Rgb(136, 192, 208),
            border: Color::Rgb(76, 86, 106),
            selection: Color::Rgb(67, 76, 94),
            selected_text: Color::Rgb(236, 239, 244),
            warning: Color::Rgb(235, 203, 139),
        },
        "ansi" => ThemePalette {
            background: Color::Black,
            surface: Color::Black,
            raised: Color::Black,
            text: Color::White,
            muted: Color::DarkGray,
            accent: Color::Green,
            border: Color::DarkGray,
            selection: Color::White,
            selected_text: Color::Black,
            warning: Color::Yellow,
        },
        // `vesper` remains a compatibility alias for saved preferences, but
        // the former blue/slate palette is intentionally retired.
        _ => ThemePalette {
            background: Color::Rgb(0, 0, 0),
            surface: Color::Rgb(10, 10, 10),
            raised: Color::Rgb(24, 24, 24),
            text: Color::Rgb(236, 236, 236),
            muted: Color::Rgb(142, 142, 142),
            accent: Color::Rgb(16, 163, 127),
            border: Color::Rgb(58, 58, 58),
            selection: Color::Rgb(32, 73, 62),
            selected_text: Color::White,
            warning: Color::Rgb(236, 190, 80),
        },
    }
}

/// Renders the transcript as a quiet terminal-native feed: user prompts carry
/// a cyan `›` marker, assistant markdown is unboxed, and thinking/tool output
/// stays visually secondary. This follows Codex/Claude terminal hierarchy
/// without chat bubbles or full-width role banners.
fn render_transcript_lines(transcript_lines: &[String], _inner_width: usize) -> Vec<Line<'static>> {
    render_transcript_lines_themed(
        transcript_lines,
        _inner_width,
        theme_palette("chatgpt-black"),
    )
}

fn render_transcript_lines_themed(
    transcript_lines: &[String],
    _inner_width: usize,
    palette: ThemePalette,
) -> Vec<Line<'static>> {
    let mut rendered: Vec<Line<'static>> = Vec::new();
    let mut previous_was_secondary = false;
    for (idx, raw) in transcript_lines.iter().enumerate() {
        let is_user_turn = raw.starts_with("user:");
        let is_assistant_turn = raw.starts_with("assistant");
        // VRO-11.5/11.6 live-region markers: `⏺` / indented `⎿` entries are
        // tool telemetry (dim) and bare `http(s)://` lines are
        // clickable URLs (cyan + underlined, own line). Both read as
        // secondary text between user/assistant turns — the same
        // visual hierarchy Claude Code and Codex use in their single
        // conversation feed.
        let is_thinking = raw.starts_with("thinking:");
        let is_activity = raw.starts_with("activity:");
        let is_commentary = raw.starts_with("commentary:");
        let is_telemetry = raw.starts_with("⏺") || raw.trim_start_matches(' ').starts_with("⎿");
        let is_url_line =
            (raw.starts_with("http://") || raw.starts_with("https://")) && !raw.contains(' ');

        // Keep human turns comfortably separated, but render consecutive
        // thinking/tool events as one compact activity group. Treating every
        // action/result as a chat turn doubled the feed height and buried the
        // actual answer beneath empty rows.
        let is_secondary =
            is_thinking || is_activity || is_commentary || is_telemetry || is_url_line;
        if idx > 0 && !(is_secondary && previous_was_secondary) {
            rendered.push(Line::raw(""));
        }

        // Strip internal role prefixes; the visible marker and text hierarchy
        // communicate the role without leaking transport labels.
        let content = if is_user_turn {
            raw.strip_prefix("user: ")
                .or_else(|| raw.strip_prefix("user:"))
                .unwrap_or(raw)
        } else if is_assistant_turn {
            // Handles both "assistant: ..." and "assistant (streaming): ..."
            raw.find(": ").map(|i| &raw[i + 2..]).unwrap_or(raw)
        } else if is_thinking {
            raw.strip_prefix("thinking: ")
                .or_else(|| raw.strip_prefix("thinking:"))
                .unwrap_or(raw)
        } else if is_activity {
            raw.strip_prefix("activity: ").unwrap_or(raw)
        } else if is_commentary {
            raw.strip_prefix("commentary: ").unwrap_or(raw)
        } else if is_telemetry {
            // Keep the ⏺ / indented ⎿ glyphs verbatim — they read as
            // Claude Code's quiet action/result markers.
            raw.as_str()
        } else {
            raw.as_str()
        };

        let lines = crate::markdown::render_markdown(content);

        if is_user_turn {
            for (line_index, mut line) in lines.into_iter().enumerate() {
                let marker = if line_index == 0 { "› " } else { "  " };
                line.spans.insert(
                    0,
                    Span::styled(
                        marker,
                        Style::default()
                            .fg(palette.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                );
                rendered.push(line);
            }
        } else if is_assistant_turn {
            rendered.extend(lines);
        } else if is_thinking || is_commentary {
            // VRO-11.5: live thinking streams as dim italic secondary text —
            // visually distinct from the final conversational answer without
            // stealing attention from it (the `🧠` header line included).
            let style = Style::default()
                .fg(palette.muted)
                .add_modifier(Modifier::ITALIC);
            for line in lines {
                rendered.push(restyle_line(line, style));
            }
        } else if is_activity {
            let style = Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD);
            for line in lines {
                rendered.push(restyle_line(line, style));
            }
        } else if is_url_line {
            // VRO-11.6: a bare URL renders as an obvious link — cyan +
            // underlined, at normal brightness — and, critically, on its own
            // unwrapped line so terminal auto-linkification works. Ctrl+O is
            // the guaranteed opener when the terminal refuses.
            let style = Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::UNDERLINED);
            for line in lines {
                rendered.push(restyle_line(line, style));
            }
        } else if is_telemetry {
            // VRO-11.6: tool telemetry (⏺ action / ⎿ result) renders dim
            // (no italics) so the lines read as quiet machine output between
            // the human-readable turns — Claude Code's exact hierarchy.
            let style = Style::default().fg(palette.muted);
            for line in lines {
                rendered.push(restyle_line(line, style));
            }
        } else {
            // System / plan context / errors: default styling.
            rendered.extend(lines);
        }
        previous_was_secondary = is_secondary;
    }
    rendered
}

/// VRO-11.9 — best-effort inverse mapping of a clicked row inside the
/// Conversation panel to the source transcript entry rendered there.
/// Returns the source line ONLY when it is a bare http(s) URL line (the
/// clickable VesperLens review link); any other hit (or a row outside the
/// transcript content) returns `None` so the caller falls through to the
/// default click behavior.
///
/// Replicates the renderer's pure pipeline (per-entry role rendering +
/// wrap estimation + the same scroll math) so the mapping is consistent
/// with what is on screen. Wrap counts are estimates; a miss is always a
/// safe no-op.
#[must_use]
pub fn bare_url_entry_at_row(model: &ViewModel, area: Rect, row: u16) -> Option<String> {
    if area.height < 3 || area.width < 3 {
        return None;
    }
    let inner_width = usize::from(area.width.saturating_sub(4));
    let visible = usize::from(area.height.saturating_sub(2));
    let top = area.y.saturating_add(1);
    if !(top..top.saturating_add(visible as u16)).contains(&row) {
        return None;
    }
    let lines = transcript_lines_for(model);
    if lines.is_empty() {
        return None;
    }
    let mut total = 0_usize;
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(lines.len());
    for (idx, entry) in lines.iter().enumerate() {
        let rendered = render_transcript_lines(std::slice::from_ref(entry), inner_width);
        let wrapped = estimated_wrapped_lines(&rendered, inner_width);
        let rows = if idx > 0 { wrapped + 1 } else { wrapped };
        spans.push((total, total + rows));
        total += rows;
    }
    let max_scroll = total.saturating_sub(visible).min(u16::MAX as usize) as u16;
    let manual = model
        .conversation_manual_scroll
        .unwrap_or(0)
        .min(max_scroll);
    let effective = max_scroll.saturating_sub(manual) as usize;
    let row_in_content = usize::from(row - top) + effective;
    for (idx, (start, end)) in spans.iter().enumerate() {
        if row_in_content >= *start && row_in_content < *end {
            let candidate = lines[idx].trim();
            if (candidate.starts_with("http://") || candidate.starts_with("https://"))
                && !candidate.contains(' ')
            {
                return Some(lines[idx].clone());
            }
            return None;
        }
    }
    None
}

/// Re-styles every span of a rendered markdown line with `style`, keeping
/// the span segmentation (so wrapped-width estimates stay identical) while
/// overriding the foreground color and modifiers. Used by the VRO-11.5
/// live-region branches (thinking + telemetry) so dim secondary text still
/// benefits from markdown parsing (inline code, bold) without inventing a
/// second renderer.
fn restyle_line(line: Line<'static>, style: Style) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|mut span| {
                span.style = style;
                span
            })
            .collect::<Vec<_>>(),
    )
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

fn render_sidebar(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    model: &ViewModel,
    palette: ThemePalette,
) {
    // Keep the rail useful and quiet: compact session facts, a dedicated
    // live TODO surface, and a bounded run summary. Tool telemetry remains
    // inline with the conversation, but plan state does not belong in chat
    // history because repeated updates bury the actual dialogue.
    let report_height = if model.last_report.is_empty() { 3 } else { 8 };
    let todo_constraint = if model.panels.tasks {
        Constraint::Min(7)
    } else {
        Constraint::Length(0)
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(palette.border))
            .style(Style::default().fg(palette.text).bg(palette.surface)),
        area,
    );
    let rail = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            todo_constraint,
            Constraint::Length(report_height),
        ])
        .split(rail);

    let model_name = superpower_value_for(model, "model").unwrap_or_else(|| "provider".into());
    let thinking = superpower_value_for(model, "thinking").unwrap_or_else(|| "enabled".into());
    let session = vec![
        Line::from(Span::styled(
            "Session",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Model       {model_name}")),
        Line::from(format!("Thinking    {thinking}")),
        Line::from(format!("Permission  {:?}", model.controls.permission_mode)),
        Line::from(format!("Mode        {:?}", model.controls.operating_mode)),
        Line::from(format!("Phase       {}", model.plan.phase().label())),
        Line::from(format!("Context     {} entries", model.transcript.len())),
    ];
    frame.render_widget(Paragraph::new(session), chunks[0]);

    let todo_lines = if model.task_plan.is_empty() {
        vec![Line::from(Span::styled(
            "No active tasks",
            Style::default().fg(palette.muted),
        ))]
    } else {
        model
            .task_plan
            .iter()
            .map(|task| {
                let (marker, color) = match task.status.as_str() {
                    "completed" => ("✓", Color::Green),
                    "in_progress" => ("●", Color::Yellow),
                    _ => ("○", palette.muted),
                };
                Line::from(vec![
                    Span::styled(format!("{marker} "), Style::default().fg(color)),
                    Span::raw(task.content.clone()),
                ])
            })
            .collect()
    };
    let completed = model
        .task_plan
        .iter()
        .filter(|task| task.status == "completed")
        .count();
    if model.panels.tasks {
        frame.render_widget(
            Paragraph::new(
                std::iter::once(Line::from(Span::styled(
                    format!("TODO {completed}/{}", model.task_plan.len()),
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                )))
                .chain(todo_lines)
                .collect::<Vec<_>>(),
            )
            .wrap(Wrap { trim: false }),
            chunks[1],
        );
    }

    let run_lines = if model.last_report.is_empty() {
        vec![Line::from(vec![
            Span::styled(
                if model.agent_running { "● " } else { "○ " },
                Style::default().fg(if model.agent_running {
                    palette.warning
                } else {
                    palette.muted
                }),
            ),
            Span::raw(if model.agent_running {
                "Working"
            } else {
                "Ready"
            }),
        ])]
    } else {
        model.last_report.iter().cloned().map(Line::from).collect()
    };
    frame.render_widget(
        Paragraph::new(
            std::iter::once(Line::from(Span::styled(
                if model.last_report.is_empty() {
                    "RUN"
                } else {
                    "LAST RUN"
                },
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            )))
            .chain(run_lines)
            .collect::<Vec<_>>(),
        )
        .wrap(Wrap { trim: false }),
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

/// How many of the most recent thinking lines stream inline in the
/// Conversation panel while a turn is running. The provider chain of
/// thought can grow for minutes on `reasoning: max` turns — a fixed tail
/// window keeps the live feed readable (Claude Code collapses thinking the
/// same way once the turn completes; while running, the newest reasoning is
/// what matters).
pub const INLINE_THINKING_TAIL_LINES: usize = 14;

/// Builds the transcript lines for the main panel: Plan Mode context first
/// (pending questions during PLANNING, the plan body during REVIEW), then the
/// accumulated transcript, then the live region (inline thinking → tool
/// telemetry → streaming response) while a turn runs.
pub fn transcript_lines_for(model: &ViewModel) -> Vec<String> {
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
    // VRO-11.5: the provider-visible chain of thought streams INLINE in the
    // Conversation panel as a dimmed `🧠 Thinking` block (the bottom
    // Reasoning panel is gone). Only a bounded tail of the newest reasoning
    // lines renders; the block disappears when the turn completes, exactly
    // like Claude Code collapsing live thinking into the final answer.
    if model.agent_running && model.panels.reasoning && !model.reasoning.is_empty() {
        let header = match model.reasoning_diagnostics.as_ref() {
            Some(diagnostics) => diagnostics.render_inline_header(),
            None => "🧠 Thinking…".to_string(),
        };
        lines.push(format!("thinking: {header}"));
        let tail: Vec<&str> = model
            .reasoning
            .lines()
            .filter(|line| !line.trim().is_empty())
            .rev()
            .take(INLINE_THINKING_TAIL_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        for line in tail {
            lines.push(format!("thinking: {line}"));
        }
    }
    // VRO-11.4: inline tool telemetry renders DIRECTLY in the Conversation
    // panel after the transcript, so the trajectory reads top-to-bottom
    // naturally with the assistant's text (matches Codex / Claude Code /
    // the host-agent rendering). Each line is already prefixed
    // with `> ` for visual distinction from user/assistant turns.
    if model.show_tool_details {
        if !model.live_trajectory.is_empty() {
            lines.push("activity: Full tool activity · Ctrl+T returns to chat".into());
            lines.extend(model.live_trajectory.iter().cloned());
        }
    } else if !model.live_trajectory.is_empty() {
        lines.push(tool_activity_summary(&model.live_trajectory));
        lines.extend(
            model
                .live_trajectory
                .iter()
                .filter(|line| line.contains("VesperLens") || line.starts_with("http"))
                .cloned(),
        );
    }
    if model.agent_running && !model.live_response.is_empty() {
        lines.push(format!("assistant (streaming): {}", model.live_response));
    }
    lines
}

/// Compact projection of a verbose provider/tool event stream.
#[must_use]
pub fn tool_activity_summary(entries: &[String]) -> String {
    let mut commands = 0_usize;
    let mut reads = 0_usize;
    let mut edits = 0_usize;
    let mut other = 0_usize;
    let mut commentary = 0_usize;
    for entry in entries {
        if entry.starts_with("commentary:") {
            commentary += 1;
            continue;
        }
        let Some(action) = entry.trim().strip_prefix('⏺') else {
            continue;
        };
        let name = action.trim().split([' ', '·']).next().unwrap_or_default();
        if name.contains("command") || name.contains("shell") {
            commands += 1;
        } else if name.contains("read")
            || name.contains("list")
            || name.contains("grep")
            || name.contains("search")
        {
            reads += 1;
        } else if name.contains("write") || name.contains("edit") || name.contains("patch") {
            edits += 1;
        } else {
            other += 1;
        }
    }
    let total = commands + reads + edits + other;
    let mut parts = vec![format!("Ran {total} tools")];
    for (count, label) in [
        (commands, "commands"),
        (reads, "reads"),
        (edits, "edits"),
        (other, "other"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    if commentary > 0 {
        parts.push(format!("{commentary} progress updates"));
    }
    format!("activity: ● {} · Ctrl+T details", parts.join(" · "))
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
            // Disable inline reasoning so this test isolates transcript text.
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
    fn wide_layout_renders_todo_in_a_dedicated_compact_sidebar() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let model = ViewModel {
            transcript: vec!["assistant: Actual conversation".into()],
            task_plan: vec![
                TaskItem {
                    content: "Inspect implementation".into(),
                    status: "completed".into(),
                    priority: "1".into(),
                },
                TaskItem {
                    content: "Open interactive review".into(),
                    status: "in_progress".into(),
                    priority: "2".into(),
                },
            ],
            agent_running: true,
            panels: PanelVisibility {
                chat_only: false,
                ..PanelVisibility::default()
            },
            ..ViewModel::default()
        };
        let backend = TestBackend::new(140, 35);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("wide sidebar render");
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(
            content.contains("TODO 1/2"),
            "dedicated TODO title: {content}"
        );
        assert!(content.contains("Inspect implementation"));
        assert!(content.contains("Open interactive review"));
        assert!(content.contains("RUN"));
        assert!(content.contains("Working"));
        assert!(content.contains("Actual conversation"));
    }

    #[test]
    fn chat_only_collapse_hides_the_rail_and_widens_the_conversation() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // F11 must be a render-time override: the sidebar rail (Session +
        // TODO + Last run) disappears while `sidebar` / `tasks` keep their
        // values, and the Conversation column takes the full body width.
        let mut model = ViewModel {
            transcript: vec!["assistant: Actual conversation".into()],
            task_plan: vec![TaskItem {
                content: "Inspect implementation".into(),
                status: "in_progress".into(),
                priority: "1".into(),
            }],
            agent_running: true,
            panels: PanelVisibility {
                chat_only: false,
                ..PanelVisibility::default()
            },
            ..ViewModel::default()
        };

        let backend = TestBackend::new(140, 35);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("wide sidebar render");
        let before: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            before.contains("TODO 0/1"),
            "rail renders by default: {before}"
        );

        model.panels.toggle_chat_only();
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("chat-only render");
        let after: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            !after.contains("TODO 1/1"),
            "chat-only collapse must hide the rail: {after}"
        );
        assert!(
            !after.contains("Session"),
            "chat-only collapse must hide the session panel: {after}"
        );
        assert!(
            after.contains("Actual conversation"),
            "conversation survives the collapse: {after}"
        );
        // The per-panel flags stay intact so a second F11 restores them.
        assert!(model.panels.sidebar);
        assert!(model.panels.tasks);
    }

    #[test]
    fn render_to_frame_styles_telemetry_dim_and_url_as_link() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // VRO-11.6: ⏺/⎿ telemetry lines render dim (quiet machine output)
        // and a bare URL line renders as an obvious link — cyan +
        // underlined at normal brightness — so terminals auto-linkify it.
        let model = ViewModel {
            agent_running: true,
            show_tool_details: true,
            live_trajectory: vec![
                "⏺ write_file".to_string(),
                "  ⎿ ✓ write_file".to_string(),
                "http://127.0.0.1:41277/review/dash".to_string(),
            ],
            ..ViewModel::default()
        };

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|f| render_to_frame(f, &model))
            .expect("telemetry + URL render must not panic");
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("write_file"), "tool name visible");
        assert!(
            content.contains("127.0.0.1:41277"),
            "the review URL must be visible inline: {content}"
        );

        let accent = theme_palette(&model.preferences.theme).accent;
        let muted = theme_palette(&model.preferences.theme).muted;
        let has_accent_link = terminal.backend().buffer().content.iter().any(|cell| {
            cell.style().fg == Some(accent)
                && cell
                    .style()
                    .add_modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
        });
        assert!(
            has_accent_link,
            "the URL line must render in the theme accent + underlined"
        );

        let has_dim_telemetry = terminal.backend().buffer().content.iter().any(|cell| {
            cell.style().fg == Some(muted)
                && !cell
                    .style()
                    .add_modifier
                    .contains(ratatui::style::Modifier::ITALIC)
        });
        assert!(
            has_dim_telemetry,
            "⏺/⎿ telemetry must render dim (non-italic) secondary text"
        );
    }

    #[test]
    fn bare_url_entry_at_row_maps_clicks_on_the_url_line() {
        use ratatui::layout::Rect;
        // VRO-11.9: with mouse capture ON the app itself must open the
        // review link — a click on the bare-URL line maps back to it, a
        // click on any other entry does not, and out-of-area rows are
        // ignored.
        let url = "http://127.0.0.1:41277/review/dash".to_string();
        let model = ViewModel {
            transcript: vec![
                "user: build a dashboard".to_string(),
                "assistant: done".to_string(),
                url.clone(),
            ],
            ..ViewModel::default()
        };
        let area = Rect {
            x: 0,
            y: 1,
            width: 80,
            height: 10,
        };
        // Content is TOP-aligned: entries start at area.y + 1. The URL is
        // the last entry — map its span start by walking the same pure
        // pipeline the helper uses, then assert that exact row resolves to
        // it (self-consistent mapping) while neighbors do not.
        let top = area.y + 1;
        // Single-entry model: the URL occupies content row 0.
        let only_url = ViewModel {
            transcript: vec![url.clone()],
            ..ViewModel::default()
        };
        assert_eq!(
            bare_url_entry_at_row(&only_url, area, top),
            Some(url.clone())
        );
        // Multi-entry rows: user prompt row 0, separator 1, assistant 2,
        // separator 3, URL 4. Click the URL's own row.
        let url_row = top + 4;
        assert_eq!(
            bare_url_entry_at_row(&model, area, url_row),
            Some(url.clone()),
            "the URL entry's own row must map to it"
        );
        // The user prompt and separator rows are not URLs.
        assert_eq!(bare_url_entry_at_row(&model, area, top), None);
        assert_eq!(bare_url_entry_at_row(&model, area, top + 1), None);
        // Outside the transcript content (border row / below the content).
        assert_eq!(bare_url_entry_at_row(&model, area, area.y), None);
        assert_eq!(
            bare_url_entry_at_row(&model, area, area.y + area.height - 1),
            None
        );
        // Degenerate area is a safe no-op.
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        assert_eq!(bare_url_entry_at_row(&model, tiny, 0), None);
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
    fn consecutive_tool_events_render_as_one_compact_group() {
        let rendered = render_transcript_lines(
            &[
                "⏺ read_file · src/main.rs".into(),
                "  ⎿ 120 lines".into(),
                "⏺ run_command · cargo test".into(),
                "  ⎿ 3 lines".into(),
            ],
            80,
        );
        assert_eq!(rendered.len(), 4, "tool groups must not gain blank rows");
    }

    #[test]
    fn render_to_frame_renders_inline_thinking_in_conversation() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // VRO-11.5: the bottom Reasoning panel is gone; live thinking must
        // stream INLINE in the Conversation panel while a turn runs, with
        // the dim italic secondary styling.
        let model = ViewModel {
            agent_running: true,
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
            .expect("inline thinking must not panic");
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        // The emoji is a double-width glyph, so the collected cell string
        // carries a wide-followup space — assert the pieces separately.
        assert!(
            content.contains("🧠") && content.contains("Thinking"),
            "the thinking header must render inline: {content}"
        );
        assert!(
            content.contains("this"),
            "thinking text should be visible inline"
        );
        // The thinking cells carry the dim italic live-region style.
        let muted = theme_palette(&model.preferences.theme).muted;
        let has_dim_italic = terminal.backend().buffer().content.iter().any(|cell| {
            cell.style()
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
                && cell.style().fg == Some(muted)
        });
        assert!(
            has_dim_italic,
            "inline thinking must render dim + italic, not as primary text"
        );
    }

    #[test]
    fn transcript_lines_gate_thinking_on_running_and_visibility() {
        // VRO-11.5: the inline thinking block requires ALL of: a running
        // turn, visible thinking (`panels.reasoning`, the F2 toggle), and
        // non-empty reasoning text.
        let base = ViewModel {
            agent_running: true,
            panels: PanelVisibility {
                reasoning: true,
                ..PanelVisibility::default()
            },
            reasoning: "let me consider the layout".to_string(),
            ..ViewModel::default()
        };
        let lines = transcript_lines_for(&base);
        assert!(
            lines.iter().any(|l| l.starts_with("thinking: ")),
            "running turn with visible reasoning must include the thinking block"
        );

        let idle = ViewModel {
            agent_running: false,
            ..base.clone()
        };
        assert!(
            !transcript_lines_for(&idle)
                .iter()
                .any(|l| l.starts_with("thinking: ")),
            "idle turns must collapse the thinking block"
        );

        let hidden = ViewModel {
            panels: PanelVisibility {
                reasoning: false,
                ..PanelVisibility::default()
            },
            ..base
        };
        assert!(
            !transcript_lines_for(&hidden)
                .iter()
                .any(|l| l.starts_with("thinking: ")),
            "F2 hidden thinking must suppress the inline block"
        );
    }

    #[test]
    fn transcript_lines_bound_the_inline_thinking_tail() {
        // A long chain of thought must not flood the conversation: only
        // INLINE_THINKING_TAIL_LINES most recent reasoning lines render.
        let model = ViewModel {
            agent_running: true,
            panels: PanelVisibility {
                reasoning: true,
                ..PanelVisibility::default()
            },
            reasoning: (0..60)
                .map(|i| format!("reasoning line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            ..ViewModel::default()
        };
        let lines = transcript_lines_for(&model);
        let thinking: Vec<&String> = lines
            .iter()
            .filter(|l| l.starts_with("thinking: "))
            .collect();
        assert_eq!(
            thinking.len(),
            INLINE_THINKING_TAIL_LINES + 1,
            "one header line + exactly the bounded tail"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&format!("reasoning line {}", 59))),
            "the newest reasoning line must be present"
        );
        assert!(
            !lines.iter().any(|l| l.contains("reasoning line 30")),
            "old reasoning lines must be dropped from the tail window"
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
        // With blank-line pacing, each entry takes ~2 rendered lines, so 200 entries
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
    fn user_turns_render_with_terminal_prompt_marker_without_chat_bubble() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

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
            .expect("terminal feed render must not panic");

        let buffer = terminal.backend().buffer();
        let content: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(
            !content.contains("user: please"),
            "the raw transport role prefix must be stripped"
        );
        assert!(
            content.contains("please"),
            "the user prompt text must remain visible"
        );
        assert!(
            buffer.content.iter().any(|cell| {
                cell.symbol() == "›"
                    && cell.style().fg == Some(theme_palette(&model.preferences.theme).accent)
                    && cell.style().add_modifier.contains(Modifier::BOLD)
            }),
            "user turns need the compact theme-accent › marker"
        );
        assert!(
            buffer.content.iter().all(|cell| {
                cell.bg != Color::Rgb(18, 49, 75) && cell.bg != Color::Rgb(23, 29, 38)
            }),
            "conversation turns must not paint legacy chat-bubble backgrounds"
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

    // ===================================================================
    // VRO-8 (PRD §8.1) — ReasoningDiagnostics header rendering.
    // ===================================================================

    #[test]
    fn reasoning_diagnostics_header_includes_strategy_and_budget() {
        // Directive 1: the header must surface the chosen strategy, mode,
        // and key budget fields so the driver sees the orchestrator's
        // decision at a glance.
        let diagnostics = ReasoningDiagnostics {
            strategy: "bounded_tree_search".into(),
            mode: "auto".into(),
            override_active: false,
            risk: "medium".into(),
            risk_escalation: false,
            max_search_depth: 3,
            max_parallel_branches: 2,
            max_model_calls: 10,
            max_repairs: 2,
            phase: String::new(),
        };
        let header = diagnostics.render_header();
        assert!(
            header.contains("**Strategy:** `bounded_tree_search`"),
            "got: {header}"
        );
        assert!(header.contains("**Mode:** `auto`"), "got: {header}");
        assert!(header.contains("**Risk:** `medium`"), "got: {header}");
        assert!(header.contains("Depth: 3"), "got: {header}");
        assert!(header.contains("Branches: 2"), "got: {header}");
        assert!(header.contains("Models: 10"), "got: {header}");
        assert!(header.contains("Repairs: 2"), "got: {header}");
        // No override flag for the auto path.
        assert!(!header.contains("override"), "got: {header}");
        assert!(
            !header.contains("RISK ESCALATION"),
            "no escalation marker for medium risk"
        );
    }

    #[test]
    fn reasoning_diagnostics_header_marks_active_override() {
        // When the user forced a mode, the header surfaces *(override)* so
        // it's clear the profiler is NOT in charge.
        let diagnostics = ReasoningDiagnostics {
            strategy: "tool_grounded_react".into(),
            mode: "deep".into(),
            override_active: true,
            risk: "low".into(),
            risk_escalation: false,
            max_search_depth: 3,
            max_parallel_branches: 3,
            max_model_calls: 10,
            max_repairs: 2,
            phase: String::new(),
        };
        let header = diagnostics.render_header();
        assert!(
            header.contains("**Mode:** `deep` *(override)*"),
            "got: {header}"
        );
    }

    #[test]
    fn reasoning_diagnostics_header_prominently_warns_on_risk_escalation() {
        // Directive 1: when the TaskProfiler escalated a task due to risk,
        // prominently display a "Risk Escalation" warning. The renderer
        // surfaces this as **⚠ RISK ESCALATION** next to the strategy line.
        let diagnostics = ReasoningDiagnostics {
            strategy: "proposer_critic_adjudicator".into(),
            mode: "deep".into(),
            override_active: false,
            risk: "high".into(),
            risk_escalation: true,
            max_search_depth: 3,
            max_parallel_branches: 3,
            max_model_calls: 10,
            max_repairs: 2,
            phase: String::new(),
        };
        let header = diagnostics.render_header();
        assert!(header.contains("**Risk:** `high`"), "got: {header}");
        assert!(header.contains("**⚠ RISK ESCALATION**"), "got: {header}");
    }

    #[test]
    fn reasoning_diagnostics_default_is_all_empty_and_renders_safely() {
        // The default (used when VRO is disabled or no turn has run) must
        // render without panic and produce a well-formed header.
        let diagnostics = ReasoningDiagnostics::default();
        let header = diagnostics.render_header();
        assert!(header.contains("**Strategy:** ``"), "got: {header}");
        assert!(header.contains("Depth: 0"), "got: {header}");
        assert!(
            !header.contains("RISK ESCALATION"),
            "default must not falsely claim escalation"
        );
    }
}
#[test]
fn themes_own_complete_palettes_and_retire_the_blue_default() {
    let retired_blue = Color::Rgb(7, 11, 18);
    for name in [
        "chatgpt-black",
        "chatgpt-white",
        "ansi",
        "light",
        "dracula",
        "nord",
    ] {
        let palette = theme_palette(name);
        assert_ne!(palette.background, retired_blue, "{name}");
        assert_ne!(palette.text, palette.background, "{name} text contrast");
        assert_ne!(palette.accent, palette.background, "{name} accent contrast");
    }
    assert_eq!(
        theme_palette("chatgpt-black").background,
        Color::Rgb(0, 0, 0)
    );
    assert_eq!(
        theme_palette("chatgpt-white").background,
        Color::Rgb(255, 255, 255)
    );
    assert_eq!(
        theme_palette("vesper").background,
        theme_palette("chatgpt-black").background,
        "saved legacy preference must migrate to black rather than resurrect blue"
    );
}
