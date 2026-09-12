//! Hive calls the real shared deadline boundary at every turn phase.
use futures_util::future::BoxFuture;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;
use vesper_swarm::hive::orchestrator::{Hive, HiveConfig, HiveError, HiveGoal};
use vesper_swarm::ledger::store::{BoundedText, EmbeddingPort, LedgerError};
use vesper_swarm::worker::{
    CancellationSignal, TurnReceipt, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

struct Embeddings;
impl EmbeddingPort for Embeddings {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0; 8]; texts.len()]) })
    }
}
struct Script {
    phase: &'static str,
    hang: bool,
    seen: Mutex<Vec<String>>,
    signal: Mutex<Option<CancellationSignal>>,
}
impl WorkerPort for Script {
    fn capabilities(&self) -> WorkerCapabilities {
        WorkerCapabilities::minimal()
    }
    fn run_turn<'a>(
        &'a self,
        task: &'a WorkerTask,
        signal: CancellationSignal,
    ) -> BoxFuture<'a, Result<TurnReceipt, WorkerError>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(task.id.clone());
            let target = task.id.ends_with(self.phase);
            if target {
                *self.signal.lock().unwrap() = Some(signal);
                if self.hang {
                    std::future::pending::<()>().await;
                }
            }
            Ok(TurnReceipt {
                task_id: task.id.clone(),
                output: r#"{"tasks":[{"prompt":"concrete work"}]}"#.into(),
                success: !target,
                duration: Duration::ZERO,
            })
        })
    }
}
fn hive(phase: &'static str, hang: bool) -> (Hive, Arc<Script>) {
    let script = Arc::new(Script {
        phase,
        hang,
        seen: Mutex::new(Vec::new()),
        signal: Mutex::new(None),
    });
    let mut config = HiveConfig::balanced(&[]);
    for role in &mut config.roles {
        role.turn_deadline = Duration::from_millis(5);
    }
    let mut hive = Hive::new(
        config,
        vec![
            ("navigator".into(), script.clone()),
            ("driver".into(), script.clone()),
        ],
        Arc::new(Embeddings),
    )
    .unwrap();
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal::new("g", "work")).unwrap();
    (hive, script)
}
#[tokio::test]
async fn every_hanging_phase_cancels_actual_signal_and_retains_goal() {
    for phase in ["decompose", "task-0", "synthesize"] {
        let (mut hive, script) = hive(phase, true);
        let result = tokio::time::timeout(Duration::from_secs(2), hive.run_tick())
            .await
            .unwrap();
        assert!(matches!(
            result,
            Err(HiveError::Worker(WorkerError::DeadlineExceeded(_)))
        ));
        assert!(
            script
                .signal
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        assert_eq!(hive.interrupted_goal().unwrap().id, "g");
        let calls = script.seen.lock().unwrap().len();
        assert!(matches!(
            hive.run_tick().await,
            Err(HiveError::Interrupted(_))
        ));
        assert_eq!(calls, script.seen.lock().unwrap().len());
    }
}
#[tokio::test]
async fn unsuccessful_receipts_never_advance_the_pipeline() {
    // Bare hives (no governance/decisions) keep exact VRO-15 turn counts;
    // the review panel composes only on governance-enabled hives.
    for (phase, calls) in [("decompose", 1), ("task-0", 2), ("synthesize", 3)] {
        let (mut hive, script) = hive(phase, false);
        assert!(hive.run_tick().await.is_err());
        assert_eq!(script.seen.lock().unwrap().len(), calls);
        assert!(hive.interrupted_goal().is_some());
    }
}
#[tokio::test]
async fn dropping_tick_cancels_worker_without_losing_goal() {
    let (mut hive, script) = hive("decompose", true);
    let mut tick = Box::pin(hive.run_tick());
    assert!(
        tick.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(tick);
    assert!(
        script
            .signal
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
    assert_eq!(hive.interrupted_goal().unwrap().id, "g");
    assert!(matches!(
        hive.run_tick().await,
        Err(HiveError::Interrupted(_))
    ));
}

struct HangingEmbedding;
impl EmbeddingPort for HangingEmbedding {
    fn embed<'a>(
        &'a self,
        _: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(std::future::pending())
    }
}
#[tokio::test]
async fn hanging_embedding_is_bounded_without_publication_or_replay() {
    let script = Arc::new(Script {
        phase: "never",
        hang: false,
        seen: Mutex::new(Vec::new()),
        signal: Mutex::new(None),
    });
    let mut config = HiveConfig::balanced(&[]);
    for role in &mut config.roles {
        role.turn_deadline = Duration::from_millis(5);
    }
    let mut hive = Hive::new(
        config,
        vec![
            ("navigator".into(), script.clone()),
            ("driver".into(), script.clone()),
        ],
        Arc::new(HangingEmbedding),
    )
    .unwrap();
    hive.admit_topology().unwrap();
    hive.submit(HiveGoal::new("embedding-hang", "work"))
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(1), hive.run_tick())
        .await
        .unwrap();
    assert!(
        matches!(outcome, Err(HiveError::Ledger(LedgerError::Embedding(message))) if message == "embedding deadline exceeded")
    );
    assert_eq!(hive.ledger().len(), 0);
    assert!(hive.interrupted_goal().is_some());
    assert!(matches!(
        hive.run_tick().await,
        Err(HiveError::Interrupted(_))
    ));
    assert_eq!(script.seen.lock().unwrap().len(), 2);
}
