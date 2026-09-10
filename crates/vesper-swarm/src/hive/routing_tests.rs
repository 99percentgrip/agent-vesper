use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Embedding;
impl EmbeddingPort for Embedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<crate::ledger::store::BoundedText>,
    ) -> futures_util::future::BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
    }
}
struct Worker(Arc<AtomicUsize>);
impl WorkerPort for Worker {
    fn capabilities(&self) -> crate::worker::WorkerCapabilities {
        crate::worker::WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        _: crate::worker::CancellationSignal,
    ) -> futures_util::future::BoxFuture<
        'a,
        Result<crate::worker::TurnReceipt, crate::worker::WorkerError>,
    > {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(crate::worker::TurnReceipt {
                task_id: task.id.clone(),
                output: if task.id.ends_with("-decompose") {
                    r#"{"tasks":[{"prompt":"work"}]}"#.into()
                } else {
                    "result".into()
                },
                success: true,
                duration: Duration::ZERO,
            })
        })
    }
}
#[tokio::test]
async fn disconnected_driver_is_never_dispatched_and_goal_is_not_replayed() {
    for kind in [
        TopologyKind::Mesh,
        TopologyKind::Hierarchical,
        TopologyKind::Centralized,
        TopologyKind::Hybrid,
    ] {
        let turns = Arc::new(AtomicUsize::new(0));
        let mut config = HiveConfig::balanced(&[]);
        config.topology_kind = kind;
        let mut hive = Hive::new(
            config,
            vec![
                (
                    "navigator".into(),
                    Arc::new(Worker(Arc::new(AtomicUsize::new(0)))),
                ),
                ("driver".into(), Arc::new(Worker(turns.clone()))),
            ],
            Arc::new(Embedding),
        )
        .unwrap();
        hive.admit_topology().unwrap();
        hive.topology.edges.clear();
        hive.submit(HiveGoal::new("disconnected", "work")).unwrap();
        assert!(hive.run_tick().await.is_err());
        assert_eq!(turns.load(Ordering::SeqCst), 0);
        assert!(hive.interrupted_goal().is_some());
        assert!(matches!(
            hive.run_tick().await,
            Err(HiveError::Interrupted(_))
        ));
    }
}
