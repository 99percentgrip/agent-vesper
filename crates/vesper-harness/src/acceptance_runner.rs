//! Executes admitted exact Rust tests. Receipts are minted only here, not from
//! model text or imported files. Build/test subprocesses retain normal permission.
use crate::acceptance_snapshot::SourceSnapshot;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use vesper_agent::{CancellationSignal, ToolContext};
use vesper_domain::acceptance::{AcceptanceCheck, AcceptanceState};

#[derive(Debug)]
pub(crate) struct CheckOutcome {
    pub state: AcceptanceState,
    pub output: String,
    pub diagnostic: String,
    pub elapsed_ms: u64,
}

const MAX_OUTPUT: usize = 1024 * 1024;

// Toolchain discovery needs the Windows installation roots even outside a
// developer shell. Without them rustc can select Git's unrelated link.exe.
// The same allowlist supplies subprocesses and binds their receipt identity.
const TOOLCHAIN_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "SYSTEMROOT",
    "SystemRoot",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "TMP",
    "TEMP",
    "TMPDIR",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "SystemDrive",
    "COMSPEC",
    "PATHEXT",
    "VSINSTALLDIR",
    "VCINSTALLDIR",
    "VCToolsInstallDir",
    "WindowsSdkDir",
    "WindowsSDKVersion",
    "UniversalCRTSdkDir",
    "UCRTVersion",
    "LIB",
    "LIBPATH",
    "INCLUDE",
];

pub(crate) fn classify(
    check: &AcceptanceCheck,
    success: bool,
    output: &str,
) -> (AcceptanceState, String) {
    if !success {
        let diagnostic = vesper_agent::vro::SecretScrubber::new().scrub(output);
        return (
            AcceptanceState::Failed,
            format!(
                "exact test process failed:\n{}",
                diagnostic
                    .chars()
                    .rev()
                    .take(6000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            ),
        );
    }
    let passed_line = format!("test {} ... ok", check.test);
    let passed = output
        .lines()
        .filter(|line| line.trim() == passed_line)
        .count();
    let summary = output
        .lines()
        .any(|line| line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored;"));
    let summaries = output
        .lines()
        .filter(|line| line.starts_with("test result:"))
        .count();
    let running = output
        .lines()
        .filter(|line| line.trim() == "running 1 test")
        .count();
    if passed != 1 || !summary || summaries != 1 || running != 1 {
        return (AcceptanceState::Inconclusive, "required exact test did not execute once and pass; zero matches, skips and arbitrary PASS text are not evidence".into());
    }
    (
        AcceptanceState::Verified,
        "exact test executed and passed".into(),
    )
}

pub(crate) fn environment(root: &Path) -> Result<String, String> {
    // Re-probe the selected toolchain, including rust-toolchain.toml overrides.
    // A channel name or host architecture alone is not a compiler identity.
    let mut identity = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    for program in ["rustc", "cargo"] {
        let (success, output) = run_process(
            &[program.into(), "--version".into(), "--verbose".into()],
            root,
            5,
            Arc::new(vesper_runtime::RuntimeCancellation::new()),
        )?;
        if !success {
            return Err("cannot identify the current Rust toolchain".into());
        }
        identity.push_str(&output);
    }
    for key in TOOLCHAIN_ENV {
        identity.push_str(key);
        identity.push_str(&std::env::var(key).unwrap_or_default());
    }
    Ok(crate::acceptance_snapshot::digest(identity.as_bytes()))
}

pub(crate) async fn run(
    check: AcceptanceCheck,
    snapshot: SourceSnapshot,
    context: &ToolContext,
) -> Result<CheckOutcome, String> {
    static SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let permit = tokio::time::timeout(
        Duration::from_secs(5),
        SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
            .clone()
            .acquire_owned(),
    )
    .await
    .map_err(|_| "verification capacity busy; retry after existing checks settle")?
    .map_err(|_| "verification capacity closed")?;
    if !check.platform.matches_host() {
        return Err("required platform differs from the running verifier; local tests cannot certify another platform".into());
    }
    struct OwnedSignal {
        parent: Arc<dyn CancellationSignal>,
        dropped: std::sync::atomic::AtomicBool,
    }
    impl CancellationSignal for OwnedSignal {
        fn is_cancelled(&self) -> bool {
            self.parent.is_cancelled() || self.dropped.load(std::sync::atomic::Ordering::Acquire)
        }
    }
    struct CancelOnDrop(Arc<OwnedSignal>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0
                .dropped
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    let cancelled = Arc::new(OwnedSignal {
        parent: context.cancellation.clone(),
        dropped: std::sync::atomic::AtomicBool::new(false),
    });
    let _owner = CancelOnDrop(cancelled.clone());
    let cancelled: Arc<dyn CancellationSignal> = cancelled;
    let sandbox = context.sandbox.clone();
    let argv = vesper_agent::acceptance::cargo_argv(&check);
    let command = argv
        .iter()
        .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    if !vesper_agent::acceptance::verification_command_allowed(context, &command) {
        return Err("verification refused by command firewall".into());
    }
    tokio::task::spawn_blocking(move || {
        // Retain capacity even if the observing async turn is dropped.
        let _permit = permit;
        let started = Instant::now();
        if cancelled.is_cancelled() {
            return Err("verification cancelled".into());
        }
        let dir = snapshot.materialize()?;
        // All argv elements are constrained identifiers or fixed flags. Shell
        // projection exists only for the existing injected sandbox route.
        let (success, output) = if let Some(route) = sandbox {
            if !route.satisfies_demand() {
                return Err(route.refusal_text());
            }
            let result = route
                .port()
                .run_command(&command, dir.path(), check.timeout_seconds, &cancelled)
                .map_err(|_| "sandbox verification failed or cleanup unverified")?;
            (!result.timed_out, result.output)
        } else {
            run_process(&argv, dir.path(), check.timeout_seconds, cancelled)?
        };
        let after = SourceSnapshot::capture(dir.path())?;
        if after.digest != snapshot.digest {
            return Err("verifier changed its protected source inputs".into());
        }
        let (state, diagnostic) = classify(&check, success, &output);
        Ok(CheckOutcome {
            state,
            output,
            diagnostic,
            elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        })
    })
    .await
    .map_err(|_| "verification worker failed")?
}

fn drain(reader: impl Read + Send + 'static) -> mpsc::Receiver<(Vec<u8>, bool)> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut result = Vec::new();
        let mut truncated = false;
        let mut chunk = [0; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let keep = n.min(MAX_OUTPUT.saturating_sub(result.len()));
                    result.extend_from_slice(&chunk[..keep]);
                    truncated |= keep != n;
                }
                Err(_) => {
                    truncated = true;
                    break;
                }
            }
        }
        let _ = tx.send((result, truncated));
    });
    rx
}

fn run_process(
    argv: &[String],
    cwd: &Path,
    timeout: u64,
    cancelled: Arc<dyn CancellationSignal>,
) -> Result<(bool, String), String> {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // No provider credentials are passed to build scripts or test programs.
    command.env_clear();
    for key in TOOLCHAIN_ENV {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TERM_COLOR", "never")
        .env("RUSTUP_AUTO_INSTALL", "0");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "cannot start cargo verification; install the Rust toolchain")?;
    let pid = child.id();
    let stdout = drain(child.stdout.take().ok_or("missing verifier stdout")?);
    let stderr = drain(child.stderr.take().ok_or("missing verifier stderr")?);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let result = loop {
        if cancelled.is_cancelled() || Instant::now() >= deadline {
            break Err(
                "verification cancelled or timed out; completion remains unverified".to_string(),
            );
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break Err("cannot observe verifier exit".into()),
        }
    };
    // The group was created by this collector. Do not leave descendants holding
    // output pipes after timeout or successful leader exit.
    #[cfg(unix)]
    let cleanup = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(windows)]
    let cleanup = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(not(any(unix, windows)))]
    let cleanup: std::io::Result<std::process::ExitStatus> =
        Err(std::io::Error::other("unsupported process cleanup"));
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    if cleanup.is_err() {
        return Err("verifier process-tree cleanup tool unavailable".into());
    }
    let (out, out_truncated) = stdout
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "verifier stdout did not close; cleanup uncertain")?;
    let (err, err_truncated) = stderr
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "verifier stderr did not close; cleanup uncertain")?;
    if out_truncated || err_truncated {
        return Err("verifier output exceeded bounded capture or failed to read".into());
    }
    let mut output = String::from_utf8(out).map_err(|_| "verifier stdout is not UTF-8")?;
    output.push('\n');
    output.push_str(&String::from_utf8_lossy(&err));
    Ok((result?, output))
}
