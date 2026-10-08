//! Descriptor-driven Settings authentication. Provider names are never branched on.
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Frame, Terminal, backend::Backend};
use vesper_provider::{
    CredentialError, CredentialRemovalScope, CredentialSource, InteractiveLoginKind,
    ProviderDescriptor,
};
use vesper_runtime::{ProviderRegistry, RuntimeCancellation};
use zeroize::Zeroizing;

use crate::provider_hub::{ProviderHub, render as render_providers};

/// Blocking settings input. Production reads the terminal; tests supply events.
pub trait SettingsEvents {
    fn next_event(&mut self) -> Result<Event, String>;
    fn poll_event(&mut self, timeout: Duration) -> Result<Option<Event>, String>;
}

/// Crossterm-backed input used by the TUI process.
#[derive(Default)]
pub struct LiveSettingsEvents {
    event_types: bool,
}

impl LiveSettingsEvents {
    fn ensure_event_types(&mut self) {
        if self.event_types {
            return;
        }
        // Konsole repeats a held arrow as further keypresses unless the
        // terminal reports press/repeat/release separately. Ask for that
        // report only while this settings reader is alive.
        if crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )
        .is_ok()
        {
            self.event_types = true;
        }
    }
}

impl Drop for LiveSettingsEvents {
    fn drop(&mut self) {
        if self.event_types {
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::event::PopKeyboardEnhancementFlags
            );
        }
    }
}

impl SettingsEvents for LiveSettingsEvents {
    fn next_event(&mut self) -> Result<Event, String> {
        self.ensure_event_types();
        crossterm::event::read().map_err(|error| error.to_string())
    }
    fn poll_event(&mut self, timeout: Duration) -> Result<Option<Event>, String> {
        if crossterm::event::poll(timeout).map_err(|error| error.to_string())? {
            self.next_event().map(Some)
        } else {
            Ok(None)
        }
    }
}

type UrlHook = Box<dyn FnMut(&str) -> Result<(), String> + Send>;

/// Browser and clipboard hooks. Tests replace both so no user credential store,
/// browser, or clipboard is touched.
pub struct AuthUiHooks {
    pub open_url: UrlHook,
    pub copy_url: UrlHook,
}

impl Default for AuthUiHooks {
    fn default() -> Self {
        Self {
            open_url: Box::new(open_https_url),
            copy_url: Box::new(copy_text),
        }
    }
}

/// Result of the provider Settings screen. Authentication commits immediately
/// and is not rolled back by Discard changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSettingsOutcome {
    pub save_provider: Option<String>,
    pub notice: String,
    pub authentication_committed: bool,
    pub committed_provider: Option<String>,
    pub committed_method: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthPanelResult {
    pub committed: bool,
    pub method: Option<String>,
    pub notice: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelAction {
    Method(usize),
    Use(usize),
    Browser(usize),
    Device(usize),
    EditKey(usize),
    OpenUrl(usize),
    RemoveKey(usize),
    SignOut,
    SaveKey,
    Confirm,
    Back,
}

struct PanelState {
    provider_id: String,
    provider_name: String,
    descriptor: ProviderDescriptor,
    inventory: vesper_provider::AuthenticationInventory,
    selected: usize,
    method_focus: usize,
    screen: Screen,
    secret: Zeroizing<String>,
    notice: String,
    busy: bool,
    generation: u64,
    committed: bool,
    removal_scope: CredentialRemovalScope,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Overview,
    Key,
    ConfirmSignOut,
    ConfirmRemove,
    ConfirmUse,
}

pub async fn run_provider_settings<E: SettingsEvents>(
    terminal: &mut Terminal<impl Backend>,
    registry: &ProviderRegistry,
    current: &str,
    theme: &str,
    events: &mut E,
    hooks: &mut AuthUiHooks,
) -> Result<ProviderSettingsOutcome, String> {
    let providers = registry.provider_ids().await;
    if providers.is_empty() {
        return Err("No providers are registered.".into());
    }
    let mut hub = ProviderHub::new(
        providers.iter().map(|id| id.as_str().to_owned()).collect(),
        current.to_owned(),
    );
    let mut outcome = ProviderSettingsOutcome {
        save_provider: None,
        notice: "Provider selection cancelled.".into(),
        authentication_committed: false,
        committed_provider: None,
        committed_method: None,
    };
    loop {
        terminal
            .draw(|frame| render_providers(frame, &hub, theme))
            .map_err(|error| format!("provider settings redraw: {error}"))?;
        let (code, clicked) = read_menu(events, terminal, hub.row_count(), hub.selected)?;
        if let Some(index) = clicked {
            hub.selected = index;
        }
        match code {
            KeyCode::Esc => return Ok(outcome),
            KeyCode::Up => hub.selected = hub.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                hub.selected = (hub.selected + 1).min(hub.row_count().saturating_sub(1))
            }
            KeyCode::Char('s' | 'S') => {
                outcome.save_provider = hub.choice().map(str::to_owned);
                outcome.notice = "Provider saved for the next launch.".into();
                return Ok(outcome);
            }
            KeyCode::Char('m' | 'M') => {
                let Some(provider_id) = hub.authentication_provider().map(str::to_owned) else {
                    continue;
                };
                apply_authentication(
                    &mut outcome,
                    terminal,
                    registry,
                    &provider_id,
                    theme,
                    events,
                    hooks,
                )
                .await?;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if hub.selected == hub.cancel_row() {
                    return Ok(outcome);
                }
                if hub.selected == hub.save_row() {
                    outcome.save_provider = hub.choice().map(str::to_owned);
                    outcome.notice = "Provider saved for the next launch.".into();
                    return Ok(outcome);
                }
                if hub.is_manage_row() {
                    let Some(provider_id) = hub.authentication_provider().map(str::to_owned) else {
                        continue;
                    };
                    apply_authentication(
                        &mut outcome,
                        terminal,
                        registry,
                        &provider_id,
                        theme,
                        events,
                        hooks,
                    )
                    .await?;
                    continue;
                }
                hub.choose();
            }
            _ => {}
        }
    }
}

async fn apply_authentication<E: SettingsEvents>(
    outcome: &mut ProviderSettingsOutcome,
    terminal: &mut Terminal<impl Backend>,
    registry: &ProviderRegistry,
    provider_id: &str,
    theme: &str,
    events: &mut E,
    hooks: &mut AuthUiHooks,
) -> Result<(), String> {
    let result =
        run_authentication_panel(terminal, registry, provider_id, theme, events, hooks, true)
            .await?;
    if result.committed {
        outcome.authentication_committed = true;
        outcome.committed_provider = Some(provider_id.to_owned());
        outcome.committed_method = result.method;
    }
    outcome.notice = result.notice;
    Ok(())
}

pub async fn run_authentication_panel<E: SettingsEvents>(
    terminal: &mut Terminal<impl Backend>,
    registry: &ProviderRegistry,
    provider_id: &str,
    theme: &str,
    events: &mut E,
    hooks: &mut AuthUiHooks,
    cancel_returns: bool,
) -> Result<AuthPanelResult, String> {
    let id = vesper_domain::ProviderId::new(provider_id)
        .map_err(|error| format!("invalid provider id: {error}"))?;
    let Some(descriptor) = registry.descriptor(&id).await else {
        return Err("That provider is no longer registered.".into());
    };
    if descriptor.authentication_methods.is_empty() {
        return Ok(AuthPanelResult {
            committed: false,
            method: None,
            notice: format!(
                "{} does not advertise an authentication method. Anonymous access is not assumed.",
                descriptor.display_name.as_str()
            ),
        });
    }
    let Some(port) = registry.credential_port(&id).await else {
        return Err("Provider authentication storage is unavailable.".into());
    };
    let inventory = spawn_inventory(port.clone()).await?;
    let removal_scope = port.removal_scope();
    let mut state = PanelState {
        provider_id: provider_id.to_owned(),
        provider_name: descriptor.display_name.as_str().to_owned(),
        method_focus: inventory
            .selected_method
            .as_ref()
            .and_then(|selected| {
                descriptor
                    .authentication_methods
                    .iter()
                    .position(|method| method.method_id.as_str() == selected)
            })
            .unwrap_or(0),
        descriptor,
        inventory,
        selected: 0,
        screen: Screen::Overview,
        secret: Zeroizing::new(String::new()),
        notice: "Nothing is saved until you confirm a key, sign-in, selection, or sign-out. Confirmed authentication is immediate and is not part of Settings Save/Discard.".into(),
        busy: false,
        generation: 0,
        committed: false,
        removal_scope,
    };
    state.selected = state.method_focus;
    let port = port;
    loop {
        let actions = actions(&state);
        if state.selected >= actions.len() {
            state.selected = actions.len().saturating_sub(1);
        }
        terminal
            .draw(|frame| render_panel(frame, &state, &actions, theme))
            .map_err(|error| format!("authentication redraw: {error}"))?;
        let (code, clicked, text) = read_auth(events, terminal, actions.len(), state.selected)?;
        if let Some(index) = clicked {
            state.selected = index;
        }
        if text == Some('\u{0}') {
            state.notice = "Control characters are rejected. The key was not changed.".into();
            continue;
        }
        if text == Some('\u{1}') && state.screen == Screen::Key {
            for character in take_paste_rest().chars() {
                insert_secret(&mut state, character);
            }
            continue;
        }
        if state.screen == Screen::Key
            && let KeyCode::Char(character) = code
        {
            insert_secret(&mut state, character);
            continue;
        }
        match code {
            KeyCode::Up => state.selected = state.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                state.selected = (state.selected + 1).min(actions.len().saturating_sub(1))
            }
            KeyCode::Backspace if state.screen == Screen::Key => {
                state.secret.pop();
            }
            KeyCode::Esc | KeyCode::Char('b' | 'B')
                if !state.busy && state.screen != Screen::Overview =>
            {
                state.secret.clear();
                state.screen = Screen::Overview;
            }
            KeyCode::Esc if !state.busy => {
                if cancel_returns {
                    return Ok(finish(&state));
                }
                return Err("authentication cancelled; a provider credential is required".into());
            }
            KeyCode::Enter | KeyCode::Char(' ') if !state.busy => {
                let Some(action) = actions.get(state.selected).copied() else {
                    continue;
                };
                match action {
                    PanelAction::Method(index) => state.method_focus = index,
                    PanelAction::Back => {
                        if state.screen == Screen::Overview {
                            if cancel_returns {
                                return Ok(finish(&state));
                            }
                            return Err(
                                "authentication cancelled; a provider credential is required"
                                    .into(),
                            );
                        }
                        state.secret.clear();
                        state.screen = Screen::Overview;
                    }
                    PanelAction::EditKey(index) => {
                        state.method_focus = index;
                        state.secret.clear();
                        state.screen = Screen::Key;
                        state.selected = 0;
                        state.notice =
                            "The field is masked. The stored secret is never shown.".into();
                    }
                    PanelAction::Use(index) => {
                        state.method_focus = index;
                        state.screen = Screen::ConfirmUse;
                        state.selected = 0;
                        state.notice = format!(
                            "Use {} for the next request? This does not delete the other method.",
                            state.descriptor.authentication_methods[index]
                                .display_name
                                .as_str()
                        );
                    }
                    PanelAction::Confirm if state.screen == Screen::ConfirmUse => {
                        let method_id = state.descriptor.authentication_methods[state.method_focus]
                            .method_id
                            .as_str()
                            .to_owned();
                        match spawn_select(port.clone(), method_id).await {
                            Ok(()) => {
                                state.committed = true;
                                state.notice = "Selected method saved immediately. It is not undone by Discard changes.".into();
                                refresh(&mut state, port.clone()).await?;
                                state.screen = Screen::Overview;
                            }
                            Err(error) => state.notice = credential_notice(&error),
                        }
                    }
                    PanelAction::SaveKey => {
                        if let Err(message) = validate_new_secret(&state.secret) {
                            state.notice = message;
                            continue;
                        }
                        let method_id = state.descriptor.authentication_methods[state.method_focus]
                            .method_id
                            .as_str()
                            .to_owned();
                        let secret = state.secret.to_string();
                        state.busy = true;
                        let saved = spawn_store(port.clone(), method_id, secret).await;
                        state.busy = false;
                        state.secret.clear();
                        match saved {
                            Ok(()) => {
                                state.committed = true;
                                state.notice = "API key saved and selected immediately. Discard changes will not roll it back.".into();
                                refresh(&mut state, port.clone()).await?;
                                state.screen = Screen::Overview;
                            }
                            Err(error) => state.notice = credential_notice(&error),
                        }
                    }
                    PanelAction::Browser(index) | PanelAction::Device(index) => {
                        state.method_focus = index;
                        let method = &state.descriptor.authentication_methods[index];
                        let kind = match action {
                            PanelAction::Browser(_) => InteractiveLoginKind::Browser,
                            _ => InteractiveLoginKind::DeviceCode,
                        };
                        if !method.interactive_login.contains(&kind) {
                            state.notice =
                                "That sign-in flow is not advertised for this method.".into();
                            continue;
                        }
                        let method_id = method.method_id.as_str().to_owned();
                        state.generation = state.generation.saturating_add(1);
                        let generation = state.generation;
                        let provider_id = state.provider_id.clone();
                        login(
                            &mut state,
                            terminal,
                            events,
                            hooks,
                            port.clone(),
                            LoginTarget {
                                kind,
                                generation,
                                provider_id: &provider_id,
                                method_id: &method_id,
                                theme,
                            },
                        )
                        .await?;
                        refresh(&mut state, port.clone()).await?;
                    }
                    PanelAction::OpenUrl(index) => {
                        let Some(url) = state.descriptor.authentication_methods[index]
                            .key_url
                            .as_ref()
                            .map(|url| url.as_str().to_owned())
                        else {
                            continue;
                        };
                        state.notice = match (hooks.open_url)(&url) {
                            Ok(()) => "Opened the provider key page.".into(),
                            Err(_) => {
                                "Could not open a browser. The key page was not changed.".into()
                            }
                        };
                    }
                    PanelAction::RemoveKey(_) => {
                        state.screen = Screen::ConfirmRemove;
                        state.selected = 0;
                        state.notice = format!(
                            "Remove the stored API key for {}? An environment variable, if set, is not removed or replaced.",
                            state.provider_name
                        );
                    }
                    PanelAction::Confirm if state.screen == Screen::ConfirmRemove => {
                        let method_id = state.descriptor.authentication_methods[state.method_focus]
                            .method_id
                            .as_str()
                            .to_owned();
                        match spawn_clear(port.clone(), method_id).await {
                            Ok(()) => {
                                state.committed = true;
                                refresh(&mut state, port.clone()).await?;
                                state.notice = removal_notice(&state);
                                state.screen = Screen::Overview;
                            }
                            Err(error) => state.notice = credential_notice(&error),
                        }
                    }
                    PanelAction::SignOut => {
                        if state.removal_scope != CredentialRemovalScope::EntireProvider {
                            state.notice =
                                "This provider cannot remove a stored credential.".into();
                            continue;
                        }
                        state.screen = Screen::ConfirmSignOut;
                        state.selected = 0;
                        state.notice = format!(
                            "Sign out removes all Vesper-stored credentials for {}. Environment variables and other providers are not changed. This is immediate.",
                            state.provider_name
                        );
                    }
                    PanelAction::Confirm if state.screen == Screen::ConfirmSignOut => {
                        match spawn_logout(port.clone()).await {
                            Ok(()) => {
                                state.committed = true;
                                refresh(&mut state, port.clone()).await?;
                                state.notice = removal_notice(&state);
                                state.screen = Screen::Overview;
                            }
                            Err(error) => {
                                state.notice = credential_notice(&error);
                                state.screen = Screen::Overview;
                            }
                        }
                    }
                    PanelAction::Confirm => {}
                }
            }
            _ => {}
        }
    }
}

struct LoginTarget<'a> {
    kind: InteractiveLoginKind,
    generation: u64,
    provider_id: &'a str,
    method_id: &'a str,
    theme: &'a str,
}

async fn login<E: SettingsEvents>(
    state: &mut PanelState,
    terminal: &mut Terminal<impl Backend>,
    events: &mut E,
    hooks: &mut AuthUiHooks,
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
    target: LoginTarget<'_>,
) -> Result<(), String> {
    state.busy = true;
    let LoginTarget {
        kind,
        generation,
        provider_id,
        method_id,
        theme,
    } = target;
    state.notice = match kind {
        InteractiveLoginKind::Browser => "Requesting browser sign-in… Esc cancels.".into(),
        InteractiveLoginKind::DeviceCode => "Requesting device-code sign-in… Esc cancels.".into(),
    };
    let cancel = Arc::new(RuntimeCancellation::new());
    let task_cancel = cancel.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, Option<String>)>(1);
    let mut task = tokio::spawn(async move {
        match kind {
            InteractiveLoginKind::Browser => {
                port.browser_login(
                    task_cancel,
                    Arc::new(move |url| {
                        let _ = tx.try_send((url, None));
                    }),
                )
                .await
            }
            InteractiveLoginKind::DeviceCode => {
                port.device_login(
                    task_cancel,
                    Arc::new(move |url, code| {
                        let _ = tx.try_send((url, Some(code)));
                    }),
                )
                .await
            }
        }
    });
    let mut link: Option<SignInLink> = None;
    let mut device_code: Option<String> = None;
    loop {
        if let Ok((url, code)) = rx.try_recv() {
            match SignInLink::new(url) {
                Ok(parsed) => {
                    device_code = code;
                    state.notice = match (hooks.open_url)(parsed.as_str()) {
                        Ok(()) => "Browser launch requested. Complete sign-in in the browser.".into(),
                        Err(_) => "Browser launch failed. Press Enter to retry or C to copy the complete link.".into(),
                    };
                    link = Some(parsed);
                }
                Err(error) => {
                    cancel.cancel();
                    state.notice = error;
                }
            }
        }
        terminal
            .draw(|frame| {
                let notice = if let Some(code) = &device_code {
                    format!(
                        "{}\nVerification link: {}\nOne-time code: {code}\nEnter retries the browser. C copies the complete link. Esc cancels.",
                        state.notice,
                        link.as_ref().map_or("", SignInLink::as_str),
                    )
                } else if link.is_some() {
                    format!(
                        "{}\nEnter opens the browser again. C copies the complete link. D uses device code when advertised. Esc cancels.",
                        state.notice
                    )
                } else {
                    state.notice.clone()
                };
                crate::settings_menu::render_menu(
                    frame,
                    &["Waiting for sign-in".into(), "Back".into()],
                    0,
                    &format!("Settings › Providers › {} › Authentication", state.provider_name),
                    &notice,
                    "Esc cancels · the complete link is never reconstructed from wrapped text",
                    theme,
                );
            })
            .map_err(|error| {
                cancel.cancel();
                error.to_string()
            })?;
        if task.is_finished() {
            let result = task
                .await
                .map_err(|_| "Sign-in task stopped.".to_string())?;
            state.busy = false;
            if state.generation != generation || state.provider_id != provider_id {
                state.notice = format!(
                    "A sign-in for {provider_id} settled after the panel changed. Its result was not applied here."
                );
                return Ok(());
            }
            match result {
                Ok(()) => {
                    state.committed = true;
                    state.notice =
                        format!("Sign-in for {method_id} completed and was saved immediately.");
                }
                Err(error) => state.notice = credential_notice(&error),
            }
            return Ok(());
        }
        if let Some(event) = events
            .poll_event(Duration::from_millis(50))
            .inspect_err(|_| cancel.cancel())?
        {
            let code = match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => key.code,
                _ => continue,
            };
            if let Some(parsed) = &link {
                match code {
                    KeyCode::Enter => {
                        state.notice = match (hooks.open_url)(parsed.as_str()) {
                            Ok(()) => "Opened the complete sign-in link.".into(),
                            Err(_) => {
                                "Browser launch failed. Press C to copy the complete link.".into()
                            }
                        };
                        continue;
                    }
                    KeyCode::Char('c' | 'C') => {
                        state.notice = match (hooks.copy_url)(parsed.as_str()) {
                            Ok(()) => "Copied the complete sign-in link.".into(),
                            Err(error) => format!("Could not copy the complete link: {error}"),
                        };
                        continue;
                    }
                    KeyCode::Char('d' | 'D')
                        if kind == InteractiveLoginKind::Browser
                            && state.descriptor.authentication_methods[state.method_focus]
                                .interactive_login
                                .contains(&InteractiveLoginKind::DeviceCode) =>
                    {
                        cancel.cancel();
                        let _ = tokio::time::timeout(Duration::from_secs(1), &mut task).await;
                        state.busy = false;
                        state.notice =
                            "Device-code sign-in was requested. Choose Device-code sign-in.".into();
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if matches!(code, KeyCode::Esc) {
                cancel.cancel();
                let settled = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
                state.busy = false;
                if let Ok(Ok(Ok(()))) = settled {
                    state.committed = true;
                    state.notice =
                        "Sign-in had already reached its save and completed. It was not discarded."
                            .into();
                } else {
                    state.notice = "Sign-in cancelled. The previous credential was kept.".into();
                }
                return Ok(());
            }
        }
    }
}

fn actions(state: &PanelState) -> Vec<PanelAction> {
    if state.screen == Screen::Key {
        let mut rows = vec![PanelAction::SaveKey];
        if state.descriptor.authentication_methods[state.method_focus]
            .key_url
            .is_some()
        {
            rows.push(PanelAction::OpenUrl(state.method_focus));
        }
        if source(state, state.method_focus) == Some(CredentialSource::Stored) {
            rows.push(PanelAction::RemoveKey(state.method_focus));
        }
        rows.push(PanelAction::Back);
        return rows;
    }
    if matches!(
        state.screen,
        Screen::ConfirmSignOut | Screen::ConfirmRemove | Screen::ConfirmUse
    ) {
        return vec![PanelAction::Confirm, PanelAction::Back];
    }
    let mut rows: Vec<PanelAction> = (0..state.descriptor.authentication_methods.len())
        .map(PanelAction::Method)
        .collect();
    let index = state.method_focus.min(
        state
            .descriptor
            .authentication_methods
            .len()
            .saturating_sub(1),
    );
    let method = &state.descriptor.authentication_methods[index];
    let available = matches!(
        source(state, index),
        Some(CredentialSource::Stored | CredentialSource::Environment | CredentialSource::Expired)
    );
    if available && state.inventory.selected_method.as_deref() != Some(method.method_id.as_str()) {
        rows.push(PanelAction::Use(index));
    }
    if method
        .interactive_login
        .contains(&InteractiveLoginKind::Browser)
    {
        rows.push(PanelAction::Browser(index));
    }
    if method
        .interactive_login
        .contains(&InteractiveLoginKind::DeviceCode)
    {
        rows.push(PanelAction::Device(index));
    }
    if !method.secret_reference_fields.is_empty() {
        rows.push(PanelAction::EditKey(index));
    }
    if method.key_url.is_some() {
        rows.push(PanelAction::OpenUrl(index));
    }
    if source(state, index) == Some(CredentialSource::Stored)
        && !method.secret_reference_fields.is_empty()
    {
        rows.push(PanelAction::RemoveKey(index));
    }
    if state.removal_scope == CredentialRemovalScope::EntireProvider
        && state.inventory.methods.iter().any(|method| {
            matches!(
                method.source,
                CredentialSource::Stored | CredentialSource::Expired
            )
        })
    {
        rows.push(PanelAction::SignOut);
    }
    rows.push(PanelAction::Back);
    rows
}

fn source(state: &PanelState, index: usize) -> Option<CredentialSource> {
    let id = state
        .descriptor
        .authentication_methods
        .get(index)?
        .method_id
        .as_str();
    state
        .inventory
        .methods
        .iter()
        .find(|method| method.method_id == id)
        .map(|method| method.source)
}

fn render_panel(frame: &mut Frame<'_>, state: &PanelState, actions: &[PanelAction], theme: &str) {
    let labels: Vec<String> = actions.iter().map(|action| label(state, *action)).collect();
    let title = format!(
        "Settings › Providers › {} › Authentication",
        state.provider_name
    );
    crate::settings_menu::render_menu(
        frame,
        &labels,
        state.selected,
        &title,
        &detail(state),
        "↑↓ select · Enter confirm · Esc back · secrets stay masked",
        theme,
    );
}

fn label(state: &PanelState, action: PanelAction) -> String {
    match action {
        PanelAction::Method(index) => {
            let method = &state.descriptor.authentication_methods[index];
            let mark =
                if state.inventory.selected_method.as_deref() == Some(method.method_id.as_str()) {
                    "●"
                } else {
                    " "
                };
            format!("[{mark}] {}", method.display_name.as_str())
        }
        PanelAction::Use(_) => "Use this method".into(),
        PanelAction::Browser(_) => "Browser sign-in / Reauthenticate".into(),
        PanelAction::Device(_) => "Device-code sign-in".into(),
        PanelAction::EditKey(_) => "Enter or replace API key".into(),
        PanelAction::OpenUrl(_) => "Open provider key-management page".into(),
        PanelAction::RemoveKey(_) => "Remove stored API key…".into(),
        PanelAction::SignOut => "Sign out…".into(),
        PanelAction::SaveKey => "Save and use API key".into(),
        PanelAction::Confirm => "Confirm".into(),
        PanelAction::Back => "Back".into(),
    }
}

fn detail(state: &PanelState) -> String {
    let method = &state.descriptor.authentication_methods[state.method_focus];
    let status = status_line(state, state.method_focus);
    let mask = if state.screen == Screen::Key {
        format!(
            "\nNew API key: {}",
            "*".repeat(state.secret.chars().count())
        )
    } else {
        String::new()
    };
    format!(
        "In use: {}\n{status}\nBilling: {}{mask}\n{}",
        state
            .inventory
            .selected_method
            .as_deref()
            .unwrap_or("not selected"),
        method.display_name.as_str(),
        state.notice
    )
}

fn status_line(state: &PanelState, index: usize) -> String {
    let method = &state.descriptor.authentication_methods[index];
    if state.busy {
        return "Credential: Checking".into();
    }
    match source(state, index) {
        Some(CredentialSource::Stored) => "Credential: Configured locally".into(),
        Some(CredentialSource::Environment) => {
            "Credential: Environment-managed. Removing a stored record does not remove it.".into()
        }
        Some(CredentialSource::Expired) => "Credential: Expired/reauthentication required".into(),
        Some(CredentialSource::Absent) | None if method.optional => {
            "Credential: No authentication required for this configuration".into()
        }
        Some(CredentialSource::Absent) | None => "Credential: Not configured".into(),
    }
}

fn removal_notice(state: &PanelState) -> String {
    if state
        .inventory
        .methods
        .iter()
        .any(|method| method.source == CredentialSource::Environment)
    {
        "Stored credentials were removed. An environment-managed credential remains and was not described as removed.".into()
    } else {
        "Vesper-stored credentials for this provider were removed. Other providers were not changed.".into()
    }
}

fn finish(state: &PanelState) -> AuthPanelResult {
    AuthPanelResult {
        committed: state.committed,
        method: state.inventory.selected_method.clone(),
        notice: if state.committed {
            format!(
                "Authentication for {} was saved immediately. Discard changes will not undo it. {}",
                state.provider_name, state.notice
            )
        } else {
            state.notice.clone()
        },
    }
}

async fn refresh(
    state: &mut PanelState,
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
) -> Result<(), String> {
    state.inventory = spawn_inventory(port).await?;
    Ok(())
}

fn validate_new_secret(secret: &str) -> Result<(), String> {
    if secret.chars().any(char::is_control) {
        return Err("The API key contains a control character and was not modified.".into());
    }
    vesper_auth::validate_secret(secret)
        .map(|_| ())
        .map_err(|_| "Enter a non-empty API key without control characters.".into())
}

fn insert_secret(state: &mut PanelState, character: char) {
    if state.screen != Screen::Key || state.busy {
        return;
    }
    if character.is_control() {
        state.notice = "Control characters are rejected. The key was not changed.".into();
        return;
    }
    if state.secret.len() < 16 * 1024 {
        state.secret.push(character);
    }
}

fn credential_notice(error: &CredentialError) -> String {
    match error {
        CredentialError::Absent => {
            "That method has no stored credential to select. The previous mode was kept.".into()
        }
        CredentialError::InvalidSecret => {
            "The API key was rejected before it was saved. The previous credential was kept.".into()
        }
        CredentialError::Unavailable => "Credential storage could not be confirmed.".into(),
        CredentialError::Failed => {
            "The credential operation failed. Its saved state could not be confirmed.".into()
        }
    }
}

fn spawn_inventory(
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
) -> Pin<Box<dyn Future<Output = Result<vesper_provider::AuthenticationInventory, String>> + Send>>
{
    Box::pin(async move {
        tokio::task::spawn_blocking(move || port.authentication_inventory())
            .await
            .map_err(|_| "Authentication status task failed.".to_string())?
            .map_err(|_| "Authentication status is unavailable.".to_string())
    })
}

async fn spawn_store(
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
    method_id: String,
    secret: String,
) -> Result<(), CredentialError> {
    tokio::task::spawn_blocking(
        move || match port.store_method_credential(&method_id, &secret) {
            Err(CredentialError::Unavailable) => port.store_credential(&secret),
            other => other,
        },
    )
    .await
    .map_err(|_| CredentialError::Failed)?
}

async fn spawn_select(
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
    method_id: String,
) -> Result<(), CredentialError> {
    tokio::task::spawn_blocking(move || port.select_authentication_method(&method_id))
        .await
        .map_err(|_| CredentialError::Failed)?
}

async fn spawn_clear(
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
    method_id: String,
) -> Result<(), CredentialError> {
    tokio::task::spawn_blocking(move || port.clear_stored_method(&method_id))
        .await
        .map_err(|_| CredentialError::Failed)?
}

async fn spawn_logout(
    port: Arc<dyn vesper_provider::ProviderCredentialPort>,
) -> Result<(), CredentialError> {
    tokio::task::spawn_blocking(move || port.logout())
        .await
        .map_err(|_| CredentialError::Failed)?
}

fn trace_nav(kind: KeyEventKind, code: KeyCode) {
    let Ok(path) = std::env::var("AGENT_VESPER_NAV_TRACE") else {
        return;
    };
    if path.is_empty() || path == "0" {
        return;
    }
    // Navigation only. Never log typed secrets or sign-in URLs.
    if matches!(code, KeyCode::Char(_) | KeyCode::Backspace) {
        return;
    }
    let kind = match kind {
        KeyEventKind::Press => "press",
        KeyEventKind::Repeat => "repeat",
        KeyEventKind::Release => "release",
    };
    let line = format!("nav {kind} {code:?}\n");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

fn read_menu<E: SettingsEvents>(
    events: &mut E,
    terminal: &Terminal<impl Backend>,
    count: usize,
    selected: usize,
) -> Result<(KeyCode, Option<usize>), String> {
    let (code, clicked, _) = read_auth(events, terminal, count, selected)?;
    Ok((code, clicked))
}

fn read_auth<E: SettingsEvents>(
    events: &mut E,
    terminal: &Terminal<impl Backend>,
    count: usize,
    selected: usize,
) -> Result<(KeyCode, Option<usize>, Option<char>), String> {
    loop {
        match events.next_event()? {
            Event::Paste(value) => {
                if value.chars().any(char::is_control) {
                    stash_paste("");
                    return Ok((KeyCode::Null, None, Some('\u{0}')));
                }
                stash_paste(&value);
                return Ok((KeyCode::Null, None, Some('\u{1}')));
            }
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) => {
                // Release is never a new command. Repeat must not walk the
                // menu: Konsole's repeat delay is 500ms and then 30ms, so one
                // held Down otherwise leaves the action under the cursor.
                // Character repeat still edits an API-key field.
                if kind == KeyEventKind::Release
                    || (kind == KeyEventKind::Repeat
                        && !matches!(code, KeyCode::Char(_) | KeyCode::Backspace))
                {
                    continue;
                }
                if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok((KeyCode::Esc, None, None));
                }
                trace_nav(kind, code);
                return Ok((code, None, None));
            }
            Event::Mouse(mouse) => {
                use crossterm::event::{MouseButton, MouseEventKind};
                match mouse.kind {
                    MouseEventKind::ScrollUp => return Ok((KeyCode::Up, None, None)),
                    MouseEventKind::ScrollDown => return Ok((KeyCode::Down, None, None)),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let size = terminal.size().map_err(|error| error.to_string())?;
                        if let Some(index) = crate::settings_menu::item_at(
                            ratatui::layout::Rect::new(0, 0, size.width, size.height),
                            count,
                            selected,
                            mouse.column,
                            mouse.row,
                        ) {
                            return Ok((KeyCode::Enter, Some(index), None));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

std::thread_local! {
    static PASTE_REST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn stash_paste(rest: &str) {
    PASTE_REST.with(|slot| *slot.borrow_mut() = rest.to_owned());
}

fn take_paste_rest() -> String {
    PASTE_REST.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

struct SignInLink(String);

impl SignInLink {
    fn new(value: String) -> Result<Self, String> {
        if value.chars().any(char::is_control) {
            return Err("Authentication service returned an invalid sign-in link".into());
        }
        let parsed = reqwest::Url::parse(&value)
            .map_err(|_| "Authentication service returned an invalid sign-in link".to_string())?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
        {
            return Err("Authentication service returned an untrusted sign-in link".into());
        }
        Ok(Self(value))
    }
    fn as_str(&self) -> &str {
        &self.0
    }
}

fn open_https_url(url: &str) -> Result<(), String> {
    let link = SignInLink::new(url.to_owned())?;
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        std::process::Command::new("explorer.exe")
    } else {
        std::process::Command::new("xdg-open")
    };
    command
        .arg(link.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn copy_text(value: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_text(value.to_owned())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use ratatui::backend::TestBackend;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;
    use vesper_domain::{BoundedString, ExtensionMap, ProviderId};
    use vesper_provider::*;

    #[test]
    fn credential_failure_notice_does_not_invent_mutation_or_rollback_receipts() {
        for error in [CredentialError::Unavailable, CredentialError::Failed] {
            let notice = credential_notice(&error);
            assert!(notice.contains("could not be confirmed"));
            assert!(!notice.contains("Nothing was changed"));
            assert!(!notice.contains("was kept"));
        }
    }

    struct Script(VecDeque<Event>);
    impl SettingsEvents for Script {
        fn next_event(&mut self) -> Result<Event, String> {
            self.0.pop_front().ok_or_else(|| "script ended".into())
        }
        fn poll_event(&mut self, _: Duration) -> Result<Option<Event>, String> {
            if self.0.is_empty() {
                Ok(None)
            } else {
                self.next_event().map(Some)
            }
        }
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn kind(code: KeyCode, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind))
    }
    fn ch(character: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
    }

    #[derive(Clone)]
    struct MemoryPort {
        id: String,
        methods: Vec<AuthMethod>,
        state: Arc<Mutex<MemoryState>>,
    }
    #[derive(Clone)]
    struct AuthMethod {
        id: &'static str,
        name: &'static str,
        secret: bool,
        browser: bool,
        device: bool,
        optional: bool,
    }
    #[derive(Default)]
    struct MemoryState {
        selected: Option<String>,
        stored: HashMap<String, String>,
        log: Vec<String>,
    }

    struct IdleSession;
    impl ProviderSession for IdleSession {
        fn start<'a>(
            &'a self,
            _: ProviderRequest,
            _: Arc<dyn CancellationSignal>,
        ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
            Box::pin(async { Err(memory_error()) })
        }
    }
    fn memory_error() -> ProviderError {
        ProviderError {
            provider_id: ProviderId::new("memory").expect("id"),
            provider_code: None,
            http_status: None,
            continuation_possible: false,
            info: vesper_domain::ErrorInfo {
                category: vesper_domain::ErrorCategory::InvalidRequest,
                retryability: vesper_domain::Retryability::Never,
                retry_after_ms: None,
                visible_output_emitted: false,
                safe_message: vesper_domain::SafeMessage::new("idle").expect("msg"),
                diagnostics: vesper_domain::RedactedDiagnostics::default(),
                provider_code: None,
                causes: Vec::new(),
            },
            metadata: ExtensionMap::default(),
        }
    }
    impl ProviderCredentialPort for MemoryPort {
        fn credential_present(&self) -> Result<bool, CredentialError> {
            Ok(self.state.lock().expect("lock").selected.is_some())
        }
        fn store_credential(&self, secret: &str) -> Result<(), CredentialError> {
            let method = self
                .methods
                .iter()
                .find(|method| method.secret)
                .map(|method| method.id.to_owned())
                .ok_or(CredentialError::Unavailable)?;
            self.store_method_credential(&method, secret)
        }
        fn store_method_credential(
            &self,
            method_id: &str,
            secret: &str,
        ) -> Result<(), CredentialError> {
            vesper_auth::validate_secret(secret).map_err(|_| CredentialError::InvalidSecret)?;
            let mut state = self.state.lock().expect("lock");
            state.stored.insert(method_id.to_owned(), secret.to_owned());
            state.selected = Some(method_id.to_owned());
            state
                .log
                .push(format!("store:{method_id}:{}", secret.len()));
            Ok(())
        }
        fn select_authentication_method(&self, method_id: &str) -> Result<(), CredentialError> {
            let mut state = self.state.lock().expect("lock");
            if !state.stored.contains_key(method_id) {
                return Err(CredentialError::Absent);
            }
            state.selected = Some(method_id.to_owned());
            state.log.push(format!("select:{method_id}"));
            Ok(())
        }
        fn clear_stored_method(&self, method_id: &str) -> Result<(), CredentialError> {
            let mut state = self.state.lock().expect("lock");
            state.stored.remove(method_id);
            if state.selected.as_deref() == Some(method_id) {
                state.selected = None;
            }
            state.log.push(format!("clear:{method_id}"));
            Ok(())
        }
        fn logout(&self) -> Result<(), CredentialError> {
            let mut state = self.state.lock().expect("lock");
            state.stored.clear();
            state.selected = None;
            state.log.push(format!("logout:{}", self.id));
            Ok(())
        }
        fn removal_scope(&self) -> CredentialRemovalScope {
            CredentialRemovalScope::EntireProvider
        }
        fn authentication_method(&self) -> Result<Option<String>, CredentialError> {
            Ok(self.state.lock().expect("lock").selected.clone())
        }
        fn authentication_inventory(&self) -> Result<AuthenticationInventory, CredentialError> {
            let state = self.state.lock().expect("lock");
            Ok(AuthenticationInventory {
                selected_method: state.selected.clone(),
                methods: self
                    .methods
                    .iter()
                    .map(|method| AuthenticationMethodState {
                        method_id: method.id.to_owned(),
                        source: if state.stored.contains_key(method.id) {
                            CredentialSource::Stored
                        } else {
                            CredentialSource::Absent
                        },
                    })
                    .collect(),
            })
        }
        fn device_login<'a>(
            &'a self,
            cancel: Arc<dyn CancellationSignal>,
            on_challenge: Arc<dyn Fn(String, String) + Send + Sync>,
        ) -> ProviderFuture<'a, Result<(), CredentialError>> {
            let port = self.clone();
            Box::pin(async move {
                on_challenge("https://example.test/device?full=1".into(), "CODE".into());
                if port
                    .state
                    .lock()
                    .unwrap()
                    .log
                    .iter()
                    .any(|row| row == "hold-device")
                {
                    while !cancel.is_cancelled() {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    return Err(CredentialError::Failed);
                }
                port.store_method_credential(port.interactive_id(), "device-token")
            })
        }
        fn browser_login<'a>(
            &'a self,
            cancel: Arc<dyn CancellationSignal>,
            on_url: Arc<dyn Fn(String) + Send + Sync>,
        ) -> ProviderFuture<'a, Result<(), CredentialError>> {
            let port = self.clone();
            Box::pin(async move {
                let url = "https://auth.example.test/oauth2/auth?client_id=fixture&redirect_uri=http%3A%2F%2F127.0.0.1%3A9%2Fcallback&state=complete-url-value-that-must-not-be-truncated-or-rebuilt-from-the-terminal";
                on_url(url.into());
                if cancel.is_cancelled() {
                    return Err(CredentialError::Failed);
                }
                port.store_method_credential(port.interactive_id(), "browser-token")
            })
        }
    }
    impl MemoryPort {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                provider_id: ProviderId::new(&self.id).expect("id"),
                display_name: BoundedString::new(self.id.clone()).expect("name"),
                authentication_methods: self
                    .methods
                    .iter()
                    .map(|method| AuthenticationMethodDescriptor {
                        method_id: BoundedString::new(method.id).expect("id"),
                        display_name: BoundedString::new(method.name).expect("name"),
                        secret_reference_fields: if method.secret {
                            vec![BoundedString::new("FIXTURE_API_KEY").expect("field")]
                        } else {
                            Vec::new()
                        },
                        external_runtime_owned: false,
                        key_url: method
                            .secret
                            .then(|| BoundedString::new("https://example.test/keys").expect("url")),
                        interactive_login: {
                            let mut kinds = Vec::new();
                            if method.browser {
                                kinds.push(InteractiveLoginKind::Browser);
                            }
                            if method.device {
                                kinds.push(InteractiveLoginKind::DeviceCode);
                            }
                            kinds
                        },
                        optional: method.optional,
                    })
                    .collect(),
                hosted_tools: Vec::new(),
                configuration: None,
                metadata: ExtensionMap::default(),
            }
        }
        fn interactive_id(&self) -> &'static str {
            self.methods
                .iter()
                .find(|method| method.browser || method.device)
                .map(|method| method.id)
                .unwrap_or("missing")
        }
    }

    fn factory_id(port: &MemoryPort) -> ProviderId {
        ProviderId::new(&port.id).expect("id")
    }

    struct IdFactory {
        id: ProviderId,
        port: MemoryPort,
    }
    impl ProviderFactory for IdFactory {
        type Session = IdleSession;
        fn provider_id(&self) -> &ProviderId {
            &self.id
        }
        fn create_session<'a>(
            &'a self,
            _: &'a ProviderConfiguration,
            _: Arc<dyn CancellationSignal>,
        ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
            Box::pin(async { Ok(IdleSession) })
        }
        fn descriptor(&self) -> ProviderDescriptor {
            self.port.descriptor()
        }
    }

    async fn registry(ports: Vec<MemoryPort>) -> (ProviderRegistry, Vec<Arc<Mutex<MemoryState>>>) {
        let registry = ProviderRegistry::new();
        let mut states = Vec::new();
        for port in ports {
            states.push(port.state.clone());
            let id = factory_id(&port);
            registry
                .register_with_credentials(
                    IdFactory {
                        id,
                        port: port.clone(),
                    },
                    port,
                )
                .await
                .expect("register");
        }
        (registry, states)
    }

    fn multi(id: &str) -> MemoryPort {
        MemoryPort {
            id: id.to_owned(),
            methods: vec![
                AuthMethod {
                    id: "subscription",
                    name: "Account subscription",
                    secret: false,
                    browser: true,
                    device: true,
                    optional: false,
                },
                AuthMethod {
                    id: "api-key",
                    name: "API key — separate billing",
                    secret: true,
                    browser: false,
                    device: false,
                    optional: false,
                },
            ],
            state: Arc::new(Mutex::new(MemoryState::default())),
        }
    }

    fn hooks() -> AuthUiHooks {
        AuthUiHooks {
            open_url: Box::new(|_| Ok(())),
            copy_url: Box::new({
                let seen = Arc::new(Mutex::new(String::new()));
                let _ = seen;
                |_| Ok(())
            }),
        }
    }

    fn frame_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[tokio::test]
    async fn held_down_repeat_stays_on_the_xai_action() {
        // Recording: manage-xai, then the zai provider row, then manage-zai, then
        // Z.ai authentication. A press plus auto-repeat must not leave the xAI action.
        let (registry, _) = registry(vec![multi("openai"), multi("xai"), multi("zai")]).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut events = Script(VecDeque::from([
            kind(KeyCode::Down, KeyEventKind::Press),
            kind(KeyCode::Down, KeyEventKind::Repeat),
            kind(KeyCode::Down, KeyEventKind::Repeat),
            kind(KeyCode::Down, KeyEventKind::Release),
            Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Moved,
                column: 10,
                row: 10,
                modifiers: KeyModifiers::NONE,
            }),
            kind(KeyCode::Enter, KeyEventKind::Press),
        ]));
        let mut hooks = hooks();
        let error = run_provider_settings(
            &mut terminal,
            &registry,
            "xai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap_err();
        assert!(error.contains("script ended"), "{error}");
        let text = frame_text(&terminal);
        assert!(
            text.contains("Providers › xai › Authentication"),
            "repeat or mouse movement left the xAI action: {text}"
        );
        assert!(
            !text.contains("Providers › zai › Authentication"),
            "repeat opened Z.ai: {text}"
        );
    }

    #[tokio::test]
    async fn arrow_down_opens_that_provider_not_the_next_one() {
        // Alphabetical registry order matches the recorded screen: xAI, then Z.ai.
        // One Down from xAI must open xAI. It must not cross onto Z.ai first.
        let (registry, states) = registry(vec![multi("openai"), multi("xai"), multi("zai")]).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut events = Script(VecDeque::from([key(KeyCode::Down), key(KeyCode::Enter)]));
        let mut hooks = hooks();
        let error = run_provider_settings(
            &mut terminal,
            &registry,
            "xai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap_err();
        assert!(error.contains("script ended"), "{error}");
        let text = frame_text(&terminal);
        assert!(
            text.contains("Providers › xai › Authentication"),
            "arrow navigation did not open xAI: {text}"
        );
        assert!(
            !text.contains("Providers › zai › Authentication"),
            "arrow navigation opened Z.ai instead of xAI: {text}"
        );
        assert!(
            states
                .iter()
                .all(|state| state.lock().unwrap().log.is_empty()),
            "opening the panel changed credentials"
        );
    }

    #[tokio::test]
    async fn arrow_down_opens_first_and_last_provider_actions() {
        for current in ["openai", "zai"] {
            let (registry, _) = registry(vec![multi("openai"), multi("xai"), multi("zai")]).await;
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut events = Script(VecDeque::from([key(KeyCode::Down), key(KeyCode::Enter)]));
            let mut hooks = hooks();
            let error = run_provider_settings(
                &mut terminal,
                &registry,
                current,
                "chatgpt-black",
                &mut events,
                &mut hooks,
            )
            .await
            .unwrap_err();
            assert!(error.contains("script ended"), "{current}: {error}");
            let text = frame_text(&terminal);
            assert!(
                text.contains(&format!("Providers › {current} › Authentication")),
                "{current}: {text}"
            );
        }
    }

    #[tokio::test]
    async fn managing_an_inactive_provider_does_not_activate_or_save_it() {
        let (registry, states) = registry(vec![multi("openai"), multi("xai"), multi("zai")]).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut events = Script(VecDeque::from([
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Esc),
            key(KeyCode::Esc),
        ]));
        let mut hooks = hooks();
        let outcome = run_provider_settings(
            &mut terminal,
            &registry,
            "openai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap();
        assert!(outcome.save_provider.is_none());
        assert!(!outcome.authentication_committed);
        let text = frame_text(&terminal);
        assert!(text.contains("[x] openai (active)"), "{text}");
        assert!(text.contains("[ ] zai"), "{text}");
        assert!(!text.contains("[x] zai"), "{text}");
        assert!(
            states
                .iter()
                .all(|state| state.lock().unwrap().log.is_empty())
        );
    }

    #[tokio::test]
    async fn shortcut_m_opens_the_same_provider_as_its_action_row() {
        for events in [
            VecDeque::from([ch('m')]),
            VecDeque::from([key(KeyCode::Down), ch('m')]),
        ] {
            let (registry, _) = registry(vec![multi("openai"), multi("xai"), multi("zai")]).await;
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut events = Script(events);
            let mut hooks = hooks();
            let error = run_provider_settings(
                &mut terminal,
                &registry,
                "xai",
                "chatgpt-black",
                &mut events,
                &mut hooks,
            )
            .await
            .unwrap_err();
            assert!(error.contains("script ended"), "{error}");
            let text = frame_text(&terminal);
            assert!(text.contains("Providers › xai › Authentication"), "{text}");
        }
    }

    #[tokio::test]
    async fn back_and_repeated_arrows_keep_the_same_authentication_target() {
        let (registry, _) = registry(vec![multi("openai"), multi("xai"), multi("zai")]).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut events = Script(VecDeque::from([
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Esc),
            key(KeyCode::Enter),
        ]));
        let mut hooks = hooks();
        let error = run_provider_settings(
            &mut terminal,
            &registry,
            "xai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap_err();
        assert!(error.contains("script ended"), "{error}");
        let text = frame_text(&terminal);
        assert!(text.contains("Providers › xai › Authentication"), "{text}");
        assert!(!text.contains("Providers › zai › Authentication"), "{text}");
    }

    #[tokio::test]
    async fn settings_navigation_reaches_authentication_for_an_unfamiliar_provider() {
        let unfamiliar = multi("fixture-unfamiliar-provider");
        let active = multi("active-provider");
        let (registry, _) = registry(vec![active, unfamiliar]).await;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut events = Script(VecDeque::from([
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
        ]));
        let mut hooks = hooks();
        let error = run_provider_settings(
            &mut terminal,
            &registry,
            "active-provider",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap_err();
        assert!(error.contains("script ended"), "{error}");
        let text = frame_text(&terminal);
        assert!(text.contains("Authentication"), "{text}");
        assert!(text.contains("Account subscription"), "{text}");
        assert!(text.contains("API key"), "{text}");
        assert!(!text.contains("fixture-unfamiliar-provider") || text.contains("Authentication"));
    }

    #[tokio::test]
    async fn two_way_switch_and_inactive_provider_isolation() {
        let xai = multi("xai");
        let openai = multi("openai");
        let xai_state = xai.state.clone();
        xai_state
            .lock()
            .unwrap()
            .stored
            .insert("subscription".into(), "sub".into());
        xai_state.lock().unwrap().selected = Some("subscription".into());
        let openai_state = openai.state.clone();
        openai_state
            .lock()
            .unwrap()
            .stored
            .insert("api-key".into(), "openai-key".into());
        openai_state.lock().unwrap().selected = Some("api-key".into());
        let (registry, _) = registry(vec![openai, xai]).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut script = vec![
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
        ];
        for character in "synthetic-xai-key".chars() {
            script.push(ch(character));
        }
        script.extend([
            key(KeyCode::Enter),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
            key(KeyCode::Esc),
            key(KeyCode::Esc),
        ]);
        let mut events = Script(script.into());
        let mut hooks = hooks();
        let outcome = run_provider_settings(
            &mut terminal,
            &registry,
            "openai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap();
        let xai = xai_state.lock().unwrap();
        assert_eq!(xai.selected.as_deref(), Some("subscription"));
        assert_eq!(
            xai.stored.get("api-key").map(String::as_str),
            Some("synthetic-xai-key")
        );
        assert!(xai.stored.contains_key("subscription"));
        assert_eq!(
            xai.log
                .iter()
                .filter(|entry| entry.starts_with("store:"))
                .count(),
            1
        );
        drop(xai);
        let openai = openai_state.lock().unwrap();
        assert!(
            openai.log.is_empty(),
            "inactive management changed openai: {:?}",
            openai.log
        );
        assert_eq!(
            openai.stored.get("api-key").map(String::as_str),
            Some("openai-key")
        );
        assert!(outcome.save_provider.is_none());
        assert!(outcome.authentication_committed);
        assert_eq!(outcome.committed_provider.as_deref(), Some("xai"));
        let text = frame_text(&terminal);
        assert!(!text.contains("synthetic-xai-key"), "{text}");
    }

    #[tokio::test]
    async fn single_method_panel_has_no_browser_and_optional_auth_is_truthful() {
        let zai = MemoryPort {
            id: "zai".into(),
            methods: vec![AuthMethod {
                id: "zai-api-key",
                name: "Z.ai API key",
                secret: true,
                browser: false,
                device: false,
                optional: false,
            }],
            state: Arc::new(Mutex::new(MemoryState::default())),
        };
        let lm = MemoryPort {
            id: "lmstudio".into(),
            methods: vec![AuthMethod {
                id: "lmstudio-api-key",
                name: "LM Studio API key (optional)",
                secret: true,
                browser: false,
                device: false,
                optional: true,
            }],
            state: Arc::new(Mutex::new(MemoryState::default())),
        };
        let (registry, _) = registry(vec![zai, lm]).await;
        let mut terminal = Terminal::new(TestBackend::new(110, 36)).unwrap();
        let mut events = Script(VecDeque::from([key(KeyCode::Up), ch('m')]));
        let mut hooks = hooks();
        let error = run_provider_settings(
            &mut terminal,
            &registry,
            "zai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap_err();
        assert!(error.contains("script ended"));
        let text = frame_text(&terminal);
        assert!(text.contains("No authentication required"), "{text}");
        assert!(!text.contains("Browser sign-in"), "{text}");
        assert!(!text.contains("Device-code"), "{text}");
    }

    #[tokio::test]
    async fn cancelled_key_entry_keeps_the_previous_credential() {
        let provider = multi("xai");
        provider
            .state
            .lock()
            .unwrap()
            .stored
            .insert("subscription".into(), "keep".into());
        provider.state.lock().unwrap().selected = Some("subscription".into());
        let state = provider.state.clone();
        let (registry, _) = registry(vec![provider]).await;
        let mut terminal = Terminal::new(TestBackend::new(100, 36)).unwrap();
        let mut events = Script(VecDeque::from([
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            ch('s'),
            ch('e'),
            ch('c'),
            ch('r'),
            ch('e'),
            ch('t'),
            ch('-'),
            ch('c'),
            ch('a'),
            ch('n'),
            ch('a'),
            ch('r'),
            ch('y'),
            key(KeyCode::Esc),
            key(KeyCode::Esc),
            key(KeyCode::Esc),
        ]));
        let mut hooks = hooks();
        let outcome = run_provider_settings(
            &mut terminal,
            &registry,
            "xai",
            "chatgpt-black",
            &mut events,
            &mut hooks,
        )
        .await
        .unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.selected.as_deref(), Some("subscription"));
        assert!(!state.stored.contains_key("api-key"));
        assert!(!outcome.authentication_committed);
        let text = frame_text(&terminal);
        assert!(!text.contains("secret-canary"), "{text}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn device_login_opens_and_displays_the_url_and_keeps_retry_copy_after_launch_failure() {
        // Exercise the production login loop, without a browser, clipboard or real account.
        for fail_launch in [false, true] {
            let port = multi("fixture-provider");
            port.state.lock().unwrap().log.push("hold-device".into());
            port.store_method_credential("subscription", "previous")
                .unwrap();
            let mut state = PanelState {
                provider_id: port.id.clone(),
                provider_name: port.id.clone(),
                descriptor: port.descriptor(),
                inventory: port.authentication_inventory().unwrap(),
                selected: 0,
                method_focus: 0,
                screen: Screen::Overview,
                secret: Zeroizing::new(String::new()),
                notice: String::new(),
                busy: false,
                generation: 0,
                committed: false,
                removal_scope: CredentialRemovalScope::EntireProvider,
            };
            struct ChallengeEvents {
                polls: usize,
            }
            impl SettingsEvents for ChallengeEvents {
                fn next_event(&mut self) -> Result<Event, String> {
                    unreachable!()
                }
                fn poll_event(&mut self, _: Duration) -> Result<Option<Event>, String> {
                    self.polls += 1;
                    std::thread::sleep(Duration::from_millis(10));
                    Ok(match self.polls {
                        10 => Some(key(KeyCode::Enter)),
                        11 => Some(ch('c')),
                        12 => Some(key(KeyCode::Esc)),
                        _ => None,
                    })
                }
            }
            let opened = Arc::new(Mutex::new(Vec::new()));
            let copied = Arc::new(Mutex::new(Vec::new()));
            let mut hooks = AuthUiHooks {
                open_url: Box::new({
                    let opened = opened.clone();
                    move |url| {
                        opened.lock().unwrap().push(url.to_owned());
                        if fail_launch {
                            Err("fixture launch refused".into())
                        } else {
                            Ok(())
                        }
                    }
                }),
                copy_url: Box::new({
                    let copied = copied.clone();
                    move |url| {
                        copied.lock().unwrap().push(url.to_owned());
                        Ok(())
                    }
                }),
            };
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
            login(
                &mut state,
                &mut terminal,
                &mut ChallengeEvents { polls: 0 },
                &mut hooks,
                Arc::new(port.clone()),
                LoginTarget {
                    kind: InteractiveLoginKind::DeviceCode,
                    generation: 0,
                    provider_id: "fixture-provider",
                    method_id: "subscription",
                    theme: "chatgpt-black",
                },
            )
            .await
            .unwrap();
            let url = "https://example.test/device?full=1";
            assert_eq!(*opened.lock().unwrap(), vec![url, url]);
            assert_eq!(*copied.lock().unwrap(), vec![url]);
            let screen = frame_text(&terminal);
            assert!(screen.contains(url), "{screen}");
            assert!(screen.contains("One-time code: CODE"), "{screen}");
            assert!(!screen.contains("Requesting device-code"), "{screen}");
            assert!(!state.busy);
            assert!(!state.committed);
            assert_eq!(
                port.state.lock().unwrap().stored["subscription"],
                "previous"
            );
        }
    }

    #[test]
    fn sign_in_link_keeps_the_complete_url_and_rejects_control_characters() {
        let url = "https://auth.x.ai/oauth2/auth?response_type=code&client_id=b1a00492-073a-47ea-816f-4c329264a828&redirect_uri=http%3A%2F%2F127.0.0.1%3A49152%2Fcallback&scope=openid+profile+email+offline_access+grok-cli%3Aaccess+api%3Aaccess&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789-_ABCDEFGHIJKLMNOPQRSTUVWXYZ&code_challenge_method=S256&state=state-value&nonce=nonce-value&referrer=agent-vesper";
        let link = SignInLink::new(url.to_owned()).unwrap();
        assert_eq!(link.as_str(), url);
        assert!(SignInLink::new(format!("{url}\ntruncated")).is_err());
        let parsed = reqwest::Url::parse(link.as_str()).unwrap();
        let fields = parsed
            .query_pairs()
            .into_owned()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(fields["state"], "state-value");
        assert_eq!(fields["nonce"], "nonce-value");
        assert!(!fields["code_challenge"].is_empty());
        let mut command = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
        } else {
            std::process::Command::new("xdg-open")
        };
        command.arg(link.as_str());
        assert_eq!(
            command.get_args().last().and_then(|value| value.to_str()),
            Some(url)
        );
    }
}
