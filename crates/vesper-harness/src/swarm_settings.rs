//! Explicit, workspace-scoped swarm preferences shared by both native hosts.
//! Reading and editing a draft never creates state or starts a worker.
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use vesper_swarm::topology::TopologyKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmSettings {
    pub enabled: bool,
    pub topology: TopologyKind,
    pub drivers: u32,
    pub failover: bool,
    #[serde(default)]
    pub shared_scope: bool,
}

impl Default for SwarmSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            topology: TopologyKind::Mesh,
            drivers: 3,
            failover: false,
            shared_scope: false,
        }
    }
}

impl SwarmSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(3..=8).contains(&self.drivers) {
            return Err("Swarm requires between 3 and 8 drivers.".into());
        }
        Ok(())
    }
}

fn check_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("Swarm settings require an existing absolute workspace root.".into());
    }
    for path in [
        root.join(".agent-vesper"),
        root.join(".agent-vesper/swarm-settings.json"),
    ] {
        match path.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("Refusing swarm settings symlink.".into());
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("Cannot inspect swarm settings: {}", error.kind()));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn load(root: &Path) -> Result<SwarmSettings, String> {
    check_root(root)?;
    let path = root.join(".agent-vesper/swarm-settings.json");
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(format!("Cannot read swarm settings: {}", error.kind())),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 4096 {
        return Err("Swarm settings exceed the 4 KiB limit.".into());
    }
    let settings: SwarmSettings = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid swarm settings; execution is disabled.".to_string())?;
    settings.validate()?;
    Ok(settings)
}

/// Persist an explicit Save. This records preference only: every execution must
/// separately validate configured embeddings, backend capability and permission.
pub fn save(root: &Path, settings: &SwarmSettings) -> Result<(), String> {
    settings.validate()?;
    check_root(root)?;
    let directory = root.join(".agent-vesper");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, settings).map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist(root.join(".agent-vesper/swarm-settings.json"))
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// ACP text controls and the TUI modal edit the same draft. Save/Cancel are
/// explicit and navigation/toggles have no filesystem side effect.
#[derive(Debug, Clone)]
pub struct SwarmSettingsDraft {
    pub settings: SwarmSettings,
}

/// Per-session command state. Hosts keep it in their existing session owner;
/// a cancelled settings dialog cannot affect another session's draft.
#[derive(Default)]
pub struct SwarmControls {
    draft: Option<SwarmSettingsDraft>,
    root: Option<std::path::PathBuf>,
}

pub enum SwarmCommandOutcome {
    Text(String),
    Run(String),
}

impl SwarmControls {
    pub fn command(&mut self, root: &Path, argument: &str) -> Result<SwarmCommandOutcome, String> {
        check_root(root)?;
        let canonical = root
            .canonicalize()
            .map_err(|_| "Cannot resolve workspace root.")?;
        if self.root.as_ref() != Some(&canonical) {
            self.draft = None;
            self.root = Some(canonical);
        }
        let argument = argument.trim();
        if let Some(goal) = argument.strip_prefix("run ") {
            if goal.trim().is_empty() || goal.len() > 65536 {
                return Err("A swarm goal must contain 1–65536 bytes.".into());
            }
            if !load(root)?.enabled {
                return Err("Swarm is off. Open Settings → Swarm, enable it and Save.".into());
            }
            return Ok(SwarmCommandOutcome::Run(goal.trim().into()));
        }
        if argument == "settings cancel" {
            self.draft = None;
            return Ok(SwarmCommandOutcome::Text(
                "Swarm settings cancelled; nothing saved.".into(),
            ));
        }
        if argument == "settings save" {
            self.draft
                .as_ref()
                .ok_or("Open swarm settings before saving.")?
                .save(root)?;
            self.draft = None;
            return Ok(SwarmCommandOutcome::Text("Swarm preferences saved. Every run still requires available embeddings, isolation and tool permission.".into()));
        }
        if argument == "settings" || argument.starts_with("settings ") {
            if self.draft.is_none() {
                self.draft = Some(SwarmSettingsDraft::open(root)?);
            }
            let draft = self.draft.as_mut().expect("opened draft");
            if let Some(edit) = argument.strip_prefix("settings ") {
                draft.apply(edit)?;
            }
            return Ok(SwarmCommandOutcome::Text(format!(
                "Swarm draft: enabled={}, drivers={}, topology={:?}, failover={}, scope={}.\n/swarm settings enabled on|off; drivers 3–8; topology mesh|hierarchical|centralized|hybrid; failover on|off; scope isolated|shared.\n/swarm settings save or /swarm settings cancel.",
                draft.settings.enabled,
                draft.settings.drivers,
                draft.settings.topology,
                draft.settings.failover,
                if draft.settings.shared_scope {
                    "shared"
                } else {
                    "isolated"
                }
            )));
        }
        if argument.is_empty() || argument == "status" {
            let saved = load(root)?;
            return Ok(SwarmCommandOutcome::Text(format!(
                "Swarm saved preference: enabled={}, drivers={}, topology={:?}, failover={}, scope={}. Availability is checked before each run.\n/swarm settings · /swarm run <goal>",
                saved.enabled,
                saved.drivers,
                saved.topology,
                saved.failover,
                if saved.shared_scope {
                    "shared"
                } else {
                    "isolated"
                }
            )));
        }
        Err("Usage: /swarm status|settings|run <goal>".into())
    }
}
impl SwarmSettingsDraft {
    pub fn open(root: &Path) -> Result<Self, String> {
        Ok(Self {
            settings: load(root)?,
        })
    }
    pub fn apply(&mut self, argument: &str) -> Result<(), String> {
        let words: Vec<_> = argument.split_whitespace().collect();
        match words.as_slice() {
            ["enabled", value @ ("on" | "off")] => self.settings.enabled = *value == "on",
            ["failover", value @ ("on" | "off")] => self.settings.failover = *value == "on",
            ["scope", value @ ("isolated" | "shared")] => self.settings.shared_scope = *value == "shared",
            ["drivers", value] => {
                let drivers = value.parse::<u32>().map_err(|_| "Drivers must be 3–8.")?;
                if !(3..=8).contains(&drivers) { return Err("Drivers must be 3–8.".into()); }
                self.settings.drivers = drivers;
            }
            ["topology", value] => self.settings.topology = match *value {
                "mesh" => TopologyKind::Mesh,
                "hierarchical" => TopologyKind::Hierarchical,
                "centralized" => TopologyKind::Centralized,
                "hybrid" => TopologyKind::Hybrid,
                _ => return Err("Unknown swarm topology.".into()),
            },
            _ => return Err("Use enabled on|off, failover on|off, drivers 3–8, scope isolated|shared, or topology mesh|hierarchical|centralized|hybrid.".into()),
        }
        Ok(())
    }
    pub fn save(&self, root: &Path) -> Result<(), String> {
        save(root, &self.settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_drafts_cannot_cross_workspaces_or_sessions() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut controls = SwarmControls::default();
        controls
            .command(first.path(), "settings enabled on")
            .unwrap();
        assert!(controls.command(first.path(), "run goal").is_err());
        assert!(
            SwarmControls::default()
                .command(first.path(), "settings save")
                .is_err()
        );
        assert!(controls.command(second.path(), "settings save").is_err());
        assert!(!first.path().join(".agent-vesper").exists());
        assert!(!second.path().join(".agent-vesper").exists());
        controls
            .command(second.path(), "settings enabled on")
            .unwrap();
        controls.command(second.path(), "settings save").unwrap();
        assert!(matches!(
            controls.command(second.path(), "run goal").unwrap(),
            SwarmCommandOutcome::Run(_)
        ));
        assert!(!load(first.path()).unwrap().enabled);
    }

    #[test]
    fn draft_cancel_and_status_are_read_only_and_save_preserves_other_config() {
        let root = tempfile::tempdir().unwrap();
        assert!(!load(root.path()).unwrap().enabled);
        let mut draft = SwarmSettingsDraft::open(root.path()).unwrap();
        draft.apply("enabled on").unwrap();
        assert!(draft.settings.enabled);
        assert!(!root.path().join(".agent-vesper").exists());
        std::fs::create_dir(root.path().join(".agent-vesper")).unwrap();
        let config = root.path().join(".agent-vesper/config.toml");
        std::fs::write(&config, "[other]\nvalue = 7\n").unwrap();
        let mut draft = SwarmSettingsDraft::open(root.path()).unwrap();
        draft.apply("enabled on").unwrap();
        assert!(draft.apply("drivers 999").is_err());
        assert_eq!(draft.settings.drivers, 3);
        draft.save(root.path()).unwrap();
        assert_eq!(load(root.path()).unwrap(), draft.settings);
        assert_eq!(
            std::fs::read_to_string(config).unwrap(),
            "[other]\nvalue = 7\n"
        );
    }

    #[test]
    fn malformed_settings_fail_closed_without_rewrite() {
        let root = tempfile::tempdir().unwrap();
        save(root.path(), &Default::default()).unwrap();
        let path = root.path().join(".agent-vesper/swarm-settings.json");
        for invalid in ["{}".to_owned(), "x".repeat(4097)] {
            std::fs::write(&path, &invalid).unwrap();
            assert!(load(root.path()).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        }
    }

    #[cfg(unix)]
    #[test]
    fn settings_directory_alias_never_reads_or_writes_target() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join(".agent-vesper")).unwrap();
        assert!(load(root.path()).is_err());
        assert!(save(root.path(), &Default::default()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
