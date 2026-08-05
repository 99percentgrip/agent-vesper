//! LM Studio provider settings hub (VRO-3.x TUI integration).
//!
//! A pure state machine + renderer that lets the user adjust the LM Studio
//! network endpoint (the LAN/localhost `api_base_url`) and an optional pinned
//! model **from inside the TUI** — not a config file. Mirrors the
//! [`crate::auth_hub`] pattern: the state machine is pure and unit-testable;
//! the binary owns the terminal event loop and persists on `Save`.
//!
//! ## Persistence
//!
//! [`LmStudioSettings`] holds only **non-secret** fields (the URL + optional
//! model) and is persisted as JSON under the Agent Vesper state dir
//! (`$AGENT_VESPER_LMSTUDIO_ROOT` or `.agent-vesper/lmstudio/settings.json`).
//! The optional API key is **not** stored here (project secret discipline);
//! it is read from the `LMSTUDIO_API_KEY` environment variable, which the
//! screen surfaces as a hint. Moving the key into the OS credential store is
//! the security follow-up.

use std::path::PathBuf;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use serde::{Deserialize, Serialize};

/// Persisted, non-secret LM Studio settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LmStudioSettings {
    /// Full base URL including the version path, e.g.
    /// `http://192.168.254.114:1234/v1`.
    #[serde(default)]
    pub api_base_url: String,
    /// Optional pinned model id; `None` means auto-discover via `/models`.
    #[serde(default)]
    pub model: Option<String>,
}

impl LmStudioSettings {
    /// `true` when no endpoint and no model are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.api_base_url.trim().is_empty()
            && self.model.as_deref().is_none_or(|m| m.trim().is_empty())
    }

    /// The model id, treating a blank/whitespace string as unset.
    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref().filter(|m| !m.trim().is_empty())
    }
}

/// Which field the hub is currently editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Url,
    Model,
}

impl Field {
    const fn next(self) -> Self {
        match self {
            Self::Url => Self::Model,
            Self::Model => Self::Url,
        }
    }
    const fn prev(self) -> Self {
        self.next()
    }
}

/// Editable LM Studio settings state machine.
pub struct LmStudioHub {
    url_buf: String,
    model_buf: String,
    focused: Field,
    status: Option<String>,
    saving: bool,
}

/// User intent emitted by the hub.
#[derive(Debug, PartialEq, Eq)]
pub enum LmStudioSettingsAction {
    /// No terminal-level action required.
    Continue,
    /// Persist these settings.
    Save { settings: LmStudioSettings },
    /// Exit without saving.
    Quit,
}

/// Maximum length of a single editable field (defensive bound).
const MAX_FIELD_CHARS: usize = 4 * 1024;

impl LmStudioHub {
    /// Seeds the editor from existing persisted settings.
    #[must_use]
    pub fn from_settings(existing: &LmStudioSettings) -> Self {
        Self {
            url_buf: existing.api_base_url.clone(),
            model_buf: existing.model.clone().unwrap_or_default(),
            focused: Field::Url,
            status: None,
            saving: false,
        }
    }

    /// An empty editor.
    #[must_use]
    pub fn empty() -> Self {
        Self::from_settings(&LmStudioSettings::default())
    }

    fn buf_mut(&mut self) -> &mut String {
        match self.focused {
            Field::Url => &mut self.url_buf,
            Field::Model => &mut self.model_buf,
        }
    }

    /// Moves focus to the next field.
    pub fn next_field(&mut self) {
        if !self.saving {
            self.focused = self.focused.next();
            self.status = None;
        }
    }

    /// Moves focus to the previous field.
    pub fn prev_field(&mut self) {
        if !self.saving {
            self.focused = self.focused.prev();
            self.status = None;
        }
    }

    /// Inserts one printable character into the focused field.
    pub fn insert(&mut self, character: char) {
        if !self.saving
            && !character.is_control()
            && self.buf_mut().chars().count() < MAX_FIELD_CHARS
        {
            self.buf_mut().push(character);
            self.status = None;
        }
    }

    /// Inserts a bounded pasted value, dropping control characters.
    pub fn paste(&mut self, value: &str) {
        for character in value.chars().filter(|c| !c.is_control()) {
            self.insert(character);
        }
    }

    /// Removes the final Unicode scalar from the focused field.
    pub fn backspace(&mut self) {
        if !self.saving {
            self.buf_mut().pop();
            self.status = None;
        }
    }

    /// Validates and emits a save request, or stays put with a status message.
    pub fn submit(&mut self) -> LmStudioSettingsAction {
        if self.saving {
            return LmStudioSettingsAction::Continue;
        }
        let url = self.url_buf.trim();
        if url.is_empty() {
            self.status =
                Some("Enter the LM Studio base URL (e.g. http://192.168.1.5:1234/v1).".into());
            return LmStudioSettingsAction::Continue;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            self.status = Some("URL must start with http:// or https://".into());
            return LmStudioSettingsAction::Continue;
        }
        let model = {
            let m = self.model_buf.trim();
            if m.is_empty() {
                None
            } else {
                Some(m.to_string())
            }
        };
        let settings = LmStudioSettings {
            api_base_url: url.to_string(),
            model,
        };
        self.saving = true;
        self.status = Some("Saving…".into());
        LmStudioSettingsAction::Save { settings }
    }

    /// Exits the editor without saving.
    pub fn cancel(&mut self) -> LmStudioSettingsAction {
        if self.saving {
            LmStudioSettingsAction::Continue
        } else {
            LmStudioSettingsAction::Quit
        }
    }

    /// Records a persistence failure so the user can retry.
    pub fn save_failed(&mut self, message: String) {
        self.saving = false;
        self.status = Some(message);
    }

    /// Records a successful save (clears the editor back to a saved state).
    pub fn save_succeeded(&mut self) {
        self.saving = false;
        self.status = Some("Saved. Press Esc to return.".into());
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Resolves the LM Studio settings directory: `$AGENT_VESPER_LMSTUDIO_ROOT`
/// or `$AGENT_VESPER_HOME/lmstudio`, falling back to `./.agent-vesper/lmstudio`.
#[must_use]
pub fn lmstudio_settings_dir() -> PathBuf {
    if let Ok(root) = std::env::var("AGENT_VESPER_LMSTUDIO_ROOT") {
        return PathBuf::from(root);
    }
    let home = std::env::var("AGENT_VESPER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".agent-vesper"));
    home.join("lmstudio")
}

fn settings_path_in(dir: &std::path::Path) -> PathBuf {
    dir.join("settings.json")
}

/// Loads persisted settings from `dir`. Missing/corrupt file ⇒ empty defaults.
pub fn load_lmstudio_settings_from(dir: &std::path::Path) -> LmStudioSettings {
    match std::fs::read_to_string(settings_path_in(dir)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => LmStudioSettings::default(),
    }
}

/// Loads persisted settings from the resolved state dir.
pub fn load_lmstudio_settings() -> LmStudioSettings {
    load_lmstudio_settings_from(&lmstudio_settings_dir())
}

/// Atomically writes settings into `dir`.
pub fn save_lmstudio_settings_to(
    dir: &std::path::Path,
    settings: &LmStudioSettings,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create settings dir: {e}"))?;
    let body =
        serde_json::to_string_pretty(settings).map_err(|e| format!("encode settings: {e}"))?;
    let final_path = settings_path_in(dir);
    let tmp = final_path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write settings: {e}"))?;
    std::fs::rename(&tmp, &final_path).map_err(|e| format!("commit settings: {e}"))?;
    Ok(())
}

/// Atomically writes settings to the resolved state dir.
pub fn save_lmstudio_settings(settings: &LmStudioSettings) -> Result<(), String> {
    save_lmstudio_settings_to(&lmstudio_settings_dir(), settings)
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

/// Renders the LM Studio settings modal.
pub fn render_lmstudio_hub(frame: &mut Frame<'_>, state: &LmStudioHub) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    let modal = centered_modal(area, 72, 16);
    let shell = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " LM Studio settings ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = shell.inner(modal);
    frame.render_widget(shell, modal);
    if inner.width < 40 || inner.height < 9 {
        frame.render_widget(
            Paragraph::new("Terminal too small for LM Studio settings. Resize to at least 44×11.")
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Point Agent Vesper at your LM Studio server",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("Set the full base URL (incl. version path). The optional model is auto-discovered if blank."),
        ])
        .style(Style::default().fg(Color::Gray)),
        rows[0],
    );

    render_field(
        frame,
        rows[1],
        " Base URL ",
        &state.url_buf,
        state.focused == Field::Url,
        Color::Cyan,
    );
    render_field(
        frame,
        rows[2],
        " Model (optional) ",
        &state.model_buf,
        state.focused == Field::Model,
        Color::DarkGray,
    );

    let key_hint = "API key (optional): set the LMSTUDIO_API_KEY environment variable if your server requires a bearer token.";
    frame.render_widget(
        Paragraph::new(key_hint)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true }),
        rows[3],
    );

    let hint = state.status.as_deref().unwrap_or(
        "Tab next field  •  Enter save  •  Esc cancel  •  the URL is saved for next launch",
    );
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(if state.status.is_some() {
            Color::Yellow
        } else {
            Color::DarkGray
        })),
        rows[4],
    );
}

fn render_field(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: &str,
    focused: bool,
    border_color: Color,
) {
    let border = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(border_color)
    };
    frame.render_widget(
        Paragraph::new(if value.is_empty() { " " } else { value })
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title(title),
            ),
        area,
    );
    if focused {
        let offset = value
            .chars()
            .count()
            .min(area.width.saturating_sub(3) as usize) as u16;
        frame.set_cursor_position(Position::new(
            area.x.saturating_add(1).saturating_add(offset),
            area.y.saturating_add(1),
        ));
    }
}

fn centered_modal(area: Rect, maximum_width: u16, maximum_height: u16) -> Rect {
    let width = area.width.min(maximum_width);
    let height = area.height.min(maximum_height);
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [v] = vertical.areas(area);
    let [h] = horizontal.areas(v);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hub_seeds_from_defaults_and_focuses_url() {
        let hub = LmStudioHub::empty();
        assert_eq!(hub.url_buf, "");
        assert_eq!(hub.model_buf, "");
        assert_eq!(hub.focused, Field::Url);
        assert!(hub.status.is_none());
    }

    #[test]
    fn from_settings_seeds_existing_values() {
        let existing = LmStudioSettings {
            api_base_url: "http://192.168.254.114:1234/v1".into(),
            model: Some("qwen3.6-27b".into()),
        };
        let hub = LmStudioHub::from_settings(&existing);
        assert_eq!(hub.url_buf, "http://192.168.254.114:1234/v1");
        assert_eq!(hub.model_buf, "qwen3.6-27b");
    }

    #[test]
    fn insert_and_backspace_edit_the_focused_field() {
        let mut hub = LmStudioHub::empty();
        hub.insert('h');
        hub.insert('i');
        assert_eq!(hub.url_buf, "hi");
        // Tab to model; edits land there.
        hub.next_field();
        hub.insert('q');
        assert_eq!(hub.url_buf, "hi", "url unchanged after focus moves");
        assert_eq!(hub.model_buf, "q");
        hub.backspace();
        assert_eq!(hub.model_buf, "");
    }

    #[test]
    fn submit_rejects_empty_url() {
        let mut hub = LmStudioHub::empty();
        assert_eq!(hub.submit(), LmStudioSettingsAction::Continue);
        assert!(hub.status.as_deref().unwrap().contains("base URL"));
    }

    #[test]
    fn submit_rejects_non_http_url() {
        let mut hub = LmStudioHub::empty();
        for c in "192.168.1.5:1234/v1".chars() {
            hub.insert(c);
        }
        assert_eq!(hub.submit(), LmStudioSettingsAction::Continue);
        assert!(hub.status.as_deref().unwrap().contains("http"));
    }

    #[test]
    fn submit_saves_a_valid_lan_url_and_blank_model_as_none() {
        let mut hub = LmStudioHub::empty();
        for c in "http://192.168.254.114:1234/v1".chars() {
            hub.insert(c);
        }
        let action = hub.submit();
        let LmStudioSettingsAction::Save { settings } = action else {
            panic!("expected Save, got {action:?}");
        };
        assert_eq!(settings.api_base_url, "http://192.168.254.114:1234/v1");
        assert_eq!(settings.model, None);
        assert!(hub.saving);
    }

    #[test]
    fn submit_saves_an_optional_model_when_set() {
        let mut hub = LmStudioHub::empty();
        for c in "http://localhost:1234/api/v0".chars() {
            hub.insert(c);
        }
        hub.next_field();
        for c in "phi-4".chars() {
            hub.insert(c);
        }
        let LmStudioSettingsAction::Save { settings } = hub.submit() else {
            panic!("expected Save");
        };
        assert_eq!(settings.api_base_url, "http://localhost:1234/api/v0");
        assert_eq!(settings.model.as_deref(), Some("phi-4"));
    }

    #[test]
    fn cancel_quits_unless_saving() {
        let mut hub = LmStudioHub::empty();
        assert_eq!(hub.cancel(), LmStudioSettingsAction::Quit);
        // While saving, cancel is ignored.
        for c in "http://x:1/v1".chars() {
            hub.insert(c);
        }
        let _ = hub.submit();
        assert_eq!(hub.cancel(), LmStudioSettingsAction::Continue);
    }

    #[test]
    fn save_failed_clears_saving_and_records_status() {
        let mut hub = LmStudioHub::empty();
        for c in "http://x:1/v1".chars() {
            hub.insert(c);
        }
        let _ = hub.submit();
        assert!(hub.saving);
        hub.save_failed("disk full".into());
        assert!(!hub.saving);
        assert_eq!(hub.status.as_deref(), Some("disk full"));
    }

    #[test]
    fn paste_drops_control_characters_and_is_bounded() {
        let mut hub = LmStudioHub::empty();
        hub.paste("http://\u{1b}x:1/v1");
        // ESC (\x1b) dropped; url kept.
        assert_eq!(hub.url_buf, "http://x:1/v1");
    }

    #[test]
    fn settings_is_empty_and_model_helpers() {
        assert!(LmStudioSettings::default().is_empty());
        assert_eq!(LmStudioSettings::default().model(), None);
        let s = LmStudioSettings {
            api_base_url: "http://x:1/v1".into(),
            model: Some("   ".into()),
        };
        // Blank model string is treated as unset.
        assert_eq!(s.model(), None);
    }

    #[test]
    fn save_and_load_round_trip_in_a_temp_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let settings = LmStudioSettings {
            api_base_url: "http://192.168.1.50:1234/v1".into(),
            model: Some("qwen3.6-27b".into()),
        };
        save_lmstudio_settings_to(tmp.path(), &settings).expect("save");
        let loaded = load_lmstudio_settings_from(tmp.path());
        assert_eq!(loaded, settings);
        assert!(!settings.api_base_url.is_empty());
    }

    #[test]
    fn load_returns_defaults_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let loaded = load_lmstudio_settings_from(tmp.path());
        assert!(loaded.is_empty());
    }
}
