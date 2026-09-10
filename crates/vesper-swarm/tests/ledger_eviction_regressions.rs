//! F16 transactional per-scope retention regressions.
use futures_util::future::BoxFuture;
use std::sync::Arc;
use vesper_swarm::ledger::hnsw::HnswConfig;
use vesper_swarm::ledger::store::{
    BoundedText, EmbeddingPort, EntryDraft, EntryKind, Ledger, LedgerError, LedgerFilter,
    MemoryScope, Provenance,
};
struct Embedding;
impl EmbeddingPort for Embedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![1.0, 0.0]; texts.len()]) })
    }
}
async fn record(ledger: &Ledger, scope: MemoryScope, confidence: f32) -> u64 {
    ledger
        .record(EntryDraft {
            scope,
            kind: EntryKind::Observation,
            text: BoundedText::new("evidence").unwrap(),
            key: Some("key".into()),
            confidence,
            provenance: Provenance {
                worker_id: "worker".into(),
                task_id: "task".into(),
                role: "driver".into(),
                sequence: 0,
            },
        })
        .await
        .unwrap()
}
#[tokio::test]
async fn swarm_eviction_removes_lowest_confidence_then_oldest_atomically() {
    let ledger = Ledger::new(2, Arc::new(Embedding)).unwrap();
    let high = record(&ledger, MemoryScope::Swarm, 1.0).await;
    let low_old = record(&ledger, MemoryScope::Swarm, 0.5).await;
    let low_new = record(&ledger, MemoryScope::Swarm, 0.5).await;
    let private = MemoryScope::Worker("private".into());
    let isolated = record(&ledger, private.clone(), 0.0).await;
    let retained = ledger.snapshot();
    assert_eq!(
        ledger.prune_scope(&MemoryScope::Swarm, 1).unwrap(),
        vec![low_old, low_new]
    );
    assert_eq!(retained.exact(&MemoryScope::Swarm, "key").len(), 3);
    assert_eq!(ledger.exact(&MemoryScope::Swarm, "key")[0].entry.id, high);
    assert_eq!(ledger.exact(&private, "key")[0].entry.id, isolated);
    let hits = ledger
        .semantic(
            &MemoryScope::Swarm,
            &BoundedText::new("evidence").unwrap(),
            100,
        )
        .await
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.entry.id).collect::<Vec<_>>(),
        vec![high]
    );
    let bytes = ledger.to_snapshot().unwrap();
    let restored = Ledger::from_snapshot(HnswConfig::new(2), &bytes, Arc::new(Embedding)).unwrap();
    assert_eq!(restored.to_snapshot().unwrap(), bytes);
    assert!(record(&restored, MemoryScope::Swarm, 1.0).await > isolated);
    assert!(
        ledger
            .prune_scope(&MemoryScope::Swarm, 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(ledger.to_snapshot().unwrap(), bytes);
}
#[tokio::test]
async fn private_retention_is_fifo_and_reclaims_real_index_capacity() {
    let mut config = HnswConfig::new(2);
    config.max_elements = 3;
    let ledger = Ledger::with_hnsw_config(config, Arc::new(Embedding)).unwrap();
    let scope = MemoryScope::Worker("private".into());
    let oldest = record(&ledger, scope.clone(), 1.0).await;
    let newest = record(&ledger, scope.clone(), 0.0).await;
    record(&ledger, MemoryScope::Swarm, 1.0).await;
    assert_eq!(ledger.prune_scope(&scope, 1).unwrap(), vec![oldest]);
    assert_eq!(
        ledger
            .select(&scope, &LedgerFilter::default(), 100)
            .unwrap()[0]
            .entry
            .id,
        newest
    );
    record(&ledger, scope.clone(), 1.0).await;
    assert_eq!(ledger.len(), 3);
    assert_eq!(ledger.prune_scope(&scope, 0).unwrap().len(), 2);
    assert!(ledger.exact(&scope, "key").is_empty());
    assert_eq!(ledger.len(), 1);
}
