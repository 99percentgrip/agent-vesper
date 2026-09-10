//! Hive membership, scoring, inbox and execution routes follow real pool scaling.
use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;
use vesper_swarm::hive::orchestrator::{Hive, HiveConfig, HiveGoal};
use vesper_swarm::ledger::store::{BoundedText, EmbeddingPort, LedgerError};
use vesper_swarm::pool::WorkerInstanceFactory;
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};
struct Embedding;
impl EmbeddingPort for Embedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
    }
}
struct Factory(bool);
struct Worker(bool, u64);
impl WorkerInstanceFactory for Factory {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        id: u64,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move { Ok(Arc::new(Worker(self.0, id)) as Arc<dyn WorkerPort>) })
    }
}
impl WorkerPort for Worker {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                success: true,
                duration: Duration::ZERO,
                output: if self.0 && task.id.ends_with("-decompose") {
                    r#"{"tasks":[{"prompt":"one"},{"prompt":"two"},{"prompt":"three"}]}"#.into()
                } else {
                    self.1.to_string()
                },
            })
        })
    }
}
async fn hive(max: u32) -> Hive {
    hive_policy(max, false).await
}
async fn hive_policy(max: u32, failover: bool) -> Hive {
    let mut config = HiveConfig::balanced(&[]);
    config.topology_config.max_agents = max;
    config.topology_config.failover_enabled = failover;
    Hive::with_factories(
        config,
        vec![
            ("navigator".into(), Arc::new(Factory(true))),
            ("driver".into(), Arc::new(Factory(false))),
        ],
        Arc::new(Embedding),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn growth_and_shrink_reconcile_topology_and_actual_execution() {
    let mut hive = hive(8).await;
    hive.admit_topology().unwrap();
    assert_eq!(hive.scale_role("driver", 2).await.unwrap(), 3);
    assert_eq!(hive.topology().node_count(), 4);
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal::new("grown", "work")).unwrap();
    hive.run_to_completion().await.unwrap();
    let entries = hive.ledger().filtered(
        &vesper_swarm::ledger::store::MemoryScope::Swarm,
        vesper_swarm::ledger::store::EntryKind::Observation,
    );
    let drivers: std::collections::BTreeSet<_> = entries
        .iter()
        .filter(|hit| hit.entry.provenance.role == "driver")
        .map(|hit| hit.entry.provenance.worker_id.clone())
        .collect();
    assert_eq!(drivers.len(), 3);
    let before: Vec<_> = hive.topology().nodes.keys().cloned().collect();
    assert_eq!(hive.scale_role("driver", -2).await.unwrap(), 1);
    for id in before {
        if !hive.topology().nodes.contains_key(&id) {
            assert!(matches!(
                hive.bus().try_recv(id.as_str()),
                Err(vesper_swarm::error::SwarmError::UnknownSubscriber(_))
            ));
        }
    }
    assert_eq!(hive.topology().node_count(), 2);
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal::new("shrunk", "work")).unwrap();
    hive.run_to_completion().await.unwrap();
}
#[tokio::test]
async fn invalid_scaling_refuses_before_membership_changes() {
    let mut hive = hive(2).await;
    hive.admit_topology().unwrap();
    assert!(hive.scale_role("driver", 1).await.is_err());
    assert!(hive.scale_role("navigator", 1).await.is_err());
    assert!(hive.scale_role("missing", 1).await.is_err());
    assert_eq!(hive.topology().node_count(), 2);
    hive.close();
    assert!(hive.scale_role("driver", 0).await.is_err());
}

#[tokio::test]
async fn post_boot_inbox_conflict_closes_hive_instead_of_dispatching_stale_routes() {
    let mut hive = hive(8).await;
    hive.admit_topology().unwrap();
    hive.bus().subscribe("driver-2", &[]).unwrap();
    assert!(hive.scale_role("driver", 1).await.is_err());
    assert!(hive.submit(HiveGoal::new("refused", "work")).is_err());
    assert!(hive.run_tick().await.is_err());
    assert!(matches!(
        hive.bus().try_recv("driver-1"),
        Err(vesper_swarm::error::SwarmError::BusClosed)
    ));
}

#[tokio::test]
async fn health_replacement_reconciles_ids_and_disabled_navigator_failover_closes() {
    let mut hive = hive_policy(8, true).await;
    hive.admit_topology().unwrap();
    assert_eq!(
        hive.maintain_workers(tokio::time::Instant::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        hive.maintain_workers(tokio::time::Instant::now() + Duration::from_secs(10))
            .await
            .unwrap(),
        2
    );
    for id in ["navigator-1", "driver-1"] {
        assert!(matches!(
            hive.bus().try_recv(id),
            Err(vesper_swarm::error::SwarmError::UnknownSubscriber(_))
        ));
    }
    for id in ["navigator-2", "driver-2"] {
        assert!(hive.topology().nodes.keys().any(|node| node.as_str() == id));
    }
    hive.submit(HiveGoal::new("replaced", "work")).unwrap();
    hive.run_to_completion().await.unwrap();
    let entries = hive.ledger().filtered(
        &vesper_swarm::ledger::store::MemoryScope::Swarm,
        vesper_swarm::ledger::store::EntryKind::Observation,
    );
    assert!(
        entries
            .iter()
            .filter(|hit| hit.entry.provenance.role == "driver")
            .all(|hit| hit.entry.provenance.worker_id == "driver-2")
    );
    let mut disabled = hive_policy(8, false).await;
    disabled.admit_topology().unwrap();
    assert!(
        disabled
            .maintain_workers(tokio::time::Instant::now() + Duration::from_secs(10))
            .await
            .is_err()
    );
    assert!(disabled.submit(HiveGoal::new("closed", "work")).is_err());
}
struct HangingReplacement(bool);
impl WorkerInstanceFactory for HangingReplacement {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn create<'a>(
        &'a self,
        id: u64,
        _: CancellationSignal,
    ) -> BoxFuture<'a, Result<Arc<dyn WorkerPort>, WorkerError>> {
        Box::pin(async move {
            if id > 1 {
                std::future::pending::<()>().await;
            }
            Ok(Arc::new(Worker(self.0, id)) as Arc<dyn WorkerPort>)
        })
    }
}
#[tokio::test]
async fn cancelled_replacement_closes_hive_before_stale_routes_can_execute() {
    use std::future::Future;
    let mut config = HiveConfig::balanced(&[]);
    config.topology_config.failover_enabled = true;
    let mut hive = Hive::with_factories(
        config,
        vec![
            ("navigator".into(), Arc::new(HangingReplacement(true))),
            ("driver".into(), Arc::new(HangingReplacement(false))),
        ],
        Arc::new(Embedding),
    )
    .await
    .unwrap();
    hive.admit_topology().unwrap();
    let mut pending =
        Box::pin(hive.maintain_workers(tokio::time::Instant::now() + Duration::from_secs(10)));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(pending.as_mut().poll(&mut context).is_pending());
    drop(pending);
    assert!(hive.submit(HiveGoal::new("refused", "work")).is_err());
    assert!(hive.run_tick().await.is_err());
}
