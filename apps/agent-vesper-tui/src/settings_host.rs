//! One Settings draft, committed only after the user confirms leaving Settings.
use super::*;
use agent_vesper_tui::settings_menu;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use vesper_harness::{acceptance_settings::AcceptanceSettings, web_settings::WebScopeConfig};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedChoices {
    providers: BTreeMap<String, BTreeMap<String, String>>,
    common: BTreeMap<String, String>,
}

fn path(root: &Path) -> std::path::PathBuf {
    root.join("settings.json")
}
fn load(root: &Path) -> Result<SavedChoices, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path(root)) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SavedChoices::default()),
        Err(_) => return Err("Could not read saved Settings.".into()),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Settings")?;
    if bytes.len() > 65536 {
        return Err("Saved Settings exceed 64 KiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Saved Settings are malformed".into())
}
fn save(root: &Path, choices: &SavedChoices) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|_| "Could not create Settings directory")?;
    let mut file = tempfile::NamedTempFile::new_in(root).map_err(|_| "Could not stage Settings")?;
    serde_json::to_writer(&mut file, choices).map_err(|_| "Could not encode Settings")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Could not sync Settings")?;
    file.persist(path(root))
        .map_err(|_| "Could not save Settings")?;
    Ok(())
}
fn ordinary(command: &str) -> bool {
    matches!(
        command,
        "/permission"
            | "/mode"
            | "/theme"
            | "/plan"
            | "/thinking"
            | "/model"
            | "/generation"
            | "/auxiliary"
            | "/mixture"
            | "/reasoning"
    )
}
fn remember(choices: &mut SavedChoices, provider: &ProviderId, command: &str) {
    let Some((name, _)) = command.split_once(' ') else {
        return;
    };
    if !ordinary(name) {
        return;
    }
    let target = if matches!(name, "/permission" | "/mode" | "/theme") {
        &mut choices.common
    } else {
        choices
            .providers
            .entry(provider.as_str().into())
            .or_default()
    };
    target.insert(name.into(), command.into());
}

pub(super) fn restore(
    state: &mut SessionState,
    commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    policy: &dyn vesper_provider::SuperpowerPolicy,
    provider: &ProviderId,
) -> Result<(), String> {
    let root = theme_preference_root().map_err(|e| e.to_string())?;
    let choices = load(&root)?;
    apply_saved(&choices, state, commands, surface, policy, provider)
}

fn apply_saved(
    choices: &SavedChoices,
    state: &mut SessionState,
    commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    policy: &dyn vesper_provider::SuperpowerPolicy,
    provider: &ProviderId,
) -> Result<(), String> {
    // Plan and model must precede model-dependent reasoning/auxiliary validation.
    for name in [
        "/permission",
        "/mode",
        "/plan",
        "/model",
        "/thinking",
        "/generation",
        "/auxiliary",
        "/mixture",
        "/reasoning",
    ] {
        if let Some(command) = choices.common.get(name).or_else(|| {
            choices
                .providers
                .get(provider.as_str())
                .and_then(|p| p.get(name))
        }) {
            if command.split_whitespace().next() != Some(name) || command.len() > 512 {
                return Err("Saved Settings contain an invalid choice".into());
            }
            let _ = dispatch(
                &CommandIntent::parse(command),
                commands,
                surface,
                policy,
                provider,
                state,
            );
        }
    }
    Ok(())
}

fn current_value(
    menu: &str,
    state: &SessionState,
    surface: &ProviderSuperpowerSurface,
) -> Option<String> {
    Some(match menu {
        "/permission" => match state.controls.permission_mode {
            SessionPermissionMode::Bypass => "bypass",
            SessionPermissionMode::ReadOnly => "read",
            _ => "ask",
        }
        .into(),
        "/mode" => if state.controls.operating_mode == SessionOperatingMode::Code {
            "code"
        } else {
            "ask"
        }
        .into(),
        "/theme" => state.preferences.theme.clone(),
        "/plan" => state.controls.endpoint_plan.clone(),
        "/generation" => state.controls.generation_profile.clone(),
        "/auxiliary" => state.controls.auxiliary_model.clone(),
        "/mixture" => state.controls.mixture_mode.clone(),
        _ => return active_superpower_choice(state, surface, menu.trim_start_matches('/')),
    })
}

/// All confirmation dialogs share keyboard and mouse geometry with Settings.
pub(super) async fn choice(
    terminal: &mut Terminal<Backend>,
    title: &str,
    detail: &str,
    labels: &[String],
    theme: &str,
) -> Result<Option<usize>, String> {
    let mut selected = 0;
    loop {
        terminal
            .draw(|f| {
                settings_menu::render_menu(
                    f,
                    labels,
                    selected,
                    title,
                    detail,
                    "↑↓ select · Enter choose · Esc back",
                    theme,
                )
            })
            .map_err(|e| e.to_string())?;
        let (key, clicked) = input(terminal, labels.len(), selected)?;
        if let Some(index) = clicked {
            selected = index;
        }
        match key {
            KeyCode::Esc => return Ok(None),
            KeyCode::Up | KeyCode::BackTab => {
                selected = (selected + labels.len() - 1) % labels.len()
            }
            KeyCode::Down | KeyCode::Tab => selected = (selected + 1) % labels.len(),
            KeyCode::Enter => return Ok(Some(selected)),
            _ => {}
        }
    }
}
pub(super) fn input(
    terminal: &Terminal<Backend>,
    count: usize,
    selected: usize,
) -> Result<(KeyCode, Option<usize>), String> {
    let key = match event::read().map_err(|e| e.to_string())? {
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                KeyCode::Esc
            } else {
                key.code
            }
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => KeyCode::Up,
            MouseEventKind::ScrollDown => KeyCode::Down,
            MouseEventKind::Down(MouseButton::Left) => {
                let size = terminal.size().map_err(|e| e.to_string())?;
                if let Some(index) = settings_menu::item_at(
                    ratatui::layout::Rect::new(0, 0, size.width, size.height),
                    count,
                    selected,
                    mouse.column,
                    mouse.row,
                ) {
                    return Ok((KeyCode::Enter, Some(index)));
                }
                KeyCode::Null
            }
            _ => KeyCode::Null,
        },
        _ => KeyCode::Null,
    };
    Ok((key, None))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn open(
    terminal: &mut Terminal<Backend>,
    session: &mut TuiSession,
    commands: &CommandRegistry,
    surface: &ProviderSuperpowerSurface,
    policy: &dyn vesper_provider::SuperpowerPolicy,
    provider: &ProviderId,
    registry: &Arc<vesper_runtime::ProviderRegistry>,
    catalog_retry: Option<&vesper_provider_openai::OpenAiFactory>,
) -> Result<String, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let preferences_root = theme_preference_root().map_err(|e| e.to_string())?;
    let mut choices = load(&preferences_root)?;
    let original_choices = choices.clone();
    let mut draft = session.state.clone();
    let mut acceptance = AcceptanceSettings::load(&root)?;
    let initial_acceptance = acceptance.clone();
    let mut web = vesper_harness::web_settings::load(&root)?;
    let initial_web = web.clone();
    #[cfg(feature = "swarm")]
    let mut swarm = vesper_harness::swarm_settings::SwarmSettingsDraft::open(&root)?;
    #[cfg(feature = "swarm")]
    let initial_swarm = swarm.settings.clone();
    let mut refreshed_catalog = None;
    let mut catalog_notice = session
        .state
        .status
        .clone()
        .unwrap_or_else(|| "No models are available.".into());
    let mut menu = "/settings".to_string();
    let mut selected = 0;
    let mut notice =
        "Changes remain a draft until you leave Settings and choose Save changes.".to_string();
    loop {
        let surface = refreshed_catalog
            .as_ref()
            .map(
                |(s, _): &(
                    ProviderSuperpowerSurface,
                    vesper_provider_openai::OpenAiSuperpowerPolicy,
                )| s,
            )
            .unwrap_or(surface);
        let policy: &dyn vesper_provider::SuperpowerPolicy = refreshed_catalog
            .as_ref()
            .map(|(_, p)| p as &dyn vesper_provider::SuperpowerPolicy)
            .unwrap_or(policy);
        if unavailable_model_menu(true, &menu, surface) {
            if choice(
                terminal,
                "Settings · model",
                &catalog_notice,
                &["Retry model list".into(), "Back".into()],
                &draft.preferences.theme,
            )
            .await?
                == Some(0)
            {
                if let Some(factory) = catalog_retry {
                    terminal
                        .draw(|f| {
                            settings_menu::render_menu(
                                f,
                                &["Loading account models…".into()],
                                0,
                                "Settings · model",
                                "",
                                "",
                                &draft.preferences.theme,
                            )
                        })
                        .map_err(|e| e.to_string())?;
                    match factory
                        .available_models(Arc::new(vesper_runtime::RuntimeCancellation::new()))
                        .await
                    {
                        Ok(available) => {
                            catalog_notice =
                                "No supported models returned. Check sign-in or retry.".into();
                            session.policy = Arc::new(available.policy());
                            refreshed_catalog = Some((
                                ProviderSuperpowerSurface::new(
                                    provider.clone(),
                                    factory.superpowers_for(&available),
                                ),
                                available.policy(),
                            ));
                        }
                        Err(error) => catalog_notice = error.info.safe_message.as_str().to_owned(),
                    }
                } else {
                    catalog_notice =
                        "Restart Vesper to refresh this provider's model catalog.".into();
                }
            } else {
                menu = "/settings".into();
                selected = 0;
            }
            continue;
        }
        let candidates = command_palette_candidates(
            &format!("{menu} "),
            commands,
            surface,
            policy,
            &session.capabilities,
            &session.provider_ids,
            &draft,
        );
        let current = current_value(&menu, &draft, surface);
        let labels: Vec<String> = candidates
            .iter()
            .map(|(command, description)| {
                if menu == "/settings" {
                    if description.contains("current ") {
                        description.clone()
                    } else {
                        description
                            .split('·')
                            .next()
                            .unwrap_or(description)
                            .trim()
                            .to_owned()
                    }
                } else {
                    let value = command
                        .split_once(' ')
                        .map(|(_, value)| value)
                        .unwrap_or(command);
                    format!(
                        "{value}{}",
                        if current.as_deref() == Some(value) {
                            " (current)"
                        } else {
                            ""
                        }
                    )
                }
            })
            .collect();
        let title = if menu == "/settings" {
            "Settings".into()
        } else {
            format!("Settings · {}", menu.trim_start_matches('/'))
        };
        terminal
            .draw(|f| {
                settings_menu::render_menu(
                    f,
                    &labels,
                    selected,
                    &title,
                    &notice,
                    "↑↓ select · Enter choose · Esc back",
                    &draft.preferences.theme,
                )
            })
            .map_err(|e| e.to_string())?;
        let (key, clicked) = input(terminal, candidates.len(), selected)?;
        if let Some(index) = clicked {
            selected = index;
        }
        match key {
            KeyCode::Up | KeyCode::BackTab => selected = selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                selected = (selected + 1).min(candidates.len().saturating_sub(1))
            }
            KeyCode::Esc if menu != "/settings" => {
                menu = "/settings".into();
                selected = 0;
            }
            KeyCode::Esc => {
                #[allow(unused_mut)]
                let mut dirty = choices != original_choices
                    || acceptance != initial_acceptance
                    || web != initial_web;
                #[cfg(feature = "swarm")]
                {
                    dirty |= swarm.settings != initial_swarm;
                }
                if !dirty {
                    return Ok("Settings unchanged.".into());
                }
                match choice(
                    terminal,
                    "Save changes?",
                    "Your changes have not been saved.",
                    &[
                        "Save changes".into(),
                        "Discard changes".into(),
                        "Keep editing".into(),
                    ],
                    &draft.preferences.theme,
                )
                .await?
                {
                    Some(0) => {
                        // Persist effective validated values, including adapter repairs
                        // made when a model change invalidated the previous reasoning choice.
                        for name in [
                            "/permission",
                            "/mode",
                            "/theme",
                            "/plan",
                            "/model",
                            "/thinking",
                            "/generation",
                            "/auxiliary",
                            "/mixture",
                        ] {
                            if let Some(value) = current_value(name, &draft, surface) {
                                let command = format!("{name} {value}");
                                if command_palette_candidates(
                                    &format!("{name} "),
                                    commands,
                                    surface,
                                    policy,
                                    &session.capabilities,
                                    &session.provider_ids,
                                    &draft,
                                )
                                .iter()
                                .any(|(candidate, _)| candidate == &command)
                                {
                                    remember(&mut choices, provider, &command);
                                }
                            }
                        }
                        let mut paths =
                            vec![preferences_root.join("theme"), path(&preferences_root)];
                        if acceptance != initial_acceptance {
                            paths.push(root.join(".agent-vesper/acceptance-settings.json"));
                        }
                        if web != initial_web {
                            paths.push(root.join(".agent-vesper/web-settings.json"));
                        }
                        #[cfg(feature = "swarm")]
                        if swarm.settings != initial_swarm {
                            paths.push(root.join(".agent-vesper/swarm-settings.json"));
                        }
                        let result = save_group(&paths, || {
                            if acceptance != initial_acceptance {
                                acceptance.save(&root)?;
                            }
                            if web != initial_web {
                                vesper_harness::web_settings::save(&root, &web)?;
                            }
                            #[cfg(feature = "swarm")]
                            if swarm.settings != initial_swarm {
                                swarm.save(&root)?;
                            }
                            save_theme_preference(&preferences_root, &draft.preferences.theme)
                                .map_err(|e| e.to_string())?;
                            save(&preferences_root, &choices)
                        });
                        if let Err(error) = result {
                            notice = format!(
                                "Save did not finish: {error}. Your draft is retained; retry saving."
                            );
                            continue;
                        }
                        session.state.overrides = draft.overrides;
                        session.state.controls = draft.controls;
                        session.state.pending_mode_update = draft.pending_mode_update;
                        session.state.pending_reasoning = draft.pending_reasoning;
                        session.state.reasoning_mode_override = draft.reasoning_mode_override;
                        session.state.preferences.theme = draft.preferences.theme;
                        if !acceptance.enabled {
                            session.acceptance = None;
                        }
                        return Ok("Settings saved. Model, reasoning and permissions apply to the next turn. Web tools require restart.".into());
                    }
                    Some(1) => return Ok("Changes discarded.".into()),
                    _ => {}
                }
            }
            KeyCode::Enter => {
                let Some((command, _)) = candidates.get(selected) else {
                    continue;
                };
                if menu == "/settings" {
                    match command.as_str() {
                        "/settings acceptance" => {
                            edit_acceptance(terminal, &mut acceptance, &draft.preferences.theme)
                                .await?
                        }
                        "/web" => edit_web(terminal, &mut web, &draft.preferences.theme).await?,
                        #[cfg(feature = "swarm")]
                        "/settings swarm" => {
                            edit_swarm(terminal, &mut swarm, &draft.preferences.theme).await?
                        }
                        "/provider" => {
                            notice = provider_settings(
                                terminal,
                                registry,
                                provider,
                                &draft.preferences.theme,
                            )
                            .await
                            .unwrap_or_else(|e| e);
                            continue;
                        }
                        _ => {
                            menu = command.clone();
                            selected = 0;
                        }
                    }
                } else {
                    let name = command.split_whitespace().next().unwrap_or("");
                    if ordinary(name) {
                        let _ = dispatch(
                            &CommandIntent::parse(command),
                            commands,
                            surface,
                            policy,
                            provider,
                            &mut draft,
                        );
                        notice = draft
                            .status
                            .clone()
                            .unwrap_or_else(|| "Choice staged; leave Settings to save.".into());
                        remember(&mut choices, provider, command);
                    }
                    menu = "/settings".into();
                    selected = 0;
                }
            }
            _ => {}
        }
    }
}

async fn provider_settings(
    terminal: &mut Terminal<Backend>,
    registry: &Arc<vesper_runtime::ProviderRegistry>,
    provider: &ProviderId,
    theme: &str,
) -> Result<String, String> {
    let Some(target) = open_provider_switcher(terminal, registry, provider, theme).await? else {
        return Ok("Provider selection cancelled.".into());
    };
    if target == "lmstudio" {
        let settings = load_lmstudio_settings();
        if (settings.api_base_url.trim().is_empty()
            || settings.api_base_url == "http://localhost:1234/v1")
            && !matches!(edit_lmstudio_settings(terminal).await?, Some(s) if !s.is_empty())
        {
            return Ok("LM Studio setup cancelled. Provider not saved.".into());
        }
    }
    if let Ok(id) = ProviderId::new(target.as_str())
        && let Some(descriptor) = registry.descriptor(&id).await
        && descriptor.authentication_methods.len() > 1
        && let Some(auth) = agent_vesper_tui::auth_provider_from_descriptor(&descriptor)
    {
        ensure_provider_authenticated(
            terminal,
            registry,
            auth,
            AuthenticationIntent::ProviderSwitch,
        )
        .await?;
    }
    save_provider_preference(&target)?;
    Ok(format!("Provider saved: {target}. Restart to apply."))
}

async fn edit_acceptance(
    terminal: &mut Terminal<Backend>,
    draft: &mut AcceptanceSettings,
    theme: &str,
) -> Result<(), String> {
    loop {
        let rows = vec![
            format!(
                "Enforced completion: {}",
                if draft.enabled { "ON" } else { "OFF" }
            ),
            "Back".into(),
        ];
        match choice(terminal, "Implementation acceptance", "When enabled, Vesper recognizes the task's PRD and remembers its path automatically. Tests and independent review still determine completion.", &rows, theme).await? {
            Some(0) => draft.enabled = !draft.enabled,
            _ => return Ok(()),
        }
    }
}
async fn edit_web(
    terminal: &mut Terminal<Backend>,
    draft: &mut WebScopeConfig,
    theme: &str,
) -> Result<(), String> {
    let mut hub = agent_vesper_tui::web_hub::WebHub::new(draft.clone());
    if hub.config.driver_image.is_none() {
        hub.notice = "Checking the installed driver…".into();
        terminal
            .draw(|f| agent_vesper_tui::web_hub::render(f, &hub, theme))
            .map_err(|e| e.to_string())?;
        match vesper_harness::web_settings::detect_driver().await {
            Ok(image) => {
                hub.config.driver_image = Some(image);
                hub.notice = "Installed driver ready. Leave Settings to save your choices.".into();
            }
            Err(error) => hub.notice = error,
        }
    }
    loop {
        let mut rows = hub.rows();
        rows.truncate(6);
        rows.push("Back".into());
        match choice(terminal, "Settings · Web tools", &hub.notice, &rows, theme).await? {
            Some(5) => match setup_web_driver_ui(terminal, &mut hub, theme).await {
                Ok(image) => hub.config.driver_image = Some(image),
                Err(error) => hub.notice = error,
            },
            Some(index) if index < 5 => {
                hub.selected = index;
                hub.toggle();
            }
            _ => {
                *draft = hub.config;
                return Ok(());
            }
        }
    }
}
#[cfg(feature = "swarm")]
async fn edit_swarm(
    terminal: &mut Terminal<Backend>,
    draft: &mut vesper_harness::swarm_settings::SwarmSettingsDraft,
    theme: &str,
) -> Result<(), String> {
    loop {
        let s = &draft.settings;
        let rows = vec![
            format!("Swarm: {}", if s.enabled { "ON" } else { "OFF" }),
            format!("Drivers: {}", s.drivers),
            format!("Topology: {:?}", s.topology),
            format!("Failover: {}", s.failover),
            format!("Shared scope: {}", s.shared_scope),
            format!("Governance: {:?}", s.governance),
            "Back".into(),
        ];
        match choice(
            terminal,
            "Settings › Swarm",
            "Every run checks embeddings, isolation and tool permissions.",
            &rows,
            theme,
        )
        .await?
        {
            Some(index) if index < 6 => {
                let mut hub = agent_vesper_tui::swarm_hub::SwarmHub::new(
                    vesper_harness::swarm_settings::SwarmSettingsDraft {
                        settings: draft.settings.clone(),
                    },
                );
                hub.selected = index;
                hub.change();
                draft.settings = hub.draft.settings;
            }
            _ => return Ok(()),
        }
    }
}

/// Preserve exact pre-save bytes if any member fails. Shared settings modules
/// retain ownership of validation and serialization; no live settings apply on error.
fn save_group(
    paths: &[std::path::PathBuf],
    write: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut previous = Vec::new();
    for path in paths {
        if path.parent().is_some_and(|p| {
            p.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        }) {
            return Err("Settings directory symlinks are refused".into());
        }
        let bytes = match path.symlink_metadata() {
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= 65536 =>
            {
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .map_err(|_| "Cannot read previous Settings")?
                    .take(65537)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Cannot read previous Settings")?;
                if bytes.len() > 65536 {
                    return Err("Previous Settings exceed the limit".into());
                }
                Some(bytes)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            _ => {
                return Err(
                    "Cannot safely stage Settings; a destination is not a bounded regular file"
                        .into(),
                );
            }
        };
        previous.push((path, bytes));
    }
    if let Err(error) = write() {
        let mut failed = false;
        for (path, bytes) in previous {
            let restored: Result<(), std::io::Error> = (|| {
                match bytes {
                    Some(bytes) => {
                        let mut file = tempfile::NamedTempFile::new_in(
                            path.parent().expect("settings parent"),
                        )?;
                        file.write_all(&bytes)?;
                        file.as_file().sync_all()?;
                        file.persist(path).map_err(|e| e.error)?;
                    }
                    None => match std::fs::remove_file(path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    },
                }
                Ok(())
            })();
            failed |= restored.is_err();
        }
        return Err(if failed {
            format!(
                "{error}. Some earlier writes could not be restored; review Settings before proceeding"
            )
        } else {
            format!("{error}. Previous settings restored")
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_save_failure_restores_exact_bytes_and_removes_new_files() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.json");
        let second = root.path().join("second.json");
        std::fs::write(&first, "original\n").unwrap();
        assert!(
            save_group(&[first.clone(), second.clone()], || {
                std::fs::write(&first, "changed").unwrap();
                std::fs::write(&second, "new").unwrap();
                Err("injected final save failure".into())
            })
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(first).unwrap(), "original\n");
        assert!(!second.exists());
    }
    #[test]
    fn saved_choices_restore_bypass_model_reasoning_and_session_controls() {
        let root = tempfile::tempdir().unwrap();
        let provider = ProviderId::new("zai").unwrap();
        let mut choices = SavedChoices::default();
        for command in [
            "/permission bypass",
            "/mode ask",
            "/model glm-5.2",
            "/thinking high",
            "/generation precise",
            "/auxiliary glm-4.7",
            "/mixture enabled",
        ] {
            remember(&mut choices, &provider, command);
        }
        assert!(
            !root.path().join("settings.json").exists(),
            "draft must not write"
        );
        save(root.path(), &choices).unwrap();
        let choices = load(root.path()).unwrap();
        let mut state = SessionState::new();
        let surface = super::super::tests::palette_surface();
        apply_saved(
            &choices,
            &mut state,
            &CommandRegistry::stage_11b(),
            &surface,
            &vesper_provider_glm::GlmSuperpowerPolicy,
            &provider,
        )
        .unwrap();
        assert_eq!(
            state.controls.permission_mode,
            SessionPermissionMode::Bypass
        );
        assert_eq!(state.controls.operating_mode, SessionOperatingMode::Plan);
        assert_eq!(active_model_label(&state, &surface), "glm-5.2");
        assert_eq!(state.controls.generation_profile, "precise");
        assert_eq!(state.controls.auxiliary_model, "glm-4.7");
        assert!(format!("{:?}", state.overrides).contains("high"));
        let other = ProviderId::new("another-provider").unwrap();
        let mut different = SessionState::new();
        apply_saved(
            &choices,
            &mut different,
            &CommandRegistry::stage_11b(),
            &surface,
            &vesper_provider_glm::GlmSuperpowerPolicy,
            &other,
        )
        .unwrap();
        assert_eq!(
            different.controls.permission_mode,
            SessionPermissionMode::Bypass
        );
        assert!(
            different.overrides.is_empty(),
            "provider choices must not leak across providers"
        );
    }
    #[test]
    fn saved_commands_cannot_execute_unrelated_actions() {
        let provider = ProviderId::new("zai").unwrap();
        let choices = SavedChoices {
            common: BTreeMap::from([("/permission".into(), "/quit".into())]),
            ..Default::default()
        };
        assert!(
            apply_saved(
                &choices,
                &mut SessionState::new(),
                &CommandRegistry::stage_11b(),
                &super::super::tests::palette_surface(),
                &vesper_provider_glm::GlmSuperpowerPolicy,
                &provider
            )
            .is_err()
        );
    }
}
