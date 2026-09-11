//! Explicit native preferences. Loading a default never writes workspace state.
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceSettings {
    pub enabled: bool,
    pub prd: String,
}

fn location(root: &Path) -> Result<std::path::PathBuf, String> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("acceptance settings require an existing workspace".into());
    }
    for path in [
        root.join(".agent-vesper"),
        root.join(".agent-vesper/acceptance-settings.json"),
    ] {
        match path.symlink_metadata() {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("acceptance settings symlinks are refused".into());
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err("cannot inspect acceptance settings".into());
            }
            _ => {}
        }
    }
    Ok(root.join(".agent-vesper/acceptance-settings.json"))
}

impl AcceptanceSettings {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = location(root)?;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err("cannot read acceptance settings".into()),
        };
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| "cannot read acceptance settings")?;
        if bytes.len() > 4096 {
            return Err("acceptance settings exceed 4 KiB".into());
        }
        let settings: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "invalid acceptance settings; execution refused")?;
        settings.validate(root)?;
        Ok(settings)
    }
    pub fn validate(&self, root: &Path) -> Result<(), String> {
        if self.prd.len() > 2048 {
            return Err("PRD path exceeds 2048 bytes".into());
        }
        if self.enabled {
            let path = vesper_agent::confinement::confine(root, &self.prd)
                .map_err(|_| "PRD must be inside workspace")?;
            if self.prd.is_empty() || !path.is_file() {
                return Err("select an existing PRD before enabling acceptance".into());
            }
        }
        Ok(())
    }
    pub fn save(&self, root: &Path) -> Result<(), String> {
        self.validate(root)?;
        let path = location(root)?;
        let parent = path.parent().ok_or("invalid settings path")?;
        std::fs::create_dir_all(parent).map_err(|_| "cannot create settings directory")?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot stage settings")?;
        serde_json::to_writer(&mut file, self).map_err(|_| "cannot encode settings")?;
        file.as_file()
            .sync_all()
            .map_err(|_| "cannot sync settings")?;
        file.persist(path).map_err(|_| "cannot save settings")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_cancel_write_nothing_explicit_save_roundtrips() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = AcceptanceSettings::load(root.path()).unwrap();
        assert!(!settings.enabled);
        settings.enabled = true;
        assert!(settings.save(root.path()).is_err());
        assert!(!root.path().join(".agent-vesper").exists());
        std::fs::write(root.path().join("PRD.md"), "Required behavior").unwrap();
        settings.prd = "PRD.md".into();
        settings.save(root.path()).unwrap();
        assert_eq!(settings, AcceptanceSettings::load(root.path()).unwrap());
    }
}
