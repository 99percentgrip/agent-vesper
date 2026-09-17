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
}
