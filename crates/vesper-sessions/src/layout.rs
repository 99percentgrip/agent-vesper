use std::path::{Path, PathBuf};

use vesper_config::{ProfileName, VesperPaths};

use crate::SessionSource;

/// Descriptive Agent Vesper session layout. Construction performs no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentVesperSessionLayout {
    root: PathBuf,
}

impl AgentVesperSessionLayout {
    /// Places future session records under the platform-specific data root.
    #[must_use]
    pub fn from_paths(paths: &VesperPaths) -> Self {
        Self {
            root: paths.data.join("sessions"),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub const fn source(&self) -> SessionSource {
        SessionSource::AgentVesper
    }
}

/// Frozen Native GLM ACP session layout. Construction performs no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySessionLayout {
    root: PathBuf,
    profile: Option<ProfileName>,
}

impl LegacySessionLayout {
    /// Describes `~/.glm-acp/sessions/`.
    #[must_use]
    pub fn default_profile(home: &Path) -> Self {
        Self {
            root: home.join(".glm-acp/sessions"),
            profile: None,
        }
    }

    /// Describes `~/.glm-acp/profiles/<profile>/sessions/`.
    #[must_use]
    pub fn named_profile(home: &Path, profile: ProfileName) -> Self {
        let root = home
            .join(".glm-acp/profiles")
            .join(profile.as_str())
            .join("sessions");
        Self {
            root,
            profile: Some(profile),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn source(&self) -> SessionSource {
        SessionSource::LegacyNativeGlm {
            profile: self
                .profile
                .as_ref()
                .map(|profile| profile.as_str().to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use vesper_config::{PathEnvironment, Platform};

    use super::*;

    #[test]
    fn layouts_are_descriptive_and_match_frozen_roots() {
        let home = Path::new("/synthetic/home");
        assert_eq!(
            LegacySessionLayout::default_profile(home).root(),
            Path::new("/synthetic/home/.glm-acp/sessions")
        );
        assert_eq!(
            LegacySessionLayout::named_profile(home, ProfileName::new("work_2").unwrap()).root(),
            Path::new("/synthetic/home/.glm-acp/profiles/work_2/sessions")
        );

        let paths = VesperPaths::resolve(
            Platform::Linux,
            &PathEnvironment {
                home: Some(home.to_path_buf()),
                ..PathEnvironment::default()
            },
        )
        .unwrap();
        assert_eq!(
            AgentVesperSessionLayout::from_paths(&paths).root(),
            Path::new("/synthetic/home/.local/share/agent-vesper/sessions")
        );
    }

    #[test]
    fn vesper_session_root_follows_each_injected_platform_data_root() {
        let home = Path::new("/synthetic/home");
        let mac = VesperPaths::resolve(
            Platform::MacOs,
            &PathEnvironment {
                home: Some(home.to_path_buf()),
                ..PathEnvironment::default()
            },
        )
        .unwrap();
        assert_eq!(
            AgentVesperSessionLayout::from_paths(&mac).root(),
            Path::new("/synthetic/home/Library/Application Support/Agent Vesper/Data/sessions")
        );

        let windows = VesperPaths::resolve(
            Platform::Windows,
            &PathEnvironment {
                app_data: Some(Path::new("C:/Roaming").to_path_buf()),
                local_app_data: Some(Path::new("C:/Local").to_path_buf()),
                ..PathEnvironment::default()
            },
        )
        .unwrap();
        assert_eq!(
            AgentVesperSessionLayout::from_paths(&windows).root(),
            Path::new("C:/Local/Agent Vesper/Data/sessions")
        );
    }
}
