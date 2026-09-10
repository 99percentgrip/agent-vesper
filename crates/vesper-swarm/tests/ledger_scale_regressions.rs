//! Bounded ledger workload measurement with correctness assertions. This does
//! not certify million-entry capacity, host latency, or detached memory usage.
use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::time::Instant;
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
                    let id: usize = text.as_str().parse().unwrap();
                    (0..16)
                        .map(|index| ((id * 31 + index * 17) % 97) as f32 / 97.0)
                        .collect()
                })
                .collect())
        })
    }
}
async fn record(ledger: &Ledger, sequence: u64) -> u64 {
    ledger
        .record(EntryDraft {
            scope: MemoryScope::Swarm,
            kind: EntryKind::Observation,
            text: BoundedText::new(sequence.to_string()).unwrap(),
            confidence: 1.0,
            key: Some("trajectory".into()),
            provenance: Provenance {
                worker_id: "worker".into(),
                task_id: "task".into(),
                role: "driver".into(),
                sequence,
            },
        })
        .await
        .unwrap()
}
#[tokio::test]
#[ignore = "explicit bounded scale measurement; run with --ignored --nocapture"]
async fn ten_thousand_records_with_retained_generation_and_lossless_reload() {
    let config = HnswConfig {
        max_elements: 10_001,
        ..HnswConfig::new(16)
    };
    let ledger = Ledger::with_hnsw_config(config.clone(), Arc::new(Embedding)).unwrap();
    record(&ledger, 0).await;
    let retained = ledger.snapshot();
    let start = Instant::now();
    for sequence in 1..10_000 {
        record(&ledger, sequence).await;
    }
    let append = start.elapsed();
    assert_eq!(retained.len(), 1);
    assert_eq!(
        retained.exact(&MemoryScope::Swarm, "trajectory")[0]
            .entry
            .provenance
            .sequence,
        0
    );
    assert_eq!(ledger.len(), 10_000);
    let start = Instant::now();
    let snapshot = ledger.to_snapshot().unwrap();
    let loaded = Ledger::from_snapshot(config, &snapshot, Arc::new(Embedding)).unwrap();
    assert_eq!(loaded.to_snapshot().unwrap(), snapshot);
    let roundtrip = start.elapsed();
    record(&ledger, 10_000).await;
    record(&loaded, 10_000).await;
    assert_eq!(loaded.to_snapshot().unwrap(), ledger.to_snapshot().unwrap());
    eprintln!(
        "ledger 10k/16D: append={append:?}, snapshot/load/compare={roundtrip:?}, encoded_bytes={}",
        snapshot.len()
    );
}
#[tokio::test]
#[ignore = "explicit bounded retention measurement; run with --ignored --nocapture"]
async fn one_thousand_retained_admissions_reclaim_capacity_and_keep_reader_generation() {
    let ledger = Ledger::with_retention(
        HnswConfig {
            max_elements: 64,
            ..HnswConfig::new(16)
        },
        Arc::new(Embedding),
        LedgerRetention::Limited {
            swarm: 64,
            worker: 64,
            task: 64,
        },
    )
    .unwrap();
    for sequence in 0..64 {
        record(&ledger, sequence).await;
    }
    let retained = ledger.snapshot();
    let start = Instant::now();
    for sequence in 64..1064 {
        record(&ledger, sequence).await;
        assert_eq!(ledger.len(), 64);
    }
    let hits = ledger.exact(&MemoryScope::Swarm, "trajectory");
    assert_eq!(hits.first().unwrap().entry.provenance.sequence, 1000);
    assert_eq!(hits.last().unwrap().entry.provenance.sequence, 1063);
    assert_eq!(
        retained
            .exact(&MemoryScope::Swarm, "trajectory")
            .first()
            .unwrap()
            .entry
            .provenance
            .sequence,
        0
    );
    eprintln!("ledger 1k retention/64 cap/16D: {:?}", start.elapsed());
}
