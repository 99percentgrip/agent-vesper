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
    layout::{Constraint, Direction, Layout},
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // status / phase
            Constraint::Min(5),    // transcript
            Constraint::Length(8), // superpowers panel
            Constraint::Length(3), // input
        ])
        .split(area);

    // Phase / status banner — phase label + one-line status so the driver can
    // tell at a glance whether the agent is thinking, waiting for input, or
    // executing.
    let phase = model.plan.phase();
    let banner_text = match (phase, model.status.as_deref()) {
        (phase, Some(status)) => format!(" Vesper TUI — {} — {status} ", phase.label()),
        (phase, None) => format!(" Vesper TUI — phase: {} ", phase.label()),
    };
    let banner_style = banner_style_for_phase(phase);
    frame.render_widget(Paragraph::new(banner_text).style(banner_style), chunks[0]);

    // Transcript — prepended with Plan Mode context (pending questions while
    // PLANNING, the plan body while REVIEW) so every phase has something
    // actionable for the driver to look at.
    let transcript_items: Vec<ListItem> = transcript_lines_for(model)
        .into_iter()
        .map(ListItem::new)
        .collect();
    frame.render_widget(
        List::new(transcript_items)
            .block(Block::default().borders(Borders::ALL).title(" Transcript ")),
        chunks[1],
    );

    // Superpowers panel — each advertised descriptor with its active override.
    let superpower_lines = superpower_lines_for(model);
    frame.render_widget(
        Paragraph::new(superpower_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Provider Superpowers "),
        ),
        chunks[2],
    );

    // Input.
    let input_value = format!("> {}", model.input);
    frame.render_widget(
        Paragraph::new(input_value).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Input (Ctrl-C to quit) "),
        ),
        chunks[3],
    );
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
        };
        let _: &dyn TerminalRenderer = &renderer;
        let mut dynamic: Box<dyn TerminalRenderer> = Box::new(renderer);
        dynamic.render(&model);
    }
}
