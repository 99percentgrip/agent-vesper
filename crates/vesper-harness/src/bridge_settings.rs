//! VB-PRD-001 Phase 2: Bridge settings persistence and shared resolution.
//!
//! Mirrors the web-tools pattern: an explicit, user-owned JSON settings
//! file under `.agent-vesper/`, read through a small vesper-config port,
//! with a holder both hosts consult through one code path. Default is
//! **off**: a missing file, an unreadable/malformed file, or an explicit
//! `false` all leave Bridge disabled (BR-30, NF-01, AT-01).
//!
//! Enabling Bridge only constructs the no-adapter service — it never
//! installs drivers, launches applications, opens transports or spawns
//! processes. Enabling Bridge with no adapter remains a truthful
//! no-capability state.

use std::path::Path;
/// Persisted Bridge settings (explicit user-owned file).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BridgeSettings {
    /// Whether the Bridge tool surface is advertised at all. Default off.
    pub enabled: bool,
}

impl BridgeSettings {
    /// The settings file path for a workspace root.
    fn path(root: &Path) -> PathBuf {
        root.join(".agent-vesper").join("bridge-settings.json")
    }

    /// Load settings. Any read/parse problem yields the disabled default —
    /// a broken settings file must not enable a control surface
    /// (fail-closed).
    pub fn load(root: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::path(root)) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Save after an explicit user action, atomically, refusing symlinked
    /// state directories (same discipline as the web settings).
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let dir = root.join(".agent-vesper");
        if dir
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("Refusing settings directory symlink.".into());
        }
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut file, self).map_err(|error| error.to_string())?;
        file.as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        file.persist(Self::path(root))
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

/// Host-neutral holder so TUI, ACP and spawned workers observe one
/// resolution of the Bridge setting per process.
pub mod holder {
    use super::BridgeSettings;
    static SETTINGS: std::sync::OnceLock<BridgeSettings> = std::sync::OnceLock::new();

    /// Resolve once from the primary workspace root. Subsequent calls
    /// return the cached value; a failure resolves to disabled.
    pub fn shared(root: &std::path::Path) -> BridgeSettings {
        *SETTINGS.get_or_init(|| BridgeSettings::load(root))
    }

    /// Test hook: pre-seed the holder before any `shared` call.
    #[cfg(test)]
    pub fn seed_for_tests(settings: BridgeSettings) {
        let _ = SETTINGS.set(settings);
    }
}

use std::path::PathBuf;

/// VB-PRD-001: shared text controls for Bridge settings, so both hosts
/// answer `/settings bridge` with identical semantics (BR-21 parity).
/// Like swarm: draft → explicit save; no filesystem effect until save.
#[derive(Debug, Clone)]
pub struct BridgeSettingsDraft {
    pub settings: BridgeSettings,
}

impl BridgeSettingsDraft {
    fn open(root: &Path) -> Self {
        Self {
            settings: BridgeSettings::load(root),
        }
    }
    fn apply(&mut self, argument: &str) -> Result<(), String> {
        let words: Vec<_> = argument.split_whitespace().collect();
        match words.as_slice() {
            ["enabled", value @ ("on" | "off")] => self.settings.enabled = *value == "on",
            _ => {
                return Err("Use enabled on|off.".into());
            }
        }
        Ok(())
    }
}

/// Per-session command state for the ACP text surface (the TUI uses the
/// native panel; both edit the same persisted shape).
#[derive(Default)]
pub struct BridgeControls {
    draft: Option<BridgeSettingsDraft>,
    root: Option<std::path::PathBuf>,
}

impl BridgeControls {
    /// Answer `/settings bridge [argument]` (ACP) with swarm-like
    /// draft/save/cancel semantics.
    pub fn command(&mut self, root: &Path, argument: &str) -> Result<String, String> {
        let canonical = root
            .canonicalize()
            .map_err(|_| "Cannot resolve workspace root.")?;
        if self.root.as_ref() != Some(&canonical) {
            self.draft = None;
            self.root = Some(canonical);
        }
        let argument = argument.trim();
        if argument == "cancel" {
            self.draft = None;
            return Ok("Bridge settings cancelled; nothing saved.".into());
        }
        if argument == "save" {
            self.draft
                .as_ref()
                .ok_or("Open bridge settings before saving.")?
                .settings
                .save(root)?;
            self.draft = None;
            return Ok(
                "Bridge preferences saved. Restart the host to apply; enabling constructs no adapter, driver or process.".into(),
            );
        }
        if argument.is_empty() || argument == "status" {
            let saved = BridgeSettings::load(root);
            return Ok(format!(
                "Bridge saved preference: enabled={}. Restart applies changes. Enabling constructs only the no-adapter tool surface.\n/settings bridge enabled on|off · /settings bridge save · /settings bridge cancel",
                saved.enabled
            ));
        }
        if self.draft.is_none() {
            self.draft = Some(BridgeSettingsDraft::open(root));
        }
        let draft = self.draft.as_mut().expect("opened draft");
        if let Some(edit) = argument.strip_prefix("enabled ") {
            draft.apply(&format!("enabled {edit}"))?;
        } else {
            return Err("Use enabled on|off, save, or cancel.".into());
        }
        Ok(format!(
            "Bridge draft: enabled={}.\n/settings bridge enabled on|off · /settings bridge save · /settings bridge cancel",
            draft.settings.enabled
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_disable_bridge() {
        let root = tempfile::tempdir().unwrap();
        assert!(!BridgeSettings::load(root.path()).enabled);
    }

    #[test]
    fn malformed_settings_fail_closed_to_disabled() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".agent-vesper")).unwrap();
        std::fs::write(BridgeSettings::path(root.path()), "{ not json").unwrap();
        assert!(!BridgeSettings::load(root.path()).enabled);
    }

    #[test]
    fn unknown_fields_are_refused_so_typos_cannot_silently_enable() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".agent-vesper")).unwrap();
        std::fs::write(BridgeSettings::path(root.path()), r#"{"enbled": true}"#).unwrap();
        assert!(
            !BridgeSettings::load(root.path()).enabled,
            "deny_unknown_fields must reject misspelled keys"
        );
    }

    #[test]
    fn explicit_false_and_true_round_trip() {
        let root = tempfile::tempdir().unwrap();
        BridgeSettings { enabled: true }.save(root.path()).unwrap();
        assert!(BridgeSettings::load(root.path()).enabled);
        BridgeSettings { enabled: false }.save(root.path()).unwrap();
        assert!(!BridgeSettings::load(root.path()).enabled);
    }

    #[test]
    fn save_does_not_touch_other_host_settings() {
        let root = tempfile::tempdir().unwrap();
        let web = root.path().join(".agent-vesper").join("web-settings.json");
        std::fs::create_dir_all(web.parent().unwrap()).unwrap();
        std::fs::write(&web, r#"{"enabled":true}"#).unwrap();
        BridgeSettings { enabled: true }.save(root.path()).unwrap();
        assert!(
            std::fs::read_to_string(&web).unwrap().contains("true"),
            "web settings untouched"
        );
        assert!(BridgeSettings::load(root.path()).enabled);
    }

    #[test]
    fn controls_draft_save_cancel_mirror_the_tui_panel() {
        // VB-PRD-001 BR-21: the ACP text controls must share semantics with
        // the TUI native panel — draft until explicit save, cancel writes
        // nothing, status is read-only.
        let root = tempfile::tempdir().unwrap();
        let mut controls = BridgeControls::default();

        // status is read-only and reports the saved (off) state
        let status = controls.command(root.path(), "status").unwrap();
        assert!(status.starts_with("Bridge saved preference: enabled=false"));

        // drafting ON does not touch disk
        controls.command(root.path(), "enabled on").unwrap();
        assert!(!root.path().join(".agent-vesper").exists());
        assert!(status.contains("/settings bridge"));

        // cancel writes nothing
        controls.command(root.path(), "cancel").unwrap();
        assert!(!BridgeSettings::load(root.path()).enabled);

        // save round-trips and is honest about restart semantics
        controls.command(root.path(), "enabled on").unwrap();
        let saved = controls.command(root.path(), "save").unwrap();
        assert!(saved.contains("Restart the host to apply"));
        assert!(BridgeSettings::load(root.path()).enabled);

        // drafts cannot cross workspaces: switching roots resets the draft,
        // so a save must fail until the new workspace drafts explicitly
        let other = tempfile::tempdir().unwrap();
        assert!(
            controls.command(other.path(), "save").is_err(),
            "a draft from another workspace must not save here"
        );
        assert!(
            !BridgeSettings::load(other.path()).enabled,
            "workspace A's draft must not enable workspace B"
        );
    }

    #[test]
    fn controls_reject_unknown_edits() {
        let root = tempfile::tempdir().unwrap();
        let mut controls = BridgeControls::default();
        assert!(controls.command(root.path(), "enabled maybe").is_err());
        assert!(controls.command(root.path(), "explode").is_err());
    }

    #[test]
    fn disabled_status_points_to_native_settings_not_hand_edited_files() {
        // House rule: feature activation belongs in Settings; the disabled
        // answer must never instruct hand-editing a JSON file.
        let root = tempfile::tempdir().unwrap();
        let text = crate::bridge_command::status_text(Some(root.path()));
        assert!(text.contains("/settings"), "must point at Settings: {text}");
        assert!(
            !text.contains("bridge-settings.json"),
            "must not instruct hand-editing: {text}"
        );
    }
}
