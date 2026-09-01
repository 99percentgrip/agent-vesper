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
//!   so plain safe `Child::kill` from the library is total teardown.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    let mut line = String::new();
    if let Some(pipe) = child.stdout.take() {
        let mut reader = BufReader::new(pipe);
        // The probe writes exactly one short report line.
        reader.read_line(&mut line).ok()?;
    }
    let status = child.wait().ok()?;
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
            let mut reader = BufReader::new(stdout);
            let mut ready = String::new();
            reader
                .read_line(&mut ready)
                .map_err(|error| SandboxError::Provision(format!("read ready line: {error}")))?;
            if !ready.starts_with("ready ") {
                return Err(SandboxError::Provision(format!(
                    "supervisor did not report ready: {ready}"
                )));
            }
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
            let mut stdout_bytes = Vec::new();
            let mut stderr_bytes = Vec::new();
            if let Some(reader) = handle.stdout.lock().ok().and_then(|mut slot| slot.take()) {
                let mut limited = reader.take((OUTPUT_CAP_BYTES + 1) as u64);
                limited
                    .read_to_end(&mut stdout_bytes)
                    .map_err(|error| SandboxError::Run(format!("read stdout: {error}")))?;
            }
            let mut child = handle
                .child
                .lock()
                .map_err(|_| SandboxError::Run("sandbox handle poisoned".into()))?;
            if let Some(pipe) = child.stderr.take() {
                let mut limited = pipe.take((OUTPUT_CAP_BYTES + 1) as u64);
                limited
                    .read_to_end(&mut stderr_bytes)
                    .map_err(|error| SandboxError::Run(format!("read stderr: {error}")))?;
            }
            // Bounded wait: poll `try_wait` until the timeout fires, then
            // kill (PDEATHSIG → PID-1 death → kernel SIGKILLs the namespace).
            let deadline = Instant::now() + Duration::from_secs(handle.timeout_seconds.max(1));
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return Ok(ExecOutput {
                            exit_code: status.code(),
                            stdout: bounded_text(&mut stdout_bytes),
                            stderr: bounded_text(&mut stderr_bytes),
                            timed_out: false,
                        });
                    }
                    Ok(None) if Instant::now() >= deadline => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(ExecOutput {
                            exit_code: None,
                            stdout: bounded_text(&mut stdout_bytes),
                            stderr: bounded_text(&mut stderr_bytes),
                            timed_out: true,
                        });
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    Err(error) => return Err(SandboxError::Run(format!("wait: {error}"))),
                }
            }
        })
    }

    fn teardown<'a>(
        &'a self,
        handle: SandboxHandle,
    ) -> SandboxFuture<'a, Result<(), SandboxError>> {
        Box::pin(async move {
            // The handle's Drop impl performs kill+wait (and the namespace
            // teardown chains through it); moving it here makes that
            // observable for the caller.
            drop(handle);
            Ok(())
        })
    }
}

/// Truncates a byte buffer to the output cap as lossy UTF-8 text.
fn bounded_text(bytes: &mut Vec<u8>) -> String {
    if bytes.len() > OUTPUT_CAP_BYTES {
        bytes.truncate(OUTPUT_CAP_BYTES);
    }
    String::from_utf8_lossy(bytes).into_owned()
}
