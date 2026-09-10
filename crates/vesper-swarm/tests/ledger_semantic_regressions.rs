//! F16 semantic threshold and configured candidate-budget acceptance.
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
        Box::pin(async move {
            Ok(texts
                .iter()
                .map(|text| match text.as_str() {
                    "opposite" => vec![-1.0, 0.0],
                    "near" => vec![0.8, 0.6],
                    _ => vec![1.0, 0.0],
                })
                .collect())
        })
    }
}
async fn record(ledger: &Ledger, scope: MemoryScope, text: &str) -> u64 {
    ledger
        .record(EntryDraft {
            scope,
            kind: EntryKind::Observation,
            text: BoundedText::new(text).unwrap(),
            key: None,
            confidence: 1.0,
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
async fn semantic_and_filtered_semantic_enforce_documented_threshold() {
    let ledger = Ledger::new(2, Arc::new(Embedding)).unwrap();
    record(&ledger, MemoryScope::Swarm, "opposite").await;
    let query = BoundedText::new("query").unwrap();
    assert!(
        ledger
            .semantic(&MemoryScope::Swarm, &query, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ledger
            .semantic_filtered(&MemoryScope::Swarm, &query, &LedgerFilter::default(), 1)
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn semantic_scope_filter_respects_configured_over_fetch_budget() {
    let mut config = HnswConfig::new(2);
    config.over_fetch_factor = 1;
    let ledger = Ledger::with_hnsw_config(config, Arc::new(Embedding)).unwrap();
    record(&ledger, MemoryScope::Worker("private".into()), "query").await;
    record(&ledger, MemoryScope::Swarm, "near").await;
    // With factor 1, only the closer private candidate is gathered then
    // excluded. Scope exclusion must not silently expand the configured budget.
    assert!(
        ledger
            .semantic(&MemoryScope::Swarm, &BoundedText::new("query").unwrap(), 1)
            .await
            .unwrap()
            .is_empty()
    );
}
