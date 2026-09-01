//! Docker sandbox backend (VRO-13 PR-4). Feature-gated behind `docker`;
//! zero new mandatory dependencies for default builds.
//!
//! Wraps the `docker` CLI through safe `std::process` exactly like the
//! namespaces backend orchestrates `sandbox_init` — no new ADR-0022
//! raw-syscall surface; all code in this module is safe.
//!
//! Lifecycle per provision:
//!
//! * **provision** — `docker run --rm -d --name agent-vesper-sbx-<slug>
//!   --cpus <n> --memory <m> --pids-limit 512 --network none
//!   --mount type=bind,source=<root>,target=/workspace -w /workspace
//!   <image> sleep <timeout>`: a detached, network-less, resource-bounded
//!   container with the primary root bind-mounted read-write at
//!   `/workspace`. `--rm` makes daemon-side removal implicit on exit.
//! * **run** — `docker exec -w <container-cwd> <name> <argv…>` with bounded
//!   output capture, wall-clocked against `spec.timeout_seconds`.
//! * **teardown** — `docker rm -f <name>` (recorded as the handle's
//!   `teardown_command`, so dropping the handle is total teardown even if
//!   the client process already exited; `rm -f` is idempotent).
//!
//! **Cold-start guard**: [`DockerBackend::probe_daemon`] runs a bounded
//! `docker version --format {{.Server.Version}}` before anything else. An
//! unreachable daemon makes every capability honestly `Unavailable`, so
//! isolation demands fail closed with the model-facing "sandbox
//! unavailable" refusal instead of hanging the turn or silently running
//! unsandboxed.
//!
//! **Network is strictly opt-in**: the default is `--network none`. The
//! only path to a network grant is an explicit
//! [`DockerSandboxConfig::network`] set by a scope/tool demand — never
//! inferred from capability discovery or config defaults.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};

use crate::{
    Argv, ExecOutput, OUTPUT_CAP_BYTES, SandboxBackend, SandboxError, SandboxFuture, SandboxHandle,
    SandboxSpec,
};

const BACKEND_ID: &str = "docker";

/// Env override for the docker binary (tests point this at a stub script).
pub const DOCKER_BIN_OVERRIDE: &str = "VESPER_DOCKER_BIN";
/// Env override for the image tag.
pub const DOCKER_IMAGE_OVERRIDE: &str = "VESPER_DOCKER_IMAGE";

/// Default container memory limit (2 GiB).
pub const DEFAULT_MEMORY_LIMIT: &str = "2g";
/// Default container CPU quota.
pub const DEFAULT_CPU_LIMIT: &str = "2";
/// Default image for ephemeral tool sandboxes.
pub const DEFAULT_IMAGE: &str = "alpine:3.20";
/// Container name prefix (PRD §2.2).
pub const CONTAINER_NAME_PREFIX: &str = "agent-vesper-sbx-";
/// Default PID ceiling inside one container.
pub const DEFAULT_PIDS_LIMIT: &str = "512";
/// Bounded wall-clock for the daemon liveness probe itself.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration for the Docker backend.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DockerSandboxConfig {
    /// Explicit network grant. `false` (the default) provisions with
    /// `--network none`. Only a tool/scope demand may set this; nothing
    /// else in this crate ever flips it.
    pub network: bool,
    /// Image tag; defaults to [`DEFAULT_IMAGE`] unless
    /// [`DOCKER_IMAGE_OVERRIDE`] is set.
    pub image: Option<String>,
    /// Docker binary; defaults to `"docker"` unless [`DOCKER_BIN_OVERRIDE`]
    /// is set.
    pub docker_bin: Option<PathBuf>,
    /// CPU quota for `--cpus`; defaults to [`DEFAULT_CPU_LIMIT`].
    pub cpus: Option<String>,
    /// Memory limit for `--memory`; defaults to [`DEFAULT_MEMORY_LIMIT`].
    pub memory: Option<String>,
    /// Session slug for the container name; a per-provision unique slug is
    /// used when absent.
    pub session_slug: Option<String>,
}

impl DockerSandboxConfig {
    /// Resolved docker binary (explicit, then env, then PATH default).
    #[must_use]
    pub fn resolved_bin(&self) -> PathBuf {
        if let Some(bin) = &self.docker_bin {
            return bin.clone();
        }
        if let Ok(from_env) = std::env::var(DOCKER_BIN_OVERRIDE)
            && !from_env.is_empty()
        {
            return PathBuf::from(from_env);
        }
        PathBuf::from("docker")
    }

    /// Resolved image tag (explicit, then env, then default).
    #[must_use]
    pub fn resolved_image(&self) -> String {
        if let Some(image) = &self.image {
            return image.clone();
        }
        if let Ok(from_env) = std::env::var(DOCKER_IMAGE_OVERRIDE)
            && !from_env.is_empty()
        {
            return from_env;
        }
        DEFAULT_IMAGE.to_owned()
    }

    /// Resolved `--cpus` value.
    #[must_use]
    pub fn resolved_cpus(&self) -> String {
        self.cpus
            .clone()
            .unwrap_or_else(|| DEFAULT_CPU_LIMIT.to_owned())
    }

    /// Resolved `--memory` value.
    #[must_use]
    pub fn resolved_memory(&self) -> String {
        self.memory
            .clone()
            .unwrap_or_else(|| DEFAULT_MEMORY_LIMIT.to_owned())
    }
}

/// Docker backend wrapping `docker run --rm`. Capabilities come from a real
/// bounded daemon probe — never assumed.
#[derive(Debug)]
pub struct DockerBackend {
    config: DockerSandboxConfig,
}

impl DockerBackend {
    /// Builds the backend with explicit configuration.
    #[must_use]
    pub fn new(config: DockerSandboxConfig) -> Self {
        Self { config }
    }

    /// Builds the backend with defaults (no network, alpine, 2 CPU / 2 GiB).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(DockerSandboxConfig::default())
    }

    /// The backend's configuration.
    #[must_use]
    pub fn config(&self) -> &DockerSandboxConfig {
        &self.config
    }

    /// Resolved docker binary path.
    #[must_use]
    pub fn docker_bin(&self) -> PathBuf {
        self.config.resolved_bin()
    }

    /// Container name for one provision.
    #[must_use]
    pub fn container_name(&self) -> String {
        let slug = self.config.session_slug.clone().unwrap_or_else(unique_slug);
        format!("{CONTAINER_NAME_PREFIX}{slug}")
    }

    /// Cold-start guard: bounded `docker version` liveness probe.
    ///
    /// Returns the daemon's server version on success. Any failure — binary
    /// missing, daemon unreachable, probe timeout — is an honest `Err`
    /// carrying the model-facing "sandbox unavailable" text, so provision
    /// fails fast instead of hanging the turn.
    pub fn probe_daemon(&self) -> Result<String, SandboxError> {
        let binary = self.docker_bin();
        let mut command = Command::new(&binary);
        command
            .args(["version", "--format", "{{.Server.Version}}"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            SandboxError::Provision(format!(
                "sandbox unavailable: cannot run docker binary {binary:?} ({error}); \
                 the operation needs isolation that cannot be provided"
            ))
        })?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = String::new();
                    if let Some(pipe) = child.stdout.take() {
                        let _ = pipe.take(1024).read_to_string(&mut stdout);
                    }
                    if status.success() {
                        let version = stdout.trim();
                        if version.is_empty() {
                            return Err(daemon_probe_error(&binary, "version output was empty"));
                        }
                        return Ok(version.to_owned());
                    }
                    let mut stderr = String::new();
                    if let Some(pipe) = child.stderr.take() {
                        let _ = pipe.take(2048).read_to_string(&mut stderr);
                    }
                    let reason = if stderr.trim().is_empty() {
                        format!("exit status {status}")
                    } else {
                        stderr.trim().to_owned()
                    };
                    return Err(daemon_probe_error(&binary, &reason));
                }
                Ok(None) => {
                    if started.elapsed() >= PROBE_TIMEOUT {
                        let _ = child.kill();
                        return Err(daemon_probe_error(&binary, "probe timed out after 5s"));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    return Err(daemon_probe_error(&binary, &error.to_string()));
                }
            }
        }
    }

    /// Builds the `docker run` argument vector for one provision.
    ///
    /// Pure and testable: no process is spawned. `root` must already be
    /// canonicalized by the caller.
    #[must_use]
    pub fn run_args(
        &self,
        name: &str,
        root: &std::path::Path,
        timeout_seconds: u64,
    ) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "run".into(),
            "--rm".into(),
            "-d".into(),
            "--name".into(),
            name.to_owned(),
            "--cpus".into(),
            self.config.resolved_cpus(),
            "--memory".into(),
            self.config.resolved_memory(),
            "--pids-limit".into(),
            DEFAULT_PIDS_LIMIT.into(),
            // Strictly no network unless explicitly granted.
            "--network".into(),
            if self.config.network {
                "bridge"
            } else {
                "none"
            }
            .into(),
            "--mount".into(),
            format!("type=bind,source={},target=/workspace", root.display()),
            "-w".into(),
            "/workspace".into(),
            self.config.resolved_image(),
        ];
        // Keep the container alive for the whole provision window; payloads
        // arrive later via `docker exec`.
        args.push("sleep".into());
        args.push(timeout_seconds.max(1).to_string());
        args
    }

    /// Builds the `docker exec` argument vector for one run.
    ///
    /// Pure and testable. `container_cwd` is the workspace-relative cwd
    /// expressed inside the container (`/workspace` prefix).
    #[must_use]
    pub fn exec_args(&self, name: &str, container_cwd: &str, argv: &Argv) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "exec".into(),
            "-w".into(),
            container_cwd.to_owned(),
            name.to_owned(),
        ];
        args.extend(argv.argv.iter().cloned());
        args
    }

    /// Maps a host workspace cwd into the container's `/workspace` path.
    #[must_use]
    pub fn container_cwd(host_root: &std::path::Path, cwd: &std::path::Path) -> String {
        match cwd.strip_prefix(host_root) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                format!("/workspace/{}", relative.display())
            }
            _ => "/workspace".to_owned(),
        }
    }
}

/// Model-facing daemon-unreachable error (PRD §2.2 cold-start guard).
fn daemon_probe_error(binary: &std::path::Path, reason: &str) -> SandboxError {
    SandboxError::Provision(format!(
        "sandbox unavailable: docker daemon probe failed for {binary:?} ({reason}); \
         the operation needs isolation that cannot be provided"
    ))
}

/// Per-provision unique slug (process counter + pid; no external state).
fn unique_slug() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:08x}-{}", count, std::process::id())
}

impl SandboxBackend for DockerBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        match self.probe_daemon() {
            Ok(version) => SandboxCapabilities {
                backend: format!("{BACKEND_ID} ({version})"),
                // A container owns its process tree outright, the bind-mount
                // plus image filesystem provide the boundary around
                // everything except the mounted workspace, and the network
                // namespace is the daemon's. These are real boundaries the
                // daemon enforces, verified by the successful probe.
                process_tree: CapabilityStatus::Available,
                filesystem: CapabilityStatus::Available,
                network: CapabilityStatus::Available,
                strength: SecurityStrength::Full,
            },
            Err(_) => SandboxCapabilities {
                backend: BACKEND_ID.to_owned(),
                process_tree: CapabilityStatus::Unavailable,
                filesystem: CapabilityStatus::Unavailable,
                network: CapabilityStatus::Unavailable,
                strength: SecurityStrength::None,
            },
        }
    }

    fn provision<'a>(
        &'a self,
        spec: &'a SandboxSpec,
    ) -> SandboxFuture<'a, Result<SandboxHandle, SandboxError>> {
        let spec = spec.clone();
        Box::pin(async move {
            // Fail-closed gate first: if the daemon is down, nothing runs.
            let caps = self.capabilities();
            if !caps.satisfies(IsolationRequirement::Filesystem) {
                return Err(SandboxError::CapabilityUnavailable {
                    requirement: IsolationRequirement::Filesystem,
                    capabilities: caps,
                });
            }
            let binary = self.docker_bin();
            let name = self.container_name();
            let root = spec.writable_root.canonicalize().map_err(|error| {
                SandboxError::Provision(format!(
                    "writable root {} cannot be canonicalized: {error}",
                    spec.writable_root.display()
                ))
            })?;
            let mut command = Command::new(&binary);
            command
                .args(self.run_args(&name, &root, spec.timeout_seconds))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child: Child = command.spawn().map_err(|error| {
                SandboxError::Provision(format!("docker run spawn failed: {error}"))
            })?;
            // Detached (`-d`) `docker run` exits once the container starts.
            let status = child
                .wait()
                .map_err(|error| SandboxError::Provision(format!("docker run wait: {error}")))?;
            if !status.success() {
                let mut stderr = String::new();
                if let Some(pipe) = child.stderr.take() {
                    let _ = pipe.take(4096).read_to_string(&mut stderr);
                }
                let reason = if stderr.trim().is_empty() {
                    format!("exit status {status}")
                } else {
                    stderr.trim().to_owned()
                };
                return Err(SandboxError::Provision(format!(
                    "docker run failed: {reason}"
                )));
            }
            // Total teardown command, recorded on the handle so Drop is
            // total even if this client process exits before teardown.
            let mut teardown = vec![
                binary.to_string_lossy().into_owned(),
                "rm".into(),
                "-f".into(),
            ];
            teardown.push(name.clone());
            // A sentinel child we own for PID bookkeeping: a short-lived
            // `docker version` re-check that exits immediately. The real
            // cleanup lever is `teardown_command` above.
            let sentinel = Command::new(&binary)
                .arg("version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| {
                    SandboxError::Provision(format!("post-run sentinel spawn failed: {error}"))
                })?;
            Ok(SandboxHandle {
                child: std::sync::Mutex::new(sentinel),
                teardown_command: Some(teardown),
                stdout: std::sync::Mutex::new(None),
                writable_root: spec.writable_root.clone(),
                timeout_seconds: spec.timeout_seconds,
            })
        })
    }

    fn run<'a>(
        &'a self,
        handle: &'a SandboxHandle,
        argv: &'a Argv,
    ) -> SandboxFuture<'a, Result<ExecOutput, SandboxError>> {
        let argv = argv.clone();
        Box::pin(async move {
            let Some(teardown) = handle.teardown_command.as_ref() else {
                return Err(SandboxError::Run(
                    "handle carries no docker teardown command; not a docker provision".into(),
                ));
            };
            let Some(name) = teardown.last() else {
                return Err(SandboxError::Run(
                    "docker handle lost its container name".into(),
                ));
            };
            let binary = self.docker_bin();
            let container_cwd = Self::container_cwd(&handle.writable_root, &argv.cwd);
            let mut command = Command::new(&binary);
            command
                .args(self.exec_args(name, &container_cwd, &argv))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command
                .spawn()
                .map_err(|error| SandboxError::Run(format!("docker exec spawn: {error}")))?;
            let started = Instant::now();
            let deadline = Duration::from_secs(handle.timeout_seconds.max(1));
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let mut stdout_bytes = Vec::new();
                        let mut stderr_bytes = Vec::new();
                        if let Some(pipe) = child.stdout.take() {
                            pipe.take((OUTPUT_CAP_BYTES + 1) as u64)
                                .read_to_end(&mut stdout_bytes)
                                .map_err(|error| {
                                    SandboxError::Run(format!("read stdout: {error}"))
                                })?;
                        }
                        if let Some(pipe) = child.stderr.take() {
                            pipe.take((OUTPUT_CAP_BYTES + 1) as u64)
                                .read_to_end(&mut stderr_bytes)
                                .map_err(|error| {
                                    SandboxError::Run(format!("read stderr: {error}"))
                                })?;
                        }
                        return Ok(ExecOutput {
                            exit_code: status.code(),
                            stdout: cap_output(&stdout_bytes),
                            stderr: cap_output(&stderr_bytes),
                            timed_out: false,
                        });
                    }
                    Ok(None) => {
                        if started.elapsed() >= deadline {
                            let _ = child.kill();
                            return Ok(ExecOutput {
                                exit_code: None,
                                stdout: String::new(),
                                stderr: format!(
                                    "sandbox run exceeded {}s and was terminated",
                                    handle.timeout_seconds
                                ),
                                timed_out: true,
                            });
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(error) => {
                        return Err(SandboxError::Run(format!("docker exec wait: {error}")));
                    }
                }
            }
        })
    }

    fn teardown<'a>(
        &'a self,
        handle: SandboxHandle,
    ) -> SandboxFuture<'a, Result<(), SandboxError>> {
        Box::pin(async move {
            let Some(argv) = handle.teardown_command.clone() else {
                // Not a docker provision (should not happen through this
                // backend); fall back to killing the recorded child so the
                // drop path stays total.
                if let Ok(mut child) = handle.child.lock() {
                    let _ = child.kill();
                }
                return Ok(());
            };
            let Some((program, rest)) = argv.split_first() else {
                return Ok(());
            };
            let status = Command::new(program)
                .args(rest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match status {
                // `docker rm -f` on an already-removed container exits 1
                // with "No such container"; that is success for teardown
                // purposes (`--rm` already collected it).
                Ok(_) => Ok(()),
                // The docker binary itself vanished (uninstalled
                // mid-session). Honest failure, surfaced to the caller.
                Err(error) => Err(SandboxError::Teardown(format!(
                    "docker rm -f failed to spawn: {error}"
                ))),
            }
        })
    }
}

/// Caps raw output bytes to [`OUTPUT_CAP_BYTES`] on a UTF-8 boundary and
/// appends the truncation marker, mirroring the namespaces backend.
fn cap_output(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if text.len() > OUTPUT_CAP_BYTES {
        let mut end = OUTPUT_CAP_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("… [truncated]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DockerSandboxConfig {
        DockerSandboxConfig {
            session_slug: Some("tests".into()),
            ..DockerSandboxConfig::default()
        }
    }

    #[test]
    fn run_args_enforce_limits_bind_and_no_network_by_default() {
        let backend = DockerBackend::new(config());
        let args = backend.run_args(
            "agent-vesper-sbx-tests",
            std::path::Path::new("/tmp/ws"),
            120,
        );
        let joined = args.join(" ");
        // Limits (PRD §2.2 Backend B).
        assert!(joined.contains("--cpus 2"), "args: {joined}");
        assert!(joined.contains("--memory 2g"), "args: {joined}");
        // Workdir bind-mount of the primary root.
        assert!(
            joined.contains("--mount type=bind,source=/tmp/ws,target=/workspace"),
            "args: {joined}"
        );
        assert!(joined.contains("-w /workspace"), "args: {joined}");
        // Strictly no network unless explicitly granted.
        assert!(joined.contains("--network none"), "args: {joined}");
        // Ephemeral.
        assert!(joined.contains("--rm"), "args: {joined}");
        assert!(
            joined.contains("--name agent-vesper-sbx-tests"),
            "args: {joined}"
        );
    }

    #[test]
    fn network_grant_is_the_only_path_off_none() {
        let mut granted = config();
        granted.network = true;
        let backend = DockerBackend::new(granted);
        let args = backend.run_args("n", std::path::Path::new("/tmp/ws"), 60);
        assert!(args.contains(&"--network".into()));
        assert!(args.contains(&"bridge".into()));
        assert!(!args.contains(&"none".into()));
    }

    #[test]
    fn exec_args_prefix_container_cwd_and_argv() {
        let backend = DockerBackend::new(config());
        let argv = Argv {
            argv: vec!["id".into(), "-u".into()],
            cwd: PathBuf::from("/tmp/ws"),
        };
        let args = backend.exec_args("c", "/workspace", &argv);
        assert_eq!(
            args,
            vec![
                "exec".to_owned(),
                "-w".to_owned(),
                "/workspace".to_owned(),
                "c".to_owned(),
                "id".to_owned(),
                "-u".to_owned(),
            ]
        );
    }

    #[test]
    fn container_cwd_maps_workspace_relative_paths() {
        let root = std::path::Path::new("/tmp/ws");
        assert_eq!(
            DockerBackend::container_cwd(root, &PathBuf::from("/tmp/ws")),
            "/workspace"
        );
        assert_eq!(
            DockerBackend::container_cwd(root, &PathBuf::from("/tmp/ws/sub/dir")),
            "/workspace/sub/dir"
        );
        // Outside the root maps to the root (the executor's confinement has
        // already rejected genuinely escaping paths; this is belt-and-braces).
        assert_eq!(
            DockerBackend::container_cwd(root, &PathBuf::from("/etc")),
            "/workspace"
        );
    }

    #[test]
    fn capabilities_fail_closed_when_daemon_is_unreachable() {
        // Point at a binary that cannot exist; the probe must fail and the
        // capabilities must report every capability Unavailable — never
        // assumed available.
        let backend = DockerBackend::new(DockerSandboxConfig {
            docker_bin: Some(PathBuf::from("/nonexistent/vesper-docker-stub")),
            ..DockerSandboxConfig::default()
        });
        let caps = backend.capabilities();
        assert_eq!(caps.process_tree, CapabilityStatus::Unavailable);
        assert_eq!(caps.filesystem, CapabilityStatus::Unavailable);
        assert_eq!(caps.network, CapabilityStatus::Unavailable);
        assert_eq!(caps.strength, SecurityStrength::None);
        assert!(!caps.satisfies(IsolationRequirement::ProcessTree));
        assert!(!caps.satisfies(IsolationRequirement::Full));
    }

    #[test]
    fn daemon_probe_error_carries_model_facing_refusal_text() {
        let backend = DockerBackend::new(DockerSandboxConfig {
            docker_bin: Some(PathBuf::from("/nonexistent/vesper-docker-stub")),
            ..DockerSandboxConfig::default()
        });
        let error = backend.probe_daemon().expect_err("probe must fail");
        let text = error.to_string();
        assert!(text.contains("sandbox unavailable"), "{text}");
        assert!(text.contains("the operation needs isolation"), "{text}");
    }

    #[test]
    fn provision_fails_fast_when_daemon_is_unreachable() {
        // Cold-start guard: provision must refuse before any `docker run`.
        let backend = DockerBackend::new(DockerSandboxConfig {
            docker_bin: Some(PathBuf::from("/nonexistent/vesper-docker-stub")),
            ..DockerSandboxConfig::default()
        });
        let spec = SandboxSpec::new(PathBuf::from("."));
        let error =
            futures_helper::block_on(backend.provision(&spec)).expect_err("provision must fail");
        assert!(
            matches!(error, SandboxError::CapabilityUnavailable { .. }),
            "unexpected error: {error}"
        );
    }

    /// Minimal inline block-on for tests. The futures in this module
    /// complete synchronously (std::process I/O, no reactor needed), so a
    /// no-op waker plus a yielding spin is sufficient and MSRV-safe.
    mod futures_helper {
        /// A waker that does nothing; these futures never re-poll.
        struct NoopWaker;
        impl std::task::Wake for NoopWaker {
            fn wake(self: std::sync::Arc<Self>) {}
        }

        pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
            let waker = std::sync::Arc::new(NoopWaker).into();
            let mut context = std::task::Context::from_waker(&waker);
            let mut future = Box::pin(future);
            loop {
                match future.as_mut().poll(&mut context) {
                    std::task::Poll::Ready(output) => return output,
                    std::task::Poll::Pending => std::thread::yield_now(),
                }
            }
        }
    }
}
