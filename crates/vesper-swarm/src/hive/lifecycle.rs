//! Caller-owned membership changes reconciled before any further Hive dispatch.
use super::*;
use crate::manager::NodeUpdate;
use crate::topology::{NodeStatus, TopologyRole};

impl Hive {
    /// Scales a driver class and updates concrete execution, scoring, topology and
    /// inbox membership together before returning. Navigator cardinality is fixed.
    /// A post-boot reconciliation failure closes the Hive rather than exposing
    /// stale or partially reconciled dispatch routes. Interrupted goals never replay.
    pub async fn scale_role(&mut self, class: &str, delta: i32) -> Result<usize, HiveError> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(HiveError::Admission("hive closed"));
        }
        if let Some(goal) = &self.active_goal {
            return Err(HiveError::Interrupted(goal.id.clone()));
        }
        if self.pools.len() != self.roles.len() || self.topology.node_count() == 0 {
            return Err(HiveError::Admission(
                "scaling requires admitted factory pools",
            ));
        }
        let index = self
            .roles
            .iter()
            .position(|role| role.name == class)
            .ok_or_else(|| HiveError::MissingPort(class.into()))?;
        if class == "navigator" {
            return Err(HiveError::Admission("navigator cardinality is fixed"));
        }
        let pool = &self.pools[index];
        if delta > 0 {
            let growth = (delta as usize)
                .min((pool.config().max_workers as usize).saturating_sub(pool.live_workers()));
            if self.workers.len().saturating_add(growth)
                > self.topology_manager.config().max_agents as usize
            {
                return Err(HiveError::Admission("scaling exceeds topology capacity"));
            }
        }
        let count = pool.scale_async(delta).await.map_err(|error| {
            HiveError::Worker(WorkerError::Failed(class.into(), error.to_string()))
        })?;
        if let Err(error) = self.reconcile_instances() {
            self.close();
            return Err(error);
        }
        Ok(count)
    }

    /// Caller-driven health/replacement pass between goals. The caller supplies
    /// monotonic time; this method does not spawn a monitor or invent heartbeats.
    /// Disabled navigator failover closes the hive on navigator loss. Cancellation
    /// during replacement also closes it: old concrete routes may be retired.
    pub async fn maintain_workers(
        &mut self,
        now: tokio::time::Instant,
    ) -> Result<usize, HiveError> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(HiveError::Admission("hive closed"));
        }
        if let Some(goal) = &self.active_goal {
            return Err(HiveError::Interrupted(goal.id.clone()));
        }
        if self.pools.len() != self.roles.len() || self.topology.node_count() == 0 {
            return Err(HiveError::Admission(
                "maintenance requires admitted factory pools",
            ));
        }
        let mut changed = false;
        let mut navigator_failed = false;
        for (role, pool) in self.roles.iter().zip(&self.pools) {
            let failed = pool.health_tick(now);
            changed |= !failed.is_empty();
            navigator_failed |= role.name == "navigator" && !failed.is_empty();
        }
        if navigator_failed && !self.topology_manager.config().failover_enabled {
            self.close();
            return Err(HiveError::Admission(
                "navigator failed with failover disabled",
            ));
        }
        if !changed {
            return Ok(0);
        }
        let mut guard = MaintenanceGuard {
            closed: &self.closed,
            bus: self.bus.clone(),
            pools: self.pools.clone(),
            armed: true,
        };
        let mut replaced = 0;
        for pool in &self.pools {
            replaced += pool.replace_failed().await.map_err(|error| {
                HiveError::Worker(WorkerError::Failed(
                    "hive maintenance".into(),
                    error.to_string(),
                ))
            })?;
        }
        guard.armed = false;
        drop(guard);
        // No await between disarming the guard and committing membership.
        if let Err(error) = self.reconcile_instances() {
            self.close();
            return Err(error);
        }
        Ok(replaced)
    }

    fn reconcile_instances(&mut self) -> Result<(), HiveError> {
        let mut identities = std::collections::BTreeSet::new();
        let mut workers = Vec::new();
        for (role, pool) in self.roles.iter().zip(&self.pools) {
            for identity in pool.instance_addresses() {
                if !identities.insert(identity) {
                    return Err(HiveError::Admission(
                        "factory aliased instances across role pools",
                    ));
                }
            }
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
        let new_ids: std::collections::BTreeSet<_> = workers
            .iter()
            .map(|worker| NodeId::new(&worker.node))
            .collect();
        let removed: Vec<_> = self
            .topology
            .join_order
            .iter()
            .filter(|id| !new_ids.contains(*id))
            .cloned()
            .collect();
        let mut staged = self.topology.clone();
        for id in &removed {
            self.topology_manager
                .remove_node(&mut staged, id)
                .map_err(|error| HiveError::Topology(error.to_string()))?;
        }
        let mut added = Vec::new();
        for worker in &workers {
            let id = NodeId::new(&worker.node);
            if staged.nodes.contains_key(&id) {
                continue;
            }
            let role = if worker.class == "navigator" {
                TopologyRole::Queen
            } else {
                TopologyRole::Worker
            };
            self.topology_manager
                .add_node(&mut staged, id.clone(), role)
                .map_err(|error| HiveError::Topology(error.to_string()))?;
            self.topology_manager
                .update_node(
                    &mut staged,
                    &id,
                    NodeUpdate {
                        status: Some(NodeStatus::Active),
                        ..NodeUpdate::none()
                    },
                )
                .map_err(|error| HiveError::Topology(error.to_string()))?;
            added.push(worker.node.clone());
        }
        // Do not force an election: preserve configured failover/vacancy policy.
        self.topology_manager
            .rebalance(&mut staged)
            .map_err(|error| HiveError::Topology(error.to_string()))?;
        let mut subscribed = Vec::new();
        for node in &added {
            if let Err(error) = self
                .bus
                .subscribe(node, &[MessageKind::TaskAssign, MessageKind::Control])
            {
                for node in subscribed {
                    let _ = self.bus.unsubscribe(node);
                }
                return Err(HiveError::Bus(error.to_string()));
            }
            subscribed.push(node.as_str());
        }
        for id in removed {
            self.bus
                .unsubscribe(id.as_str())
                .map_err(|error| HiveError::Bus(error.to_string()))?;
        }
        let mut loads = worker_loads(&self.roles, &workers);
        for (id, load) in &mut loads {
            if let Some(previous) = self.loads.get(id) {
                *load = previous.clone();
            }
        }
        self.workers = workers;
        self.loads = loads;
        self.topology = staged;
        Ok(())
    }
}

struct MaintenanceGuard<'a> {
    closed: &'a std::sync::atomic::AtomicBool,
    bus: MessageBus,
    pools: Vec<Arc<WorkerPool>>,
    armed: bool,
}
impl Drop for MaintenanceGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.closed
                .store(true, std::sync::atomic::Ordering::Release);
            self.bus.close();
            for pool in &self.pools {
                pool.close();
            }
        }
    }
}
