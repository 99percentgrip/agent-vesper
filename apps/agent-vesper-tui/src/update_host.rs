//! Explicit native update installation using the installer shipped in this build.
use super::*;
use std::path::Path;

pub(super) fn valid_version(tag: &str) -> bool {
    let parts: Vec<_> = tag.strip_prefix('v').unwrap_or(tag).split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 10
                && p.bytes().all(|c| c.is_ascii_digit())
                && p.parse::<u32>().is_ok()
        })
}

#[cfg(not(windows))]
fn installer_command(
    script: &Path,
    tag: &str,
    bundle: &Path,
) -> Result<tokio::process::Command, String> {
    if !valid_version(tag) {
        return Err("Invalid release version".into());
    }
    let mut command = tokio::process::Command::new("sh");
    command
        .arg(script)
        .env("AGENT_VESPER_VERSION", tag)
        .env("AGENT_VESPER_BUNDLE_DIR", bundle)
        .env_remove("AGENT_VESPER_RELEASE_BASE_URL")
        .stdin(std::process::Stdio::null());
    Ok(command)
}

/// Returns true when Vesper must close (installed or handed off on Windows).
pub(super) async fn install(
    terminal: &mut Terminal<Backend>,
    tag: &str,
    theme: &str,
) -> Result<bool, String> {
    if !valid_version(tag) {
        return Err("Invalid update version".into());
    }
    let exe = std::env::current_exe().map_err(|_| "Cannot locate this installation")?;
    let bundle = exe.parent().ok_or("Cannot locate this installation")?;
    let acp = if cfg!(windows) {
        "agent-vesper-acp.exe"
    } else {
        "agent-vesper-acp"
    };
    if !bundle.join(acp).is_file() {
        return Err("This executable is not in a complete installed Vesper bundle. Use the installer in the installation guide to create one.".into());
    }
    let detail = format!(
        "Install {tag} into {}? The installer verifies the package checksum and includes the web driver. Vesper will close after installation; reopen it to use the update.",
        bundle.display()
    );
    if settings_host::choice(
        terminal,
        "Update available",
        &detail,
        &["Install update".into(), "Not now".into()],
        theme,
    )
    .await?
        != Some(0)
    {
        return Ok(false);
    }
    install_confirmed(terminal, tag, theme, bundle).await
}

#[cfg(not(windows))]
async fn install_confirmed(
    terminal: &mut Terminal<Backend>,
    tag: &str,
    theme: &str,
    bundle: &Path,
) -> Result<bool, String> {
    let temp = tempfile::tempdir().map_err(|_| "Cannot stage updater")?;
    let script = temp.path().join("install.sh");
    std::fs::write(&script, include_str!("../../../scripts/install.sh"))
        .map_err(|_| "Cannot stage installer")?;
    let log_path = temp.path().join("install.log");
    let log = std::fs::File::create(&log_path).map_err(|_| "Cannot open updater log")?;
    let mut command = installer_command(&script, tag, bundle)?;
    command
        .stdout(log.try_clone().map_err(|_| "Cannot open updater log")?)
        .stderr(log);
    let mut child = command
        .spawn()
        .map_err(|_| "Could not start installer; sh is required")?;
    loop {
        let tail = log_tail(&log_path);
        terminal
            .draw(|f| {
                agent_vesper_tui::settings_menu::render_menu(
                    f,
                    &["Installing update…".into()],
                    0,
                    "Update in progress",
                    &tail,
                    "Please keep Vesper open while the installer finishes",
                    theme,
                )
            })
            .map_err(|e| e.to_string())?;
        if let Some(status) = child.try_wait().map_err(|_| "Cannot observe installer")? {
            let success = status.success();
            let title = if success {
                "Update installed"
            } else {
                "Update failed"
            };
            let detail = if success {
                format!("Installed {tag}. Reopen Vesper to use it. Restart editor agents too.")
            } else {
                format!("The installer failed. {}", log_tail(&log_path))
            };
            let _ = settings_host::choice(
                terminal,
                title,
                &detail,
                &[if success {
                    "Close Vesper".into()
                } else {
                    "Back".into()
                }],
                theme,
            )
            .await?;
            return Ok(success);
        }
        // Consume input to avoid replaying keys after an in-progress installation.
        if event::poll(std::time::Duration::from_millis(100)).map_err(|e| e.to_string())? {
            let _ = event::read();
        }
    }
}

#[cfg(not(windows))]
fn log_tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return "Starting verified installer…".into();
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(8192)));
    let mut bytes = Vec::new();
    let _ = file.take(8192).read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}

#[cfg(windows)]
async fn install_confirmed(
    _terminal: &mut Terminal<Backend>,
    tag: &str,
    _theme: &str,
    bundle: &Path,
) -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    // Windows locks running executables. A separate installer console waits for
    // this host to exit before replacing payloads, and displays the actual result.
    let temp = tempfile::tempdir().map_err(|_| "Cannot stage updater")?;
    let script = temp.path().join("install.ps1");
    std::fs::write(&script, include_str!("../../../scripts/install.ps1"))
        .map_err(|_| "Cannot stage installer")?;
    let wrapper = temp.path().join("update.ps1");
    std::fs::write(
        &wrapper,
        r#"param([int]$ParentProcess, [string]$Version, [string]$InstallDir)
$ErrorActionPreference = 'Stop'
try {
    Wait-Process -Id $ParentProcess -ErrorAction SilentlyContinue
    & (Join-Path $PSScriptRoot 'install.ps1') -Version $Version -InstallDir $InstallDir
    Write-Host 'Update installed. Reopen Vesper and restart editor agents.'
} catch { Write-Host ('Update failed: ' + $_.Exception.Message) }
finally {
    Read-Host 'Press Enter to close'
    Remove-Item -LiteralPath $PSScriptRoot -Recurse -Force
}
"#,
    )
    .map_err(|_| "Cannot stage updater")?;
    let install_dir = bundle.parent().ok_or("Invalid installation directory")?;
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&wrapper)
        .arg("-ParentProcess")
        .arg(std::process::id().to_string())
        .arg("-Version")
        .arg(tag)
        .arg("-InstallDir")
        .arg(install_dir)
        .env_remove("AGENT_VESPER_RELEASE_BASE_URL")
        .creation_flags(0x00000010)
        .spawn()
        .map_err(|_| "Could not open the Windows installer")?;
    let _ = temp.keep();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updater_accepts_only_bounded_release_versions() {
        for good in ["v1.2.3", "0.22.1"] {
            assert!(valid_version(good));
        }
        for bad in [
            "",
            "latest",
            "v1.2",
            "1.2.3;touch /tmp/unsafe",
            "1.2.3-beta",
            "1.2.4294967296",
        ] {
            assert!(!valid_version(bad));
        }
    }
    #[cfg(not(windows))]
    #[test]
    fn installer_uses_exact_version_and_current_bundle_without_shell_interpolation() {
        let command = installer_command(
            Path::new("/tmp/path with spaces/install.sh"),
            "v9.8.7",
            Path::new("/tmp/custom bundle"),
        )
        .unwrap();
        let command = command.as_std();
        assert_eq!(command.get_program(), "sh");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new("/tmp/path with spaces/install.sh")]
        );
        let env: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env[std::ffi::OsStr::new("AGENT_VESPER_VERSION")],
            Some(std::ffi::OsStr::new("v9.8.7"))
        );
        assert_eq!(
            env[std::ffi::OsStr::new("AGENT_VESPER_RELEASE_BASE_URL")],
            None
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn updater_installs_verified_fixture_and_refuses_bad_checksum_without_touching_user_state()
     {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
        let bin = root.path().join("fixture-bin");
        let stage = root.path().join("stage/agent-vesper-acp");
        let bundle = root.path().join("installed");
        for path in [
            &bin,
            &stage,
            &bundle,
            &root.path().join("tmp"),
            &root.path().join("home"),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        symlink(fixtures.join("update_curl_fixture.sh"), bin.join("curl")).unwrap();
        for name in ["agent-vesper-acp", "agent-vesper-tui"] {
            std::fs::copy(fixtures.join("update_payload_fixture.sh"), stage.join(name)).unwrap();
            std::fs::write(bundle.join(name), "old payload").unwrap();
        }
        std::fs::write(bundle.join("user-state"), "keep this memory").unwrap();
        let platform = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        let asset = format!(
            "agent-vesper-acp-{platform}-{}.tar.gz",
            std::env::consts::ARCH
        );
        let archive = root.path().join(&asset);
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(stage.parent().unwrap())
                .arg("agent-vesper-acp")
                .status()
                .unwrap()
                .success()
        );
        let digest: String = Sha256::digest(std::fs::read(&archive).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let script = root.path().join("install.sh");
        std::fs::write(&script, include_str!("../../../scripts/install.sh")).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths).unwrap();
        for valid in [false, true] {
            std::fs::write(
                root.path().join(format!("{asset}.sha256")),
                format!(
                    "{}  {asset}\n",
                    if valid {
                        digest.clone()
                    } else {
                        "0".repeat(64)
                    }
                ),
            )
            .unwrap();
            let mut command = installer_command(&script, "v9.8.7", &bundle).unwrap();
            command
                .env("PATH", &path)
                .env("HOME", root.path().join("home"))
                .env("TMPDIR", root.path().join("tmp"))
                .env("VESPER_UPDATE_FIXTURES", root.path())
                .env("AGENT_VESPER_INSTALL_DIR", root.path().join("launchers"))
                .env("AGENT_VESPER_MEMORY_ROOT", root.path().join("memory"))
                .env("AGENT_VESPER_SHELL_PROFILE", root.path().join("profile"))
                .kill_on_drop(true);
            let output = tokio::time::timeout(std::time::Duration::from_secs(20), command.output())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                output.status.success(),
                valid,
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                std::fs::read_to_string(bundle.join("user-state")).unwrap(),
                "keep this memory"
            );
            if valid {
                assert_eq!(
                    std::fs::read(bundle.join("agent-vesper-tui")).unwrap(),
                    std::fs::read(fixtures.join("update_payload_fixture.sh")).unwrap()
                );
                assert!(root.path().join("launchers/agent-vesper-tui").is_file());
            } else {
                assert_eq!(
                    std::fs::read_to_string(bundle.join("agent-vesper-tui")).unwrap(),
                    "old payload"
                );
                assert!(!root.path().join("launchers").exists());
            }
        }
    }
}
