//! Independent async instance lifecycle for factory-backed pools.
use super::*;
use crate::worker::CancellationSignal;
use futures_util::future::BoxFuture;

/// Creates independently owned execution instances at the composition boundary.
/// Dropped or cancelled creation must clean up its resources. Detached work is
/// the factory's responsibility; this pure coordinator does not spawn a reaper.
pub trait WorkerInstanceFactory: Send + Sync {
    /// Conservative class capabilities used to reject impossible growth.
    fn capabilities(&self) -> WorkerCapabilities;
    /// Boots one distinct worker, honoring the supplied cancellation signal.
    fn create<'a>(
        &'a self,
        id: u64,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>>;
}
struct BootGuard(CancelFlag);
impl Drop for BootGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
// Successful members of an unfinished boot wave still own external destructors.
// Dropping try_join_all on sibling failure/cancellation delegates their retirement
// through the same bounded ownership path as normal replacement.
struct StagedInstance<'a> {
    id: u64,
    port: Option<Arc<dyn WorkerPort>>,
    owner: &'a WorkerPool,
}
impl Drop for StagedInstance<'_> {
    fn drop(&mut self) {
        if let Some(port) = self.port.take() {
            self.owner.dispatch_retirement(vec![port]);
        }
    }
}
impl WorkerPool {
    /// Creates an empty factory-backed pool. `initialize` performs actual async
    /// boot, not capability probes. Use `scale_async` for this pool's scaling.
    pub fn with_factory(
        config: PoolConfig,
        factory: Arc<dyn WorkerInstanceFactory>,
    ) -> Result<Self, PoolError> {
        config.validate().map_err(PoolError::InvalidConfig)?;
        Ok(Self {
            config,
            port: None,
            factory: Some(factory),
            instances: Mutex::new(Default::default()),
            lifecycle: tokio::sync::Mutex::new(()),
            shared: Arc::new(Shared::default()),
        })
    }

    // Called under the async lifecycle gate, never under the state lock.
    // IDs are consumed on failed boot too: an external instance ID is never reused.
    async fn boot_instances(
        &self,
        count: usize,
    ) -> Result<Vec<(u64, Arc<dyn WorkerPort>)>, PoolError> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            PoolError::Initialization("worker lifecycle requires a native async runtime".into())
        })?;
        *self
            .shared
            .retirement_runtime
            .lock()
            .expect("retirement runtime") = Some(runtime);
        if count == 0 {
            self.check_open()?;
            return Ok(Vec::new());
        }
        let ids = {
            let mut inner = self.shared.lock();
            self.check_open()?;
            let end = inner
                .next_id
                .checked_add(count as u64)
                .ok_or_else(|| PoolError::Initialization("worker identity exhaustion".into()))?;
            let ids = ((inner.next_id + 1)..=end).collect::<Vec<_>>();
            inner.next_id = end;
            ids
        };
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let factory = self.factory.as_ref().expect("factory pool");
        let guard = BootGuard(CancelFlag::new());
        let closed = self.shared.close_notify.notified();
        tokio::pin!(closed);
        closed.as_mut().enable();
        self.check_open()?;
        let boots = ids.into_iter().map(|id| {
            let signal = guard.0.signal();
            async move {
                factory.create(id, signal).await.map(|port| StagedInstance {
                    id,
                    port: Some(port),
                    owner: self,
                })
            }
        });
        let resolved = tokio::select! {
            biased;
            _ = &mut closed => return Err(PoolError::Closed),
            _ = tokio::time::sleep(self.config.default_turn_deadline) => return Err(PoolError::Initialization("worker boot deadline exceeded".into())),
            result = futures_util::future::try_join_all(boots) => result.map_err(|error| PoolError::Initialization(error.to_string()))?,
        };
        if resolved.iter().any(|staged| {
            staged
                .port
                .as_ref()
                .expect("staged port")
                .capabilities()
                .max_concurrent_tasks
                == 0
        }) {
            return Err(PoolError::Initialization(
                "factory returned a zero-capacity instance".into(),
            ));
        }
        let instances = self.instances.lock().expect("instance lock");
        for (index, staged) in resolved.iter().enumerate() {
            let port = staged.port.as_ref().expect("staged port");
            if instances.values().any(|old| Arc::ptr_eq(old, port))
                || resolved[..index]
                    .iter()
                    .any(|other| Arc::ptr_eq(other.port.as_ref().expect("staged port"), port))
            {
                return Err(PoolError::Initialization(
                    "factory returned an aliased or zero-capacity instance".into(),
                ));
            }
        }
        drop(instances);
        Ok(resolved
            .into_iter()
            .map(|mut staged| (staged.id, staged.port.take().expect("staged port")))
            .collect())
    }

    fn publish_instances(&self, batch: Vec<(u64, Arc<dyn WorkerPort>)>) -> Result<(), PoolError> {
        let batch: Vec<_> = batch
            .into_iter()
            .map(|(id, port)| StagedInstance {
                id,
                port: Some(port),
                owner: self,
            })
            .collect();
        let capabilities: Vec<_> = batch
            .iter()
            .map(|staged| staged.port.as_ref().expect("staged port").capabilities())
            .collect();
        let mut inner = self.shared.lock();
        self.check_open()?;
        let mut instances = self.instances.lock().expect("instance lock");
        let now = tokio::time::Instant::now();
        inner.concurrent_boot_high_water = inner.concurrent_boot_high_water.max(batch.len());
        for (mut staged, capabilities) in batch.into_iter().zip(capabilities) {
            let id = staged.id;
            let port = staged.port.take().expect("staged port");
            inner.workers.push(PooledWorker::fresh(
                id,
                capabilities,
                WorkerStatus::Idle,
                now,
            ));
            instances.insert(id, port);
            inner.push_event(PoolEvent::WorkerCreated(id));
        }
        Ok(())
    }

    pub(super) async fn initialize_instances(&self) -> Result<(), PoolError> {
        let _gate = self.lifecycle.lock().await;
        self.check_open()?;
        let missing = (self.config.min_workers as usize).saturating_sub(self.shared.lock().total());
        let batch = self.boot_instances(missing).await?;
        self.publish_instances(batch)
    }

    fn take_idle_instance(
        &self,
        required: &[String],
    ) -> Result<Option<(WorkerLease, WorkerCapabilities)>, PoolError> {
        let mut inner = self.shared.lock();
        self.check_open()?;
        let Some(worker) = inner.workers.iter_mut().find(|worker| {
            worker.status == WorkerStatus::Idle
                && worker.capabilities.max_concurrent_tasks > 0
                && worker.capabilities.supports(required)
        }) else {
            return Ok(None);
        };
        worker.status = WorkerStatus::Busy;
        worker.leased = true;
        worker.last_seen = tokio::time::Instant::now();
        let (id, capabilities) = (worker.id, worker.capabilities.clone());
        inner.push_event(PoolEvent::WorkerAcquired(id, String::new()));
        Ok(Some((
            WorkerLease {
                worker_id: id,
                shared: Arc::clone(&self.shared),
                released: AtomicBool::new(false),
            },
            capabilities,
        )))
    }

    pub(super) async fn acquire_instance(
        &self,
        required: &[String],
    ) -> Result<(WorkerLease, WorkerCapabilities), PoolError> {
        let _gate = self.lifecycle.lock().await;
        if let Some(lease) = self.take_idle_instance(required)? {
            return Ok(lease);
        }
        let capabilities = self.factory.as_ref().expect("factory pool").capabilities();
        if self.shared.lock().total() < self.config.max_workers as usize
            && capabilities.max_concurrent_tasks > 0
            && capabilities.supports(required)
        {
            let batch = self.boot_instances(1).await?;
            if !batch[0].1.capabilities().supports(required) {
                return Err(PoolError::Initialization(
                    "created worker does not support requested capabilities".into(),
                ));
            }
            self.publish_instances(batch)?;
            if let Some(lease) = self.take_idle_instance(required)? {
                return Ok(lease);
            }
        }
        let mut inner = self.shared.lock();
        let error = PoolError::Exhausted {
            idle: inner.idle() as u32,
            total: inner.total() as u32,
            max: self.config.max_workers,
        };
        inner.push_event(PoolEvent::CapacityRefused(self.config.max_workers));
        Err(error)
    }

    pub(super) async fn replace_instances(&self) -> Result<usize, PoolError> {
        let _gate = self.lifecycle.lock().await;
        self.check_open()?;
        let failed: Vec<_> = self
            .shared
            .lock()
            .workers
            .iter()
            .filter(|worker| worker.status == WorkerStatus::Failed && !worker.leased)
            .map(|worker| worker.id)
            .collect();
        let observed = {
            let instances = self.instances.lock().expect("instance lock");
            failed
                .iter()
                .filter_map(|id| instances.get(id).cloned())
                .collect::<Vec<_>>()
        };
        if observed.iter().any(|port| port.pending_work()) {
            return Err(PoolError::Initialization(
                "failed instance still owns unfinished work".into(),
            ));
        }
        drop(observed);
        // Failed slots stay quarantined until replacement publication, but
        // physical instances must retire before allocating replacements. An
        // external owner means Drop cannot establish retirement, so fail closed.
        let retired = {
            let mut instances = self.instances.lock().expect("instance lock");
            if failed.iter().any(|id| {
                instances
                    .get(id)
                    .is_some_and(|port| Arc::strong_count(port) != 1 || Arc::weak_count(port) != 0)
            }) {
                return Err(PoolError::Initialization(
                    "failed instance still externally owned".into(),
                ));
            }
            failed
                .iter()
                .filter_map(|id| instances.remove(id))
                .collect::<Vec<_>>()
        };
        // Never invoke external destructors under the pool state/instance locks.
        // Detached backend resources still require the host cleanup contract.
        self.dispatch_retirement(retired);
        if !self
            .settle_retirements(self.config.default_turn_deadline)
            .await
        {
            return Err(PoolError::Initialization(
                "worker retirement deadline exceeded; capacity remains reserved".into(),
            ));
        }
        let batch = self.boot_instances(failed.len()).await?;
        let mut inner = self.shared.lock();
        self.check_open()?;
        let mut instances = self.instances.lock().expect("instance lock");
        let now = tokio::time::Instant::now();
        let mut retired = Vec::new();
        for (old, (id, port)) in failed.iter().zip(batch) {
            let worker = inner
                .workers
                .iter_mut()
                .find(|worker| worker.id == *old)
                .expect("lifecycle gate preserves failed slots");
            *worker = PooledWorker::fresh(id, port.capabilities(), WorkerStatus::Idle, now);
            retired.extend(instances.remove(old));
            instances.insert(id, port);
            inner.push_event(PoolEvent::WorkerReplaced(*old, id));
        }
        drop(instances);
        drop(inner);
        self.dispatch_retirement(retired);
        Ok(failed.len())
    }

    /// Scales factory pools with bounded real asynchronous creation; shrink drops
    /// only idle instances and preserves the configured floor. Legacy pools use
    /// their existing synchronous implementation.
    pub async fn scale_async(&self, delta: i32) -> Result<usize, PoolError> {
        if self.factory.is_none() {
            return self.scale(delta);
        }
        let _gate = self.lifecycle.lock().await;
        self.check_open()?;
        if delta > 0 {
            let room =
                (self.config.max_workers as usize).saturating_sub(self.shared.lock().total());
            let batch = self.boot_instances((delta as usize).min(room)).await?;
            self.publish_instances(batch)?;
        } else if delta < 0 {
            let candidates = {
                let inner = self.shared.lock();
                let count = (delta.unsigned_abs() as usize).min(
                    inner
                        .total()
                        .saturating_sub(self.config.min_workers as usize),
                );
                inner
                    .workers
                    .iter()
                    .filter(|worker| worker.status == WorkerStatus::Idle)
                    .take(count)
                    .map(|worker| worker.id)
                    .collect::<std::collections::BTreeSet<_>>()
            };
            let observed = {
                let instances = self.instances.lock().expect("instance lock");
                candidates
                    .iter()
                    .filter_map(|id| instances.get(id).cloned())
                    .collect::<Vec<_>>()
            };
            if observed.iter().any(|port| port.pending_work()) {
                return Err(PoolError::Initialization(
                    "instance still owns unfinished work".into(),
                ));
            }
            drop(observed);
            let retired = {
                let mut inner = self.shared.lock();
                self.check_open()?;
                let mut remaining = (delta.unsigned_abs() as usize).min(
                    inner
                        .total()
                        .saturating_sub(self.config.min_workers as usize),
                );
                let mut instances = self.instances.lock().expect("instance lock");
                // Validate the entire chosen FIFO shrink set before removing any
                // slot. External owners would otherwise outlive freed capacity.
                if inner
                    .workers
                    .iter()
                    .filter(|worker| {
                        worker.status == WorkerStatus::Idle && candidates.contains(&worker.id)
                    })
                    .take(remaining)
                    .any(|worker| {
                        instances.get(&worker.id).is_none_or(|port| {
                            Arc::strong_count(port) != 1 || Arc::weak_count(port) != 0
                        })
                    })
                {
                    return Err(PoolError::Initialization(
                        "idle instance still externally owned or missing".into(),
                    ));
                }
                let mut retired = Vec::new();
                inner.workers.retain(|worker| {
                    if remaining > 0
                        && worker.status == WorkerStatus::Idle
                        && candidates.contains(&worker.id)
                    {
                        remaining -= 1;
                        retired.extend(instances.remove(&worker.id));
                        false
                    } else {
                        true
                    }
                });
                retired
            };
            self.dispatch_retirement(retired);
            if !self
                .settle_retirements(self.config.default_turn_deadline)
                .await
            {
                return Err(PoolError::Initialization(
                    "worker retirement deadline exceeded; admission remains closed".into(),
                ));
            }
        }
        Ok(self.shared.lock().total())
    }
}

impl WorkerPool {
    pub(crate) fn instance_snapshots(&self) -> Vec<(u64, WorkerCapabilities)> {
        self.shared
            .lock()
            .workers
            .iter()
            .map(|worker| (worker.id, worker.capabilities.clone()))
            .collect()
    }
    pub(crate) fn instance_addresses(&self) -> Vec<usize> {
        self.instances
            .lock()
            .expect("instance lock")
            .values()
            .map(|port| Arc::as_ptr(port) as *const () as usize)
            .collect()
    }
    pub(crate) fn acquire_selected(
        &self,
        id: u64,
        required: &[String],
    ) -> Result<WorkerLease, PoolError> {
        let mut inner = self.shared.lock();
        self.check_open()?;
        let Some(worker) = inner.workers.iter_mut().find(|worker| {
            worker.id == id
                && worker.status == WorkerStatus::Idle
                && !worker.leased
                && worker.capabilities.max_concurrent_tasks > 0
                && worker.capabilities.supports(required)
        }) else {
            return Err(PoolError::Exhausted {
                idle: inner.idle() as u32,
                total: inner.total() as u32,
                max: self.config.max_workers,
            });
        };
        worker.status = WorkerStatus::Busy;
        worker.leased = true;
        worker.last_seen = tokio::time::Instant::now();
        inner.push_event(PoolEvent::WorkerAcquired(id, String::new()));
        Ok(WorkerLease {
            worker_id: id,
            shared: self.shared.clone(),
            released: AtomicBool::new(false),
        })
    }
}
