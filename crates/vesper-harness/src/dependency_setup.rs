//! Explicit user setup. Never registered as a model-facing tool.
//! Package installation is separate from feature permissions and workspace drafts.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const MACHINE: &str = "agent-vesper";
const MAX_OUTPUT: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    pub binary: PathBuf,
    pub connection: Option<String>,
    pub managed_machine: bool,
    #[serde(default)]
    pub verified: bool,
}

impl Engine {
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        if let Some(connection) = &self.connection {
            command.args(["--connection", connection]);
        }
        command
    }
    fn validate(&self) -> Result<(), String> {
        if !self.binary.is_absolute()
            || self.connection.as_deref().is_some_and(|value| {
                value.is_empty()
                    || value.len() > 64
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            || (self.managed_machine && self.connection.as_deref() != Some(MACHINE))
        {
            return Err("Invalid saved runtime. Run Set up features again.".into());
        }
        Ok(())
    }
}

fn state_path() -> Result<PathBuf, String> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Cannot locate your user home for setup preferences.")?;
    Ok(home.join(".agent-vesper").join("runtime.json"))
}

pub fn saved_engine() -> Result<Option<Engine>, String> {
    read_engine(&state_path()?)
}
fn read_engine(path: &Path) -> Result<Option<Engine>, String> {
    use std::io::Read;
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
        || path.parent().is_some_and(|dir| {
            dir.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        })
    {
        return Err("Refusing symlinked runtime preferences.".into());
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read saved runtime preferences.".into()),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read runtime preferences.")?;
    if bytes.len() > 4096 {
        return Err("Runtime preferences exceed their size limit.".into());
    }
    let engine: Engine = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid runtime preferences. Run Set up features again.")?;
    engine.validate()?;
    Ok(Some(engine))
}
fn save_engine(engine: &Engine) -> Result<(), String> {
    engine.validate()?;
    let path = state_path()?;
    let dir = path.parent().ok_or("Invalid setup directory.")?;
    if dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Refusing a symlinked setup directory.".into());
    }
    std::fs::create_dir_all(dir).map_err(|_| "Cannot create setup preferences.")?;
    let mut file =
        tempfile::NamedTempFile::new_in(dir).map_err(|_| "Cannot stage setup preferences.")?;
    serde_json::to_writer(&mut file, engine).map_err(|_| "Cannot write setup preferences.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Cannot flush setup preferences.")?;
    file.persist(path)
        .map_err(|_| "Cannot save setup preferences.")?;
    Ok(())
}

pub fn find_program(name: &str) -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // A newly installed engine may not be in this already-running process's PATH.
    if cfg!(target_os = "macos") {
        dirs.push("/opt/podman/bin".into());
    }
    if cfg!(windows)
        && let Some(root) = std::env::var_os("ProgramFiles")
    {
        dirs.push(PathBuf::from(root).join("RedHat/Podman"));
    }
    dirs.into_iter()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(&executable))
        .find(|path| path.is_file())
}

/// Bounded subprocess output; raw output is never included in user diagnostics.
async fn run(mut command: Command, seconds: u64) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        format!(
            "Setup could not start a required program ({:?}).",
            error.kind()
        )
    })?;
    let stdout = child.stdout.take().ok_or("Setup output unavailable.")?;
    let result = tokio::time::timeout(Duration::from_secs(seconds), async {
        let mut bytes = Vec::new();
        let mut stdout = stdout;
        let mut chunk = [0u8; 8192];
        loop {
            let count = stdout.read(&mut chunk).await.map_err(|_| "Setup output failed.")?;
            if count == 0 { break; }
            let retain = count.min((MAX_OUTPUT as usize).saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk[..retain]);
        }
        let status = child.wait().await.map_err(|_| "Setup process failed.")?;
        if !status.success() { return Err(format!("Setup step failed (exit {}). Retry setup; OS approval, network access or a restart may be required.", status.code().map_or("unknown".into(), |c| c.to_string()))); }
        Ok(bytes)
    }).await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        other => {
            let _ = child.kill().await;
            other.unwrap_or_else(|_| {
                Err("Setup step timed out. Retry after checking the runtime and network.".into())
            })
        }
    }
}

async fn health(engine: &Engine) -> Result<(), String> {
    let mut command = engine.command();
    command.args(["info", "--format", "{{.OSType}}"]);
    // Docker exposes OSType; Podman's Host.OS is the corresponding field.
    let mut result = run(command, 5).await;
    if result.is_err() && is_podman(&engine.binary) {
        let mut command = engine.command();
        command.args(["info", "--format", "{{.Host.OS}}"]);
        result = run(command, 5).await;
    }
    if result?.trim_ascii() != b"linux" {
        return Err("The runtime must provide Linux containers.".into());
    }
    Ok(())
}
fn is_podman(path: &Path) -> bool {
    path.file_stem().is_some_and(|name| name == "podman")
}
fn remote_override() -> bool {
    [
        "DOCKER_HOST",
        "CONTAINER_HOST",
        "CONTAINER_CONNECTION",
        "DOCKER_CONTEXT",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}
async fn local_endpoint(engine: &Engine) -> Result<(), String> {
    if remote_override() {
        return Err("An explicit container endpoint/context is configured. Automatic setup will not change it or send a probe there. Use a local runtime context and retry.".into());
    }
    if engine.managed_machine {
        return Ok(());
    }
    if is_podman(&engine.binary) {
        if cfg!(target_os = "linux") && engine.connection.is_none() {
            return Ok(());
        }
        let mut command = Command::new(&engine.binary);
        command.args(["system", "connection", "list", "--format", "json"]);
        let records: serde_json::Value = serde_json::from_slice(&run(command, 5).await?)
            .map_err(|_| "Cannot inspect Podman connections.")?;
        let record = records
            .as_array()
            .and_then(|rows| {
                rows.iter().find(|row| {
                    engine.connection.as_ref().map_or_else(
                        || row["Default"].as_bool() == Some(true),
                        |name| row["Name"].as_str() == Some(name),
                    )
                })
            })
            .ok_or("No local Podman connection is selected.")?;
        let uri = record["URI"]
            .as_str()
            .and_then(|value| url::Url::parse(value).ok())
            .ok_or("Invalid Podman connection endpoint.")?;
        if uri.scheme() != "ssh"
            || !matches!(uri.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            return Err("Automatic setup will not use a remote Podman connection.".into());
        }
        return Ok(());
    }
    let mut command = engine.command();
    command.args([
        "context",
        "inspect",
        "--format",
        "{{.Endpoints.docker.Host}}",
    ]);
    let bytes = run(command, 5).await?;
    let endpoint = String::from_utf8_lossy(&bytes);
    if !endpoint.trim().starts_with("unix://") && !endpoint.trim().starts_with("npipe://") {
        return Err("Automatic setup requires a local container endpoint.".into());
    }
    Ok(())
}

/// Discovery is read-only. Explicit binary choices are never silently replaced.
pub async fn discover() -> Result<Engine, String> {
    if let Some(binary) = std::env::var_os("VESPER_DOCKER_BIN").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(binary);
        let binary = if path.is_absolute() {
            path
        } else {
            path.to_str().and_then(find_program).unwrap_or(path)
        };
        let engine = Engine {
            binary,
            connection: None,
            managed_machine: false,
            verified: false,
        };
        local_endpoint(&engine).await?;
        health(&engine).await?;
        return Ok(engine);
    }
    if let Some(engine) = saved_engine()? {
        local_endpoint(&engine).await?;
        if health(&engine).await.is_ok() {
            return Ok(engine);
        }
    }
    let candidates = ["docker", "podman"]
        .iter()
        .filter_map(|name| find_program(name))
        .map(|binary| Engine {
            binary,
            connection: None,
            managed_machine: false,
            verified: false,
        })
        .collect::<Vec<_>>();
    if let Some(engine) = select_healthy(&candidates).await {
        return Ok(engine);
    }
    Err("No ready local container engine. Choose Set up features to install or repair the optional runtime. Core coding remains available.".into())
}

/// No package or image operations: a status page may call this freely.
pub async fn status() -> String {
    match discover().await {
        Ok(engine) => format!(
            "Core coding: available. Local container engine: {}. Set up features verifies the bundled browser and isolation. Voice, local models and Swarm embeddings have separate setup.",
            if is_podman(&engine.binary) {
                "Podman"
            } else {
                "Docker"
            }
        ),
        Err(error) => format!("Core coding: available. {error}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LinuxPackage {
    Apt,
    Dnf,
}
fn linux_package(os_release: &str) -> Result<LinuxPackage, String> {
    let id = os_release
        .lines()
        .find_map(|line| line.strip_prefix("ID="))
        .unwrap_or("")
        .trim_matches('"');
    match id {
        "ubuntu" | "debian" => Ok(LinuxPackage::Apt),
        "fedora" => Ok(LinuxPackage::Dnf),
        _ => Err("Automatic package setup currently supports Debian, Ubuntu and Fedora. Core coding remains available; install Podman using your distribution's package manager, then Retry.".into()),
    }
}

pub const CONSENT: &str = "Set up web and isolated tools? Vesper reuses a working local engine or installs Podman from verified packages. Your OS may ask for administrator approval. macOS/Windows need a Linux VM (2 CPUs, 2 GiB RAM, up to 20 GiB disk; downloads can exceed 1 GiB). Setup checks the bundled browser and saves the runtime choice. It does not enable web permissions. Existing runtimes and workloads are preserved. A required restart is followed by Retry setup.";

/// Explicit confirmation only. Progress consists of fixed, credential-free phases.
pub async fn setup(progress: impl Fn(&str)) -> Result<String, String> {
    setup_cancellable(progress, || false).await
}

/// Cancellation waits for the active OS transaction and cleanup; it never kills a package manager halfway through an install.
pub async fn setup_cancellable(
    progress: impl Fn(&str),
    cancel: impl Fn() -> bool,
) -> Result<String, String> {
    let checkpoint = |phase: &str| -> Result<(), String> {
        if cancel() {
            return Err("Setup stopped between steps. Installed packages and completed imports remain; Retry resumes. Web settings were not saved.".into());
        }
        progress(phase);
        Ok(())
    };
    // Serializes explicit setup within either host; no package transaction overlap.
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = LOCK
        .try_lock()
        .map_err(|_| "Another dependency setup is already running.")?;
    checkpoint("Checking the bundled browser package…")?;
    if !cfg!(feature = "docker") {
        return Err("This build does not include container support. Install a complete Vesper release to use guided setup.".into());
    }
    crate::web_settings::verify_bundled_driver().await?;
    checkpoint("Checking the local container runtime…")?;
    let _file_lock = setup_lock()?;
    let mut engine = match discover().await {
        Ok(engine) => engine,
        Err(error) => {
            if std::env::var_os("VESPER_DOCKER_BIN").is_some() || remote_override() {
                return Err(error);
            }
            if let Some(engine) = saved_engine()?
                && engine.managed_machine
            {
                checkpoint("Starting the Vesper container machine…")?;
                start_machine(&engine).await?;
                engine
            } else {
                if let Some(binary) = find_program("docker") {
                    checkpoint("Starting the installed Docker runtime…")?;
                    let candidate = Engine {
                        binary,
                        connection: None,
                        managed_machine: false,
                        verified: false,
                    };
                    if start_docker(&candidate).await.is_ok() {
                        checkpoint("Verifying and importing the bundled browser image…")?;
                        let image =
                            crate::web_settings::setup_driver_with_engine(&candidate).await?;
                        checkpoint("Checking isolated browser execution and cleanup…")?;
                        verify_browser(&candidate, &image).await?;
                        checkpoint("Saving verified runtime choice…")?;
                        save_engine(&Engine {
                            verified: true,
                            ..candidate
                        })?;
                        return Ok(image);
                    }
                }
                let binary = match find_program("podman") {
                    Some(binary) => binary,
                    None => {
                        checkpoint(
                            "Installing Podman; approve the operating system prompt if requested…",
                        )?;
                        install_podman().await?;
                        find_program("podman").ok_or("Podman installation finished but its executable is unavailable. Reopen Vesper and Retry setup.")?
                    }
                };
                let mut engine = Engine {
                    binary,
                    connection: None,
                    managed_machine: false,
                    verified: false,
                };
                if cfg!(target_os = "macos") || cfg!(windows) {
                    checkpoint(
                        "Preparing the Vesper Linux VM; this may download its system image…",
                    )?;
                    prepare_virtualization().await?;
                    prepare_machine(&mut engine).await?;
                }
                engine
            }
        }
    };
    local_endpoint(&engine).await?;
    health(&engine).await.map_err(|_| "Podman is installed but Linux containers are not ready. Approve required OS virtualization setup, restart if requested, then Retry. No existing machine was reset.")?;
    checkpoint("Verifying and importing the bundled browser image…")?;
    let image = crate::web_settings::setup_driver_with_engine(&engine).await?;
    checkpoint("Checking isolated browser execution and cleanup…")?;
    verify_browser(&engine, &image).await?;
    checkpoint("Saving verified runtime choice…")?;
    engine.verified = true;
    save_engine(&engine)?;
    checkpoint("Ready. Save your web choices in Settings; network approval still applies.")?;
    Ok(image)
}

async fn start_machine(engine: &Engine) -> Result<(), String> {
    if health(engine).await.is_ok() {
        return Ok(());
    }
    let mut inventory = Command::new(&engine.binary);
    inventory.args(["machine", "list", "--format", "json"]);
    let inventory: serde_json::Value = serde_json::from_slice(&run(inventory, 10).await?)
        .map_err(|_| "Cannot inspect machine state.")?;
    let machines = inventory.as_array().ok_or("Invalid machine inventory.")?;
    if !machines
        .iter()
        .any(|machine| machine["Name"].as_str() == Some(MACHINE))
    {
        if !machines.is_empty() {
            return Err("Machine state changed; existing machines are preserved.".into());
        }
        let mut create = Command::new(&engine.binary);
        create.args([
            "machine",
            "init",
            "--cpus",
            "2",
            "--memory",
            "2048",
            "--disk-size",
            "20",
            MACHINE,
        ]);
        run(create, 1200).await?;
    }
    let mut command = Command::new(&engine.binary);
    command.args(["machine", "start", "--update-connection=false", MACHINE]);
    run(command, 180).await?;
    health(engine).await
}
async fn prepare_machine(engine: &mut Engine) -> Result<(), String> {
    let mut command = Command::new(&engine.binary);
    command.args(["machine", "list", "--format", "json"]);
    let machines: serde_json::Value = serde_json::from_slice(&run(command, 10).await?)
        .map_err(|_| "Cannot read Podman machine inventory.")?;
    let list = machines
        .as_array()
        .ok_or("Invalid Podman machine inventory.")?;
    if !list.is_empty() {
        return start_existing_machine(engine, list).await;
    }
    let mut command = Command::new(&engine.binary);
    command.args(["system", "connection", "list", "--format", "json"]);
    let connections: serde_json::Value = serde_json::from_slice(&run(command, 10).await?)
        .map_err(|_| "Cannot read Podman connections.")?;
    if !connections.as_array().is_some_and(|list| list.is_empty()) {
        return Err("Existing Podman connections are preserved. Automatic creation requires a fresh local Podman setup.".into());
    }
    engine.connection = Some(MACHINE.into());
    engine.managed_machine = true;
    // Persist intent before creation: a restart can retry/start, never reset the VM.
    save_engine(engine)?;
    start_machine(engine).await
}

async fn install_podman() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        let release = std::fs::read_to_string("/etc/os-release")
            .map_err(|_| "Cannot identify this Linux distribution.")?;
        let package = linux_package(&release)?;
        let manager = match package {
            LinuxPackage::Apt => "/usr/bin/apt-get",
            LinuxPackage::Dnf => "/usr/bin/dnf",
        };
        let elevate = find_program("pkexec").ok_or("A graphical administrator prompt (pkexec) is unavailable. Install your distribution's Podman package, then Retry setup; core coding remains available.")?;
        if package == LinuxPackage::Apt {
            let mut command = Command::new(&elevate);
            command.args([manager, "update", "-qq"]);
            run(command, 900).await?;
        }
        let mut command = Command::new(elevate);
        command.args([manager, "install", "-y", "podman"]);
        run(command, 1200).await?;
        return Ok(());
    }
    install_desktop_package().await
}

async fn install_desktop_package() -> Result<(), String> {
    let (name, version, checksum) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => (
            "podman-installer-macos-arm64.pkg",
            "v6.1.1",
            "9c7b90b406681e5458d69cdb1164a589f7c9b214cab1ca6705fe375876491c09",
        ),
        ("macos", "x86_64") => (
            "podman-installer-macos-amd64.pkg",
            "v5.8.3",
            "ed6fc5b060ec8d1b9d84cf19725851a3e37480b5003f9c1cafa34b1adfad4de6",
        ),
        ("windows", "x86_64") => (
            "podman-installer-windows-amd64.msi",
            "v6.1.1",
            "91d0e8ea0846c0151d531c88c329bb2729387231e4d1e42306a8e3ae9d09fc8a",
        ),
        _ => return Err("Automatic Podman installation is unavailable on this platform.".into()),
    };
    let temp = tempfile::tempdir().map_err(|_| "Cannot stage the runtime installer.")?;
    let path = temp.path().join(name);
    let mut command = Command::new(if cfg!(windows) {
        "curl.exe"
    } else {
        "/usr/bin/curl"
    });
    command
        .args([
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "900",
            "--max-filesize",
            "536870912",
            "--output",
        ])
        .arg(&path)
        .arg(format!(
            "https://github.com/podman-container-tools/podman/releases/download/{version}/{name}"
        ));
    run(command, 920).await?;
    let verify_path = path.clone();
    tokio::task::spawn_blocking(move || verify_package(&verify_path, checksum))
        .await
        .map_err(|_| "Runtime package verification failed.")??;
    if cfg!(target_os = "macos") {
        let mut command = Command::new("/usr/bin/osascript");
        command.args(["-e", "on run argv", "-e", "do shell script \"/usr/sbin/installer -pkg \" & quoted form of (item 1 of argv) & \" -target /\" with administrator privileges", "-e", "end run"]).arg(&path);
        run(command, 1200).await?;
    } else {
        let mut command = Command::new("powershell.exe");
        command.env("VESPER_SETUP_PACKAGE", &path).args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; $p = Start-Process msiexec.exe -Verb RunAs -Wait -PassThru -ArgumentList @('/i', ('\"' + $env:VESPER_SETUP_PACKAGE + '\"'), '/passive', '/norestart'); exit $p.ExitCode"]);
        run(command, 1200).await?;
    }
    Ok(())
}

async fn verify_browser(engine: &Engine, image: &str) -> Result<(), String> {
    // Unique name is generated locally; cleanup never addresses an existing workload.
    let id = tempfile::Builder::new()
        .prefix("vesper-setup-")
        .tempdir()
        .map_err(|_| "Cannot allocate setup probe identity.")?;
    let name = id
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid probe identity.")?;
    let mut command = engine.command();
    command.args(["run", "--pull=never", "--name", name, "--network", "none", "--read-only", "--cap-drop=ALL", "--security-opt", "no-new-privileges", "--pids-limit", "128", "--memory", "512m", "--tmpfs", "/tmp:rw,nosuid,nodev,size=128m", image,
        "/usr/bin/timeout", "50", "/bin/sh", "-c", "test ! -e /workspace && test ! -w /etc && /usr/bin/chromium-headless-shell --no-sandbox --disable-gpu --dump-dom 'data:text/html,<title>vesper-ready</title>'"]);
    let result = run(command, 60).await;
    let mut cleanup = engine.command();
    cleanup.args(["rm", "-f", name]);
    let removed = run(cleanup, 10).await;
    if removed.is_err() {
        return Err("Setup probe cleanup could not be verified. Check the runtime before retrying; readiness was not granted.".into());
    }
    let mut inspect = engine.command();
    inspect.args([
        "ps",
        "--all",
        "--filter",
        &format!("name={name}"),
        "--format",
        "{{.Names}}",
    ]);
    if !run(inspect, 5).await?.trim_ascii().is_empty() {
        return Err("Setup probe still exists after cleanup; readiness was not granted.".into());
    }
    let bytes = result?;
    if !String::from_utf8_lossy(&bytes).contains("<title>vesper-ready</title>") {
        return Err("The contained browser did not pass its readiness check.".into());
    }
    Ok(())
}

/// Called only on an actual opt-in tool path, never at application boot.
/// Starting a machine is allowed only by the saved explicit setup choice.
pub async fn runtime_ready() -> Result<Engine, String> {
    if remote_override() || std::env::var_os("VESPER_DOCKER_BIN").is_some_and(|v| !v.is_empty()) {
        // An operator's existing runtime choice is not automatic setup consent.
        // Preserve that choice, but never install/start anything on this path.
        let engine = runtime_snapshot()?;
        health(&engine).await?;
        return Ok(engine);
    }
    if let Some(engine) = saved_engine()?
        && engine.managed_machine
        && engine.verified
        && !remote_override()
        && std::env::var_os("VESPER_DOCKER_BIN").is_none()
    {
        ensure_engine(&engine).await?;
        return Ok(engine);
    }
    discover().await
}

/// Recheck a previously selected runtime after sleep without rerouting active containers.
pub(crate) async fn ensure_engine(engine: &Engine) -> Result<(), String> {
    if health(engine).await.is_ok() {
        return Ok(());
    }
    if !engine.managed_machine || !engine.verified || remote_override() {
        return Err(
            "Container runtime unavailable. Open Settings → Web tools → Set up features / repair."
                .into(),
        );
    }
    // Runtime recovery may start the owned VM, but never initialize/download one.
    let mut command = Command::new(&engine.binary);
    command.args(["machine", "start", "--update-connection=false", MACHINE]);
    run(command, 180).await?;
    health(engine).await
}

/// Read one consistent executable/connection pair without probing or starting it.
/// Explicit operator configuration takes precedence over saved automatic setup.
pub fn runtime_snapshot() -> Result<Engine, String> {
    let explicit = std::env::var_os("VESPER_DOCKER_BIN").filter(|value| !value.is_empty());
    let endpoint_kind = if ["DOCKER_HOST", "DOCKER_CONTEXT"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
    {
        Some("docker")
    } else if ["CONTAINER_HOST", "CONTAINER_CONNECTION"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
    {
        Some("podman")
    } else {
        None
    };
    if let Some(value) = explicit {
        let path = PathBuf::from(value);
        let binary = if path.is_absolute() {
            path
        } else {
            path.to_str().and_then(find_program).unwrap_or(path)
        };
        return Ok(Engine {
            binary,
            connection: None,
            managed_machine: false,
            verified: false,
        });
    }
    if let Some(name) = endpoint_kind {
        return Ok(Engine {
            binary: find_program(name).unwrap_or_else(|| name.into()),
            connection: None,
            managed_machine: false,
            verified: false,
        });
    }
    if let Some(engine) = saved_engine()? {
        return Ok(engine);
    }
    Ok(Engine {
        binary: find_program("docker")
            .or_else(|| find_program("podman"))
            .unwrap_or_else(|| "docker".into()),
        connection: None,
        managed_machine: false,
        verified: false,
    })
}

async fn prepare_virtualization() -> Result<(), String> {
    if !cfg!(windows) {
        return Ok(());
    }
    let mut status = Command::new("wsl.exe");
    status.arg("--status");
    if run(status, 30).await.is_ok() {
        return Ok(());
    }
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; $p = Start-Process wsl.exe -Verb RunAs -Wait -PassThru -ArgumentList @('--install', '--no-distribution'); exit $p.ExitCode"]);
    let _ = run(command, 1200).await;
    Err("Windows Linux virtualization setup was requested. Complete the OS prompt, restart Windows if requested, then open Vesper and Retry setup. No restart is forced.".into())
}

#[cfg(test)]
#[path = "dependency_setup_tests.rs"]
mod tests;

fn setup_lock() -> Result<std::fs::File, String> {
    use fs2::FileExt;
    let path = state_path()?;
    let dir = path.parent().ok_or("Invalid runtime directory.")?;
    if dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Refusing symlinked setup directory.".into());
    }
    std::fs::create_dir_all(dir).map_err(|_| "Cannot create setup directory.")?;
    let path = dir.join("runtime-setup.lock");
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Refusing symlinked setup lock.".into());
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| "Cannot open setup lock.")?;
    file.try_lock_exclusive()
        .map_err(|_| "Dependency setup is already running in another Vesper process.")?;
    Ok(file)
}

async fn select_healthy(candidates: &[Engine]) -> Option<Engine> {
    for engine in candidates {
        if local_endpoint(engine).await.is_ok() && health(engine).await.is_ok() {
            return Some(engine.clone());
        }
    }
    None
}

async fn start_existing_machine(
    engine: &mut Engine,
    machines: &[serde_json::Value],
) -> Result<(), String> {
    let mut command = Command::new(&engine.binary);
    command.args(["system", "connection", "list", "--format", "json"]);
    let connections: serde_json::Value = serde_json::from_slice(&run(command, 10).await?)
        .map_err(|_| "Cannot read Podman connections.")?;
    let connections = connections
        .as_array()
        .ok_or("Invalid Podman connections.")?;
    let selected = connections.iter().find(|row| row["Default"].as_bool() == Some(true)).or_else(|| {
        if machines.len() == 1 { connections.iter().find(|row| row["Name"] == machines[0]["Name"]) } else { None }
    }).ok_or("Several existing Podman machines have no selected default. Select your preferred local connection in Podman, then Retry; no machine was changed.")?;
    let connection = selected["Name"]
        .as_str()
        .ok_or("Invalid Podman connection name.")?;
    let machine = machines
        .iter()
        .filter_map(|row| row["Name"].as_str())
        .find(|name| connection == *name || connection == format!("{name}-root"))
        .ok_or("The selected connection does not identify a local Podman machine.")?;
    engine.connection = Some(connection.into());
    engine.validate()?;
    local_endpoint(engine).await?;
    if health(engine).await.is_ok() {
        return Ok(());
    }
    let mut command = Command::new(&engine.binary);
    command.args(["machine", "start", "--update-connection=false", machine]);
    run(command, 180).await?;
    health(engine).await
}

async fn start_docker(engine: &Engine) -> Result<(), String> {
    // Verify the selected endpoint before starting any service or desktop app.
    local_endpoint(engine).await?;
    let mut command = if cfg!(target_os = "linux") {
        let mut command =
            Command::new(find_program("pkexec").ok_or("OS authorization unavailable.")?);
        command.args(["/usr/bin/systemctl", "start", "docker"]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("/usr/bin/open");
        command.args(["-g", "-a", "Docker"]);
        command
    } else if cfg!(windows) {
        let path = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .ok_or("Cannot locate Docker Desktop.")?
            .join("Docker/Docker/Docker Desktop.exe");
        if !path.is_file() {
            return Err("Docker Desktop is not installed.".into());
        }
        let mut command = Command::new("powershell.exe");
        command.env("VESPER_DOCKER_DESKTOP", path).args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference = 'Stop'; Start-Process -FilePath $env:VESPER_DOCKER_DESKTOP",
        ]);
        command
    } else {
        return Err("Runtime startup unavailable on this OS.".into());
    };
    command.env_remove("BASH_ENV").env_remove("ENV");
    run(command, 120).await?;
    for _ in 0..12 {
        if health(engine).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err("Docker has not become ready. Complete its own startup prompts and Retry.".into())
}

fn verify_package(path: &Path, checksum: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|_| "Runtime download is missing.")?;
    if file
        .metadata()
        .map_err(|_| "Cannot inspect runtime download.")?
        .len()
        > 512 * 1024 * 1024
    {
        return Err("Runtime download exceeds 512 MiB.".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| "Cannot verify runtime download.")?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    drop(file);
    if hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        != checksum
    {
        return Err(
            "Runtime installer checksum mismatch. Nothing was installed; Retry setup.".into(),
        );
    }
    Ok(())
}
