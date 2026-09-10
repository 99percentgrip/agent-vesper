//! Worker-scoped sandbox leases over the real backend and native command route.
//! One reserved worker boundary owns at most one supervisor at a time. Backends
//! with a one-run protocol rotate supervisors only after verified teardown,
//! retaining the worker's lease throughout all native tool continuations.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vesper_agent::sandbox_route::{
    SandboxBackendChoice, SandboxBackendPort, SandboxDemand, SandboxOutcome, SandboxRoute,
    SandboxRunError,
};
use vesper_sandbox::{SandboxBackend, SandboxHandle, SandboxSpec};
use vesper_swarm::sandbox::{
    CleanupReport, Lease, LeaseBook, LeaseError, LeaseMode, LeaseSpec, SandboxLeasePort,
};
use vesper_swarm::worker::CancellationSignal;

use crate::sandbox_backend::{BlockingBridge, build_spec, finish_run, shell_argv};

struct BoundaryState {
    handle: Option<SandboxHandle>,
    running: bool,
    uncertain: bool,
}
struct Boundary {
    spec: LeaseSpec,
    backend_spec: SandboxSpec,
    state: Mutex<BoundaryState>,
}
struct NativePort {
    backend: Arc<dyn SandboxBackend>,
    boundaries: Mutex<BTreeMap<String, Arc<Boundary>>>,
}
impl NativePort {
    fn boundary(&self, spec: &LeaseSpec) -> Result<Arc<Boundary>, LeaseError> {
        self.boundaries
            .lock()
            .expect("native boundaries")
            .get(&spec.worker_id)
            .filter(|boundary| boundary.spec == *spec)
            .cloned()
            .ok_or_else(|| LeaseError::PortRefused {
                worker: spec.worker_id.clone(),
                reason: "worker has no matching native scope grant".into(),
            })
    }
}
impl SandboxLeasePort for NativePort {
    fn acquire(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        let boundary = self.boundary(spec)?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            BlockingBridge::block_on(self.backend.provision(&boundary.backend_spec))
        }));
        let mut state = boundary.state.lock().expect("native boundary");
        match outcome {
            Ok(Ok(handle)) => {
                state.handle = Some(handle);
                Ok(())
            }
            Ok(Err(vesper_sandbox::SandboxError::CapabilityUnavailable { .. })) => {
                Err(LeaseError::PortRefused {
                    worker: spec.worker_id.clone(),
                    reason: "sandbox capability unavailable".into(),
                })
            }
            Ok(Err(error)) => {
                state.uncertain = true;
                Err(LeaseError::ProvisioningUncertain {
                    worker: spec.worker_id.clone(),
                    reason: error.to_string(),
                })
            }
            Err(_) => {
                state.uncertain = true;
                Err(LeaseError::BackendPanicked("acquire"))
            }
        }
    }
    fn release(&self, spec: &LeaseSpec) -> Result<(), LeaseError> {
        let boundary = self.boundary(spec)?;
        let handle = {
            let mut state = boundary.state.lock().expect("native boundary");
            if state.running || state.uncertain {
                return Err(LeaseError::PortRefused {
                    worker: spec.worker_id.clone(),
                    reason: "native worker cleanup is unverified".into(),
                });
            }
            state.handle.take()
        };
        if let Some(handle) = handle {
            // Consuming teardown cannot safely be replayed if it fails. The
            // lease book retains quarantine; no invented retry/reset is exposed.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                BlockingBridge::block_on(self.backend.teardown(handle))
            }));
            match outcome {
                Ok(Ok(())) => {}
                other => {
                    boundary.state.lock().expect("native boundary").uncertain = true;
                    return Err(match other {
                        Ok(Err(error)) => LeaseError::PortRefused {
                            worker: spec.worker_id.clone(),
                            reason: error.to_string(),
                        },
                        _ => LeaseError::BackendPanicked("release"),
                    });
                }
            }
        }
        self.boundaries
            .lock()
            .expect("native boundaries")
            .remove(&spec.worker_id);
        Ok(())
    }
}

/// Shared capacity and cleanup owner for a native Hive. Explicit scope roots
/// remain as worker output artifacts; cleanup never recursively deletes results.
/// Runtime activation and permission decisions belong to the host service.
pub struct NativeSandboxLeases {
    book: LeaseBook,
    port: Arc<NativePort>,
    demand: SandboxDemand,
    choice: SandboxBackendChoice,
    root: PathBuf,
    network_grant: String,
}
impl NativeSandboxLeases {
    /// Construct on a blocking composition task: capability probes and root
    /// canonicalization can block. The supplied root must already exist, be
    /// canonical, and be dedicated to this explicitly authorized swarm run.
    /// No directory or backend is provisioned by construction.
    pub fn new(
        backend: Arc<dyn SandboxBackend>,
        demand: SandboxDemand,
        choice: SandboxBackendChoice,
        capacity: usize,
        root: PathBuf,
        network_grant: String,
    ) -> Result<Self, LeaseError> {
        if !demand.is_active()
            || (demand.allow_network && network_grant.is_empty())
            || (!demand.allow_network && !network_grant.is_empty())
            || network_grant.len() > 1024
        {
            return Err(LeaseError::ResourceLimit(
                "explicit active demand and matching network grant required",
            ));
        }
        let canonical = std::fs::canonicalize(&root)
            .map_err(|_| LeaseError::ResourceLimit("native worker root unavailable"))?;
        if canonical != root || !root.is_dir() {
            return Err(LeaseError::ResourceLimit(
                "native worker root must be a canonical directory",
            ));
        }
        let capabilities = backend.capabilities();
        let required = if demand.allow_network {
            vesper_agent::sandbox_route::IsolationRequirement::Full
        } else {
            vesper_agent::sandbox_route::IsolationRequirement::Filesystem
        };
        if !capabilities.satisfies(required) {
            return Err(LeaseError::CapabilityDenied {
                requirement: required,
                axis: "native worker filesystem/network confinement",
            });
        }
        let port = Arc::new(NativePort {
            backend,
            boundaries: Mutex::new(BTreeMap::new()),
        });
        let book = LeaseBook::new(capacity, demand.requirement, capabilities, port.clone())?;
        Ok(Self {
            book,
            port,
            demand,
            choice,
            root,
            network_grant,
        })
    }
    /// Observe/close all workers through the same shared lease book. Held routes
    /// must be retired by the Hive owner; detached commands keep their own lease.
    pub async fn shutdown(&self, timeout: Duration) -> Result<CleanupReport, LeaseError> {
        self.book.shutdown(timeout).await
    }
    /// Observe pending retirements without closing admission for scale/replacement.
    pub async fn settle(&self, timeout: Duration) -> Result<CleanupReport, LeaseError> {
        self.book.settle(timeout).await
    }
    /// Current held, pending and quarantined capacity, without backend calls.
    #[must_use]
    pub fn cleanup_report(&self) -> CleanupReport {
        self.book.cleanup_report()
    }
    /// Bounded diagnostics for cleanup that did not release reserved capacity.
    #[must_use]
    pub fn cleanup_errors(&self) -> Vec<String> {
        self.book.release_errors()
    }

    pub(crate) async fn worker(
        self: &Arc<Self>,
        role: &str,
        id: u64,
        cancellation: CancellationSignal,
    ) -> Result<(PathBuf, Arc<SandboxRoute>), LeaseError> {
        if role.is_empty()
            || role.len() > 128
            || !role
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(LeaseError::ResourceLimit("native worker role identity"));
        }
        if cancellation.is_cancelled() || self.book.is_closed() {
            return Err(LeaseError::Closed);
        }
        let worker = format!("{role}-{id}");
        let owner = self.clone();
        let prepared = tokio::task::spawn_blocking(move || owner.prepare(&worker))
            .await
            .map_err(|_| LeaseError::BackendPanicked("worker scope"))??;
        // Once prepared, the future must enter acquisition without another await.
        // The lease book owns any provisioning that outlives this caller.
        let spec = &prepared.spec;
        let acquire = self.book.acquire(spec.clone());
        let lease = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(LeaseError::Closed),
            lease = acquire => lease,
        };
        let lease = lease?;
        let boundary = self.port.boundary(spec)?;
        let root = boundary.backend_spec.writable_root.clone();
        let port = Arc::new(ScopedCommandPort {
            owner: self.clone(),
            boundary,
            _lease: lease,
        });
        let route = Arc::new(SandboxRoute::new(
            self.demand.clone(),
            self.choice.clone(),
            port,
        ));
        Ok((root, route))
    }
    fn worker_spec(&self, root: &Path, timeout: u64) -> SandboxSpec {
        let mut spec = build_spec(root, &self.demand, timeout);
        spec.private_root_label = true;
        spec
    }
    fn prepare(self: &Arc<Self>, worker: &str) -> Result<PreparedWorker, LeaseError> {
        if self.book.is_closed() {
            return Err(LeaseError::Closed);
        }
        let root = self.root.join(worker);
        let spec = LeaseSpec {
            requirement: self.demand.requirement,
            network_grant: self.network_grant.clone(),
            mode: LeaseMode::Isolated,
            worker_id: worker.into(),
            write_path: root.to_string_lossy().into_owned(),
        };
        if spec.write_path.len() > 4096 {
            return Err(LeaseError::ResourceLimit("native worker path bytes"));
        }
        // Reserve identity under this short metadata lock. Filesystem and backend
        // work never run under the lease book lock.
        let mut boundaries = self.port.boundaries.lock().expect("native boundaries");
        if boundaries.len() >= 4096 {
            return Err(LeaseError::ResourceLimit("4096 native worker scopes"));
        }
        if boundaries.contains_key(worker) {
            return Err(LeaseError::WorkerReserved(worker.into()));
        }
        boundaries.insert(
            worker.into(),
            Arc::new(Boundary {
                spec: spec.clone(),
                backend_spec: self.worker_spec(&root, 120),
                state: Mutex::new(BoundaryState {
                    handle: None,
                    running: false,
                    uncertain: false,
                }),
            }),
        );
        drop(boundaries);
        let prepared = PreparedWorker {
            owner: self.clone(),
            spec,
        };
        // create_dir refuses existing paths, including symlink aliases. Recheck
        // the canonical parent/root before giving either backend or tools access.
        std::fs::create_dir(&root)
            .map_err(|_| LeaseError::ResourceLimit("worker scope must be a fresh directory"))?;
        if std::fs::canonicalize(&root).ok().as_ref() != Some(&root)
            || std::fs::canonicalize(&self.root).ok().as_ref() != Some(&self.root)
        {
            return Err(LeaseError::ResourceLimit(
                "native worker scope alias refused",
            ));
        }

        Ok(prepared)
    }
}
// Covers both a dropped spawn_blocking observer and cancellation before/while
// entering LeaseBook. Only the book may release a dispatched native boundary.
struct PreparedWorker {
    owner: Arc<NativeSandboxLeases>,
    spec: LeaseSpec,
}
impl Drop for PreparedWorker {
    fn drop(&mut self) {
        if !self.owner.book.boundary_reserved(&self.spec.worker_id) {
            self.owner
                .port
                .boundaries
                .lock()
                .expect("native boundaries")
                .remove(&self.spec.worker_id);
        }
    }
}
struct ScopedCommandPort {
    owner: Arc<NativeSandboxLeases>,
    boundary: Arc<Boundary>,
    // Last field: releasing its lease schedules cleanup only after other owned
    // route fields have been dropped. A detached command owns this entire port.
    _lease: Lease,
}
impl SandboxBackendPort for ScopedCommandPort {
    fn capabilities(&self) -> vesper_agent::sandbox_route::SandboxCapabilities {
        self.owner.port.backend.capabilities()
    }
    fn run_command(
        &self,
        command: &str,
        cwd: &Path,
        timeout_seconds: u64,
        cancellation: &Arc<dyn vesper_provider::CancellationSignal>,
    ) -> Result<SandboxOutcome, SandboxRunError> {
        if cancellation.is_cancelled() {
            return Err(SandboxRunError::Cancelled);
        }
        let root = &self.boundary.backend_spec.writable_root;
        let canonical = std::fs::canonicalize(cwd)
            .map_err(|_| SandboxRunError::Backend("worker command directory unavailable".into()))?;
        if !canonical.starts_with(root) || std::fs::canonicalize(root).ok().as_ref() != Some(root) {
            return Err(SandboxRunError::Backend(
                "worker command outside scoped grant".into(),
            ));
        }
        let mut handle = {
            let mut state = self.boundary.state.lock().expect("native boundary");
            if state.running || state.uncertain {
                return Err(SandboxRunError::Backend(
                    "worker route busy or quarantined".into(),
                ));
            }
            state.running = true;
            state.handle.take()
        };
        // Catch all backend future construction/polling inside the ownership
        // guard; run failures still reach teardown when a handle exists.
        let backend = &self.owner.port.backend;
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_none() {
                let spec = self.owner.worker_spec(root, timeout_seconds);
                handle = Some(BlockingBridge::block_on(backend.provision(&spec))?);
            }
            let handle = handle.as_mut().expect("provisioned handle");
            handle.timeout_seconds = timeout_seconds.max(1);
            if cancellation.is_cancelled() {
                return Err(vesper_sandbox::SandboxError::Run(
                    "cancelled before dispatch".into(),
                ));
            }
            BlockingBridge::block_on(backend.run(handle, &shell_argv(command, &canonical)))
        }));
        let run_panicked = run.is_err();
        let outcome = run.unwrap_or_else(|_| {
            Err(vesper_sandbox::SandboxError::Run(
                "native worker backend panicked".into(),
            ))
        });
        let missing_handle = handle.is_none();
        let cleanup = if let Some(handle) = handle {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                BlockingBridge::block_on(backend.teardown(handle))
            }))
            .unwrap_or_else(|_| {
                Err(vesper_sandbox::SandboxError::Teardown(
                    "native worker teardown panicked".into(),
                ))
            })
        } else {
            Err(vesper_sandbox::SandboxError::Teardown(
                "native worker provisioning unverified".into(),
            ))
        };
        {
            let mut state = self.boundary.state.lock().expect("native boundary");
            state.running = false;
            state.uncertain |= run_panicked || missing_handle || cleanup.is_err();
        }
        finish_run(outcome, cleanup, cancellation.is_cancelled())
    }
}

#[cfg(test)]
#[path = "swarm_sandbox_tests.rs"]
mod tests;
