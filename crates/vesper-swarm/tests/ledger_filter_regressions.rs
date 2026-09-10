//! Desired-behavior coverage for F16 structured selection and transfer filters.
use futures_util::future::BoxFuture;
use std::sync::Arc;
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

#[tokio::test]
async fn original_timestamps_survive_transfer_retention_and_snapshot() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use vesper_swarm::ledger::hnsw::HnswConfig;
    let clock = Arc::new(AtomicU64::new(1_700_000_000_000));
    let source = clock.clone();
    let ledger = Ledger::new(2, Arc::new(Embedding))
        .unwrap()
        .with_timestamp_source(Arc::new(move || Some(source.load(Ordering::SeqCst))));
    let scope = MemoryScope::Worker("worker".into());
    let first = record(
        &ledger,
        scope.clone(),
        EntryKind::Artifact,
        "worker",
        999,
        1.0,
    )
    .await;
    let retained = ledger.snapshot();
    clock.store(1_700_000_000_010, Ordering::SeqCst);
    record(
        &ledger,
        scope.clone(),
        EntryKind::Artifact,
        "worker",
        1,
        1.0,
    )
    .await;
    let filter = LedgerFilter {
        timestamp_min_ms: Some(1_700_000_000_000),
        timestamp_max_ms: Some(1_700_000_000_000),
        ..Default::default()
    };
    assert_eq!(
        ledger.select(&scope, &filter, 10).unwrap()[0].entry.id,
        first
    );
    let copies = ledger
        .transfer_filtered(&scope, &MemoryScope::Swarm, &[first], &filter)
        .await
        .unwrap()
        .0;
    let copied = ledger.select(&MemoryScope::Swarm, &filter, 10).unwrap();
    assert_eq!(copied[0].entry.id, copies[0]);
    assert_eq!(copied[0].entry.timestamp_ms, Some(1_700_000_000_000));
    ledger.prune_scope(&scope, 0).unwrap();
    assert_eq!(retained.select(&scope, &filter, 10).unwrap().len(), 1);
    let bytes = ledger.to_snapshot().unwrap();
    assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
    let mut legacy = bytes.clone();
    legacy[8..12].copy_from_slice(&2u32.to_le_bytes());
    assert!(Ledger::from_snapshot(HnswConfig::new(2), &legacy, Arc::new(Embedding)).is_err());
    let split = 20 + u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
    let mut log: serde_json::Value = serde_json::from_slice(&bytes[split..]).unwrap();
    for entry in log["entries"].as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("timestamp_ms");
    }
    legacy.truncate(split);
    legacy.extend(serde_json::to_vec(&log).unwrap());
    let migrated = Ledger::from_snapshot(HnswConfig::new(2), &legacy, Arc::new(Embedding)).unwrap();
    assert_eq!(migrated.len(), ledger.len());
    assert!(
        migrated
            .select(&MemoryScope::Swarm, &filter, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(&migrated.to_snapshot().unwrap()[8..12], &3u32.to_le_bytes());
    let loaded = Ledger::from_snapshot(
        HnswConfig::new(2),
        &ledger.to_snapshot().unwrap(),
        Arc::new(Embedding),
    )
    .unwrap();
    assert_eq!(
        loaded.select(&MemoryScope::Swarm, &filter, 10).unwrap(),
        copied
    );
    record(
        &loaded,
        MemoryScope::Swarm,
        EntryKind::Artifact,
        "worker",
        1_700_000_000_000,
        1.0,
    )
    .await;
    assert_eq!(
        loaded
            .select(&MemoryScope::Swarm, &filter, 10)
            .unwrap()
            .len(),
        1,
        "unknown time must not match via sequence"
    );
    assert!(
        loaded
            .select(
                &MemoryScope::Swarm,
                &LedgerFilter {
                    timestamp_min_ms: Some(2),
                    timestamp_max_ms: Some(1),
                    ..Default::default()
                },
                1
            )
            .is_err()
    );
}
async fn record(
    ledger: &Ledger,
    scope: MemoryScope,
    kind: EntryKind,
    worker: &str,
    sequence: u64,
    confidence: f32,
) -> u64 {
    ledger
        .record(EntryDraft {
            scope,
            kind,
            text: BoundedText::new("evidence").unwrap(),
            provenance: Provenance {
                worker_id: worker.into(),
                task_id: "task".into(),
                role: "driver".into(),
                sequence,
            },
            confidence,
            key: None,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn conjunctive_filters_preserve_scope_provenance_and_retained_generations() {
    let ledger = Ledger::new(2, Arc::new(Embedding)).unwrap();
    let scope = MemoryScope::Worker("private".into());
    let first = record(
        &ledger,
        scope.clone(),
        EntryKind::Artifact,
        "worker",
        10,
        0.9,
    )
    .await;
    record(
        &ledger,
        scope.clone(),
        EntryKind::Artifact,
        "other",
        10,
        0.9,
    )
    .await;
    record(
        &ledger,
        MemoryScope::Swarm,
        EntryKind::Artifact,
        "worker",
        10,
        0.9,
    )
    .await;
    let filter = LedgerFilter {
        worker_id: Some("worker".into()),
        task_id: Some("task".into()),
        role: Some("driver".into()),
        kind: Some(EntryKind::Artifact),
        sequence_min: Some(10),
        sequence_max: Some(10),
        confidence_min: Some(0.9),
        ..Default::default()
    };
    let retained = ledger.snapshot();
    let second = record(
        &ledger,
        scope.clone(),
        EntryKind::Artifact,
        "worker",
        10,
        1.0,
    )
    .await;
    assert_eq!(
        retained
            .select(&scope, &filter, usize::MAX)
            .unwrap()
            .iter()
            .map(|hit| hit.entry.id)
            .collect::<Vec<_>>(),
        vec![first]
    );
    assert_eq!(
        ledger
            .select(&scope, &filter, usize::MAX)
            .unwrap()
            .iter()
            .map(|hit| hit.entry.id)
            .collect::<Vec<_>>(),
        vec![second, first]
    );
    assert!(ledger.select(&scope, &filter, 0).unwrap().is_empty());
    let copy = ledger
        .transfer_filtered(&scope, &MemoryScope::Swarm, &[first], &filter)
        .await
        .unwrap()
        .0[0];
    let hits = ledger.select(&MemoryScope::Swarm, &filter, 100).unwrap();
    assert_eq!(hits[0].entry.id, copy);
    assert_eq!(hits[0].entry.provenance.sequence, 10);
}

#[tokio::test]
async fn selective_transfer_reports_exclusions_and_rolls_back_foreign_ids() {
    let ledger = Ledger::new(2, Arc::new(Embedding)).unwrap();
    let dest = MemoryScope::Task("destination".into());
    let selected = record(
        &ledger,
        MemoryScope::Swarm,
        EntryKind::Artifact,
        "worker",
        1,
        0.9,
    )
    .await;
    let category = record(
        &ledger,
        MemoryScope::Swarm,
        EntryKind::Metric,
        "worker",
        2,
        1.0,
    )
    .await;
    let low = record(
        &ledger,
        MemoryScope::Swarm,
        EntryKind::Artifact,
        "worker",
        3,
        0.7,
    )
    .await;
    let foreign = record(&ledger, dest.clone(), EntryKind::Metric, "worker", 4, 1.0).await;
    let filter = LedgerFilter {
        kind: Some(EntryKind::Artifact),
        ..Default::default()
    };
    let before = ledger.to_snapshot().unwrap();
    assert!(
        ledger
            .transfer_filtered(&MemoryScope::Swarm, &dest, &[selected, foreign], &filter)
            .await
            .is_err()
    );
    assert_eq!(ledger.to_snapshot().unwrap(), before);
    let (copies, dropped) = ledger
        .transfer_filtered(
            &MemoryScope::Swarm,
            &dest,
            &[selected, category, low],
            &filter,
        )
        .await
        .unwrap();
    assert_eq!((copies.len(), dropped), (1, 2));
    let copied = ledger.select(&dest, &filter, 100).unwrap();
    assert_eq!(copied[0].entry.provenance.sequence, 1);
}

#[tokio::test]
async fn invalid_filters_refuse_without_mutation_and_results_are_capped() {
    let ledger = Ledger::new(2, Arc::new(Embedding)).unwrap();
    for sequence in 0..105 {
        record(
            &ledger,
            MemoryScope::Swarm,
            EntryKind::Metric,
            "worker",
            sequence,
            1.0,
        )
        .await;
    }
    assert_eq!(
        ledger
            .select(&MemoryScope::Swarm, &LedgerFilter::default(), usize::MAX)
            .unwrap()
            .len(),
        100
    );
    let before = ledger.to_snapshot().unwrap();
    for filter in [
        LedgerFilter {
            sequence_min: Some(2),
            sequence_max: Some(1),
            ..Default::default()
        },
        LedgerFilter {
            confidence_min: Some(f32::NAN),
            ..Default::default()
        },
        LedgerFilter {
            confidence_min: Some(-0.1),
            ..Default::default()
        },
        LedgerFilter {
            worker_id: Some("x".repeat(257)),
            ..Default::default()
        },
        LedgerFilter {
            task_id: Some(String::new()),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            ledger.select(&MemoryScope::Swarm, &filter, 1),
            Err(LedgerError::InvalidFilter(_))
        ));
        assert!(matches!(
            ledger
                .transfer_filtered(
                    &MemoryScope::Swarm,
                    &MemoryScope::Task("dest".into()),
                    &[1],
                    &filter
                )
                .await,
            Err(LedgerError::InvalidFilter(_))
        ));
        assert_eq!(ledger.to_snapshot().unwrap(), before);
    }
}

#[tokio::test]
async fn semantic_predicates_apply_before_top_k_without_cross_scope_leakage() {
    let ledger = Ledger::new(2, Arc::new(Embedding)).unwrap();
    for sequence in 0..5 {
        record(
            &ledger,
            MemoryScope::Swarm,
            EntryKind::Metric,
            "other",
            sequence,
            1.0,
        )
        .await;
    }
    let wanted = record(
        &ledger,
        MemoryScope::Swarm,
        EntryKind::Artifact,
        "worker",
        10,
        1.0,
    )
    .await;
    record(
        &ledger,
        MemoryScope::Task("private".into()),
        EntryKind::Artifact,
        "worker",
        10,
        1.0,
    )
    .await;
    let filter = LedgerFilter {
        worker_id: Some("worker".into()),
        kind: Some(EntryKind::Artifact),
        ..Default::default()
    };
    let hits = ledger
        .semantic_filtered(
            &MemoryScope::Swarm,
            &BoundedText::new("evidence").unwrap(),
            &filter,
            1,
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entry.id, wanted);
    assert_eq!(hits[0].similarity, Some(1.0));
}
