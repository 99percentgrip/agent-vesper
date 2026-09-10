//! Linux user+mount+PID+network namespaces backend (VRO-13 PR-3, ADR 0022).
//!
//! This module contains **no unsafe code**: every raw syscall (`unshare`,
//! `mount`, uid/gid map writes, `fork`, `prctl(PR_SET_PDEATHSIG)`, `execv`)
//! lives in the dedicated `sandbox_init` supervisor binary that this backend
//! spawns through safe `std::process` calls. The supervisor speaks a fixed
//! protocol:
//!
//! * `sandbox_init probe` — creates user+mount+PID+net namespaces and mounts
//!   a tmpfs; prints one capability line and exits 0 only if every namespace
//!   actually provisioned. Any failure exits non-zero, which this backend
//!   reports honestly as `CapabilityStatus::Unavailable`.
//! * `sandbox_init hold --root <dir> [--env K=V]…` — provisions the
//!   namespaces, builds the bind-mount root (workspace read-write;
//!   `/usr`, `/bin`, `/lib*`, `/etc`, `/dev` read-only; fresh `/proc` and
//!   `/tmp`), prints `ready <pid>`, then reads one unit-separator-delimited
//!   command line on stdin, forks, and the child `execv`s the payload as
//!   PID 1 of the new PID namespace. The parent relays the payload's exit
//!   status. Killing the supervisor chains `PR_SET_PDEATHSIG` into the
//!   namespace init, and the kernel then SIGKILLs every namespace member —
//!   so successful `Child::kill` starts namespace teardown. Explicit cleanup
//!   reports kill/reap failures rather than treating Drop as confirmation.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Mutex;
use std::time::Duration;

use vesper_security::{
    CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength,
};

use crate::{
    Argv, ExecOutput, OUTPUT_CAP_BYTES, SandboxBackend, SandboxError, SandboxFuture, SandboxHandle,
    SandboxSpec, encode_run_line, parse_capability_line, spawn_hold, spawn_probe,
    strength_from_statuses, supervisor_path,
};

const BACKEND_ID: &str = "linux-namespaces";

/// Linux namespaces backend. Capabilities come from a real probe, never
/// assumptions. Constructed only when a tool explicitly demands isolation.
#[derive(Debug)]
pub struct NamespacesBackend {
    capabilities: SandboxCapabilities,
}

impl NamespacesBackend {
    /// Probes the host (once per process, `OnceLock`-cached) and builds the
    /// backend. On a host that forbids unprivileged namespaces the backend
    /// still exists but reports every capability `Unavailable`, so every
    /// isolation demand fails closed instead of assuming success.
    #[must_use]
    pub fn probe_and_build() -> Self {
        Self {
            capabilities: probe_cached().unwrap_or_else(unavailable_caps),
        }
    }

    /// Builds the backend without probing. The first `capabilities()` call
    /// performs the probe lazily and caches it process-wide.
    #[must_use]
    pub fn new() -> Self {
        Self {
            capabilities: SandboxCapabilities {
                backend: BACKEND_ID.to_owned(),
                process_tree: CapabilityStatus::Unknown,
                filesystem: CapabilityStatus::Unknown,
                network: CapabilityStatus::Unknown,
                strength: SecurityStrength::None,
            },
        }
    }
}

impl Default for NamespacesBackend {
    fn default() -> Self {
        Self::probe_and_build()
    }
}

/// All-`Unavailable` report for hosts where the probe cannot run.
fn unavailable_caps() -> SandboxCapabilities {
    SandboxCapabilities {
        backend: BACKEND_ID.to_owned(),
        process_tree: CapabilityStatus::Unavailable,
        filesystem: CapabilityStatus::Unavailable,
        network: CapabilityStatus::Unavailable,
        strength: SecurityStrength::None,
    }
}

/// Process-cached probe outcome. `None` = namespaces unavailable here.
fn probe_cached() -> Option<SandboxCapabilities> {
    static CACHE: std::sync::OnceLock<Option<SandboxCapabilities>> = std::sync::OnceLock::new();
    CACHE.get_or_init(probe_once).clone()
}

/// Runs `sandbox_init probe` and turns its report into capabilities.
fn probe_once() -> Option<SandboxCapabilities> {
    let supervisor = supervisor_path().ok()?;
    let mut child = spawn_probe(&supervisor).ok()?;
    let pipe = child.stdout.take()?;
    let (line, _) = match first_line(BufReader::new(pipe)) {
        Ok(line) => line,
        Err(_) => {
            let _ = child.kill();
            let _ = crate::wait_bounded(&mut child, Duration::from_millis(500));
            return None;
        }
    };
    let status = crate::wait_bounded(&mut child, Duration::from_secs(5)).ok()?;
    if !status.success() {
        return None;
    }
    let (backend, process_tree, filesystem, network) = parse_capability_line(line.lines().next()?)?;
    Some(SandboxCapabilities {
        backend,
        process_tree,
        filesystem,
        network,
        strength: strength_from_statuses(process_tree, filesystem, network),
    })
}

impl SandboxBackend for NamespacesBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        // `new()` may have deferred the probe; probe lazily and honestly.
        if self.capabilities.process_tree == CapabilityStatus::Unknown {
            probe_cached().unwrap_or_else(unavailable_caps)
        } else {
            self.capabilities.clone()
        }
    }

    fn provision<'a>(
        &'a self,
        spec: &'a SandboxSpec,
    ) -> SandboxFuture<'a, Result<SandboxHandle, SandboxError>> {
        let spec = spec.clone();
        Box::pin(async move {
            // Fail closed before spawning anything if the probe says this
            // host cannot create the namespaces the backend depends on.
            let caps = self.capabilities();
            if !caps.satisfies(IsolationRequirement::Filesystem) {
                return Err(SandboxError::CapabilityUnavailable {
                    requirement: IsolationRequirement::Filesystem,
                    capabilities: caps,
                });
            }
            let supervisor = supervisor_path()?;
            let mut child = spawn_hold(&supervisor, &spec)?;
            // Readiness handshake: the supervisor prints `ready <pid>` once
            // the namespaces, mounts, and pivot are complete. Until that
            // line arrives nothing has been provisioned.
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| SandboxError::Provision("supervisor stdout missing".into()))?;
            let handshake = first_line(BufReader::new(stdout));
            let (ready, reader) = match handshake {
                Ok(value) if value.0.starts_with("ready ") => value,
                _ => {
                    let _ = child.kill();
                    crate::wait_bounded(&mut child, Duration::from_millis(500)).map_err(|_| {
                        SandboxError::Provision(
                            "supervisor handshake and cleanup unverified".into(),
                        )
                    })?;
                    return Err(SandboxError::Provision(
                        "supervisor did not report ready".into(),
                    ));
                }
            };
            let _ = ready;
            Ok(SandboxHandle {
                child: Mutex::new(child),
                teardown_command: None,
                stdout: Mutex::new(Some(reader)),
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
        Box::pin(async move {
            // One run per provision: send the command line, close stdin (EOF
            // tells the supervisor to fork+exec), then read bounded output.
            {
                let mut child = handle
                    .child
                    .lock()
                    .map_err(|_| SandboxError::Run("sandbox handle poisoned".into()))?;
                let Some(mut stdin) = child.stdin.take() else {
                    return Err(SandboxError::Run("sandbox already used".into()));
                };
                stdin
                    .write_all(encode_run_line(argv).as_bytes())
                    .and_then(|()| stdin.flush())
                    .map_err(|error| SandboxError::Run(format!("send command: {error}")))?;
                // stdin drops here → EOF → supervisor forks and execs.
            }
            let stdout = handle
                .stdout
                .lock()
                .map_err(|_| SandboxError::Run("stdout ownership poisoned".into()))?
                .take()
                .ok_or_else(|| SandboxError::Run("stdout already consumed".into()))?;
            let mut child = handle
                .child
                .lock()
                .map_err(|_| SandboxError::Run("sandbox handle poisoned".into()))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| SandboxError::Run("stderr already consumed".into()))?;
            let stdout = drain_output(stdout);
            let stderr = drain_output(stderr);
            let status = crate::wait_bounded(
                &mut child,
                Duration::from_secs(handle.timeout_seconds.max(1)),
            );
            let (exit_code, timed_out) = match status {
                Ok(status) => (status.code(), false),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                    if child
                        .try_wait()
                        .map_err(|error| SandboxError::Teardown(error.to_string()))?
                        .is_none()
                    {
                        return Err(SandboxError::Teardown(
                            "namespace supervisor reap unresolved".into(),
                        ));
                    }
                    (None, true)
                }
                Err(error) => return Err(SandboxError::Run(error.to_string())),
            };
            let receive = |receiver: std::sync::mpsc::Receiver<Result<Vec<u8>, SandboxError>>| {
                receiver
                    .recv_timeout(Duration::from_millis(500))
                    .map_err(|_| {
                        SandboxError::Teardown("namespace output pipe did not close".into())
                    })?
            };
            Ok(ExecOutput {
                exit_code,
                stdout: bounded_text(&mut receive(stdout)?),
                stderr: bounded_text(&mut receive(stderr)?),
                timed_out,
            })
        })
    }

    fn teardown<'a>(
        &'a self,
        handle: SandboxHandle,
    ) -> SandboxFuture<'a, Result<(), SandboxError>> {
        Box::pin(async move {
            // Explicitly observe cleanup errors; Drop remains a best-effort
            // fallback and cannot certify that termination succeeded.
            handle.terminate_supervisor()
        })
    }
}

/// A malformed or stalled supervisor cannot hold a caller in read_line forever.
fn first_line<R: Read + Send + 'static>(
    reader: BufReader<R>,
) -> Result<(String, BufReader<R>), SandboxError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let mut limited = reader.take(1025);
            let mut bytes = Vec::new();
            limited
                .read_until(b'\n', &mut bytes)
                .map_err(|_| SandboxError::Provision("supervisor handshake read failed".into()))?;
            if bytes.len() > 1024 || bytes.last() != Some(&b'\n') {
                return Err(SandboxError::Provision(
                    "invalid supervisor handshake".into(),
                ));
            }
            let line = String::from_utf8(bytes).map_err(|_| {
                SandboxError::Provision("invalid supervisor handshake encoding".into())
            })?;
            Ok((line, limited.into_inner()))
        })();
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| SandboxError::Provision("supervisor handshake deadline exceeded".into()))?
}

/// Drain both pipes concurrently without retaining unbounded output. Receivers
/// observe completion with a deadline; a stalled OS pipe is never joined forever.
fn drain_output(
    mut pipe: impl Read + Send + 'static,
) -> std::sync::mpsc::Receiver<Result<Vec<u8>, SandboxError>> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let mut kept = Vec::new();
            let mut bytes = [0; 8192];
            loop {
                let count = pipe
                    .read(&mut bytes)
                    .map_err(|error| SandboxError::Run(error.to_string()))?;
                if count == 0 {
                    return Ok(kept);
                }
                let remaining = (OUTPUT_CAP_BYTES + 1).saturating_sub(kept.len());
                kept.extend_from_slice(&bytes[..count.min(remaining)]);
            }
        })();
        let _ = sender.send(result);
    });
    receiver
}

/// Truncates a byte buffer to the output cap as lossy UTF-8 text.
fn bounded_text(bytes: &mut Vec<u8>) -> String {
    if bytes.len() > OUTPUT_CAP_BYTES {
        bytes.truncate(OUTPUT_CAP_BYTES);
    }
    String::from_utf8_lossy(bytes).into_owned()
}
