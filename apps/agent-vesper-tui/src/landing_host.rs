//! Startup landing event loop with explicit update check and installation consent.
use super::*;
use agent_vesper_tui::landing::{self, LandingAction, LandingState};

pub(super) async fn open(
    terminal: &mut Terminal<Backend>,
    provider: &str,
    model: &str,
    theme: &str,
) -> Result<LandingAction, String> {
    let mut state = LandingState::default();
    let mut task: Option<tokio::task::JoinHandle<Result<ReleaseCheck, String>>> = None;
    let started = std::time::Instant::now();
    let result = async {
        loop {
            if task.as_ref().is_some_and(|task| task.is_finished()) {
                let result = task.take().expect("finished task exists").await;
                state.checking = false;
                state.notice = match result {
                    Ok(Ok(check)) => {
                        if let Some(tag) = check.update {
                            match update_host::install(terminal, &tag, theme).await {
                                Ok(true) => return Ok(LandingAction::Quit),
                                Ok(false) => {},
                                Err(error) => { state.notice = error; continue; }
                            }
                        }
                        check.notice
                    },
                    Ok(Err(error)) => error,
                    Err(_) => "Update check interrupted; press U to retry.".into(),
                };
            }
            terminal.draw(|frame| landing::render(frame, &state, provider, model, env!("CARGO_PKG_VERSION"), (started.elapsed().as_millis() / 125) as u64, theme)).map_err(|error| error.to_string())?;
            if !event::poll(std::time::Duration::from_millis(100)).map_err(|error| error.to_string())? {
                continue;
            }
            let action = match event::read().map_err(|error| error.to_string())? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(LandingAction::Quit),
                    KeyCode::Esc | KeyCode::Char('q') => Some(LandingAction::Quit),
                    KeyCode::Up | KeyCode::BackTab => { state.navigate(false); None }
                    KeyCode::Down | KeyCode::Tab => { state.navigate(true); None }
                    KeyCode::Enter => Some(state.activate()),
                    KeyCode::Char('c') => Some(LandingAction::Code),
                    KeyCode::Char('s') => Some(LandingAction::Settings),
                    KeyCode::Char('u') => Some(LandingAction::CheckUpdates),
                    KeyCode::Char('r') => {
                        state.notice = match lens_opener_command(landing::RELEASES_URL).spawn() {
                            Ok(mut child) => {
                                // Reap the opener without blocking terminal input.
                                std::thread::spawn(move || { let _ = child.wait(); });
                                "Requested release page in your browser.".into()
                            }
                            Err(_) => "Could not open browser. Visit github.com/99percentgrip/agent-vesper/releases".into(),
                        };
                        None
                    }
                    _ => None,
                },
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    let size = terminal.size().map_err(|error| error.to_string())?;
                    if size.width >= 30 && size.height >= 14 {
                        landing::action_at(ratatui::layout::Rect::new(0, 0, size.width, size.height), mouse.column, mouse.row).map(|index| { state.selected = index; state.activate() })
                    } else { None }
                }
                _ => None,
            };
            match action {
                Some(LandingAction::CheckUpdates) if task.is_none() => {
                    state.checking = true;
                    task = Some(tokio::spawn(check_latest_release()));
                }
                Some(LandingAction::CheckUpdates) | None => {}
                Some(action) => return Ok(action),
            }
        }
    }.await;
    if let Some(task) = task {
        task.abort();
    }
    result
}

struct ReleaseCheck {
    notice: String,
    update: Option<String>,
}

async fn check_latest_release() -> Result<ReleaseCheck, String> {
    // Public, credential-free GitHub API; no provider calls or project data.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize update check.".to_owned())?;
    let mut response = client
        .get("https://api.github.com/repos/99percentgrip/agent-vesper/releases/latest")
        .header(
            "User-Agent",
            concat!("agent-vesper-tui/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| {
            "Update check unavailable (network or timeout). Press U to retry.".to_owned()
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub returned HTTP {}. Press U to retry.",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Release response interrupted. Press U to retry.".to_owned())?
    {
        if bytes.len() + chunk.len() > 262_144 {
            return Err("Release response too large. Press R for releases.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    release_response(&bytes)
}

fn release_response(bytes: &[u8]) -> Result<ReleaseCheck, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "Invalid release response. Press R for releases.".to_owned())?;
    if value.get("draft").and_then(serde_json::Value::as_bool) != Some(false)
        || value.get("prerelease").and_then(serde_json::Value::as_bool) != Some(false)
    {
        return Err("No stable release found. Press R for releases.".into());
    }
    let tag = value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or("Release version missing. Press R for releases.")?;
    if !update_host::valid_version(tag) {
        return Err("Invalid release version".into());
    }
    let notice = landing::release_notice(env!("CARGO_PKG_VERSION"), tag).map_err(str::to_owned)?;
    let update = notice
        .starts_with(&format!("{tag} available"))
        .then(|| tag.to_owned());
    Ok(ReleaseCheck { notice, update })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_api_response_requires_a_stable_real_version() {
        let good = serde_json::json!({"draft": false, "prerelease": false, "tag_name": env!("CARGO_PKG_VERSION")});
        assert!(
            release_response(&serde_json::to_vec(&good).unwrap())
                .unwrap()
                .notice
                .contains("Up to date")
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"draft": true, "prerelease": false, "tag_name": "v99.0.0"}),
            serde_json::json!({"draft": false, "prerelease": true, "tag_name": "v99.0.0"}),
            serde_json::json!({"draft": false, "prerelease": false, "tag_name": "oops"}),
        ] {
            assert!(release_response(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        assert!(release_response(b"not JSON").is_err());
        let newer = serde_json::json!({"draft":false, "prerelease":false, "tag_name":"v99.0.0"});
        assert_eq!(
            release_response(&serde_json::to_vec(&newer).unwrap())
                .unwrap()
                .update
                .as_deref(),
            Some("v99.0.0")
        );
        assert!(
            release_response(&serde_json::to_vec(&good).unwrap())
                .unwrap()
                .update
                .is_none()
        );
    }
}
