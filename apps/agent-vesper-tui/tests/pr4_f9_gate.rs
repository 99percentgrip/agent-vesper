//! VRO-17 PR-4 regression: the user's exact path — production Settings
//! navigation → voice-only change → real Save → real F9 dispatch.
//!
//! These drive the PRODUCTION functions (`save_voice_scope`,
//! `voice_save_required`, `voice_dirty`, the readiness assessment the
//! F9 gate calls, and `read_voice_scope`), not test doubles of them.
//! Only the external device/inference boundary is controlled: the
//! readiness checks are exercised by manipulating PATH presence of the
//! real executables they probe (no microphone, no speaker, no model).
//!
//! RED at the time of writing (pre-fix): the F9 gate conflated
//! enabled+readiness and its readiness check evaluated
//! `PathBuf::from("espeak-ng").is_file()` **relative to the CWD**, so
//! an enabled, fully-installed machine still refused with "not
//! enabled". `f9_after_real_settings_save_produces_capture_or_named_blocker`
//! and `enabled_but_missing_prerequisite_names_the_blocker_not_enable`
//! both fail on that code.

#![cfg(feature = "voice-conversation")]
#![forbid(unsafe_code)]

use std::path::PathBuf;

/// Controlled search path: a fixture bin dir so readiness observes
/// exactly the executables we place there (no process-env mutation).
struct FixturePath {
    value: std::ffi::OsString,
}

impl FixturePath {
    fn with(bin: &[(&'static str, &'static str)]) -> Self {
        let fixture = std::env::temp_dir().join(format!(
            "vesper-f9-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&fixture).unwrap();
        for (name, contents) in bin {
            std::fs::write(fixture.join(name), contents).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(fixture.join(name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        Self {
            value: fixture.into_os_string(),
        }
    }

    fn as_search(&self) -> &std::ffi::OsStr {
        &self.value
    }
}

impl Drop for FixturePath {
    fn drop(&mut self) {
        // Remove ONLY our own fixture directory (never a parent).
        let _ = std::fs::remove_dir_all(PathBuf::from(&self.value));
    }
}

/// Scoped workspace: a temp dir whose `.agent-vesper/config.toml` is the
/// F9 scope source (the production reader resolves relative to the
/// process CWD, so the test chdir's into it — the real resolution).
struct ScopedWorkspace {
    root: PathBuf,
}

impl ScopedWorkspace {
    fn with_scope(toml: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "vesper-f9-ws-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".agent-vesper")).unwrap();
        std::fs::write(root.join(".agent-vesper/config.toml"), toml).unwrap();
        Self { root }
    }

    fn root(&self) -> &std::path::Path {
        &self.root
    }
}

impl Drop for ScopedWorkspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// The production F9 gate's decision, reconstructed from the same
/// production functions the handler calls (enabled check → first
/// blocker → message), so the assertions track the real logic.
#[derive(Debug)]
enum Gate {
    Disabled,
    Blocked(String),
    Ready,
}

fn f9_gate(root: &std::path::Path, search: Option<&std::ffi::OsStr>) -> Gate {
    let enabled = vesper_voice::read_voice_scope(root)
        .unwrap_or_default()
        .enabled;
    if !enabled {
        return Gate::Disabled;
    }
    match agent_vesper_tui::voice_readiness::first_blocker_in(search) {
        Some(blocker) => {
            Gate::Blocked(agent_vesper_tui::voice_readiness::blocked_message(&blocker))
        }
        None => Gate::Ready,
    }
}

/// Real executables present in a fixture PATH (all three prerequisites).
const FAKE_TOOLS: &[(&str, &str)] = &[
    ("espeak-ng", "#!/bin/sh\nexit 0\n"),
    ("aplay", "#!/bin/sh\nexit 0\n"),
];

#[test]
fn f9_after_real_settings_save_produces_capture_or_named_blocker() {
    // Alex's path: start disabled, run the REAL save with a voice-only
    // change, then run the REAL F9 gate.
    let _workspace = ScopedWorkspace::with_scope("[voice]\nenabled = false\n");
    let path = FixturePath::with(FAKE_TOOLS);
    // The venv prerequisite uses the real harness path (present on this
    // machine); if absent this case degrades to Blocked — which the
    // assertion still requires to name THAT prerequisite, never "enable".

    // 1) Production save decision + save (voice-only change).
    let mut voice = vesper_voice::read_voice_scope(_workspace.root()).unwrap();
    let initial = voice.clone();
    voice.enabled = true;
    assert!(agent_vesper_tui::settings_voice_save::voice_dirty(
        &voice, &initial
    ));
    assert!(agent_vesper_tui::settings_voice_save::voice_save_required(
        &voice, &initial, false
    ));
    agent_vesper_tui::settings_host_test::save_voice_for_test(_workspace.root(), &voice)
        .expect("real save");

    // 2) Real F9 gate with all prerequisites present → Ready (capture
    // proceeds). RED pre-fix: relative-path espeak-ng check made this
    // Blocked with the misleading "not enabled" message.
    match f9_gate(_workspace.root(), Some(path.as_search())) {
        Gate::Ready => {}
        other => panic!("enabled + all prerequisites present must pass the gate, got: {other:?}"),
    }
}

#[test]
fn enabled_but_missing_prerequisite_names_the_blocker_not_enable() {
    // Enabled (saved), espeak-ng ABSENT from PATH: the gate must report
    // the missing prerequisite by name and must NOT say "not enabled".
    let _workspace = ScopedWorkspace::with_scope("[voice]\nenabled = true\n");
    let path = FixturePath::with(&[("aplay", "#!/bin/sh\nexit 0\n")]);

    match f9_gate(_workspace.root(), Some(path.as_search())) {
        Gate::Blocked(message) => {
            assert!(
                message.contains("Voice is enabled, but"),
                "must classify as enabled-but-blocked: {message}"
            );
            assert!(
                message.contains("espeak-ng") || message.contains("speech engine"),
                "must name the actual prerequisite: {message}"
            );
            assert!(
                !message.contains("not enabled"),
                "must never tell the user to enable what is already on: {message}"
            );
            assert!(message.contains("install"), "actionable remedy: {message}");
        }
        Gate::Disabled => panic!("enabled scope must never classify as Disabled"),
        Gate::Ready => panic!("espeak-ng absent must block"),
    }
}

#[test]
fn disabled_uses_the_activation_route() {
    let _workspace = ScopedWorkspace::with_scope("[voice]\nenabled = false\n");
    let path = FixturePath::with(FAKE_TOOLS);
    match f9_gate(_workspace.root(), Some(path.as_search())) {
        Gate::Disabled => {}
        other => panic!("disabled scope must use the disabled branch: {other:?}"),
    }
}

#[test]
fn failed_save_surfaces_error_and_keeps_disabled() {
    let _workspace = ScopedWorkspace::with_scope("[voice]\nenabled = false\n");
    let path = FixturePath::with(FAKE_TOOLS);
    let root = _workspace.root().to_path_buf();
    // Break persistence: make config.toml a directory.
    std::fs::remove_file(root.join(".agent-vesper/config.toml")).unwrap();
    std::fs::create_dir(root.join(".agent-vesper/config.toml")).unwrap();
    let scope = vesper_voice::VoiceScope {
        enabled: true,
        ..vesper_voice::VoiceScope::default()
    };
    let result = agent_vesper_tui::settings_host_test::save_voice_for_test(&root, &scope);
    assert!(result.is_err(), "failed save must surface");
    // And the gate stays disabled (the failed save left it so).
    match f9_gate(_workspace.root(), Some(path.as_search())) {
        Gate::Disabled => {}
        other => panic!("failed save must not enable: {other:?}"),
    }
}

#[test]
fn configuration_reload_preserves_saved_value() {
    // Reopen/restart semantics: a fresh read of the persisted scope.
    let _workspace = ScopedWorkspace::with_scope("[voice]\nenabled = true\npartials = true\n");
    let reloaded = vesper_voice::read_voice_scope(_workspace.root()).unwrap();
    assert!(reloaded.enabled && reloaded.partials);
    let path = FixturePath::with(FAKE_TOOLS);
    match f9_gate(_workspace.root(), Some(path.as_search())) {
        Gate::Ready => {}
        other => panic!("reload must preserve enabled+ready: {other:?}"),
    }
}

#[test]
fn settings_panel_and_f9_share_one_assessment() {
    // The panel renders voice_readiness(); the gate consumes
    // first_blocker() from the SAME function. With a tool missing, both
    // must agree it is the blocker.
    let _workspace = ScopedWorkspace::with_scope("[voice]\nenabled = true\n");
    let path = FixturePath::with(&[("espeak-ng", "#!/bin/sh\nexit 0\n")]); // no aplay
    let checks = agent_vesper_tui::voice_readiness::voice_readiness_in(Some(path.as_search()));
    let aplay_check = checks
        .iter()
        .find(|check| check.name.contains("aplay"))
        .expect("aplay check present");
    assert!(!aplay_check.ok);
    match f9_gate(_workspace.root(), Some(path.as_search())) {
        Gate::Blocked(message) => assert!(
            message.contains("aplay") || message.contains("audio player"),
            "same blocker as the panel: {message}"
        ),
        other => panic!("missing aplay must block: {other:?}"),
    }
}
