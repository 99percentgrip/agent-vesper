//! One existing container supervisor with per-command Landlock confinement.
//! Unique non-root UIDs identify descendants; cleanup verifies their absence
//! before restoring artifact ownership to the original bind-mount owner.
use super::*;
use vesper_sandbox::{Argv, ExecOutput, SandboxError};

pub(super) struct SharedScope {
    spec: SandboxSpec,
    handle: Mutex<Option<Arc<SandboxHandle>>>,
    uncertain: std::sync::atomic::AtomicBool,
}
impl SharedScope {
    pub(super) fn new(spec: SandboxSpec) -> Self {
        Self {
            spec,
            handle: Mutex::new(None),
            uncertain: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub(super) fn acquire(&self, backend: &dyn SandboxBackend) -> Result<(), LeaseError> {
        let mut handle =
            BlockingBridge::block_on(backend.provision(&self.spec)).map_err(|error| {
                LeaseError::ProvisioningUncertain {
                    worker: "shared".into(),
                    reason: error.to_string(),
                }
            })?;
        handle.timeout_seconds = 120;
        // Retain ownership before probing: failed probes still require verified
        // teardown and cannot silently free the lease-book reservation.
        *self.handle.lock().expect("shared handle") = Some(Arc::new(handle));
        let handle = self.handle()?;
        let probe = Argv { cwd: self.spec.writable_root.clone(), argv: vec!["/bin/sh".into(), "-ec".into(),
            "test -x /usr/local/bin/vesper-setpriv; command -v pkill; command -v pgrep; chmod 0711 /workspace /workspace/w; /usr/local/bin/vesper-setpriv --no-new-privs --bounding-set=-all --inh-caps=-all --ambient-caps=-all --landlock-access fs --landlock-rule path-beneath:execute,read-file,read-dir:/usr /bin/true".into()] };
        if checked(backend, &handle, &probe).is_err() {
            drop(handle);
            if self.release(backend).is_err() {
                return Err(LeaseError::ProvisioningUncertain {
                    worker: "shared".into(),
                    reason: "shared confinement probe cleanup is unverified".into(),
                });
            }
            return Err(LeaseError::PortRefused {
                worker: "shared".into(),
                reason: "shared scope requires the bundled Landlock helper and enforcing kernel"
                    .into(),
            });
        }
        Ok(())
    }
    fn handle(&self) -> Result<Arc<SandboxHandle>, LeaseError> {
        self.handle
            .lock()
            .expect("shared handle")
            .clone()
            .ok_or(LeaseError::ResourceLimit("shared supervisor unavailable"))
    }
    pub(super) fn release(&self, backend: &dyn SandboxBackend) -> Result<(), LeaseError> {
        let handle = self.handle.lock().expect("shared handle").take();
        if let Some(handle) = handle {
            let handle = Arc::try_unwrap(handle).map_err(|handle| {
                *self.handle.lock().expect("shared handle") = Some(handle);
                LeaseError::ResourceLimit("shared commands still own supervisor")
            })?;
            BlockingBridge::block_on(backend.teardown(handle)).map_err(|error| {
                LeaseError::PortRefused {
                    worker: "shared".into(),
                    reason: error.to_string(),
                }
            })?;
        }
        if self.uncertain.load(std::sync::atomic::Ordering::Acquire) {
            return Err(LeaseError::PortRefused { worker: "shared".into(), reason: "supervisor removed, but prior worker cleanup or artifact ownership remains unverified".into() });
        }
        Ok(())
    }
    pub(super) fn run(
        &self,
        backend: &dyn SandboxBackend,
        boundary: &Boundary,
        command: &str,
        cwd: &Path,
        timeout: u64,
        cancellation: &Arc<dyn vesper_provider::CancellationSignal>,
    ) -> Result<SandboxOutcome, SandboxRunError> {
        use std::sync::atomic::Ordering;
        if self.uncertain.load(Ordering::Acquire) {
            return Err(SandboxRunError::Backend(
                "shared scope cleanup is unverified".into(),
            ));
        }
        {
            let mut state = boundary.state.lock().expect("native boundary");
            if state.running || state.uncertain {
                return Err(SandboxRunError::Backend(
                    "worker route busy or quarantined".into(),
                ));
            }
            state.running = true;
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let handle = self
                .handle()
                .map_err(|error| SandboxError::Run(error.to_string()))?;
            let root = format!("/workspace/w/{}", boundary.spec.worker_id);
            let uid = boundary.uid.to_string();
            let setup = Argv {
                cwd: self.spec.writable_root.clone(),
                argv: vec![
                    "/bin/sh".into(),
                    "-ec".into(),
                    "umask 077; stat -c %u:%g -- \"$2\" > \"/tmp/vesper-owner-$1\"; mkdir -p -- \"$2/.tmp\"; chown -hR -- \"$1:$1\" \"$2\"; chmod 0700 -- \"$2\""
                        .into(),
                    "swarm-setup".into(),
                    uid.clone(),
                    root.clone(),
                ],
            };
            let setup_result = checked(backend, &handle, &setup);
            let mut argv = vec![
                "/usr/local/bin/vesper-setpriv".into(),
                "--no-new-privs".into(),
                "--bounding-set=-all".into(),
                "--inh-caps=-all".into(),
                "--ambient-caps=-all".into(),
                "--clear-groups".into(),
                format!("--reuid={uid}"),
                format!("--regid={uid}"),
                "--landlock-access".into(),
                "fs".into(),
            ];
            for rule in [
                format!("path-beneath:fs:{root}"),
                "path-beneath:execute,read-file,read-dir:/usr".into(),
                "path-beneath:read-file,read-dir:/etc".into(),
                "path-beneath:read-file,write-file:/dev/null".into(),
                "path-beneath:read-file:/dev/urandom".into(),
            ] {
                // setpriv's rule access is the comma-separated filesystem mask,
                // with an empty mask denoting all supported filesystem rights.
                argv.extend([
                    "--landlock-rule".into(),
                    rule.replace("path-beneath:fs:", "path-beneath::"),
                ]);
            }
            argv.extend([
                "/usr/bin/env".into(),
                format!("HOME={root}"),
                format!("TMPDIR={root}/.tmp"),
                "/usr/bin/timeout".into(),
                "--kill-after=1s".into(),
                format!("{}s", timeout.clamp(1, 120)),
                "/bin/sh".into(),
                "-c".into(),
                command.into(),
            ]);
            let run = if let Err(error) = setup_result {
                Err(error)
            } else if cancellation.is_cancelled() {
                Err(SandboxError::Run("cancelled before dispatch".into()))
            } else {
                BlockingBridge::block_on(backend.run(
                    &handle,
                    &Argv {
                        argv,
                        cwd: cwd.to_path_buf(),
                    },
                ))
            };
            // A shell may leave forked/setsid children after returning. Every
            // descendant retains this unique UID and cannot regain credentials.
            let cleanup = Argv { cwd: self.spec.writable_root.clone(), argv: vec!["/bin/sh".into(), "-ec".into(),
                "n=0; while :; do if pgrep -u \"$1\" >/dev/null; then pkill -KILL -u \"$1\" || test $? -eq 1; else test $? -eq 1; break; fi; n=$((n+1)); test \"$n\" -lt 30; sleep 0.1; done; owner=$(cat \"/tmp/vesper-owner-$1\"); chown -hR -- \"$owner\" \"$2\"; rm -- \"/tmp/vesper-owner-$1\"".into(), "swarm-cleanup".into(), uid, root] };
            if checked(backend, &handle, &cleanup).is_err() {
                self.uncertain.store(true, Ordering::Release);
                return Err(SandboxError::Teardown(
                    "shared worker descendants or artifact ownership unresolved".into(),
                ));
            }
            run
        }));
        let mut state = boundary.state.lock().expect("native boundary");
        state.running = false;
        if outcome.is_err() {
            self.uncertain.store(true, Ordering::Release);
            state.uncertain = true;
        }
        drop(state);
        let outcome = outcome
            .unwrap_or_else(|_| Err(SandboxError::Run("shared worker backend panicked".into())));
        finish_run(
            outcome,
            if self.uncertain.load(Ordering::Acquire) {
                Err(SandboxError::Teardown(
                    "shared scope cleanup unverified".into(),
                ))
            } else {
                Ok(())
            },
            cancellation.is_cancelled(),
        )
    }
}
fn checked(
    backend: &dyn SandboxBackend,
    handle: &SandboxHandle,
    argv: &Argv,
) -> Result<ExecOutput, SandboxError> {
    let output = BlockingBridge::block_on(backend.run(handle, argv))?;
    if output.exit_code != Some(0) || output.timed_out {
        return Err(SandboxError::Run(
            "shared scope setup or cleanup refused".into(),
        ));
    }
    Ok(output)
}
