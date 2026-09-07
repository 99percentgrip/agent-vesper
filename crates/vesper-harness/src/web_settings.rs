//! Explicit user-owned web settings operations shared by both hosts.
use std::path::{Path, PathBuf};
pub use vesper_config::WebScopeConfig;

/// Read the workspace's effective settings without creating state.
pub fn load(root: &Path) -> Result<WebScopeConfig, String> {
    vesper_config::read_web_scope(root).map_err(|error| error.to_string())
}

/// Save only after an explicit user action; leave hand-written TOML untouched.
pub fn save(root: &Path, config: &WebScopeConfig) -> Result<(), String> {
    if config
        .driver_image
        .as_deref()
        .is_some_and(|image| !vesper_config::is_digest_pinned_image(image))
    {
        return Err("Driver image must be immutable (sha256 ID or registry digest).".into());
    }
    let dir = root.join(".agent-vesper");
    if dir
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("Refusing settings directory symlink.".into());
    }
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, config).map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist(dir.join("web-settings.json"))
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Honor explicit operator selection, otherwise find Docker or Podman on PATH.
pub fn container_cli() -> PathBuf {
    if let Some(value) = std::env::var_os("VESPER_DOCKER_BIN").filter(|value| !value.is_empty()) {
        return value.into();
    }
    for name in ["docker", "podman"] {
        let executable = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        };
        if let Some(path) = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(&executable))
                .find(|path| path.is_file())
        }) {
            return path;
        }
    }
    PathBuf::from("docker")
}

/// Inspect the already-installed driver; never pull or start a container.
pub async fn detect_driver() -> Result<String, String> {
    let dir = bundled_driver_dir()?;
    let reference = if dir.join("image-id").exists() {
        read_image_id(&dir)?
    } else {
        "vesper-web-driver:validated".into()
    };
    inspect_driver(&container_cli(), &reference).await
}

fn normalize_image_id(value: &str) -> Option<String> {
    let digest = value.trim().strip_prefix("sha256:").unwrap_or(value.trim());
    let image = format!("sha256:{digest}");
    vesper_config::is_digest_pinned_image(&image).then_some(image)
}

fn bundled_driver_dir() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let root = std::env::var_os("AGENT_VESPER_BUNDLE_DIR")
        .map(PathBuf::from)
        .or_else(|| executable.parent().map(Path::to_path_buf))
        .ok_or("Cannot locate the application bundle.")?;
    Ok(root.join("web-driver"))
}

#[cfg(feature = "docker")]
pub(crate) fn bundled_image_id() -> Result<String, String> {
    read_image_id(&bundled_driver_dir()?)
}

fn read_image_id(dir: &Path) -> Result<String, String> {
    normalize_image_id(&read_small_file(&dir.join("image-id"))?)
        .ok_or("Bundled driver has an invalid image ID. Reinstall Agent Vesper.".into())
}

fn read_small_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path).map_err(|_| "The driver is missing from this installation package. Reinstall Agent Vesper with the complete bundle.".to_string())?
        .take(1025).read_to_string(&mut text).map_err(|error| error.to_string())?;
    if text.len() > 1024 {
        return Err("Invalid oversized driver metadata.".into());
    }
    Ok(text)
}

fn verify_bundle(dir: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let image = read_image_id(dir)?;
    let checksum = read_small_file(&dir.join("image.sha256"))?;
    let expected = checksum
        .split_whitespace()
        .next()
        .ok_or("Missing driver checksum.")?;
    let mut archive =
        std::fs::File::open(dir.join("image.tar.gz")).map_err(|error| error.to_string())?;
    if archive.metadata().map_err(|error| error.to_string())?.len() > 2 * 1024 * 1024 * 1024 {
        return Err("Bundled driver exceeds the 2 GiB limit.".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = archive
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        != expected
    {
        return Err(
            "Bundled driver checksum mismatch. Reinstall Agent Vesper; no image was loaded.".into(),
        );
    }
    Ok(image)
}

/// Load the checksum-verified image supplied by the installation package.
/// Explicit setup only: no downloads, containers, or workspace settings writes.
pub async fn setup_driver() -> Result<String, String> {
    setup_driver_from(&bundled_driver_dir()?, &container_cli()).await
}

async fn setup_driver_from(dir: &Path, cli: &Path) -> Result<String, String> {
    let source = dir.to_owned();
    let expected = tokio::task::spawn_blocking(move || verify_bundle(&source))
        .await
        .map_err(|error| error.to_string())??;
    if inspect_driver(cli, &expected)
        .await
        .is_ok_and(|actual| actual == expected)
    {
        return Ok(expected);
    }
    let mut child = tokio::process::Command::new(cli)
        .args(["load", "--input"])
        .arg(dir.join("image.tar.gz"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| {
            "Driver is bundled. Install/start Docker or Podman, then choose Set up / repair driver."
                .to_string()
        })?;
    let status = match tokio::time::timeout(std::time::Duration::from_secs(180), child.wait()).await
    {
        Ok(status) => status.map_err(|error| error.to_string())?,
        Err(_) => {
            let _ = child.kill().await;
            return Err("Driver import timed out. Check Docker/Podman and retry Setup.".into());
        }
    };
    if !status.success() {
        return Err("Driver is bundled, but import failed. Start Docker/Podman with Linux containers and retry Setup.".into());
    }
    let actual = inspect_driver(cli, &expected).await?;
    if actual != expected {
        return Err("Imported driver identity does not match the bundled image.".into());
    }
    Ok(expected)
}

/// Installer preflight shared by both binaries; never starts a provider or UI.
pub async fn handle_setup_flag() -> Option<bool> {
    if !std::env::args().any(|arg| arg == "--setup-web-driver") {
        return None;
    }
    Some(match setup_driver().await {
        Ok(image) => {
            eprintln!("Web driver ready: {image}. Enable web access in Settings > Web tools.");
            true
        }
        Err(error) => {
            eprintln!("Web driver setup: {error}");
            false
        }
    })
}

async fn inspect_driver(cli: &Path, reference: &str) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let mut command = tokio::process::Command::new(cli);
    command
        .args(["image", "inspect", reference, "--format", "{{.Id}}"])
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().map_err(|_| {
        "Docker/Podman is unavailable. Install or start your container engine.".to_string()
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Driver inspection stdout unavailable.")?;
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        stdout.take(128).read_to_end(&mut bytes).await?;
        child.wait().await
    })
    .await;
    let status = match result {
        Ok(status) => status.map_err(|_| "Driver inspection failed.".to_string())?,
        Err(_) => {
            let _ = child.kill().await;
            return Err("Driver detection timed out.".into());
        }
    };
    if status.success()
        && let Some(image) = normalize_image_id(&String::from_utf8_lossy(&bytes))
    {
        Ok(image)
    } else {
        Err("Driver is not loaded or the container engine is unavailable. Choose Set up / repair driver.".into())
    }
}

/// Text host equivalent of the terminal settings controls.
pub async fn command(root: &Path, argument: &str) -> Result<String, String> {
    let mut config = load(root)?;
    let words: Vec<_> = argument.split_whitespace().collect();
    match words.as_slice() {
        [] | ["status"] => {
            return Ok(format!(
                "Web tools (saved; restart host to apply): enabled={}, fetch={}, render={}, interact={}, robots={}. Driver: {}.\n/web <enabled|fetch|render|interact|robots> <on|off>; /web setup (bundled driver); /web detect (read-only)",
                config.enabled,
                config.fetch_enabled,
                config.render_enabled,
                config.interact_enabled,
                config.respect_robots,
                config.driver_image.as_deref().unwrap_or("not configured")
            ));
        }
        ["detect"] => config.driver_image = Some(detect_driver().await?),
        ["setup"] => config.driver_image = Some(setup_driver().await?),
        [key, value @ ("on" | "off")] => {
            let flag = *value == "on";
            match *key {
                "enabled" => config.enabled = flag,
                "fetch" => config.fetch_enabled = flag,
                "render" => config.render_enabled = flag,
                "interact" => config.interact_enabled = flag,
                "robots" => config.respect_robots = flag,
                _ => return Err("Unknown web setting.".into()),
            }
        }
        _ => {
            return Err(
                "Usage: /web [status|detect|setup|<enabled|fetch|render|interact|robots> <on|off>]"
                    .into(),
            );
        }
    }
    save(root, &config)?;
    Ok(
        "Web settings saved. Restart the TUI/ACP host to apply. Network approval still applies."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_fixture(dir: &Path) -> String {
        use sha2::{Digest, Sha256};
        let image = format!("sha256:{}", "a".repeat(64));
        std::fs::write(dir.join("image-id"), &image).unwrap();
        std::fs::write(dir.join("image.tar.gz"), b"offline-image-fixture").unwrap();
        std::fs::write(
            dir.join("image.sha256"),
            format!(
                "{}  original-name.tar.gz",
                Sha256::digest(b"offline-image-fixture")
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
        )
        .unwrap();
        image
    }

    #[test]
    fn bundle_checksum_and_identity_are_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let image = bundle_fixture(root.path());
        assert_eq!(verify_bundle(root.path()).unwrap(), image);
        std::fs::write(root.path().join("image.tar.gz"), b"corrupt").unwrap();
        assert!(
            verify_bundle(root.path())
                .unwrap_err()
                .contains("checksum mismatch")
        );
        bundle_fixture(root.path());
        std::fs::write(root.path().join("image-id"), "untrusted:latest").unwrap();
        assert!(verify_bundle(root.path()).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bundled_setup_imports_once_and_corruption_never_reaches_engine() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let expected = bundle_fixture(root.path());
        let cli = root.path().join("engine");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = image ]; then\n  test -f \"$0.loaded\" || exit 1\n  printf '%s\\n' '{}'\nelif [ \"$1\" = load ] && [ \"$2\" = --input ]; then\n  printf 'load\\n' >> \"$0.loaded\"\nelse\n  exit 1\nfi\n",
            "a".repeat(64)
        );
        std::fs::write(&cli, script).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        for _ in 0..2 {
            assert_eq!(
                setup_driver_from(root.path(), &cli).await.unwrap(),
                expected
            );
        }
        assert_eq!(
            std::fs::read_to_string(root.path().join("engine.loaded")).unwrap(),
            "load\n"
        );
        std::fs::write(root.path().join("image.tar.gz"), b"corrupt").unwrap();
        let missing_cli = root.path().join("must-not-execute");
        assert!(
            setup_driver_from(root.path(), &missing_cli)
                .await
                .unwrap_err()
                .contains("checksum mismatch")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn setup_rejects_wrong_imported_identity() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        bundle_fixture(root.path());
        let cli = root.path().join("engine");
        std::fs::write(
            &cli,
            format!(
                "#!/bin/sh\nif [ \"$1\" = image ]; then printf '%s\\n' '{}'; fi\n",
                "b".repeat(64)
            ),
        )
        .unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            setup_driver_from(root.path(), &cli)
                .await
                .unwrap_err()
                .contains("identity does not match")
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn driver_detection_pins_only_valid_installed_ids() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        for (index, output) in [
            format!("sha256:{}", "a".repeat(64)),
            "a".repeat(64),
            "image:latest".into(),
        ]
        .into_iter()
        .enumerate()
        {
            // Each executable is immutable once spawned; never rewrite an
            // executable inode that the OS may still retain after child exit.
            let cli = root.path().join(format!("engine-{index}"));
            std::fs::write(&cli, format!("#!/bin/sh\nprintf '%s\\n' '{output}'\n")).unwrap();
            std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = inspect_driver(&cli, "vesper-web-driver:validated").await;
            assert_eq!(
                result.is_ok(),
                output != "image:latest",
                "{output}: {result:?}"
            );
        }
    }
    #[test]
    fn settings_round_trip_preserves_toml_and_is_explicit() {
        let root = tempfile::tempdir().unwrap();
        assert!(!load(root.path()).unwrap().enabled);
        assert!(!root.path().join(".agent-vesper").exists());
        std::fs::create_dir(root.path().join(".agent-vesper")).unwrap();
        let path = root.path().join(".agent-vesper/config.toml");
        let original = "[web]\nallowlist = [\"https://example.com\"]\n[other]\nvalue = 42\n";
        std::fs::write(&path, original).unwrap();
        let mut config = load(root.path()).unwrap();
        config.enabled = true;
        config.render_enabled = true;
        save(root.path(), &config).unwrap();
        assert_eq!(load(root.path()).unwrap(), config);
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
        config.enabled = false;
        save(root.path(), &config).unwrap();
        assert!(!load(root.path()).unwrap().enabled);
    }
    #[tokio::test]
    async fn slash_controls_save_and_reject_unknown_options() {
        let root = tempfile::tempdir().unwrap();
        for key in ["enabled", "fetch", "render", "interact", "robots"] {
            for value in ["on", "off"] {
                assert!(
                    command(root.path(), &format!("{key} {value}"))
                        .await
                        .unwrap()
                        .contains("Restart")
                );
            }
        }
        assert!(command(root.path(), "enabled maybe").await.is_err());
        assert!(command(root.path(), "private on").await.is_err());
    }
}
