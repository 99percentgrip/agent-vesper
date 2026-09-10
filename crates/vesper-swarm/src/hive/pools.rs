//! Concrete pool-instance routing for Hive, without provider or host dependencies.
use super::*;
use crate::pool::{PoolConfig, WorkerInstanceFactory, WorkerPool};
use crate::worker::{CancellationSignal, TurnReceipt};
use futures_util::future::BoxFuture;

struct InstancePort {
    pool: Arc<WorkerPool>,
    id: u64,
    capabilities: WorkerCapabilities,
}
impl WorkerPort for InstancePort {
    fn capabilities(&self) -> WorkerCapabilities {
        self.capabilities.clone()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(WorkerError::Cancelled(task.id.clone()));
            }
            let lease = self
                .pool
                .acquire_selected(self.id, &task.required_capabilities)
                .map_err(|error| WorkerError::Failed(task.id.clone(), error.to_string()))?;
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(WorkerError::Cancelled(task.id.clone())),
                result = self.pool.run_leased_task(lease, task.clone()) => result,
            }
        })
    }
}
impl Hive {
    /// Validates the complete composition before booting independent class pools.
    /// The navigator is exactly one instance. Driver floors boot concurrently;
    /// topology admission remains explicit through `admit_topology`.
    pub async fn with_factories(
        config: HiveConfig,
        factories: Vec<(String, Arc<dyn WorkerInstanceFactory>)>,
        embedding: Arc<dyn EmbeddingPort>,
    ) -> Result<Self, HiveError> {
        if config.roles.first().is_none_or(|role| {
            role.name != "navigator" || role.min_workers != 1 || role.max_workers != 1
        }) || config.roles.len() > 128
            || config
                .roles
                .iter()
                .map(|role| u64::from(role.min_workers))
                .sum::<u64>()
                > u64::from(config.topology_config.max_agents)
            || config
                .roles
                .iter()
                .map(|role| u64::from(role.max_workers))
                .sum::<u64>()
                > 4096
            || config
                .roles
                .iter()
                .any(|role| role.name.is_empty() || role.name.len() > 128)
        {
            return Err(HiveError::Admission("factory hive role/aggregate bounds"));
        }
        let mut pools = Vec::new();
        let mut ports: Vec<(String, Arc<dyn WorkerPort>)> = Vec::new();
        for role in &config.roles {
            let factory = factories
                .iter()
                .find(|(name, _)| name == &role.name)
                .ok_or_else(|| HiveError::MissingPort(role.name.clone()))?
                .1
                .clone();
            let capabilities = factory.capabilities();
            let pool = Arc::new(
                WorkerPool::with_factory(
                    PoolConfig {
                        min_workers: role.min_workers,
                        max_workers: role.max_workers,
                        default_turn_deadline: role.turn_deadline,
                        ..PoolConfig::default()
                    },
                    factory,
                )
                .map_err(|error| {
                    HiveError::Worker(WorkerError::Failed(role.name.clone(), error.to_string()))
                })?,
            );
            ports.push((
                role.name.clone(),
                Arc::new(InstancePort {
                    pool: pool.clone(),
                    id: 1,
                    capabilities,
                }),
            ));
            pools.push(pool);
        }
        let mut hive = Self::assemble(config, ports, embedding)?;
        hive.pools = pools;
        // Dropping this constructor on failure/cancellation closes all completed
        // pools and cancels their pending boot waves through Hive's Drop.
        futures_util::future::try_join_all(hive.pools.iter().map(|pool| pool.initialize()))
            .await
            .map_err(|error| {
                HiveError::Worker(WorkerError::Failed("hive boot".into(), error.to_string()))
            })?;
        let mut identities = std::collections::BTreeSet::new();
        for pool in &hive.pools {
            for identity in pool.instance_addresses() {
                if !identities.insert(identity) {
                    return Err(HiveError::Admission(
                        "factory aliased instances across role pools",
                    ));
                }
            }
        }
        let mut workers = Vec::new();
        for (role, pool) in hive.roles.iter().zip(&hive.pools) {
            for (id, capabilities) in pool.instance_snapshots() {
                workers.push(HiveWorker {
                    class: role.name.clone(),
                    node: format!("{}-{id}", role.name),
                    port: Arc::new(InstancePort {
                        pool: pool.clone(),
                        id,
                        capabilities,
                    }),
                });
            }
        }
        hive.loads = worker_loads(&hive.roles, &workers);
        hive.workers = workers;
        Ok(hive)
    }

    /// Stops admission and signals active turns/boots. Detached resource teardown
    /// remains port-owned; this does not claim verified supervisor shutdown.
    pub fn close(&self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.bus.close();
        for pool in &self.pools {
            pool.close();
        }
    }
}
impl Drop for Hive {
    fn drop(&mut self) {
        self.close();
    }
}

#[path = "lifecycle.rs"]
mod lifecycle;
