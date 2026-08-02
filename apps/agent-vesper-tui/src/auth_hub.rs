//! Provider-driven Hermes authentication hub.

use std::fmt;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use zeroize::Zeroizing;

/// One real provider credential exposed by the active build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthProvider {
    /// Runtime registry identity.
    pub id: &'static str,
    /// Human-facing provider name.
    pub name: &'static str,
    /// Environment variable accepted as a higher-precedence source.
    pub environment_variable: &'static str,
    /// Provider-owned key-management page.
    pub key_url: &'static str,
}

/// Startup destination derived from credential availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupRoute {
    /// Credentials exist; enter the main harness.
    Main,
    /// Required credentials are absent; intercept into Hermes.
    Auth,
}

/// Pure startup gate used by production and tests.
#[must_use]
pub const fn startup_route(required_credential_present: bool) -> StartupRoute {
    if required_credential_present {
        StartupRoute::Main
    } else {
        StartupRoute::Auth
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthStep {
    Provider,
    Secret,
}

/// User intent emitted by the pure Auth Hub state machine.
#[derive(PartialEq, Eq)]
pub enum AuthHubAction {
    /// No terminal-level action is required.
    Continue,
    /// Persist this provider credential through the secure store.
    Save {
        /// Selected provider identity.
        provider_id: &'static str,
        /// Secret value; its debug representation is redacted by `Zeroizing`'s
        /// wrapped string discipline and it is cleared on drop.
        secret: Zeroizing<String>,
    },
    /// Exit without entering an unauthenticated main screen.
    Quit,
}

impl fmt::Debug for AuthHubAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Continue => formatter.write_str("Continue"),
            Self::Quit => formatter.write_str("Quit"),
            Self::Save {
                provider_id,
                secret,
            } => formatter
                .debug_struct("Save")
                .field("provider_id", provider_id)
                .field("secret_length", &secret.chars().count())
                .finish(),
        }
    }
}

/// Pure, secret-safe state for the interactive authentication screen.
pub struct AuthHubState {
    providers: Vec<AuthProvider>,
    selected: usize,
    step: AuthStep,
    secret: Zeroizing<String>,
    status: Option<String>,
    saving: bool,
}

impl fmt::Debug for AuthHubState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthHubState")
            .field("providers", &self.providers)
            .field("selected", &self.selected)
            .field("step", &self.step)
            .field("secret_length", &self.secret.chars().count())
            .field("status", &self.status)
            .field("saving", &self.saving)
            .finish()
    }
}

impl AuthHubState {
    /// Creates a hub from credentials advertised by real registered providers.
    pub fn new(providers: Vec<AuthProvider>) -> Result<Self, &'static str> {
        if providers.is_empty() {
            return Err("the active build has no provider authentication descriptors");
        }
        Ok(Self {
            providers,
            selected: 0,
            step: AuthStep::Provider,
            secret: Zeroizing::new(String::new()),
            status: None,
            saving: false,
        })
    }

    /// Currently selected registered provider.
    #[must_use]
    pub fn provider(&self) -> AuthProvider {
        self.providers[self.selected]
    }

    /// Moves selection to the preceding provider.
    pub fn previous_provider(&mut self) {
        if self.step == AuthStep::Provider && !self.saving {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.providers.len() - 1);
        }
    }

    /// Moves selection to the following provider.
    pub fn next_provider(&mut self) {
        if self.step == AuthStep::Provider && !self.saving {
            self.selected = (self.selected + 1) % self.providers.len();
        }
    }

    /// Inserts one printable character into the masked field.
    pub fn insert(&mut self, character: char) {
        if self.step == AuthStep::Secret
            && !self.saving
            && !character.is_control()
            && self.secret.len() < 16 * 1024
        {
            self.secret.push(character);
            self.status = None;
        }
    }

    /// Inserts a bounded pasted value, dropping control characters.
    pub fn paste(&mut self, value: &str) {
        for character in value.chars().filter(|character| !character.is_control()) {
            self.insert(character);
        }
    }

    /// Removes the final Unicode scalar from the masked field.
    pub fn backspace(&mut self) {
        if self.step == AuthStep::Secret && !self.saving {
            self.secret.pop();
            self.status = None;
        }
    }

    /// Advances provider selection or emits a validated save request.
    pub fn submit(&mut self) -> AuthHubAction {
        if self.saving {
            return AuthHubAction::Continue;
        }
        match self.step {
            AuthStep::Provider => {
                self.step = AuthStep::Secret;
                self.status = None;
                AuthHubAction::Continue
            }
            AuthStep::Secret => match vesper_auth::validate_secret(self.secret.as_str()) {
                Ok(secret) => {
                    self.saving = true;
                    self.status = Some("Saving securely…".into());
                    AuthHubAction::Save {
                        provider_id: self.provider().id,
                        secret: Zeroizing::new(secret.to_owned()),
                    }
                }
                Err(_) => {
                    self.status =
                        Some("Enter a non-empty API key without control characters.".into());
                    AuthHubAction::Continue
                }
            },
        }
    }

    /// Returns to provider selection, or quits from the first step.
    pub fn cancel(&mut self) -> AuthHubAction {
        if self.saving {
            return AuthHubAction::Continue;
        }
        if self.step == AuthStep::Secret {
            self.secret.clear();
            self.step = AuthStep::Provider;
            self.status = None;
            AuthHubAction::Continue
        } else {
            AuthHubAction::Quit
        }
    }

    /// Reports a secret-safe persistence failure and allows another attempt.
    pub fn save_failed(&mut self, message: impl Into<String>) {
        self.saving = false;
        self.status = Some(message.into());
    }

    /// Masked field projection; the backing secret is never returned.
    #[must_use]
    pub fn masked_secret(&self) -> String {
        "*".repeat(self.secret.chars().count())
    }

    #[must_use]
    fn entering_secret(&self) -> bool {
        self.step == AuthStep::Secret
    }
}

/// Renders the responsive centered Hermes authentication screen.
pub fn render_auth_hub(frame: &mut Frame<'_>, state: &AuthHubState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(9, 14, 23))),
        area,
    );
    let modal = centered_modal(area, 76, 22);
    frame.render_widget(Clear, modal);
    let shell = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(59, 160, 255)))
        .style(Style::default().bg(Color::Rgb(13, 21, 33)))
        .title(Line::from(vec![
            Span::styled(" ◆ ", Style::default().fg(Color::Cyan)),
            Span::styled(
                "Hermes Authentication Hub ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    let inner = shell.inner(modal);
    frame.render_widget(shell, modal);
    if inner.width < 28 || inner.height < 10 {
        frame.render_widget(
            Paragraph::new("Terminal too small for secure setup. Resize to at least 32×12.")
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Connect a real provider",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("Credentials are stored in your OS credential manager when available."),
        ])
        .style(Style::default().fg(Color::Gray)),
        rows[0],
    );

    let progress = if state.entering_secret() {
        "  1  Provider  ─────────  2  API key"
    } else {
        "  1  Provider             2  API key"
    };
    frame.render_widget(
        Paragraph::new(progress).style(Style::default().fg(Color::Cyan)),
        rows[1],
    );

    if state.entering_secret() {
        render_secret_step(frame, state, rows[2]);
    } else {
        render_provider_step(frame, state, rows[2]);
    }
    let hint = state
        .status
        .as_deref()
        .unwrap_or(if state.entering_secret() {
            "Enter save  •  Esc back  •  input is masked"
        } else {
            "↑/↓ choose  •  Enter continue  •  Esc quit"
        });
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(if state.status.is_some() {
            Color::Yellow
        } else {
            Color::DarkGray
        })),
        rows[3],
    );
}

fn render_provider_step(frame: &mut Frame<'_>, state: &AuthHubState, area: Rect) {
    let items = state
        .providers
        .iter()
        .map(|provider| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {}  ", provider.name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(provider.id, Style::default().fg(Color::Cyan)),
            ]))
        })
        .collect::<Vec<_>>();
    let mut list_state = ListState::default().with_selected(Some(state.selected));
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Registered providers "),
            )
            .highlight_symbol("▶")
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(17, 49, 75))
                    .add_modifier(Modifier::BOLD),
            ),
        area,
        &mut list_state,
    );
}

fn render_secret_step(frame: &mut Frame<'_>, state: &AuthHubState, area: Rect) {
    let provider = state.provider();
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{} credential • environment override: {}\nGet a key: {}",
            provider.name, provider.environment_variable, provider.key_url
        ))
        .style(Style::default().fg(Color::Gray)),
        rows[0],
    );
    let masked = state.masked_secret();
    frame.render_widget(
        Paragraph::new(masked.as_str())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" API key (masked) "),
            ),
        rows[1],
    );
    if !state.saving {
        let cursor_offset = masked.len().min(rows[1].width.saturating_sub(3) as usize) as u16;
        frame.set_cursor_position(Position::new(
            rows[1].x.saturating_add(1).saturating_add(cursor_offset),
            rows[1].y.saturating_add(1),
        ));
    }
}

fn centered_modal(area: Rect, maximum_width: u16, maximum_height: u16) -> Rect {
    if area.width <= 36 || area.height <= 14 {
        return area;
    }
    let width = maximum_width.min(area.width.saturating_sub(4));
    let height = maximum_height.min(area.height.saturating_sub(2));
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [modal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(vertical);
    modal
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    const PROVIDER: AuthProvider = AuthProvider {
        id: "zai",
        name: "Z.ai",
        environment_variable: "ZAI_API_KEY",
        key_url: "https://z.ai/manage-apikey/apikey-list",
    };

    #[test]
    fn missing_credentials_route_to_auth_and_present_credentials_route_main() {
        assert_eq!(startup_route(false), StartupRoute::Auth);
        assert_eq!(startup_route(true), StartupRoute::Main);
    }

    #[test]
    fn secret_state_and_debug_are_masked() {
        let mut state = AuthHubState::new(vec![PROVIDER]).unwrap();
        state.submit();
        state.paste("secret-canary");
        assert_eq!(state.masked_secret(), "*************");
        assert!(!format!("{state:?}").contains("secret-canary"));
        let action = state.submit();
        assert!(!format!("{action:?}").contains("secret-canary"));
    }

    #[test]
    fn rendered_frame_contains_mask_but_not_secret() {
        let mut state = AuthHubState::new(vec![PROVIDER]).unwrap();
        state.submit();
        state.paste("secret-canary");
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_auth_hub(frame, &state))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("*************"));
        assert!(!rendered.contains("secret-canary"));
    }

    #[test]
    fn small_terminal_render_does_not_panic() {
        let state = AuthHubState::new(vec![PROVIDER]).unwrap();
        let backend = TestBackend::new(28, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_auth_hub(frame, &state))
            .unwrap();
    }
}
