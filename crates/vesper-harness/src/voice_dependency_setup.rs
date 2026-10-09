//! Explicit local speech setup. Not a model tool and never invoked by readiness.
use super::{LinuxPackage, find_program, linux_package, run, setup_lock, verify_package};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Consent covers optional local components only; it does not enable voice.
pub const VOICE_SETUP_CONSENT: &str = "Set up local voice dependencies? Vesper verifies or repairs the private speech-recognition environment and installs missing espeak-ng pronunciation tools and the Microsoft C++ runtime on Windows. If needed, it downloads verified uv and Python. macOS may also install Homebrew and ask for Apple developer tools. Your OS may request administrator approval. Existing healthy components are reused. Natural Voice model installation is a separate confirmed step; your coding provider and saved voice selection are unchanged. Esc stops between package transactions; completed installations remain.";

/// Application-owned paths supplied by the native Settings composition.
pub struct VoiceSetupPaths {
    pub venv: PathBuf,
    pub python: PathBuf,
    pub tools: PathBuf,
    pub uv: Option<PathBuf>,
}

/// Fixed package identity. URLs and digests never come from model input.
struct Package {
    url: &'static str,
    size: u64,
    digest: &'static str,
}
const ESPEAK: Package = Package {
    url: "https://github.com/espeak-ng/espeak-ng/releases/download/1.52.0/espeak-ng.msi",
    size: 12_765_862,
    digest: "7f673c709ea5dd579d3b5ebb98688cc575328a6ab7438d2bc405b88cedaeafb9",
};
// This official signed package supports both Intel and Apple Silicon; newer
// installers removed Intel. Reuse a healthy existing Homebrew first.
const BREW: Package = Package {
    url: "https://github.com/Homebrew/brew/releases/download/4.6.17/Homebrew-4.6.17.pkg",
    size: 122_586_592,
    digest: "afdfde7f1913164aae4c5644f088ed47b89d48d2d734ff8106e140948a9ef408",
};

const VC_RUNTIME: Package = Package {
    url: "https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe",
    size: 25635768,
    digest: "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b",
};

fn vc_runtime_present(root: &Path) -> bool {
    [
        "MSVCP140.dll",
        "MSVCP140_1.dll",
        "VCRUNTIME140.dll",
        "VCRUNTIME140_1.dll",
    ]
    .into_iter()
    .all(|name| root.join("System32").join(name).is_file())
}

async fn prepare_windows_runtime(
    progress: &impl Fn(&str),
    cancel: &impl Fn() -> bool,
) -> Result<(), String> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or("Cannot locate Windows for the native speech runtime check.")?;
    if vc_runtime_present(&root) {
        return Ok(());
    }
    let stage =
        tempfile::tempdir().map_err(|_| "Cannot stage the native speech runtime installer.")?;
    let path = stage.path().join("VC_redist.x64.exe");
    download(&VC_RUNTIME, &path, progress, cancel).await?;
    progress("Installing Microsoft C++ runtime · complete the Windows administrator prompt…");
    let mut command = Command::new("powershell.exe");
    command.env("VESPER_SETUP_PACKAGE", &path).args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath $env:VESPER_SETUP_PACKAGE -Verb RunAs -Wait -PassThru -ArgumentList @('/install', '/passive', '/norestart'); exit $p.ExitCode"]);
    run(command, 1200).await?;
    if !vc_runtime_present(&root) {
        return Err("Native speech runtime installation is unconfirmed. Complete any required restart, then Retry setup.".into());
    }
    Ok(())
}

fn uv_package(os: &str, arch: &str) -> Result<Package, String> {
    let (filename, size, digest) = match (os, arch) {
        ("windows", "x86_64") => (
            "uv-x86_64-pc-windows-msvc.zip",
            15_685_515,
            "7c38608c8a18ee137d748a1773053b07ec8f3a30fab49aebaa6f4e4efeceb019",
        ),
        ("macos", "x86_64") => (
            "uv-x86_64-apple-darwin.tar.gz",
            18_120_254,
            "4fa82e37cb94767661f532b001e470b67a186c7260e305bd84ddb78fd545c0b6",
        ),
        ("macos", "aarch64") => (
            "uv-aarch64-apple-darwin.tar.gz",
            14_650_202,
            "0c4346de7abdb49495b393b9ec809fe387aa43e586be20fecb972216c1e71732",
        ),
        ("linux", "x86_64") => (
            "uv-x86_64-unknown-linux-gnu.tar.gz",
            17_249_236,
            "b4dfaef47d491a7296981f8374a4595f55dbf84e8937c8ecd2983574d8bb3da6",
        ),
        ("linux", "aarch64") => (
            "uv-aarch64-unknown-linux-gnu.tar.gz",
            16_478_994,
            "5231be65f496304623895dacdbf1de8504fec90303684bdf05805aa34414dd21",
        ),
        _ => return Err("No verified voice dependency package exists for this target.".into()),
    };
    let url = match filename {
        "uv-x86_64-pc-windows-msvc.zip" => {
            "https://github.com/astral-sh/uv/releases/download/0.12.24/uv-x86_64-pc-windows-msvc.zip"
        }
        "uv-x86_64-apple-darwin.tar.gz" => {
            "https://github.com/astral-sh/uv/releases/download/0.12.24/uv-x86_64-apple-darwin.tar.gz"
        }
        "uv-aarch64-apple-darwin.tar.gz" => {
            "https://github.com/astral-sh/uv/releases/download/0.12.24/uv-aarch64-apple-darwin.tar.gz"
        }
        "uv-x86_64-unknown-linux-gnu.tar.gz" => {
            "https://github.com/astral-sh/uv/releases/download/0.12.24/uv-x86_64-unknown-linux-gnu.tar.gz"
        }
        _ => {
            "https://github.com/astral-sh/uv/releases/download/0.12.24/uv-aarch64-unknown-linux-gnu.tar.gz"
        }
    };
    Ok(Package { url, size, digest })
}

/// Resolve newly installed components even before the invoking PATH is refreshed.
pub fn voice_phonemizer() -> Option<PathBuf> {
    find_program("espeak-ng").or_else(|| {
        let mut dirs = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        if cfg!(windows) {
            dirs.clear();
            for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(root) = std::env::var_os(variable) {
                    dirs.push(PathBuf::from(&root).join("eSpeak NG"));
                    dirs.push(PathBuf::from(root).join("eSpeak NG/bin"));
                }
            }
        }
        dirs.into_iter()
            .map(|dir| {
                dir.join(if cfg!(windows) {
                    "espeak-ng.exe"
                } else {
                    "espeak-ng"
                })
            })
            .find(|path| path.is_file())
    })
}

fn checkpoint(cancel: &impl Fn() -> bool) -> Result<(), String> {
    if cancel() {
        Err("Voice setup stopped. Completed package installations remain; Retry checks them again. Voice settings were not saved.".into())
    } else {
        Ok(())
    }
}
async fn health_phonemizer(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    command.args(["-q", "--ipa", "-v", "en-us", "ready"]);
    let output = run(command, 15).await?;
    if output.is_empty() {
        return Err(
            "Pronunciation check produced no phonemes; choose Repair local voice dependencies."
                .into(),
        );
    }
    Ok(())
}
async fn health_python(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    command.args(["-c", "import faster_whisper"]);
    run(command, 30).await.map(|_| ())
}

/// Host confirmation is mandatory. Cancellation does not interrupt OS transactions.
pub async fn setup_voice_cancellable(
    paths: VoiceSetupPaths,
    progress: impl Fn(&str),
    cancel: impl Fn() -> bool,
) -> Result<String, String> {
    if !paths.venv.is_absolute()
        || !paths.python.starts_with(&paths.venv)
        || !paths.tools.is_absolute()
    {
        return Err("Voice setup requires absolute application-owned data paths.".into());
    }
    checkpoint(&cancel)?;
    let _lock = setup_lock()?;
    if cfg!(windows) {
        prepare_windows_runtime(&progress, &cancel).await?;
        checkpoint(&cancel)?;
    }
    progress("Checking installed pronunciation tools…");
    let healthy = match voice_phonemizer() {
        Some(path) => health_phonemizer(&path).await.is_ok(),
        None => false,
    };
    if !healthy
        || (cfg!(target_os = "linux")
            && (find_program("aplay").is_none() || find_program("arecord").is_none()))
    {
        checkpoint(&cancel)?;
        install_phonemizer(&progress, &cancel, voice_phonemizer().is_some()).await?;
        checkpoint(&cancel)?;
        let path = voice_phonemizer().ok_or("Pronunciation installation could not be located. Retry setup after any required restart.")?;
        health_phonemizer(&path).await?;
    }
    checkpoint(&cancel)?;
    progress("Checking the local speech-recognition environment…");
    if health_python(&paths.python).await.is_err() {
        let uv = prepare_uv(&paths, &progress, &cancel).await?;
        checkpoint(&cancel)?;
        if !paths.python.is_file() {
            progress("Preparing private Python 3.12 for speech recognition…");
            let mut command = Command::new(&uv);
            command.args(["venv", "--python", "3.12"]).arg(&paths.venv);
            run(command, 1200).await?;
        }
        checkpoint(&cancel)?;
        progress("Installing the local speech-recognition dependencies…");
        let mut command = Command::new(&uv);
        command
            .args([
                "pip",
                "install",
                "--reinstall",
                "faster-whisper==1.2.1",
                "--python",
            ])
            .arg(&paths.python);
        run(command, 1200).await?;
        checkpoint(&cancel)?;
        progress("Verifying the speech-recognition environment…");
        health_python(&paths.python).await?;
    }
    checkpoint(&cancel)?;
    Ok("Local voice dependencies verified. Device permissions are checked when recording/playback starts; install Natural Voice separately if selected.".into())
}

async fn download(
    package: &Package,
    path: &Path,
    progress: &impl Fn(&str),
    cancel: &impl Fn() -> bool,
) -> Result<(), String> {
    checkpoint(cancel)?;
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
        ])
        .arg(package.size.to_string())
        .arg("--output")
        .arg(path)
        .arg(package.url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start the verified voice-package download.")?;
    let started = Instant::now();
    let mut observed = None;
    loop {
        if cancel() || started.elapsed() > Duration::from_secs(920) {
            let _ = child.kill().await;
            return Err(
                "Voice-package download stopped; Retry setup. No downloaded program was launched."
                    .into(),
            );
        }
        let bytes = std::fs::metadata(path).map_or(0, |m| m.len());
        if bytes > package.size {
            let _ = child.kill().await;
            return Err("Voice-package download exceeded its pinned size.".into());
        }
        if observed != Some(bytes) {
            progress(&format!(
                "Downloading verified voice prerequisite · {bytes} / {} bytes",
                package.size
            ));
            observed = Some(bytes);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "Cannot observe the voice-package download.")?
        {
            if !status.success() {
                return Err(
                    "Voice-package download failed. Check network access and Retry setup.".into(),
                );
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if std::fs::metadata(path)
        .map_err(|_| "Downloaded voice package is missing.")?
        .len()
        != package.size
    {
        return Err("Downloaded voice package has an incorrect size.".into());
    }
    progress("Verifying the downloaded voice prerequisite…");
    let path = path.to_path_buf();
    let digest = package.digest;
    tokio::task::spawn_blocking(move || verify_package(&path, digest))
        .await
        .map_err(|_| "Voice-package verification failed.")??;
    checkpoint(cancel)
}

async fn prepare_uv(
    paths: &VoiceSetupPaths,
    progress: &impl Fn(&str),
    cancel: &impl Fn() -> bool,
) -> Result<PathBuf, String> {
    let name = if cfg!(windows) { "uv.exe" } else { "uv" };
    for path in [
        paths.uv.clone(),
        Some(paths.tools.join(name)),
        find_program("uv"),
    ]
    .into_iter()
    .flatten()
    {
        let mut command = Command::new(&path);
        command.arg("--version");
        if run(command, 10).await.is_ok() {
            return Ok(path);
        }
    }
    let package = uv_package(std::env::consts::OS, std::env::consts::ARCH)?;
    let stage = tempfile::tempdir().map_err(|_| "Cannot stage the voice environment manager.")?;
    let archive = stage
        .path()
        .join(if cfg!(windows) { "uv.zip" } else { "uv.tar.gz" });
    download(&package, &archive, progress, cancel).await?;
    let mut command = Command::new(if cfg!(windows) { "tar.exe" } else { "tar" });
    command.arg("-xf").arg(&archive).arg("-C").arg(stage.path());
    run(command, 60).await?;
    let extracted = if cfg!(windows) {
        stage.path().join(name)
    } else {
        let directory = package
            .url
            .rsplit('/')
            .next()
            .unwrap()
            .trim_end_matches(".tar.gz");
        stage.path().join(directory).join(name)
    };
    let mut command = Command::new(&extracted);
    command.arg("--version");
    run(command, 10).await?;
    if paths
        .tools
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Refusing symlinked managed voice tools.".into());
    }
    std::fs::create_dir_all(&paths.tools)
        .map_err(|_| "Cannot create the managed voice tools directory.")?;
    let destination = paths.tools.join(name);
    if destination
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Refusing symlinked voice environment manager.".into());
    }
    std::fs::copy(&extracted, &destination)
        .map_err(|_| "Cannot retain the verified voice environment manager.")?;
    Ok(destination)
}

async fn install_phonemizer(
    progress: &impl Fn(&str),
    cancel: &impl Fn() -> bool,
    repair: bool,
) -> Result<(), String> {
    match std::env::consts::OS {
        "windows" => {
            let stage = tempfile::tempdir().map_err(|_| "Cannot stage pronunciation setup.")?;
            let path = stage.path().join("espeak-ng.msi");
            download(&ESPEAK, &path, progress, cancel).await?;
            progress("Installing pronunciation tools · complete the Windows administrator prompt…");
            let mut command = Command::new("powershell.exe");
            command.env("VESPER_SETUP_PACKAGE", &path).env("VESPER_SETUP_REPAIR", if repair { "1" } else { "0" }).args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; $args = @('/i', ('\"' + $env:VESPER_SETUP_PACKAGE + '\"'), '/passive', '/norestart'); if ($env:VESPER_SETUP_REPAIR -eq '1') { $args += @('REINSTALL=ALL', 'REINSTALLMODE=amus') }; $p = Start-Process msiexec.exe -Verb RunAs -Wait -PassThru -ArgumentList $args; exit $p.ExitCode"]);
            run(command, 1200).await?;
        }
        "macos" => {
            let mut brew = find_program("brew").or_else(|| {
                ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
                    .into_iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            });
            if brew.is_none() {
                let stage = tempfile::tempdir().map_err(|_| "Cannot stage pronunciation setup.")?;
                let path = stage.path().join("Homebrew.pkg");
                download(&BREW, &path, progress, cancel).await?;
                progress("Installing Homebrew · complete the macOS administrator prompt…");
                let mut command = Command::new("/usr/bin/osascript");
                command.args(["-e", "on run argv", "-e", "do shell script \"/usr/sbin/installer -pkg \" & quoted form of (item 1 of argv) & \" -target /\" with administrator privileges", "-e", "end run"]).arg(&path);
                run(command, 1200).await?;
                checkpoint(cancel)?;
                brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
                    .into_iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file());
            }
            let brew = brew.ok_or("Homebrew installation is unconfirmed. Complete OS developer-tool prompts, then Retry setup.")?;
            progress("Installing pronunciation tools using Homebrew…");
            let mut command = Command::new(brew);
            command
                .args([if repair { "reinstall" } else { "install" }, "espeak-ng"])
                .env("HOMEBREW_NO_AUTO_UPDATE", "1");
            run(command, 1200).await?;
        }
        "linux" => {
            let release = std::fs::read_to_string("/etc/os-release")
                .map_err(|_| "Cannot identify this distribution for voice setup.")?;
            let package = linux_package(&release)?;
            let manager = match package {
                LinuxPackage::Apt => "/usr/bin/apt-get",
                LinuxPackage::Dnf => "/usr/bin/dnf",
            };
            let elevate = find_program("pkexec").ok_or("Graphical administrator authorization is unavailable; pronunciation setup could not start. Core coding remains available.")?;
            progress(
                "Installing pronunciation/audio tools · complete the system administrator prompt…",
            );
            if package == LinuxPackage::Apt {
                let mut command = Command::new(&elevate);
                command.args([manager, "update", "-qq"]);
                run(command, 900).await?;
                checkpoint(cancel)?;
            }
            let mut command = Command::new(&elevate);
            command.args([manager, "install", "-y", "espeak-ng", "alsa-utils"]);
            run(command, 1200).await?;
        }
        _ => return Err("No verified voice dependency setup exists for this platform.".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voice_dependency_packages_cover_every_release_target() {
        for (os, arch) in [
            ("windows", "x86_64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
        ] {
            let package = uv_package(os, arch).unwrap();
            assert!(
                package
                    .url
                    .starts_with("https://github.com/astral-sh/uv/releases/download/0.12.24/")
            );
            assert!(package.size < 32 * 1024 * 1024);
            assert_eq!(package.digest.len(), 64);
        }
        assert!(uv_package("unknown", "x86_64").is_err());
        assert_eq!(ESPEAK.digest.len(), 64);
        assert_eq!(BREW.digest.len(), 64);
    }
    #[test]
    fn cancellation_preserves_completed_transactions() {
        assert!(checkpoint(&|| false).is_ok());
        let error = checkpoint(&|| true).unwrap_err();
        assert!(error.contains("Completed package installations remain"));
    }

    #[test]
    fn windows_runtime_requires_all_speech_dll_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("System32");
        std::fs::create_dir(&dir).unwrap();
        assert!(!vc_runtime_present(temp.path()));
        for name in ["MSVCP140.dll", "MSVCP140_1.dll", "VCRUNTIME140.dll"] {
            std::fs::write(dir.join(name), []).unwrap();
        }
        assert!(!vc_runtime_present(temp.path()));
        std::fs::write(dir.join("VCRUNTIME140_1.dll"), []).unwrap();
        assert!(vc_runtime_present(temp.path()));
    }
}
