//! Host-neutral native swarm execution. Opt-in preferences never constitute
//! tool permission, a network grant, or proof of available isolation.
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use vesper_agent::sandbox_route::{IsolationRequirement, SandboxBackendChoice, SandboxDemand};
use vesper_agent::{AgentProgressPort, PermissionPort, ToolRegistry};
use vesper_domain::{SessionOperatingMode, SessionPermissionMode};
use vesper_sandbox::SandboxBackend;
use vesper_swarm::{
    hive::orchestrator::{Hive, HiveConfig, HiveEvent},
    ledger::store::{EmbeddingPort, EntryKind, MemoryScope},
    pool::WorkerInstanceFactory,
    sandbox::CleanupReport,
};

pub use vesper_swarm::{
    hive::orchestrator::HiveGoal,
    worker::{CancelFlag, CancellationSignal},
};

/// Keep parent acceptance cancellation tied to the same owned hive run.
pub fn parent_cancellation(
    signal: CancellationSignal,
) -> Arc<dyn vesper_agent::CancellationSignal> {
    struct Bridge(CancellationSignal);
    impl vesper_agent::CancellationSignal for Bridge {
        fn is_cancelled(&self) -> bool {
            self.0.is_cancelled()
        }
    }
    Arc::new(Bridge(signal))
}

/// Resolve the installed native backend without downloads or implicit network
/// permission. Docker builds use the same digest-pinned bundle as Web Settings.
pub async fn configured_backend(
    root: &std::path::Path,
) -> Result<(Arc<dyn SandboxBackend>, SandboxBackendChoice), String> {
    let selection = std::env::var("AGENT_VESPER_SANDBOX").ok();
    if backend_selection(selection.as_deref(), cfg!(feature = "docker"))?
        == SandboxBackendChoice::Default
    {
        let backend = tokio::task::spawn_blocking(vesper_sandbox::default_backend)
            .await
            .map_err(|_| "Native sandbox capability probe failed.")?;
        return Ok((backend, SandboxBackendChoice::Default));
    }
    #[cfg(feature = "docker")]
    {
        let config = crate::web_settings::load(root)?;
        let image = match config.driver_image {
            Some(image) => image,
            None => crate::web_settings::detect_driver().await?,
        };
        if !vesper_config::is_digest_pinned_image(&image) {
            return Err("Swarm requires a verified immutable driver image.".into());
        }
        let backend = vesper_sandbox::DockerBackend::new(vesper_sandbox::DockerSandboxConfig {
            image: Some(image),
            docker_bin: Some(crate::web_settings::container_cli()),
            ..Default::default()
        });
        Ok((Arc::new(backend), SandboxBackendChoice::Docker))
    }
    #[cfg(not(feature = "docker"))]
    {
        let _ = root;
        Err("This build does not include the requested container sandbox backend.".into())
    }
}

fn backend_selection(value: Option<&str>, docker: bool) -> Result<SandboxBackendChoice, String> {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("off" | "0" | "false" | "none") => {
            Err("Swarm requires sandbox isolation; sandbox execution is disabled.".into())
        }
        Some("namespaces") => Ok(SandboxBackendChoice::Default),
        Some("docker" | "container") if !docker => {
            Err("This build does not include the requested container sandbox backend.".into())
        }
        Some("docker" | "container") => Ok(SandboxBackendChoice::Docker),
        None if docker => Ok(SandboxBackendChoice::Docker),
        None => Ok(SandboxBackendChoice::Default),
        Some(_) => Err("Unknown sandbox backend selection.".into()),
    }
}

use crate::{
    WorkerFactory, swarm_adapter::ProviderWorkerPort, swarm_inputs::ProjectInputs,
    swarm_sandbox::NativeSandboxLeases, swarm_settings::SwarmSettings,
};

/// Real adapters and permission/progress channels supplied by the active host.
/// The embedding option must be absent when no real configured adapter exists.
#[derive(Clone)]
pub struct SwarmRunContext {
    pub root: PathBuf,
    pub factory: WorkerFactory,
    pub tools: ToolRegistry,
    pub allowed_tools: Vec<String>,
    pub mode: SessionOperatingMode,
    pub permission: SessionPermissionMode,
    pub permission_port: Arc<dyn PermissionPort>,
    pub progress: Option<Arc<dyn AgentProgressPort>>,
    pub recalled_context: Option<String>,
    pub embedding: Option<(Arc<dyn EmbeddingPort>, usize)>,
    pub backend: Arc<dyn SandboxBackend>,
    pub backend_choice: SandboxBackendChoice,
}

#[derive(Debug, Clone)]
pub struct SwarmRunReport {
    pub success: bool,
    pub cancelled: bool,
    pub output: String,
    pub artifacts: PathBuf,
    pub cleanup: CleanupReport,
    pub workers_settled: bool,
    pub cleanup_error: Option<String>,
    pub events: Vec<HiveEvent>,
    pub workers: Vec<crate::swarm_journal::NativeWorkerRecord>,
}

#[derive(Default)]
pub struct NativeSwarmService {
    busy: AtomicBool,
    quarantined: AtomicBool,
    last: Mutex<Option<SwarmRunReport>>,
    stopping: CancelFlag,
    idle: tokio::sync::Notify,
}
struct RunOwner(Arc<NativeSwarmService>);
impl Drop for RunOwner {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
        self.0.idle.notify_waiters();
    }
}
struct CancelOnDrop(CancelFlag);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl NativeSwarmService {
    /// Close admission, cancel the owned run and observe cleanup before the host
    /// drops its runtime. False reports unresolved work, never forced cleanup.
    pub async fn shutdown(&self, timeout: Duration) -> bool {
        self.stopping.cancel();
        let idle = async {
            loop {
                let notified = self.idle.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if !self.is_running() {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(timeout, idle).await.is_ok()
            && !self.quarantined.load(Ordering::Acquire)
    }
    pub fn is_running(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub fn last_report(&self) -> Option<SwarmRunReport> {
        self.last.lock().expect("swarm report").clone()
    }

    /// One admitted goal per service. Caller drop requests cancellation while
    /// the owned task retains Hive/lease cleanup and the last partial report.
    pub async fn run(
        self: &Arc<Self>,
        settings: SwarmSettings,
        context: SwarmRunContext,
        goal: HiveGoal,
        cancellation: CancellationSignal,
    ) -> Result<SwarmRunReport, String> {
        settings.validate()?;
        if self.stopping.signal().is_cancelled() {
            return Err("Swarm service is shutting down.".into());
        }
        if context
            .recalled_context
            .as_ref()
            .is_some_and(|text| text.len() > 65536)
        {
            return Err("Recalled context exceeds 64 KiB.".into());
        }
        if self.quarantined.load(Ordering::Acquire) {
            return Err("Previous swarm cleanup is unresolved; restart only after inspecting its resources.".into());
        }
        if !settings.enabled {
            return Err("Swarm is off. Enable it in Settings → Swarm and Save.".into());
        }
        if cancellation.is_cancelled() {
            return Err("Swarm cancelled before admission.".into());
        }
        if goal.id.is_empty()
            || goal.id.len() > 128
            || goal.prompt.trim().is_empty()
            || goal.prompt.len() > 65536
        {
            return Err("Swarm goal requires a nonempty bounded identity and prompt.".into());
        }
        let (_, dimensions) = context.embedding.as_ref().ok_or(
            "Swarm requires a configured real embedding adapter. No synthetic fallback is used.",
        )?;
        if *dimensions == 0 || *dimensions > 4096 {
            return Err("Unsupported embedding dimension (1–4096).".into());
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("A swarm goal is already running.".into());
        }
        let owner = RunOwner(self.clone());
        if self.stopping.signal().is_cancelled() {
            drop(owner);
            return Err("Swarm service is shutting down.".into());
        }
        let drop_cancel = CancelOnDrop(CancelFlag::new());
        let dropped = drop_cancel.0.signal();
        let stopping = self.stopping.signal();
        tokio::spawn(async move {
            owner.0.quarantined.store(true, Ordering::Release);
            let report = run_owned(settings, context, goal, cancellation, dropped, stopping).await;
            owner.0.quarantined.store(
                report.as_ref().is_ok_and(|report| {
                    !report.cleanup.is_clean()
                        || !report.workers_settled
                        || report.cleanup_error.is_some()
                }),
                Ordering::Release,
            );
            if let Ok(report) = &report {
                *owner.0.last.lock().expect("swarm report") = Some(report.clone());
            }
            drop(owner);
            report
        })
        .await
        .map_err(|_| {
            "Native swarm task failed; inspect sandbox cleanup before retrying.".to_string()
        })?
    }
}

async fn run_owned(
    settings: SwarmSettings,
    context: SwarmRunContext,
    goal: HiveGoal,
    cancellation: CancellationSignal,
    dropped: CancellationSignal,
    stopping: CancellationSignal,
) -> Result<SwarmRunReport, String> {
    let backend = context.backend.clone();
    let capabilities = tokio::task::spawn_blocking(move || backend.capabilities())
        .await
        .map_err(|_| "Sandbox capability probe failed.")?;
    if !capabilities.satisfies(IsolationRequirement::Filesystem) {
        return Err("Swarm unavailable: filesystem isolation capability is required.".into());
    }
    if cancellation.is_cancelled() || dropped.is_cancelled() || stopping.is_cancelled() {
        return Err("Swarm cancelled before workspace capture.".into());
    }
    let root = context.root.clone();
    let inputs = tokio::task::spawn_blocking(move || ProjectInputs::capture(&root))
        .await
        .map_err(|_| "Project capture failed.")??;
    if cancellation.is_cancelled() || dropped.is_cancelled() || stopping.is_cancelled() {
        return Err("Swarm cancelled before provisioning.".into());
    }
    let backend = context.backend.clone();
    let choice = context.backend_choice.clone();
    let root = context.root.clone();
    let drivers = settings.drivers;
    let shared_scope = settings.shared_scope;
    let (leases, artifacts) = tokio::task::spawn_blocking(move || {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let parent = root.join(".agent-vesper");
        if parent
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err("Swarm state directory alias refused.".to_string());
        }
        std::fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
        let run = tempfile::Builder::new()
            .prefix("swarm-run-")
            .tempdir_in(parent)
            .map_err(|error| error.to_string())?
            .keep();
        let demand = SandboxDemand {
            requirement: IsolationRequirement::Filesystem,
            allow_network: false,
            cpu_limit: Some(1.0),
            memory_limit_bytes: Some(512 * 1024 * 1024),
        };
        let leases = NativeSandboxLeases::new(
            backend,
            demand,
            choice,
            drivers as usize + 1,
            run.clone(),
            String::new(),
        )
        .map_err(|error| error.to_string())?
        .with_project_inputs(Arc::new(inputs));
        let leases = if shared_scope {
            leases
                .with_shared_scope()
                .map_err(|error| error.to_string())?
        } else {
            leases
        };
        Ok::<_, String>((Arc::new(leases), run))
    })
    .await
    .map_err(|_| "Native swarm scope setup failed.")??;
    let mut config = HiveConfig::balanced(
        &context
            .allowed_tools
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    config.roles[1].min_workers = drivers;
    config.roles[1].max_workers = drivers;
    config.topology_kind = settings.topology;
    config.topology_config.max_agents = drivers + 1;
    config.topology_config.failover_enabled = settings.failover;
    let (embedding, dimensions) = context.embedding.expect("admitted embedding");
    config.dimensions = dimensions;
    let journal = Arc::new(crate::swarm_journal::NativeWorkerJournal::default());
    let factories = config
        .roles
        .iter()
        .map(|role| {
            let mut port = ProviderWorkerPort::new(
                context.factory.clone(),
                context.tools.clone(),
                role.allowed_tools.clone(),
                context.mode,
                context.permission,
                context.permission_port.clone(),
            );
            port = port
                .with_context(context.recalled_context.clone())
                .expect("admitted context bound");
            port = port.with_journal(journal.clone(), role.name.clone());
            if let Some(progress) = &context.progress {
                port = port.with_progress_port(progress.clone());
            }
            (
                role.name.clone(),
                Arc::new(
                    port.into_instance_factory()
                        .with_sandbox_leases(leases.clone(), role.name.clone()),
                ) as Arc<dyn WorkerInstanceFactory>,
            )
        })
        .collect();
    let retirements_settled = AtomicBool::new(true);
    let execution = async {
        let hive = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err("Swarm cancelled during startup.".into()),
            _ = dropped.cancelled() => return Err("Swarm caller closed during startup.".into()),
            _ = stopping.cancelled() => return Err("Swarm service closed during startup.".into()),
            result = Hive::with_factories(config, factories, embedding) => result.map_err(|error| error.to_string())?,
        };
        retirements_settled.store(false, Ordering::Release);
        let mut hive = hive.with_timestamp_source(Arc::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|time| u64::try_from(time.as_millis()).ok())
        }));
        hive.admit_topology().map_err(|error| error.to_string())?;
        hive.submit(goal.clone())
            .map_err(|error| error.to_string())?;
        let (result, cancelled) = tokio::select! {
            biased;
            _ = cancellation.cancelled() => (Err("Swarm cancelled; completed worker evidence is retained.".into()), true),
            _ = dropped.cancelled() => (Err("Swarm caller closed; completed worker evidence is retained.".into()), true),
            _ = stopping.cancelled() => (Err("Swarm service closed; completed worker evidence is retained.".into()), true),
            result = hive.run_to_completion() => (result.map(|_| ()).map_err(|error| error.to_string()), false),
        };
        let mut output = hive
            .ledger()
            .exact(&MemoryScope::Swarm, &format!("{}-synthesis", goal.id))
            .first()
            .map(|hit| hit.entry.text.as_str().to_owned())
            .unwrap_or_default();
        if let Err(error) = &result {
            output = error.clone();
            for hit in hive
                .ledger()
                .filtered(&MemoryScope::Swarm, EntryKind::Observation)
            {
                let evidence = format!(
                    "\n{}: {}",
                    hit.entry.provenance.worker_id,
                    hit.entry.text.as_str()
                );
                if output.len().saturating_add(evidence.len()) > 1_048_576 {
                    break;
                }
                output.push_str(&evidence);
            }
        }
        let events = hive.events();
        hive.close();
        if !hive.settle_workers(Duration::from_secs(10)).await {
            output.push_str("\nWorker retirement remains unresolved.");
            return Ok((false, cancelled, output, events));
        }
        retirements_settled.store(true, Ordering::Release);
        drop(hive);
        Ok::<_, String>((result.is_ok(), cancelled, output, events))
    };
    let result = execution.await;
    let settled = journal.settle(Duration::from_secs(10)).await
        && retirements_settled.load(Ordering::Acquire);
    let (cleanup, cleanup_error) = match leases.shutdown(Duration::from_secs(10)).await {
        Ok(report) => (report, None),
        Err(error) => (leases.cleanup_report(), Some(error.to_string())),
    };
    let (success, cancelled, mut output, events) = result.unwrap_or_else(|error| {
        (
            false,
            cancellation.is_cancelled() || dropped.is_cancelled() || stopping.is_cancelled(),
            error,
            Vec::new(),
        )
    });
    let workers = journal.records();
    if cancelled || !success {
        let completed = events
            .iter()
            .filter_map(|event| match event {
                HiveEvent::TurnCompleted(task, true) => Some(task.as_str()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        for worker in &workers {
            if completed.contains(worker.task.as_str()) {
                continue;
            }
            let visible = worker
                .history
                .iter()
                .filter(|message| message.role == vesper_domain::MessageRole::Assistant)
                .flat_map(|message| &message.content)
                .filter_map(|part| match part {
                    vesper_domain::ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !visible.is_empty() && output.len().saturating_add(visible.len()) < 1_000_000 {
                output.push_str(&format!(
                    "\nPartial native worker {} ({}):\n{}",
                    worker.worker, worker.task, visible
                ));
            }
        }
    }
    if !settled {
        output
            .push_str("\nNative worker state is still settling; no automatic replay is permitted.");
    }
    if !cleanup.is_clean() || cleanup_error.is_some() {
        output.push_str(
            "\nSandbox cleanup is unresolved; reserved resources were not reported as released.",
        );
    }
    // Leave room for each host's artifact path and status envelope, preserving
    // valid UTF-8 and explicitly reporting omitted output.
    if output.len() > 980_000 {
        let mut end = 980_000;
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        output.push_str(
            "\n[Additional output omitted from the host message; retained in worker histories.]",
        );
    }
    Ok(SwarmRunReport {
        success: success && settled && cleanup.is_clean() && cleanup_error.is_none(),
        cancelled,
        output,
        artifacts,
        cleanup,
        workers_settled: settled,
        cleanup_error,
        events,
        workers,
    })
}

#[cfg(test)]
mod backend_selection_tests {
    use super::*;
    #[test]
    fn explicit_disable_and_backend_choices_never_fall_back() {
        for value in ["off", "0", "FALSE", "none"] {
            for docker in [false, true] {
                assert!(backend_selection(Some(value), docker).is_err());
            }
        }
        assert!(backend_selection(Some("docker"), false).is_err());
        assert!(backend_selection(Some("unknown"), true).is_err());
        assert_eq!(
            backend_selection(Some("namespaces"), true).unwrap(),
            SandboxBackendChoice::Default
        );
        assert_eq!(
            backend_selection(Some("container"), true).unwrap(),
            SandboxBackendChoice::Docker
        );
        assert_eq!(
            backend_selection(None, true).unwrap(),
            SandboxBackendChoice::Docker
        );
        assert_eq!(
            backend_selection(None, false).unwrap(),
            SandboxBackendChoice::Default
        );
    }
}
