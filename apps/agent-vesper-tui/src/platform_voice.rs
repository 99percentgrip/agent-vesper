//! Platform paths for the host-owned local voice tools. No device or setup I/O.
use std::path::{Path, PathBuf};

/// The interpreter layout created by Python and uv on the current platform.
pub fn venv_python(root: &Path) -> PathBuf {
    venv_python_for(root, cfg!(windows))
}

fn venv_python_for(root: &Path, windows: bool) -> PathBuf {
    root.join(if windows {
        "Scripts/python.exe"
    } else {
        "bin/python"
    })
}

/// Per-user application data; explicit XDG overrides remain usable for isolation.
/// Missing native user paths return None; private audio never relocates to temp.
pub fn data_root() -> Option<PathBuf> {
    data_root_from(
        cfg!(windows),
        cfg!(target_os = "macos"),
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("LOCALAPPDATA").as_deref(),
        std::env::var_os("USERPROFILE").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .map(|root| root.join("agent-vesper"))
}

fn data_root_from(
    windows: bool,
    macos: bool,
    xdg: Option<&std::ffi::OsStr>,
    local: Option<&std::ffi::OsStr>,
    profile: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg.filter(|value| Path::new(value).is_absolute()) {
        return Some(xdg.into());
    }
    if windows {
        return local
            .filter(|value| Path::new(value).is_absolute())
            .map(PathBuf::from)
            .or_else(|| {
                profile
                    .filter(|value| Path::new(value).is_absolute())
                    .map(|value| PathBuf::from(value).join("AppData/Local"))
            });
    }
    home.filter(|value| Path::new(value).is_absolute())
        .map(|value| {
            PathBuf::from(value).join(if macos {
                "Library/Application Support"
            } else {
                ".local/share"
            })
        })
}

/// Resolve a bare executable without interpreting it as a shell command.
/// Windows executable suffixes are checked even when PATH contains spaces.
pub fn resolve_executable_in(name: &str, search: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    resolve_executable_for(name, search, cfg!(windows))
}

fn resolve_executable_for(
    name: &str,
    search: Option<&std::ffi::OsStr>,
    windows: bool,
) -> Option<PathBuf> {
    let path = Path::new(name);
    let variants = if windows && path.extension().is_none() {
        vec![PathBuf::from(name), PathBuf::from(format!("{name}.exe"))]
    } else {
        vec![path.to_path_buf()]
    };
    if path.is_absolute() || path.components().count() > 1 {
        return variants.into_iter().find(|candidate| candidate.is_file());
    }
    let search = search
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&search)
        .flat_map(|dir| variants.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn venv_layouts_match_native_python() {
        let root = Path::new("voice venv");
        assert_eq!(venv_python_for(root, true), root.join("Scripts/python.exe"));
        assert_eq!(venv_python_for(root, false), root.join("bin/python"));
    }
    #[test]
    fn executable_suffix_and_spaces_are_resolved() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("Program Files");
        std::fs::create_dir(&dir).unwrap();
        let executable = dir.join("espeak-ng.exe");
        std::fs::write(&executable, []).unwrap();
        let search = std::env::join_paths([&dir]).unwrap();
        assert_eq!(
            resolve_executable_for("espeak-ng", Some(&search), true),
            Some(executable)
        );
        assert_eq!(
            resolve_executable_for("espeak-ng", Some(&search), false),
            None
        );
    }
    #[test]
    fn native_user_data_roots_do_not_require_home_on_windows() {
        let temp = tempfile::tempdir().unwrap();
        let local = temp.path().as_os_str();
        assert_eq!(
            data_root_from(true, false, None, Some(local), None, None),
            Some(temp.path().to_path_buf())
        );
        assert_eq!(
            data_root_from(false, true, None, None, None, Some(local)),
            Some(temp.path().join("Library/Application Support"))
        );
    }
    #[test]
    fn missing_or_relative_user_data_never_relocates_private_audio() {
        assert!(data_root_from(true, false, None, None, None, None).is_none());
        assert!(data_root_from(false, false, None, None, None, None).is_none());
        assert!(
            data_root_from(
                true,
                false,
                None,
                Some(std::ffi::OsStr::new("relative")),
                None,
                None
            )
            .is_none()
        );
    }
}
