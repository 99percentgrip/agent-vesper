#![forbid(unsafe_code)]
//! Opt-in OS sandbox backends with honest capability reporting (VRO-13 PR-3).
//!
//! This library is **100% safe code**. Every raw syscall (`unshare`,
//! `mount`, uid/gid map writes, `fork`, `prctl(PR_SET_PDEATHSIG)`, `chroot`,
//! `execve`) lives in the dedicated `sandbox_init` supervisor binary, which
//! the backend spawns through plain `std::process::Command`. The safety
//! argument and the raw-syscall exception are recorded in ADR 0022 and
//! machine-enforced by `cargo xtask architecture`.
//!
//! Sandboxing is strictly opt-in: nothing in this crate is constructed
//! unless a tool explicitly demands isolation
//! ([`vesper_security::IsolationRequirement`]). With no demand the executor
//! path is unchanged and byte-identical to the pre-sandbox path.
//!
//! Capabilities are **probed, never assumed**: a backend that could not
//! create a network namespace reports `network: Unavailable`, and the policy
//! layer then denies `IsolationRequirement::Network` demands fail-closed
//! (`DecisionReason::IsolationUnavailable`).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use vesper_security::{CapabilityStatus, SandboxCapabilities, SecurityStrength};

mod linux;
mod stub;

#[cfg(feature = "docker")]
pub mod docker;

pub use linux::NamespacesBackend as LinuxNamespacesBackend;
pub use stub::UnavailableBackend;

#[cfg(feature = "docker")]
pub use docker::{DockerBackend, DockerSandboxConfig};

/// Honest capability report for the platform-default backend, probed once
/// per process. Hosts (TUI/ACP) use this to build their
/// `SandboxBackendPort` implementation without constructing a backend they
/// will never run; a failed probe is reported as all-`Unavailable` rather
/// than assumed available.
#[must_use]
pub fn probe_default_backend_caps() -> SandboxCapabilities {
    let _probe = default_backend();
    #[cfg(target_os = "linux")]
    {
        crate::linux::NamespacesBackend::probe_and_build().capabilities()
    }
    #[cfg(not(target_os = "linux"))]
    {
        crate::stub::unavailable_caps()
    }
}

/// Default backend for the current platform.
///
/// Linux: the namespaces backend (capability-probed at construction; on a
/// host that forbids unprivileged namespaces every capability reports
/// `Unavailable`). Other platforms: the honest stub that reports
/// `Unavailable` for everything, so any demand fails closed.
#[must_use]
pub fn default_backend() -> Arc<dyn SandboxBackend> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxNamespacesBackend::probe_and_build())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Arc::new(UnavailableBackend)
    }
}

/// What one provisioned sandbox may run.
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxSpec {
    /// Directory mounted read-write inside the sandbox (the workspace root).
    pub writable_root: PathBuf,
    /// Maximum wall-clock seconds for one `run` before teardown.
    pub timeout_seconds: u64,
    /// Environment allowlist carried into the sandbox. The supervisor
    /// clears every inherited variable and installs exactly this list —
    /// credential hygiene is enforced by the boundary, not the caller.
    pub env_allowlist: Vec<String>,
    /// Hard CPU quota for one provisioned sandbox, in CPU units
    /// (Docker `--cpus`). PRD §2.2 Backend B: cpu/memory limits.
    pub cpu_limit: Option<f64>,
    /// Hard memory ceiling for one provisioned sandbox, in bytes
    /// (Docker `--memory`). PRD §2.2 Backend B.
    pub memory_limit_bytes: Option<u64>,
    /// Explicit network grant. `false` (the default and the only safe
    /// default) provisions the sandbox with **no network**. `true` is only
    /// reachable when a tool or scope explicitly demanded network isolation
    /// *grants* — never silently.
    pub allow_network: bool,
}

impl SandboxSpec {
    /// Minimal spec: writable root plus the fixed credential-free baseline.
    #[must_use]
    pub fn new(writable_root: PathBuf) -> Self {
        Self {
            writable_root,
            timeout_seconds: 120,
            env_allowlist: baseline_env(),
            cpu_limit: None,
            memory_limit_bytes: None,
            allow_network: false,
        }
    }

    /// Builder: attaches a CPU quota (Docker `--cpus`).
    #[must_use]
    pub fn with_cpu_limit(mut self, cpus: f64) -> Self {
        self.cpu_limit = Some(cpus);
        self
    }

    /// Builder: attaches a memory ceiling in bytes (Docker `--memory`).
    #[must_use]
    pub fn with_memory_limit_bytes(mut self, bytes: u64) -> Self {
        self.memory_limit_bytes = Some(bytes);
        self
    }

    /// Builder: grants network access inside the sandbox.
    ///
    /// Opt-in only — the default is no network. Granting requires an
    /// explicit scope/tool demand; nothing in the default path sets this.
    #[must_use]
    pub fn with_network_grant(mut self) -> Self {
        self.allow_network = true;
        self
    }
}

/// Fixed, credential-free environment baseline (PRD §2.4).
///
/// No provider keys, tokens, or cognition-root paths are ever provisioned
/// into a sandbox; authentication stays in the harness process.
#[must_use]
pub fn baseline_env() -> Vec<String> {
    vec![
        "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        "HOME=/tmp/vesper-sandbox-home".into(),
        "LANG=C.UTF-8".into(),
        "TERM=dumb".into(),
        "PAGER=cat".into(),
        "GIT_TERMINAL_PROMPT=0".into(),
        "DEBIAN_FRONTEND=noninteractive".into(),
    ]
}

/// Maximum bytes of stdout/stderr kept per run (each stream independently).
pub const OUTPUT_CAP_BYTES: usize = 64 * 1024;

/// A provisioned sandbox. Dropping it tears the sandbox down.
pub struct SandboxHandle {
    /// Supervisor process (`hold` mode: namespace-ready, waiting for one
    /// run request on stdin). Its `stdin` field is the run channel.
    pub(crate) child: Mutex<Child>,
    /// VRO-13 PR-4 (Docker): total teardown command recorded at provision
    /// time (`docker rm -f <name>`). `None` for the namespaces backend,
    /// whose teardown is `Child::kill` + PDEATHSIG chaining.
    pub(crate) teardown_command: Option<Vec<String>>,
    /// Payload stdout relay, taken by the first `run` call.
    pub(crate) stdout: Mutex<Option<std::io::BufReader<ChildStdout>>>,
    /// Writable root recorded for assertions and bookkeeping.
    pub writable_root: PathBuf,
    /// Wall-clock bound for one run, carried from the provisioning spec.
    pub timeout_seconds: u64,
}

impl SandboxHandle {
    /// PID of the supervisor process. `run` communicates over its stdin;
    /// teardown kills it, which chains SIGKILL into the namespace init
    /// (`PR_SET_PDEATHSIG`) and the kernel then reaps every namespace member.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child
            .lock()
            .map(|child| child.id())
            .unwrap_or_default()
    }
}

impl Drop for SandboxHandle {
    fn drop(&mut self) {
        // VRO-13 PR-4 (Docker): the recorded teardown command is total even
        // if the client process already exited (--rm is daemon-side and
        // `rm -f` is idempotent against an already-removed container).
        if let Some(argv) = self.teardown_command.as_ref()
            && let Some((program, rest)) = argv.split_first()
        {
            let _ = Command::new(program)
                .args(rest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        // Kill the supervisor → PDEATHSIG kills the namespace init → the
        // kernel SIGKILLs every remaining process in the PID namespace.
        // The mount namespace dies with its last member, so no host mount
        // survives: teardown is total without any unsafe call from here.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl std::fmt::Debug for SandboxHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SandboxHandle")
            .field("pid", &self.pid())
            .field("writable_root", &self.writable_root)
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

/// One command execution request inside a provisioned sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Argv {
    /// Argument vector; `argv[0]` is the program.
    pub argv: Vec<String>,
    /// Working directory inside the sandbox.
    pub cwd: PathBuf,
}

/// Bounded execution outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    /// Exit code, when the process ran to completion.
    pub exit_code: Option<i32>,
    /// Bounded stdout.
    pub stdout: String,
    /// Bounded stderr.
    pub stderr: String,
    /// True when the timeout fired first.
    pub timed_out: bool,
}

/// Every sandbox failure mode, honestly named.
#[derive(Debug)]
pub enum SandboxError {
    /// The backend cannot provide what the request demands.
    CapabilityUnavailable {
        /// What the caller demanded.
        requirement: vesper_security::IsolationRequirement,
        /// What the backend honestly provides.
        capabilities: SandboxCapabilities,
    },
    /// Provisioning failed before the sandbox existed.
    Provision(String),
    /// Running inside the sandbox failed.
    Run(String),
    /// Teardown failed.
    Teardown(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapabilityUnavailable {
                requirement,
                capabilities,
            } => write!(
                formatter,
                "sandbox unavailable: backend {:?} cannot satisfy {:?} \
                 (process_tree={:?}, filesystem={:?}, network={:?})",
                capabilities.backend,
                requirement,
                capabilities.process_tree,
                capabilities.filesystem,
                capabilities.network
            ),
            Self::Provision(message) => write!(formatter, "sandbox provision failed: {message}"),
            Self::Run(message) => write!(formatter, "sandbox run failed: {message}"),
            Self::Teardown(message) => write!(formatter, "sandbox teardown failed: {message}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// Boxed backend future (runtime-agnostic, mirroring the provider ports).
pub type SandboxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Backend contract. `Send + Sync` so hosts can hold one behind an `Arc`.
pub trait SandboxBackend: Send + Sync {
    /// Honest, probed capabilities. Never claims what it has not verified.
    fn capabilities(&self) -> SandboxCapabilities;

    /// Provision one ephemeral sandbox for `spec`.
    fn provision<'a>(
        &'a self,
        spec: &'a SandboxSpec,
    ) -> SandboxFuture<'a, Result<SandboxHandle, SandboxError>>;

    /// Run `argv` inside the provisioned sandbox and wait for it (bounded
    /// by `spec.timeout_seconds`).
    fn run<'a>(
        &'a self,
        handle: &'a SandboxHandle,
        argv: &'a Argv,
    ) -> SandboxFuture<'a, Result<ExecOutput, SandboxError>>;

    /// Explicitly tear the sandbox down (dropping the handle also works).
    fn teardown<'a>(&'a self, handle: SandboxHandle)
    -> SandboxFuture<'a, Result<(), SandboxError>>;
}

/// Locates the supervisor binary next to the current executable.
///
/// Production ships the supervisor beside the host binary; callers (tests,
/// embedders) may override with `VESPER_SANDBOX_INIT`.
pub(crate) fn supervisor_path() -> Result<PathBuf, SandboxError> {
    if let Ok(from_env) = std::env::var("VESPER_SANDBOX_INIT") {
        return Ok(PathBuf::from(from_env));
    }
    let exe = std::env::current_exe()
        .map_err(|error| SandboxError::Provision(format!("current_exe failed: {error}")))?;
    let sibling = exe
        .parent()
        .ok_or_else(|| SandboxError::Provision("current_exe has no parent".into()))?
        .join("sandbox_init");
    if sibling.exists() {
        return Ok(sibling);
    }
    Err(SandboxError::Provision(format!(
        "sandbox supervisor not found beside {} (set VESPER_SANDBOX_INIT to override)",
        exe.display()
    )))
}

/// Spawns the supervisor for `hold` mode: namespaces ready, waiting for one
/// command line on stdin.
pub(crate) fn spawn_hold(
    supervisor: &std::path::Path,
    spec: &SandboxSpec,
) -> Result<Child, SandboxError> {
    let mut command = Command::new(supervisor);
    command
        .arg("hold")
        .arg("--root")
        .arg(&spec.writable_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for pair in &spec.env_allowlist {
        command.arg("--env").arg(pair);
    }
    command
        .spawn()
        .map_err(|error| SandboxError::Provision(format!("spawn {:?}: {error}", supervisor)))
}

/// Spawns the supervisor's `probe` subcommand.
pub(crate) fn spawn_probe(supervisor: &std::path::Path) -> Result<Child, SandboxError> {
    Command::new(supervisor)
        .arg("probe")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SandboxError::Provision(format!("spawn probe {:?}: {error}", supervisor)))
}

/// Parses the supervisor's one-line probe report:
/// `<backend> <process_tree> <filesystem> <network>`.
pub(crate) fn parse_capability_line(
    line: &str,
) -> Option<(String, CapabilityStatus, CapabilityStatus, CapabilityStatus)> {
    let mut parts = line.split_whitespace();
    let backend = parts.next()?.to_owned();
    let parse = |value: Option<&str>| match value {
        Some("available") => Some(CapabilityStatus::Available),
        Some("unavailable") => Some(CapabilityStatus::Unavailable),
        Some("unknown") => Some(CapabilityStatus::Unknown),
        _ => None,
    };
    Some((
        backend,
        parse(parts.next())?,
        parse(parts.next())?,
        parse(parts.next())?,
    ))
}

/// Strength from three honest statuses (never upgraded past a gap).
#[must_use]
pub fn strength_from_statuses(
    process_tree: CapabilityStatus,
    filesystem: CapabilityStatus,
    network: CapabilityStatus,
) -> SecurityStrength {
    let process = matches!(process_tree, CapabilityStatus::Available);
    let fs = matches!(filesystem, CapabilityStatus::Available);
    let net = matches!(network, CapabilityStatus::Available);
    match (process, fs, net) {
        (true, true, true) => SecurityStrength::Full,
        (true, true, false) | (true, false, true) => SecurityStrength::Filesystem,
        (true, false, false) => SecurityStrength::Process,
        _ => SecurityStrength::None,
    }
}

/// Encodes one run request for the supervisor's stdin protocol.
///
/// `<cwd><US><argv[0]><US><argv[1]>…` — the supervisor splits on the ASCII
/// unit separator, so paths and arguments may contain spaces.
pub(crate) fn encode_run_line(argv: &Argv) -> String {
    let mut line = argv.cwd.to_string_lossy().into_owned();
    for argument in &argv.argv {
        line.push('\x1f');
        line.push_str(argument);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_never_upgrades_partial_availability() {
        // process+network but no filesystem must NOT report Full.
        let strength = strength_from_statuses(
            CapabilityStatus::Available,
            CapabilityStatus::Unavailable,
            CapabilityStatus::Available,
        );
        assert_eq!(strength, SecurityStrength::Filesystem);
        let none = strength_from_statuses(
            CapabilityStatus::Unavailable,
            CapabilityStatus::Available,
            CapabilityStatus::Available,
        );
        assert_eq!(none, SecurityStrength::None);
    }

    #[test]
    fn capability_line_parses_honest_reports_only() {
        assert!(parse_capability_line("linux-namespaces available available available").is_some());
        assert!(
            parse_capability_line("linux-namespaces available unavailable available").is_some(),
            "a probed-failed capability must still parse as Unavailable"
        );
        assert!(parse_capability_line("linux-namespaces unknown unknown unknown").is_some());
        assert!(parse_capability_line("garbage that is not a report").is_none());
        assert!(parse_capability_line("linux-namespaces available available").is_none());
    }

    #[test]
    fn baseline_env_is_credential_free() {
        for pair in baseline_env() {
            let lower = pair.to_lowercase();
            assert!(
                !lower.contains("token") && !lower.contains("secret") && !lower.contains("key="),
                "credential-shaped env leaked: {pair}"
            );
        }
    }

    #[test]
    fn spec_defaults_to_baseline_env_and_bounded_timeout() {
        let spec = SandboxSpec::new(PathBuf::from("/tmp/ws"));
        assert_eq!(spec.env_allowlist, baseline_env());
        assert_eq!(spec.timeout_seconds, 120);
    }

    #[test]
    fn run_line_encoding_preserves_spaces_and_paths() {
        let argv = Argv {
            argv: vec!["/bin/sh".into(), "-c".into(), "echo 'a b'".into()],
            cwd: PathBuf::from("/tmp/some dir"),
        };
        let line = encode_run_line(&argv);
        assert!(line.starts_with("/tmp/some dir\x1f/bin/sh\x1f-c\x1fecho 'a b'"));
    }
}
