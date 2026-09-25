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
fn ordinary(command: &str, surface: &ProviderSuperpowerSurface) -> bool {
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
    ) || surface.by_alias(command.trim_start_matches('/')).is_some()
}
fn remember(
    choices: &mut SavedChoices,
    provider: &ProviderId,
    surface: &ProviderSuperpowerSurface,
    command: &str,
) {
    let Some((name, _)) = command.split_once(' ') else {
        return;
    };
    if !ordinary(name, surface) {
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
    if let Some(saved) = choices.providers.get(provider.as_str()) {
        for descriptor in surface.descriptors() {
            let Some(alias) = descriptor.command_alias.as_ref() else {
                continue;
            };
            let name = format!("/{}", alias.as_str());
            if matches!(
                name.as_str(),
                "/plan"
                    | "/model"
                    | "/thinking"
                    | "/generation"
                    | "/auxiliary"
                    | "/mixture"
                    | "/reasoning"
            ) {
                continue;
            }
            if let Some(command) = saved.get(&name) {
                if command.split_whitespace().next() != Some(name.as_str()) || command.len() > 512 {
                    return Err("Saved Settings contain an invalid provider choice".into());
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
    let mut skills = vesper_harness::skill_routing_settings::load(&root)?;
    let initial_skills = skills.clone();
    let mut acceptance = AcceptanceSettings::load(&root)?;
    let initial_acceptance = acceptance.clone();
    let mut web = vesper_harness::web_settings::load(&root)?;
    let initial_web = web.clone();
    #[cfg(feature = "voice-conversation")]
    let mut voice = vesper_voice::read_voice_scope(&root).unwrap_or_default();
    #[cfg(feature = "voice-conversation")]
    let initial_voice = voice.clone();
    #[cfg(feature = "swarm")]
    let mut swarm = vesper_harness::swarm_settings::SwarmSettingsDraft::open(&root)?;
    #[cfg(feature = "swarm")]
    let initial_swarm = swarm.settings.clone();
    #[cfg(feature = "bridge")]
    let mut bridge = vesper_harness::bridge_settings::BridgeSettings::load(&root);
    #[cfg(feature = "bridge")]
    let initial_bridge = bridge;
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
                    || web != initial_web
                    || skills != initial_skills;
                #[cfg(feature = "voice-conversation")]
                {
                    dirty |= settings_voice_save::voice_dirty(&voice, &initial_voice);
                }
                #[cfg(feature = "swarm")]
                {
                    dirty |= swarm.settings != initial_swarm;
                }
                #[cfg(feature = "bridge")]
                {
                    dirty |= bridge != initial_bridge;
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
                        let mut names = vec![
                            "/permission".to_owned(),
                            "/mode".to_owned(),
                            "/theme".to_owned(),
                            "/plan".to_owned(),
                            "/model".to_owned(),
                            "/thinking".to_owned(),
                            "/generation".to_owned(),
                            "/auxiliary".to_owned(),
                            "/mixture".to_owned(),
                        ];
                        names.extend(surface.descriptors().iter().filter_map(|descriptor| {
                            descriptor
                                .command_alias
                                .as_ref()
                                .map(|alias| format!("/{}", alias.as_str()))
                        }));
                        names.sort_unstable();
                        names.dedup();
                        for name in names {
                            if let Some(value) = current_value(&name, &draft, surface) {
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
                                    remember(&mut choices, provider, surface, &command);
                                }
                            }
                        }
                        let mut paths =
                            vec![preferences_root.join("theme"), path(&preferences_root)];
                        if skills != initial_skills {
                            paths.push(vesper_harness::skill_routing_settings::path(&root));
                        }
                        if acceptance != initial_acceptance {
                            paths.push(root.join(".agent-vesper/acceptance-settings.json"));
                        }
                        if web != initial_web {
                            paths.push(root.join(".agent-vesper/web-settings.json"));
                        }
                        #[cfg(feature = "voice-conversation")]
                        if voice != initial_voice {
                            paths.push(root.join(".agent-vesper/config.toml"));
                        }
                        #[cfg(feature = "swarm")]
                        if swarm.settings != initial_swarm {
                            paths.push(root.join(".agent-vesper/swarm-settings.json"));
                        }
                        #[cfg(feature = "bridge")]
                        if bridge != initial_bridge {
                            paths.push(root.join(".agent-vesper/bridge-settings.json"));
                        }
                        let result = save_group(&paths, || {
                            if skills != initial_skills {
                                vesper_harness::skill_routing_settings::save(&root, &skills)?;
                            }
                            if acceptance != initial_acceptance {
                                acceptance.save(&root)?;
                            }
                            if web != initial_web {
                                vesper_harness::web_settings::save(&root, &web)?;
                            }
                            #[cfg(feature = "voice-conversation")]
                            if settings_voice_save::voice_save_required(
                                &voice,
                                &initial_voice,
                                web != initial_web,
                            ) {
                                save_voice_scope(&root, &voice)?;
                            }
                            #[cfg(feature = "swarm")]
                            if swarm.settings != initial_swarm {
                                swarm.save(&root)?;
                            }
                            #[cfg(feature = "bridge")]
                            if bridge != initial_bridge {
                                bridge.save(&root)?;
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
                        // VRO-17 PR-4: the live conversation host follows the
                        // saved scope immediately (enable takes effect on the
                        // next F9 without a restart; disable tears it down
                        // with bounded speech cleanup).
                        #[cfg(feature = "voice-conversation")]
                        if let Some(host) = session.voice_conversation_host.as_mut() {
                            host.set_scope_enabled(voice.enabled);
                            // VRO-17 R3: engine/voice selection re-resolves
                            // at the next unit boundary (never mid-sentence).
                            host.reload_engine_selection();
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
                        "/settings skills" => {
                            edit_skills(terminal, &mut skills, &draft.preferences.theme).await?
                        }
                        "/settings acceptance" => {
                            edit_acceptance(terminal, &mut acceptance, &draft.preferences.theme)
                                .await?
                        }
                        "/web" => edit_web(terminal, &mut web, &draft.preferences.theme).await?,
                        #[cfg(feature = "voice-conversation")]
                        "/settings voice" => {
                            edit_voice(terminal, &mut voice, &draft.preferences.theme).await?
                        }
                        #[cfg(feature = "bridge")]
                        "/settings bridge" => {
                            edit_bridge(terminal, &mut bridge, &draft.preferences.theme).await?
                        }
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
                    if ordinary(name, surface) {
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
                        remember(&mut choices, provider, surface, command);
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

async fn edit_skills(
    terminal: &mut Terminal<Backend>,
    preferences: &mut vesper_harness::skill_routing_settings::RoutingPreferences,
    theme: &str,
) -> Result<(), String> {
    use vesper_harness::skill_routing_settings::RoutingMode;
    let stores = MemoryStores::open_default();
    let catalog = stores.skills.as_ref().map(|s| s.list()).unwrap_or_default();
    loop {
        let mut labels = vec![format!(
            "Routing: {}",
            if preferences.mode == RoutingMode::Standard {
                "Standard"
            } else {
                "Enhanced (preview)"
            }
        )];
        labels.push(format!(
            "Model-assisted selection: {}",
            if preferences.model_assistance {
                "ON (Enhanced only)"
            } else {
                "OFF"
            }
        ));
        labels.extend(catalog.iter().map(|skill| {
            format!(
                "[{}] {}",
                if preferences.disabled.contains(&skill.slug) {
                    " "
                } else {
                    "x"
                },
                skill.slug
            )
        }));
        labels.push("Back".into());
        let Some(index) = choice(terminal, "Settings · Skills", "Enhanced is a preview. Model assistance sends the task and bounded skill metadata to your configured provider, adding one call, latency and usage. Your library stays intact. Save on exit applies this project draft.", &labels, theme).await? else { return Ok(()); };
        if index == 0 {
            preferences.mode = match preferences.mode {
                RoutingMode::Standard => RoutingMode::Enhanced,
                RoutingMode::Enhanced => RoutingMode::Standard,
            };
        } else if index == 1 {
            preferences.model_assistance = !preferences.model_assistance;
            if preferences.model_assistance {
                preferences.mode = RoutingMode::Enhanced;
            }
        } else if let Some(skill) = catalog.get(index - 2) {
            if !preferences.disabled.remove(&skill.slug) {
                preferences.disabled.insert(skill.slug.clone());
            }
        } else {
            return Ok(());
        }
    }
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
/// Settings → Voice panel: activation, readiness truth, the Natural
/// Voice pack lifecycle, and policy.
/// Inspection never installs, downloads, opens devices, or runs speech.
#[cfg(feature = "voice-conversation")]
#[path = "settings_voice_save.rs"]
mod settings_voice_save;
#[cfg(feature = "voice-conversation")]
use settings_voice_save::save_voice_scope;

/// The saved selection label for the speech engine row (feature-oriented
/// wording; engine details live behind Details).
#[cfg(feature = "voice-conversation")]
fn engine_row_label(voice: &vesper_voice::VoiceScope) -> String {
    let _ = voice;
    #[cfg(feature = "voice-kokoro")]
    {
        if agent_vesper_tui::voice_readiness::neural_voice_selected(voice.tts.as_ref()) {
            let voice_name = match voice.voice.as_deref() {
                Some("am_michael") => "Michael",
                _ => "Heart",
            };
            return format!("current Neural voice · {voice_name}");
        }
    }
    "current System voice (espeak-ng)".to_owned()
}

#[cfg(feature = "voice-conversation")]
async fn edit_voice(
    terminal: &mut Terminal<Backend>,
    voice: &mut vesper_voice::VoiceScope,
    theme: &str,
) -> Result<(), String> {
    loop {
        let mut rows = vec![
            format!(
                "Voice conversation · current {}",
                if voice.enabled { "ON" } else { "OFF" }
            ),
            agent_vesper_tui::voice_accel::partials_settings_row(voice),
            format!("Speech engine · {}", engine_row_label(voice)),
            format!(
                "Speech recognition compute · current {}",
                voice.stt_compute.label()
            ),
            format!(
                "Speech synthesis compute · current {}",
                voice.tts_compute.label()
            ),
        ];
        // Pack lifecycle rows exist only in builds with the capability
        // (a build without it must say so, never offer an install that
        // can never work — directive §2).
        #[cfg(feature = "voice-kokoro")]
        {
            rows.push("Natural Voice pack · install, verify or remove…".into());
        }
        // VRO-17 R16: the accelerated-recognition (FLM) row exists only
        // where a real route can exist — the runtime is present on this
        // machine or is honestly absent (never a bogus setup offer).
        if agent_vesper_tui::voice_flm_assets::flm_executable().is_some() {
            rows.push("Accelerated recognition (FLM NPU) · verify or review…".into());
        }
        rows.push("Readiness · providers, backend and limits".into());
        rows.push("Back".into());
        let pack_row_index = if cfg!(feature = "voice-kokoro") {
            Some(5usize)
        } else {
            None
        };
        let flm_row_index = if cfg!(feature = "voice-kokoro") { 6 } else { 5 };
        let flm_row_offered = agent_vesper_tui::voice_flm_assets::flm_executable().is_some();
        let readiness_index = if cfg!(feature = "voice-kokoro") {
            if flm_row_offered { 7 } else { 6 }
        } else if flm_row_offered {
            6
        } else {
            4
        };
        let notice = "Changes are a draft until you leave Settings and choose Save changes (Esc → Save changes). Local speech does not change your main coding provider.";
        match choice(terminal, "Settings · Voice", notice, &rows, theme).await? {
            Some(0) => voice.enabled = !voice.enabled,
            Some(1) => {
                // Capability gate: flipping is meaningful only for a
                // partial-capable backend; on a final-only backend the
                // control explains itself and never appears flippable.
                if agent_vesper_tui::voice_accel::selected_stt_partials_mode(voice).is_some() {
                    voice.partials = !voice.partials;
                }
            }
            Some(2) => {
                edit_speech_engine(terminal, voice, theme).await?;
            }
            Some(3) => {
                edit_stage_compute(terminal, voice, vesper_voice::SpeechStage::Stt, theme).await?;
            }
            Some(4) => {
                edit_stage_compute(terminal, voice, vesper_voice::SpeechStage::Tts, theme).await?;
            }
            Some(index) if Some(index) == pack_row_index => {
                #[cfg(feature = "voice-kokoro")]
                manage_voice_pack(terminal, voice, theme).await?;
                #[cfg(not(feature = "voice-kokoro"))]
                {
                    let _ = index;
                }
            }
            Some(index) if flm_row_offered && index == flm_row_index => {
                manage_flm_recognition(terminal, theme).await?;
            }
            Some(index) if index == readiness_index => {
                voice_readiness_panel(terminal, voice, theme).await?;
            }
            _ => return Ok(()),
        }
    }
}

/// The speech engine selection row → per-provider choice (draft-only:
/// selection persists with Save, installation is separate and explicit).
#[cfg(feature = "voice-conversation")]
async fn edit_speech_engine(
    terminal: &mut Terminal<Backend>,
    voice: &mut vesper_voice::VoiceScope,
    theme: &str,
) -> Result<(), String> {
    let rows = vec![
        "System voice (espeak-ng) · built-in, robotic".into(),
        "Neural voice (Kokoro) · natural, local after setup".into(),
        "Back".into(),
    ];
    match choice(terminal, "Settings · Voice · Speech engine", "The engine choice is a draft until Save changes. Selecting the neural voice does not install anything by itself; installation is its own confirmed step.", &rows, theme).await? {
        Some(0) => {
            voice.tts = None;
            voice.voice = None;
        }
        Some(1) => {
            #[cfg(feature = "voice-kokoro")]
            {
                use vesper_domain::ProviderId;
                voice.tts = Some(
                    ProviderId::new(vesper_voice_kokoro::PROVIDER_ID)
                        .map_err(|_| "invalid provider id".to_owned())?,
                );
                if voice.voice.as_deref().is_none_or(|v| {
                    v != "af_heart" && v != "am_michael"
                }) {
                    voice.voice = Some("af_heart".to_owned());
                }
                choose_neural_voice(terminal, voice, theme).await?;
            }
            #[cfg(not(feature = "voice-kokoro"))]
            {
                return Err(
                    "This build does not include the Natural Voice pack capability. Install a complete Vesper build to use it; your settings are unchanged."
                        .to_owned(),
                );
            }
        }
        _ => {}
    }
    Ok(())
}

/// The voice selection submenu when the neural engine is selected
/// (catalog-verified display names; exact ids in Details).
#[cfg(feature = "voice-kokoro")]
async fn choose_neural_voice(
    terminal: &mut Terminal<Backend>,
    voice: &mut vesper_voice::VoiceScope,
    theme: &str,
) -> Result<(), String> {
    let rows = vec![
        format!(
            "Heart · current {}",
            if voice.voice.as_deref() == Some("af_heart") || voice.voice.is_none() {
                "yes"
            } else {
                "no"
            }
        ),
        format!(
            "Michael · current {}",
            if voice.voice.as_deref() == Some("am_michael") {
                "yes"
            } else {
                "no"
            }
        ),
        "Back".into(),
    ];
    match choice(
        terminal,
        "Settings · Voice · Voice",
        "Voice choice is a draft until Save changes.",
        &rows,
        theme,
    )
    .await?
    {
        Some(0) => voice.voice = Some("af_heart".to_owned()),
        Some(1) => voice.voice = Some("am_michael".to_owned()),
        _ => {}
    }
    Ok(())
}

/// The per-stage execution-policy submenu (draft-only). Rows are honest
/// about this machine: the strict NPU option appears only when a route
/// is registered for that stage (never a decorative control), and every
/// screen explains that CPU stays fully usable.
#[cfg(feature = "voice-conversation")]
async fn edit_stage_compute(
    terminal: &mut Terminal<Backend>,
    voice: &mut vesper_voice::VoiceScope,
    stage: vesper_voice::SpeechStage,
    theme: &str,
) -> Result<(), String> {
    let stage_name = stage_word_for_menu(stage);
    let current = match stage {
        vesper_voice::SpeechStage::Stt => voice.stt_compute,
        vesper_voice::SpeechStage::Tts => voice.tts_compute,
    };
    // The strict option is offered only when a route is registered for
    // this stage in this build (decorative controls are prohibited);
    // every build offers CPU and Automatic.
    let routes_registered = agent_vesper_tui::voice_accel::registered_routes()
        .iter()
        .any(|route| route.stage == stage);
    let rows: Vec<String> = [
        format!(
            "CPU · current {}",
            if current == vesper_voice::StageExecutionPolicy::Cpu {
                "yes"
            } else {
                "no"
            }
        ),
        format!(
            "Automatic · compatible acceleration only · current {}",
            if current == vesper_voice::StageExecutionPolicy::AutomaticAccelerator {
                "yes"
            } else {
                "no"
            }
        ),
    ]
    .into_iter()
    .chain(routes_registered.then(|| {
        format!(
            "NPU required · current {}",
            if current == vesper_voice::StageExecutionPolicy::NpuRequired {
                "yes"
            } else {
                "no"
            }
        )
    }))
    .chain(std::iter::once("Back".to_owned()))
    .collect();
    let effective_line = agent_vesper_tui::voice_accel::execution_rows(voice)
        .into_iter()
        .find(|row| row.stage == stage_name)
        .map(|row| format!("Effective now: {} ({})", row.backend, row.reason))
        .unwrap_or_default();
    let notice = format!(
        "The choice is a draft until Save changes. CPU speech stays fully usable on every machine; acceleration runs only after a verified compatible route for this stage exists on this machine. When no compatible accelerator is present, Automatic uses CPU as an ordinary supported outcome — not a warning, not a setup failure. {effective_line}"
    );
    match choice(
        terminal,
        &format!("Settings · Voice · {stage_name} compute"),
        &notice,
        &rows,
        theme,
    )
    .await?
    {
        Some(0) => set_stage_policy(voice, stage, vesper_voice::StageExecutionPolicy::Cpu),
        Some(1) => set_stage_policy(
            voice,
            stage,
            vesper_voice::StageExecutionPolicy::AutomaticAccelerator,
        ),
        Some(2) if routes_registered => set_stage_policy(
            voice,
            stage,
            vesper_voice::StageExecutionPolicy::NpuRequired,
        ),
        _ => {}
    }
    Ok(())
}

/// Applies the draft policy to the correct stage field (stages are
/// independent; selecting on one stage never rewrites the other).
#[cfg(feature = "voice-conversation")]
fn set_stage_policy(
    voice: &mut vesper_voice::VoiceScope,
    stage: vesper_voice::SpeechStage,
    policy: vesper_voice::StageExecutionPolicy,
) {
    match stage {
        vesper_voice::SpeechStage::Stt => voice.stt_compute = policy,
        vesper_voice::SpeechStage::Tts => voice.tts_compute = policy,
    }
}

/// Menu-facing stage word (title case).
#[cfg(feature = "voice-conversation")]
fn stage_word_for_menu(stage: vesper_voice::SpeechStage) -> &'static str {
    match stage {
        vesper_voice::SpeechStage::Stt => "Speech recognition",
        vesper_voice::SpeechStage::Tts => "Speech synthesis",
    }
}

/// The shared readiness panel (same assessment the F9 gate uses), now
/// including Natural Voice pack rows and per-stage execution rows when
/// applicable.
#[cfg(feature = "voice-conversation")]
async fn voice_readiness_panel(
    terminal: &mut Terminal<Backend>,
    voice: &vesper_voice::VoiceScope,
    theme: &str,
) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut checks = agent_vesper_tui::voice_readiness::voice_readiness();
    #[cfg(feature = "voice-kokoro")]
    if agent_vesper_tui::voice_readiness::neural_voice_selected(voice.tts.as_ref()) {
        checks.extend(agent_vesper_tui::voice_readiness::neural_voice_checks());
    }
    let execution_lines = {
        let mut lines = agent_vesper_tui::voice_accel::machine_capability_lines(voice);
        lines.push(agent_vesper_tui::voice_accel::last_stt_route_line());
        lines
    };
    let lines: Vec<String> = std::iter::once(format!(
        "Voice mode (configured): {}",
        if voice.enabled { "yes" } else { "no" }
    ))
    .chain(checks.iter().map(|check| {
        format!(
            "{}: {}",
            check.name,
            if check.ok {
                "yes".to_owned()
            } else {
                format!("no — {}", check.remedy)
            }
        )
    }))
    .chain(execution_lines)
    .collect();
    let body = format!(
        "{}

Detected ≠ device-accepted: microphone and speaker acceptance is verified only by your own voice test.",
        lines.join("
")
    );
    choice(
        terminal,
        "Settings · Voice · readiness",
        &body,
        &["Back".into()],
        theme,
    )
    .await?;
    Ok(())
}

/// One selectable action of the Natural Voice pack screen. Rows and
/// action indexes are derived from ONE ordered list so they cannot
/// disagree when Preview is policy-hidden.
#[cfg(feature = "voice-kokoro")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackAction {
    Preview,
    Repair,
    Remove,
    Install,
}

/// The pack screen's notice: the standard explanation plus, when the
/// draft TTS policy refuses Preview, the exact stage-specific refusal
/// (never silent hiding; never a setup prompt for absent hardware).
#[cfg(feature = "voice-kokoro")]
fn pack_screen_notice(voice: &vesper_voice::VoiceScope) -> String {
    let base = "Runs locally after setup. Installing the pack never selects the engine, enables conversation, or plays audio; engine and voice choices are Settings drafts until Save changes.";
    match agent_vesper_tui::voice_accel::preview_policy_gate(voice) {
        Ok(()) => base.to_owned(),
        Err(message) => format!("{base}\n\nPreview is unavailable: {message}"),
    }
}

/// VRO-17 R16: the accelerated-recognition (FLM NPU) management screen.
/// Shows the passive pack state and offers the real bounded Verify: a
/// controlled loopback semantic check through the production adapter
/// (no microphone, no speaker, no download). Installed does not mean
/// Ready — verification is what permits accelerated dispatch in this
/// process, and the compute choice remains a separate saved draft.
#[cfg(feature = "voice-conversation")]
async fn manage_flm_recognition(
    terminal: &mut Terminal<Backend>,
    theme: &str,
) -> Result<(), String> {
    use agent_vesper_tui::voice_flm_assets::{PackState, assess_pack};
    let state = assess_pack();
    let state_label = match &state {
        PackState::Installed => "model installed (files present, digest verified)".to_owned(),
        PackState::RuntimeMissing => "runtime missing on this machine".to_owned(),
        PackState::AssetsMissing => "model files missing".to_owned(),
        PackState::SizeMismatch { actual } => {
            format!("model file size mismatch ({actual} bytes; expected the pinned revision)")
        }
        PackState::DigestMismatch => "model failed its integrity check".to_owned(),
    };
    // The Verify row exists only where the composition is compiled and
    // the pack is verifiable (never a decorative action).
    #[cfg(feature = "voice-flm")]
    let verify_available = matches!(state, PackState::Installed);
    #[cfg(not(feature = "voice-flm"))]
    let verify_available = false;
    let verified_now = cfg!(feature = "voice-flm")
        && matches!(
            agent_vesper_tui::voice_accel::stage_readiness(vesper_voice::SpeechStage::Stt),
            vesper_voice::AcceleratorReadiness::Ready { .. }
        );
    let status_line = if verified_now {
        "Verified in this session: accelerated recognition may be selected for speech recognition compute."
    } else if verify_available {
        "Not yet verified in this session: accelerated dispatch stays unavailable until Verify completes."
    } else {
        "Verification is unavailable on this machine/build; CPU recognition remains fully usable."
    };
    let mut rows = vec![format!("Status · {state_label}")];
    if verify_available {
        rows.push("Verify accelerated recognition · bounded local check…".into());
    }
    rows.push("Back".into());
    let notice = format!(
        "The accelerated recognizer uses the local FLM runtime with the installed Whisper model on this machine's NPU. Speech detection (VAD) runs on CPU first; only speech-containing audio is sent to the recognizer. Verifying never sends audio anywhere else, never records, and never downloads. The compute choice (CPU / Automatic / NPU required) is a separate draft on this screen's parent, saved with Save changes.\n\n{status_line}"
    );
    match choice(
        terminal,
        "Settings · Voice · Accelerated recognition",
        &notice,
        &rows,
        theme,
    )
    .await?
    {
        Some(1) if verify_available => {
            #[cfg(feature = "voice-flm")]
            run_flm_verify(terminal, theme).await?;
            #[cfg(not(feature = "voice-flm"))]
            {
                let _ = theme;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The bounded real-model Verify: starts the owned loopback service,
/// sends one synthetic nonsensitive speech-like fixture through the
/// production VAD + adapter composition, and requires a valid
/// response shape. Success records the in-process verification; a
/// failure reports the exact boundary and never flips Ready.
#[cfg(feature = "voice-flm")]
async fn run_flm_verify(terminal: &mut Terminal<Backend>, theme: &str) -> Result<(), String> {
    use std::sync::Arc;
    use vesper_voice::audio::PcmFrame;
    use vesper_voice::cancel::VoiceCancel;
    use vesper_voice::composition::blocking::ThreadPoolExecutor;
    use vesper_voice::ports::VoiceStt;

    let draw_status = |terminal: &mut Terminal<Backend>, line: &str| {
        let _ = terminal.draw(|frame| {
            let block = ratatui::widgets::Paragraph::new(format!(
                "Verifying accelerated recognition…\n\n{line}\n\nThis runs a local check only: it never records audio and never downloads anything."
            ))
            .wrap(ratatui::widgets::Wrap { trim: true });
            frame.render_widget(block, frame.area());
        });
    };
    draw_status(
        terminal,
        "Starting the local recognizer service (this loads the model once)…",
    );
    let pool = Arc::new(ThreadPoolExecutor::new(1));
    let erased: Arc<dyn vesper_voice::composition::blocking::ValueExecutor> = pool;
    let adapter = agent_vesper_tui::voice_flm::FlmNpuStt::new(erased);
    // Nonsensitive synthetic fixture: the SAME three fixed-frequency
    // sin²-enveloped segments the recorded backend gate proved
    // detectable by the installed Silero defaults (detected window
    // [0, 1.584 s)). It proves request/response execution through the
    // full composition, not word accuracy — the receipt says exactly
    // that.
    let segments = [
        (0.25_f64, 0.60_f64, 190.0_f64),
        (0.95, 0.70, 240.0),
        (2.10, 0.55, 210.0),
    ];
    let total = (4.0 * 16_000.0) as usize;
    let mut pcm: Vec<u8> = Vec::with_capacity(total * 2);
    for index in 0..total {
        let t = index as f64 / 16_000.0;
        let mut sample: f64 = 0.0;
        for &(start, duration, f0) in &segments {
            if (start..start + duration).contains(&t) {
                let local = t - start;
                let envelope = (std::f64::consts::PI * local / duration).sin().max(0.0);
                sample = envelope
                    * ((2.0 * std::f64::consts::PI * f0 * local).sin()
                        + 0.5 * (2.0 * std::f64::consts::PI * 2.0 * f0 * local).sin()
                        + 0.25 * (2.0 * std::f64::consts::PI * 3.0 * f0 * local).sin())
                    / 1.75;
            }
        }
        let quantized = (sample * 22_000.0).clamp(-32_768.0, 32_767.0) as i16;
        pcm.extend_from_slice(&quantized.to_le_bytes());
    }
    let audio: Vec<PcmFrame> = pcm
        .chunks(2)
        .map(|chunk| PcmFrame::from_aligned(chunk.to_vec()).expect("aligned"))
        .collect();
    draw_status(
        terminal,
        "Running one local recognition check through the production composition…",
    );
    let cancel = VoiceCancel::default();
    let future = adapter.transcribe(&audio, &cancel);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    let outcome = loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(result) => {
                break result;
            }
            std::task::Poll::Pending => {
                if std::time::Instant::now() > deadline {
                    adapter.shutdown();
                    return Err(
                        "Verification timed out; accelerated recognition stays unavailable. CPU recognition is unaffected."
                            .to_owned(),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };

    adapter.shutdown();
    let ok = outcome
        .as_ref()
        .is_ok_and(|transcript| !transcript.text.as_str().trim().is_empty());
    let message = match &outcome {
        Ok(transcript) => {
            if transcript.text.as_str().trim().is_empty() {
                // VAD zero-speech on the fixture: the detector decided
                // there was no speech. Honest failure — no Ready flip.
                "The local speech detector found no speech in the check fixture; verification did not complete.".to_owned()
            } else {
                agent_vesper_tui::voice_accel::record_flm_stt_verification();
                "Verified: the accelerated recognizer answered a local check through the full composition (CPU speech detection + local NPU recognition). It may now be selected for speech recognition compute.".to_owned()
            }
        }
        Err(error) => format!(
            "Verification failed at the local boundary: {error}. Accelerated recognition stays unavailable; CPU recognition is unaffected."
        ),
    };
    let ok = ok
        && matches!(
            agent_vesper_tui::voice_accel::stage_readiness(vesper_voice::SpeechStage::Stt),
            vesper_voice::AcceleratorReadiness::Ready { .. }
        );
    let rows = vec!["Close".to_owned()];
    let _ = choice(
        terminal,
        if ok {
            "Accelerated recognition · verified"
        } else {
            "Accelerated recognition · not verified"
        },
        &message,
        &rows,
        theme,
    )
    .await?;
    Ok(())
}

/// The Natural Voice pack management screen: status, install with the
/// full confirmation dialog, real progress, repair, and removal.
#[cfg(feature = "voice-kokoro")]
async fn manage_voice_pack(
    terminal: &mut Terminal<Backend>,
    voice: &vesper_voice::VoiceScope,
    theme: &str,
) -> Result<(), String> {
    // Keep one worker for this screen. It prepares while the user reads the
    // menu and is reused by immediate repeat previews, instead of paying pack
    // verification + ORT session construction after every Preview click.
    let mut preview_worker: Option<agent_vesper_tui::voice_speech_worker::SpeechWorker> = None;
    let mut preview_segment = 0u64;
    loop {
        let root = vesper_voice_kokoro::pack_root();
        let state = match vesper_voice_kokoro::assess_pack(&root) {
            Ok(None) => "Ready — synthesis verified".to_owned(),
            Ok(Some(problem)) => match problem {
                vesper_voice_kokoro::PackProblem::NotInstalled => "Not installed".to_owned(),
                other => format!("Blocked — {}", other.description()),
            },
            Err(error) => format!("Unavailable — {error}"),
        };
        let mut rows = vec![format!("Voice pack · {state}")];
        let installed = matches!(
            vesper_voice_kokoro::assess_pack(&root),
            Ok(None)
                | Ok(Some(
                    vesper_voice_kokoro::PackProblem::ComponentInvalid { .. }
                ))
        );
        if installed && preview_worker.is_none() {
            let voice_id = voice.voice.clone().unwrap_or_else(|| "af_heart".to_owned());
            let playback =
                std::sync::Arc::new(agent_vesper_tui::voice_playback::PlaybackOwner::new(
                    agent_vesper_tui::resolve_player_for_preview(),
                    None,
                ));
            preview_worker = Some(agent_vesper_tui::voice_speech_worker::SpeechWorker::spawn(
                agent_vesper_tui::voice_conversation::EngineSelection::Neural { voice_id },
                playback,
            ));
        }
        if installed {
            // VRO-17 §2: Preview resolves the TTS execution policy through
            // the SAME shared rule the F9 gate uses (against this screen's
            // DRAFT). A strict policy that cannot run here refuses Preview
            // identically — never a silent CPU synthesis that F9 would
            // refuse. CPU/Automatic stay ordinary outcomes.
            if agent_vesper_tui::voice_accel::preview_policy_gate(voice).is_ok() {
                rows.push("Preview voice".into());
            }
            rows.push("Repair / Verify".into());
            rows.push("Remove voice pack".into());
        } else {
            rows.push("Install voice pack".into());
        }
        rows.push("Details".into());
        rows.push("Back".into());
        let action = choice(
            terminal,
            "Settings · Voice · Natural Voice pack",
            &pack_screen_notice(voice),
            &rows,
            theme,
        )
        .await?;
        let back = rows.len() - 1;
        let details_index = rows.len() - 2;
        // Action indexes are computed from the same ordered list that
        // built the rows, offset by the status row (row 0), so the
        // rows and the handlers can never disagree when Preview is
        // policy-hidden (the PTY loop regression caught this class:
        // a mismatched index made the Preview row fire Repair).
        let mut action_rows = Vec::new();
        if installed {
            if agent_vesper_tui::voice_accel::preview_policy_gate(voice).is_ok() {
                action_rows.push(PackAction::Preview);
            }
            action_rows.push(PackAction::Repair);
            action_rows.push(PackAction::Remove);
        } else {
            action_rows.push(PackAction::Install);
        }
        let action_index = |action: PackAction| {
            action_rows
                .iter()
                .position(|candidate| *candidate == action)
                .map(|index| index + 1)
        };
        let install_index = action_index(PackAction::Install);
        let preview_index = action_index(PackAction::Preview);
        let repair_index = action_index(PackAction::Repair);
        let remove_index = action_index(PackAction::Remove);
        match action {
            Some(index) if Some(index) == install_index => {
                install_voice_pack(terminal, theme).await?;
            }
            Some(index) if Some(index) == preview_index => {
                let Some(worker) = preview_worker.as_ref() else {
                    return Err("the Preview voice worker could not start".to_owned());
                };
                preview_segment = preview_segment.wrapping_add(1);
                preview_neural_voice(terminal, worker, preview_segment, theme).await?;
            }
            Some(index) if Some(index) == repair_index => {
                repair_voice_pack(terminal, theme).await?;
            }
            Some(index) if Some(index) == remove_index => {
                remove_voice_pack(terminal, theme).await?;
            }
            Some(index) if index == details_index => {
                pack_details(terminal, theme).await?;
            }
            Some(index) if index == back => return Ok(()),
            // Row 0 is the STATUS display row (and any other key): stay
            // in the screen — only Back leaves. (Alex's "biggest button":
            // the status row previously fell through to an implicit exit,
            // so pressing Enter on it silently left the pack screen.)
            _ => continue,
        }
    }
}

/// The pre-install confirmation dialog: real numbers from the pinned
/// manifest and the observed free space (no fabricated estimates).
#[cfg(feature = "voice-kokoro")]
async fn install_voice_pack(terminal: &mut Terminal<Backend>, theme: &str) -> Result<(), String> {
    let setup = vesper_voice_kokoro::setup::VoicePackSetup::new(vesper_voice_kokoro::pack_root());
    let plan = setup.plan().map_err(|error| error.message())?;
    let mib = |bytes: u64| format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0));
    let space_line = plan
        .available_bytes
        .map(|available| {
            format!(
                "Available at destination: {} ({available} bytes)",
                mib(available)
            )
        })
        .unwrap_or_else(|| {
            "Available at destination: unknown (setup will stop if space runs out)".to_owned()
        });
    let reused_line = if plan.phonemizer_present {
        "Reused from your system: espeak-ng pronunciation engine".to_owned()
    } else {
        "Missing prerequisite: espeak-ng (setup cannot continue without it)".to_owned()
    };
    let summary = format!(
        "Install the Natural Voice pack?\n\nDownload size: {}\nAdded after install: {}\nPeak extra space during setup: {}\n{space_line}\n{reused_line}\n\nRuns locally after setup. Speech stays on this machine. It does not change your main coding provider.",
        mib(plan.transfer_bytes),
        mib(plan.retained_bytes),
        mib(plan.peak_bytes),
    );
    match choice(
        terminal,
        "Install Natural Voice pack",
        &summary,
        &["Install".into(), "Cancel".into()],
        theme,
    )
    .await?
    {
        Some(0) => {}
        _ => return Ok(()),
    }
    run_setup_progress(terminal, setup, theme).await
}

/// Real progress loop: stage labels + exact bytes; Esc requests stop
/// after the current step (existing dependency-setup conventions).
#[cfg(feature = "voice-kokoro")]
async fn run_setup_progress(
    terminal: &mut Terminal<Backend>,
    setup: vesper_voice_kokoro::setup::VoicePackSetup,
    theme: &str,
) -> Result<(), String> {
    let (sender, mut progress) =
        tokio::sync::watch::channel(vesper_voice_kokoro::setup::StageProgress {
            stage: vesper_voice_kokoro::setup::SetupStage::Prepare,
            bytes_done: 0,
            bytes_total: 0,
        });
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task_cancel = cancel.clone();
    let mut task = tokio::spawn(async move {
        setup
            .run(
                |stage_progress| {
                    let _ = sender.send(stage_progress);
                },
                || task_cancel.load(std::sync::atomic::Ordering::Acquire),
                None,
            )
            .await
    });
    loop {
        let stage_progress = progress.borrow_and_update().clone();
        let mut line = stage_progress.stage.label().to_owned();
        if stage_progress.bytes_total > 0 {
            line.push_str(&format!(
                " · {} / {} bytes",
                stage_progress.bytes_done, stage_progress.bytes_total
            ));
        }
        let stopping = cancel.load(std::sync::atomic::Ordering::Acquire);
        terminal
            .draw(|frame| {
                agent_vesper_tui::settings_menu::render_menu(
                    frame,
                    &["Installing Natural Voice pack…".into()],
                    0,
                    "Natural Voice pack",
                    &line,
                    if stopping {
                        "Stopping after the current step…"
                    } else {
                        "Esc requests stop after the current step"
                    },
                    theme,
                );
            })
            .map_err(|error| error.to_string())?;
        tokio::select! {
            result = &mut task => {
                return match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(error)) => Err(error.message()),
                    Err(_) => Err("Voice pack setup stopped unexpectedly.".to_owned()),
                };
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                while event::poll(std::time::Duration::ZERO).map_err(|error| error.to_string())? {
                    if let Ok(event::Event::Key(key)) = event::read() && key.code == KeyCode::Esc {
                        cancel.store(true, std::sync::atomic::Ordering::Release);
                    }
                }
            }
        }
    }
}

/// Preview: fixed nonsensitive phrase through the REAL selected adapter
/// and the existing playback owner, only on explicit action, with a
/// visible Stop control; never submits a turn, never uses the
/// microphone, never commits unsaved settings.
#[cfg(feature = "voice-kokoro")]
async fn preview_neural_voice(
    terminal: &mut Terminal<Backend>,
    worker: &agent_vesper_tui::voice_speech_worker::SpeechWorker,
    segment: u64,
    theme: &str,
) -> Result<(), String> {
    const PREVIEW_PHRASE: &str = "This is a preview of the selected voice.";
    worker.enqueue(agent_vesper_tui::voice_speech_worker::SpeechJob {
        segment,
        text: PREVIEW_PHRASE.into(),
    });
    loop {
        for outcome in worker.drain() {
            use agent_vesper_tui::voice_speech_worker::SpeechOutcome;
            match outcome {
                SpeechOutcome::Spoke {
                    segment: outcome_segment,
                    ..
                } if outcome_segment == segment => return Ok(()),
                SpeechOutcome::Failed {
                    segment: outcome_segment,
                    error,
                } if outcome_segment == segment || outcome_segment == 0 => return Err(error),
                SpeechOutcome::Stale {
                    segment: outcome_segment,
                } if outcome_segment == segment => return Ok(()),
                // A prior stopped preview may settle after the next one was
                // admitted. Ignore only that older generation's receipt.
                SpeechOutcome::Progress {
                    segment: outcome_segment,
                    ..
                }
                | SpeechOutcome::Spoke {
                    segment: outcome_segment,
                    ..
                }
                | SpeechOutcome::Stale {
                    segment: outcome_segment,
                }
                | SpeechOutcome::Failed {
                    segment: outcome_segment,
                    ..
                } if outcome_segment != segment => {}
                _ => {}
            }
        }
        terminal.draw(|frame| {
            agent_vesper_tui::settings_menu::render_menu(
                frame, &[worker.stage_status()], 0, "Preview voice",
                "Fixed phrase through the selected neural voice. No agent turn or microphone. Playback success is not proof you heard it.",
                "S stops the preview · Esc returns", theme);
        }).map_err(|error| error.to_string())?;
        if event::poll(std::time::Duration::from_millis(50)).map_err(|error| error.to_string())?
            && let Ok(event::Event::Key(key)) = event::read()
            && matches!(
                key.code,
                KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Esc
            )
        {
            worker.stop();
            return Ok(());
        }
        tokio::task::yield_now().await;
    }
}

/// Repair/Verify: revalidates the pack in place; asks before any new
/// transfer (an install is only started by an explicit second choice).
#[cfg(feature = "voice-kokoro")]
async fn repair_voice_pack(terminal: &mut Terminal<Backend>, theme: &str) -> Result<(), String> {
    let root = vesper_voice_kokoro::pack_root();
    match vesper_voice_kokoro::assess_pack(&root) {
        Ok(None) => {
            choice(
                terminal,
                "Repair / Verify voice pack",
                "Every pack file matches its verified source. Nothing to repair.",
                &["Back".into()],
                theme,
            )
            .await?;
            Ok(())
        }
        Ok(Some(vesper_voice_kokoro::PackProblem::ComponentInvalid { detail })) => {
            match choice(
                terminal,
                "Repair / Verify voice pack",
                &format!("A pack file is invalid: {detail}\n\nRe-download the affected files now?"),
                &["Repair now".into(), "Cancel".into()],
                theme,
            )
            .await?
            {
                Some(0) => {
                    let setup = vesper_voice_kokoro::setup::VoicePackSetup::new(
                        vesper_voice_kokoro::pack_root(),
                    );
                    run_setup_progress(terminal, setup, theme).await
                }
                _ => Ok(()),
            }
        }
        Ok(Some(problem)) => {
            choice(
                terminal,
                "Repair / Verify voice pack",
                &problem.description(),
                &["Back".into()],
                theme,
            )
            .await?;
            Ok(())
        }
        Err(error) => {
            choice(
                terminal,
                "Repair / Verify voice pack",
                &format!("The pack could not be inspected: {error}"),
                &["Back".into()],
                theme,
            )
            .await?;
            Ok(())
        }
    }
}

/// Removal: confirms the measured reclaimable amount, deletes only
/// pack-owned assets, refuses while another live process holds the pack,
/// and explains that the selected voice becomes unavailable.
#[cfg(feature = "voice-kokoro")]
async fn remove_voice_pack(terminal: &mut Terminal<Backend>, theme: &str) -> Result<(), String> {
    let root = vesper_voice_kokoro::pack_root();
    let reclaim = vesper_voice_kokoro::pack::RETAINED_PACK_BYTES;
    let mib = format!("{:.1} MiB", reclaim as f64 / (1024.0 * 1024.0));
    match choice(
        terminal,
        "Remove voice pack",
        &format!(
            "Remove the Natural Voice pack? About {mib} of disk space is reclaimed. Only pack-owned files are deleted. The selected voice becomes unavailable unless you choose another speech engine and save; nothing switches automatically."
        ),
        &["Remove".into(), "Cancel".into()],
        theme,
    )
    .await?
    {
        Some(0) => {
            vesper_voice_kokoro::setup::remove(&root).map_err(|error| error.message())?;
            choice(
                terminal,
                "Remove voice pack",
                "Voice pack removed.",
                &["Back".into()],
                theme,
            )
            .await?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Details: versions, provenance, licenses, architecture, and the
/// managed location (engine details available, never forced).
#[cfg(feature = "voice-kokoro")]
async fn pack_details(terminal: &mut Terminal<Backend>, theme: &str) -> Result<(), String> {
    let location = vesper_voice_kokoro::pack_root();
    let body = format!(
        "Voice model: Kokoro-82M (q8f16 ONNX export), revision {}\nVoices: Heart (af_heart), Michael (am_michael)\nPronunciation: espeak-ng IPA (your system installation)\nInference runtime: ONNX Runtime {} (CPU, x86_64 Linux)\n\nLicenses: model/voices/export Apache-2.0; ONNX Runtime MIT; espeak-ng GPL-3.0 (system component, used at a process boundary)\nNotices are shown in the application about screen and preserved beside the runtime library.\nManaged location: {}\n\nThe local neural voice does not change your separately configured main reasoning provider.",
        vesper_voice_kokoro::PINNED_REVISION,
        vesper_voice_kokoro::RUNTIME_VERSION,
        location.display(),
    );
    choice(
        terminal,
        "Natural Voice pack · Details",
        &body,
        &["Back".into()],
        theme,
    )
    .await?;
    Ok(())
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
                Ok(image) => {
                    hub.config.driver_image = Some(image);
                    hub.notice =
                        "Browser and isolation ready. Leave Settings to save your web choices."
                            .into();
                }
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
/// VB-PRD-001: native Settings panel for Bridge activation. House rule:
/// feature activation belongs in Settings, never in hand-edited JSON.
/// Single draft; saved with the other Settings choices via Save changes /
/// Discard changes on exit. Enabling constructs only the no-adapter tool
/// surface — no drivers, processes or transports are started.
#[cfg(feature = "bridge")]
async fn edit_bridge(
    terminal: &mut Terminal<Backend>,
    draft: &mut vesper_harness::bridge_settings::BridgeSettings,
    theme: &str,
) -> Result<(), String> {
    loop {
        let rows = vec![
            format!("Bridge: {}", if draft.enabled { "ON" } else { "OFF" }),
            "Back".into(),
        ];
        match choice(
            terminal,
            "Settings › Bridge",
            "Application control (experimental). ON advertises the Bridge tools for the next launch; enabling constructs no adapter, driver or process. Restart the host to apply.",
            &rows,
            theme,
        )
        .await?
        {
            Some(0) => draft.enabled = !draft.enabled,
            _ => return Ok(()),
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
        let surface = super::super::tests::palette_surface();
        for command in [
            "/permission bypass",
            "/mode ask",
            "/model glm-5.2",
            "/thinking high",
            "/generation precise",
            "/auxiliary glm-4.7",
            "/mixture enabled",
        ] {
            remember(&mut choices, &provider, &surface, command);
        }
        assert!(
            !root.path().join("settings.json").exists(),
            "draft must not write"
        );
        save(root.path(), &choices).unwrap();
        let choices = load(root.path()).unwrap();
        let mut state = SessionState::new();
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
