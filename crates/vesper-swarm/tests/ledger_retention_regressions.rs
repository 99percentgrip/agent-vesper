//! Automatic retention preserves atomic admission and persisted policy.
use futures_util::future::BoxFuture;
use std::sync::Arc;
use vesper_swarm::ledger::hnsw::HnswConfig;
use vesper_swarm::ledger::store::{
    BoundedText, EmbeddingPort, EntryDraft, EntryKind, Ledger, LedgerError, LedgerRetention,
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
                .map(|text| {
                    if text.as_str() == "bad" {
                        vec![f32::NAN, 0.0]
                    } else {
                        vec![1.0, 0.0]
                    }
                })
                .collect())
        })
    }
}
fn ledger(global: usize, cap: usize) -> Ledger {
    let mut config = HnswConfig::new(2);
    config.max_elements = global;
    Ledger::with_retention(
        config,
        Arc::new(Embedding),
        LedgerRetention::Limited {
            swarm: cap,
            worker: cap,
            task: cap,
        },
    )
    .unwrap()
}
async fn record(
    ledger: &Ledger,
    scope: MemoryScope,
    confidence: f32,
    text: &str,
) -> Result<u64, LedgerError> {
    ledger
        .record(EntryDraft {
            scope,
            kind: EntryKind::Observation,
            text: BoundedText::new(text).unwrap(),
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
}
#[tokio::test]
async fn admission_evicts_within_scope_and_survives_snapshot_reload() {
    let ledger = ledger(8, 2);
    let high = record(&ledger, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    record(&ledger, MemoryScope::Swarm, 0.1, "ok")
        .await
        .unwrap();
    let retained = ledger.snapshot();
    let fresh = record(&ledger, MemoryScope::Swarm, 0.9, "ok")
        .await
        .unwrap();
    assert_eq!(
        ledger
            .exact(&MemoryScope::Swarm, "key")
            .iter()
            .map(|hit| hit.entry.id)
            .collect::<Vec<_>>(),
        vec![high, fresh]
    );
    assert_eq!(retained.len(), 2);
    let bytes = ledger.to_snapshot().unwrap();
    let mut config = HnswConfig::new(2);
    config.max_elements = 8;
    let loaded = Ledger::from_snapshot(config, &bytes, Arc::new(Embedding)).unwrap();
    assert_eq!(loaded.to_snapshot().unwrap(), bytes);
    record(&loaded, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    assert_eq!(loaded.len(), 2);
    let scope = MemoryScope::Worker("private".into());
    let oldest = record(&loaded, scope.clone(), 1.0, "ok").await.unwrap();
    record(&loaded, scope.clone(), 0.0, "ok").await.unwrap();
    record(&loaded, scope.clone(), 0.9, "ok").await.unwrap();
    assert!(
        loaded
            .exact(&scope, "key")
            .iter()
            .all(|hit| hit.entry.id != oldest)
    );
    assert_eq!(loaded.exact(&MemoryScope::Swarm, "key").len(), 2);
}
#[tokio::test]
async fn failed_admission_does_not_commit_eviction_or_steal_other_scope_capacity() {
    let ledger = ledger(2, 1);
    record(&ledger, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    record(&ledger, MemoryScope::Worker("private".into()), 1.0, "ok")
        .await
        .unwrap();
    let before = ledger.to_snapshot().unwrap();
    assert!(
        record(&ledger, MemoryScope::Swarm, 1.0, "bad")
            .await
            .is_err()
    );
    assert_eq!(ledger.to_snapshot().unwrap(), before);
    assert!(
        record(&ledger, MemoryScope::Task("new".into()), 1.0, "ok")
            .await
            .is_err()
    );
    assert_eq!(ledger.to_snapshot().unwrap(), before);
}
#[tokio::test]
async fn transfer_reserves_whole_batch_and_never_returns_evicted_copies() {
    let ledger = ledger(10, 2);
    let a = record(&ledger, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    let b = record(&ledger, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    let dest = MemoryScope::Task("dest".into());
    record(&ledger, dest.clone(), 1.0, "ok").await.unwrap();
    let before = ledger.to_snapshot().unwrap();
    assert!(
        ledger
            .transfer(&MemoryScope::Swarm, &dest, &[a, b, a])
            .await
            .is_err()
    );
    assert_eq!(ledger.to_snapshot().unwrap(), before);
    assert!(
        ledger
            .transfer(&MemoryScope::Swarm, &dest, &[a, u64::MAX])
            .await
            .is_err()
    );
    assert_eq!(ledger.to_snapshot().unwrap(), before);
    let (copies, dropped) = ledger
        .transfer(&MemoryScope::Swarm, &dest, &[a, b])
        .await
        .unwrap();
    assert_eq!(dropped, 0);
    assert_eq!(
        ledger
            .exact(&dest, "key")
            .iter()
            .map(|hit| hit.entry.id)
            .collect::<Vec<_>>(),
        copies
    );
}
#[tokio::test]
async fn snapshot_refuses_missing_invalid_or_violated_policy_and_old_version() {
    let ledger = ledger(4, 2);
    record(&ledger, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    record(&ledger, MemoryScope::Swarm, 1.0, "ok")
        .await
        .unwrap();
    let bytes = ledger.to_snapshot().unwrap();
    let split = 20 + u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
    let log: serde_json::Value = serde_json::from_slice(&bytes[split..]).unwrap();
    let mut config = HnswConfig::new(2);
    config.max_elements = 4;
    for policy in [
        serde_json::Value::Null,
        serde_json::json!({"limited":{"swarm":0,"worker":2,"task":2}}),
        serde_json::json!({"limited":{"swarm":1,"worker":2,"task":2}}),
    ] {
        let mut log = log.clone();
        if policy.is_null() {
            log.as_object_mut().unwrap().remove("retention");
        } else {
            log["retention"] = policy;
        }
        let mut corrupt = bytes[..split].to_vec();
        corrupt.extend(serde_json::to_vec(&log).unwrap());
        assert!(Ledger::from_snapshot(config.clone(), &corrupt, Arc::new(Embedding)).is_err());
    }
    let mut old = bytes;
    old[8..12].copy_from_slice(&1u32.to_le_bytes());
    assert!(Ledger::from_snapshot(config, &old, Arc::new(Embedding)).is_err());
}
