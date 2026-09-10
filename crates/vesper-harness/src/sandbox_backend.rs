//! Host-resolved sandbox backend adapter (VRO-13 PR-4).
//!
//! [`BackendPort`] implements `vesper-agent`'s `SandboxBackendPort` over a
//! real `vesper-sandbox` backend. This is the *only* place a concrete
//! backend type crosses into the agent layer: `vesper-agent` sees the port
//! trait, `vesper-harness` owns the backend, its probe, and the blocking
//! bridge.
//!
//! Honesty rules enforced here:
//! - `capabilities()` delegates to the backend's own probed report.
//! - `run_command` provisions, runs, tears down with the demand's bounds,
//!   and folds output into the same bounded combined shape as the
//!   unsandboxed executor.
//! - A demand the platform cannot satisfy yields a refusal, never a silent
//!   fallback to unsandboxed execution.
//!
//! Backend futures are driven on `vesper-agent`'s blocking pool; inside
//! this sync port method they are polled to completion with a park/unpark
//! waker bridge, because no ambient tokio context may be assumed on a
//! `spawn_blocking` thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vesper_agent::sandbox_route::{
    SandboxBackendChoice, SandboxBackendPort, SandboxDemand, SandboxOutcome, SandboxRoute,
    SandboxRunError,
};
use vesper_sandbox::{Argv, SandboxBackend, SandboxSpec};

#[cfg(test)]
#[path = "sandbox_outcome_tests.rs"]
mod outcome_tests;

#[cfg(test)]
#[path = "sandbox_panic_tests.rs"]
mod panic_tests;

/// Single-shot blocking bridge for backend futures on a `spawn_blocking`
/// thread, where no ambient runtime context may exist.
pub(crate) struct BlockingBridge;

impl BlockingBridge {
    /// Polls `future` to completion without an ambient tokio runtime.
    pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        struct ThreadWaker(std::thread::Thread);
        impl std::task::Wake for ThreadWaker {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let mut future = Box::pin(future);
        let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::park(),
            }
        }
    }
}

/// Adapts one concrete backend instance to the agent's port trait.
pub struct BackendPort {
    /// The host-resolved backend, built at boot with its config.
    backend: Arc<dyn SandboxBackend>,
    /// This port's view of the file-driven demand (resource bounds).
    demand: SandboxDemand,
    /// Sticky refusal after unverified teardown. No automatic replay or reset.
    cleanup_unverified: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for BackendPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendPort")
            .field("demand", &self.demand)
            .finish_non_exhaustive()
    }
}

impl BackendPort {
    /// Wraps a host-constructed backend.
    #[must_use]
    pub fn new(backend: Arc<dyn SandboxBackend>, demand: SandboxDemand) -> Self {
        Self {
            backend,
            demand,
            cleanup_unverified: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl BackendPort {
    // Catch both future construction and polling. A panicking backend may have
    // partially changed external state, so never trust this instance again.
    // AssertUnwindSafe does not certify backend recovery: sticky quarantine is
    // the recovery policy. Do not include arbitrary panic payloads in tool output.
    fn backend_call<T>(
        &self,
        phase: &'static str,
        call: impl FnOnce() -> Result<T, vesper_sandbox::SandboxError>,
    ) -> Result<T, vesper_sandbox::SandboxError> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
            Ok(result) => result,
            Err(_) => {
                self.cleanup_unverified
                    .store(true, std::sync::atomic::Ordering::Release);
                Err(vesper_sandbox::SandboxError::Run(format!(
                    "sandbox backend panicked during {phase}; route quarantined, cleanup unverified"
                )))
            }
        }
    }
}

impl SandboxBackendPort for BackendPort {
    fn capabilities(&self) -> vesper_agent::sandbox_route::SandboxCapabilities {
        self.backend.capabilities()
    }

    fn run_command(
        &self,
        command: &str,
        cwd: &Path,
        timeout_seconds: u64,
        cancellation: &Arc<dyn vesper_agent::CancellationSignal>,
    ) -> Result<SandboxOutcome, SandboxRunError> {
        // Fail fast on an already-cancelled turn before provisioning.
        if cancellation.is_cancelled() {
            return Err(SandboxRunError::Cancelled);
        }
        if self
            .cleanup_unverified
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(SandboxRunError::Backend(
                "sandbox route quarantined after unverified teardown; verify backend cleanup before rebuilding the route".into(),
            ));
        }
        let spec = build_spec(cwd, &self.demand, timeout_seconds);
        let handle = self
            .backend_call("provision", || {
                BlockingBridge::block_on(self.backend.provision(&spec))
            })
            .map_err(|error| SandboxRunError::Backend(format!("provision failed: {error}")))?;
        let argv = shell_argv(command, cwd);
        // Cancellation can arrive during provisioning. Never dispatch a command
        // after observing it, but always clean up the provisioned handle.
        let outcome = if cancellation.is_cancelled() {
            Err(vesper_sandbox::SandboxError::Run(
                "cancelled before dispatch".into(),
            ))
        } else {
            self.backend_call("run", || {
                BlockingBridge::block_on(self.backend.run(&handle, &argv))
            })
        };
        // Even a panicked run must reach explicit teardown. Teardown itself may
        // panic during future construction, polling or owned-handle destruction.
        let cleanup = self.backend_call("teardown", || {
            BlockingBridge::block_on(self.backend.teardown(handle))
        });
        if cleanup.is_err() {
            self.cleanup_unverified
                .store(true, std::sync::atomic::Ordering::Release);
        }
        finish_run(outcome, cleanup, cancellation.is_cancelled())
    }
}

/// Arbitrate both outcomes: unverified teardown cannot become tool success or
/// disappear behind an execution error/cancellation. Keep bounded backend output
/// in the failure diagnostic so the host does not lose the command's evidence.
pub(crate) fn finish_run(
    outcome: Result<vesper_sandbox::ExecOutput, vesper_sandbox::SandboxError>,
    cleanup: Result<(), vesper_sandbox::SandboxError>,
    cancelled: bool,
) -> Result<SandboxOutcome, SandboxRunError> {
    if let Err(error) = cleanup {
        let execution = match outcome {
            Ok(output) => format!("stdout: {}\nstderr: {}", output.stdout, output.stderr),
            Err(error) => format!("run failed: {error}"),
        };
        return Err(SandboxRunError::Backend(format!(
            "teardown unverified: {error}\n{execution}"
        )));
    }
    if cancelled {
        return Err(SandboxRunError::Cancelled);
    }
    let output =
        outcome.map_err(|error| SandboxRunError::Backend(format!("run failed: {error}")))?;
    let mut combined = output.stdout;
    if !output.stderr.is_empty() {
        combined.push_str("\n[stderr]\n");
        combined.push_str(&output.stderr);
    }
    if !output.timed_out && output.exit_code != Some(0) {
        return Err(SandboxRunError::Backend(format!(
            "command exited {:?}: {combined}",
            output.exit_code
        )));
    }
    Ok(SandboxOutcome {
        output: combined,
        timed_out: output.timed_out,
    })
}

/// Builds the sandbox spec from the demand, with the run timeout applied.
pub(crate) fn build_spec(cwd: &Path, demand: &SandboxDemand, timeout_seconds: u64) -> SandboxSpec {
    let mut spec = SandboxSpec::new(cwd.to_path_buf());
    spec.timeout_seconds = timeout_seconds.max(1);
    if let Some(cpus) = demand.cpu_limit {
        spec = spec.with_cpu_limit(cpus);
    }
    if let Some(bytes) = demand.memory_limit_bytes {
        spec = spec.with_memory_limit_bytes(bytes);
    }
    if demand.allow_network {
        spec = spec.with_network_grant();
    }
    spec
}

/// All registered sandbox backends execute Linux payloads, including Linux
/// containers hosted by Windows/macOS. Never use the host shell or PATH lookup.
pub(crate) fn shell_argv(command: &str, cwd: &Path) -> Argv {
    Argv {
        argv: vec!["/bin/sh".into(), "-c".into(), command.to_owned()],
        cwd: cwd.to_path_buf(),
    }
}

/// Values of `AGENT_VESPER_SANDBOX` selecting the Docker backend.
const DOCKER_VALUES: [&str; 2] = ["docker", "container"];
/// Values of `AGENT_VESPER_SANDBOX` disabling the sandbox entirely.
const OFF_VALUES: [&str; 4] = ["off", "0", "false", "none"];

/// One-process route resolution mirroring the firewall holder pattern.
///
/// Reads `AGENT_VESPER_SANDBOX=docker|namespaces|off` plus the project
/// scope's `[sandbox]` demand from `.agent-vesper/config.toml`. With no
/// demand the holder stays `None` (the zero-cost legacy path), exactly as
/// `AGENT_VESPER_FIREWALL=off` leaves the firewall holder empty.
pub mod holder {
    use super::*;
    use std::sync::OnceLock;
    use vesper_sandbox::LinuxNamespacesBackend;
    #[cfg(not(feature = "docker"))]
    use vesper_sandbox::UnavailableBackend;

    static ROUTE_HOLDER: OnceLock<Option<Arc<SandboxRoute>>> = OnceLock::new();

    /// Resolves the process-global sandbox route once at host boot.
    ///
    /// Mirrors `vesper_policy::firewall::holder::install_from_env`: first
    /// resolution wins, immutable for the process lifetime.
    #[must_use]
    pub fn install_from_env() -> Option<Arc<SandboxRoute>> {
        let env_backend = std::env::var("AGENT_VESPER_SANDBOX").ok();
        ROUTE_HOLDER
            .get_or_init(|| resolve_route(env_backend.as_deref()))
            .clone()
    }

    /// Returns the shared route when one was installed, else `None`.
    #[must_use]
    pub fn shared() -> Option<Arc<SandboxRoute>> {
        ROUTE_HOLDER.get().and_then(|cell| cell.clone())
    }

    /// Instance identity for parity diagnostics (mirrors the firewall id).
    #[must_use]
    pub fn route_id() -> usize {
        ROUTE_HOLDER
            .get()
            .and_then(|cell| cell.as_ref().map(|route| route.instance_id()))
            .unwrap_or(0)
    }

    fn resolve_route(env_backend: Option<&str>) -> Option<Arc<SandboxRoute>> {
        let off = env_backend
            .is_some_and(|value| OFF_VALUES.contains(&value.to_ascii_lowercase().as_str()));
        if off {
            return None;
        }
        let scope_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let demand_config = vesper_config::read_sandbox_scope(&scope_root).ok()?;
        if !demand_config.is_active() {
            return None;
        }
        let demand = SandboxDemand {
            requirement: demand_config.resolved_requirement(),
            allow_network: demand_config.network,
            cpu_limit: demand_config.cpu_limit.map(f64::from),
            memory_limit_bytes: demand_config
                .memory_limit_mib
                .map(|mib| u64::from(mib) * 1024 * 1024),
        };
        let choice = if env_backend
            .is_some_and(|value| DOCKER_VALUES.contains(&value.to_ascii_lowercase().as_str()))
        {
            SandboxBackendChoice::Docker
        } else {
            SandboxBackendChoice::Default
        };
        build_route(demand, choice)
    }

    fn build_route(
        demand: SandboxDemand,
        choice: SandboxBackendChoice,
    ) -> Option<Arc<SandboxRoute>> {
        let backend: Arc<dyn SandboxBackend> = match choice {
            SandboxBackendChoice::Docker => {
                #[cfg(feature = "docker")]
                {
                    Arc::new(vesper_sandbox::DockerBackend::new(Default::default()))
                }
                // Feature off: refuse honestly rather than silently falling
                // back to another backend. The all-Unavailable capability
                // report makes the executor's gate refuse the demand.
                #[cfg(not(feature = "docker"))]
                {
                    Arc::new(UnavailableBackend)
                }
            }
            SandboxBackendChoice::Default => Arc::new(LinuxNamespacesBackend::new()),
        };
        let port: Arc<dyn SandboxBackendPort> = Arc::new(BackendPort::new(backend, demand.clone()));
        Some(Arc::new(SandboxRoute::new(demand, choice, port)))
    }
}
