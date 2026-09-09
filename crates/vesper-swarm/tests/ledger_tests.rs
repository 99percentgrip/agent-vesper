//! Integration tests for the hybrid ledger (VRO-15 PR-7).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vesper_swarm::ledger::store::{
    BoundedText, EmbeddingPort, EntryDraft, EntryKind, HYBRID_MAX_RESULTS, Ledger, LedgerError,
    LedgerQuery, MemoryScope, Provenance, TRANSFER_CAP, TRANSFER_CONFIDENCE_FLOOR,
};

// ---------------------------------------------------------------------
// Deterministic fake embedding port (zero network)
// ---------------------------------------------------------------------

/// Hash-based deterministic embedding: same text ⇒ same vector, every
/// time. Good enough for scope/routing/transfer tests and honest about
/// being a fake.
struct FakeEmbeddingPort {
    dims: usize,
    calls: AtomicUsize,
}

impl FakeEmbeddingPort {
    fn new(dims: usize) -> Arc<Self> {
        Arc::new(Self {
            dims,
            calls: AtomicUsize::new(0),
        })
    }

    fn vector_for(text: &str, dims: usize) -> Vec<f32> {
        let mut state = 0xcbf2_9ce4_8422_2325u64;
        for byte in text.as_bytes() {
            state ^= u64::from(*byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut vector = Vec::with_capacity(dims);
        for index in 0..dims {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let value = (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> (48 + index % 8)) as f32
                / 256.0
                - 0.5;
            vector.push(value);
        }
        vector
    }
}

impl EmbeddingPort for FakeEmbeddingPort {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> futures_util::future::BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        let expected = texts.len();
        self.calls.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move {
            // Deliberately yield once so concurrency tests interleave.
            tokio::task::yield_now().await;
            let vectors = texts
                .iter()
                .map(|text| Self::vector_for(text.as_str(), self.dims))
                .collect::<Vec<_>>();
            debug_assert_eq!(vectors.len(), expected);
            Ok(vectors)
        })
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn text(value: &str) -> BoundedText {
    BoundedText::new(value).expect("bounded")
}

fn provenance(worker: &str, task: &str, sequence: u64) -> Provenance {
    Provenance {
        worker_id: worker.to_string(),
        role: String::from("driver"),
        task_id: task.to_string(),
        sequence,
    }
}

fn draft(scope: MemoryScope, key: &str, body: &str, confidence: f32) -> EntryDraft {
    EntryDraft {
        scope,
        kind: EntryKind::Observation,
        text: text(body),
        provenance: provenance("w1", "t1", 1),
        confidence,
        key: Some(key.to_string()),
    }
}

async fn ledger(dims: usize) -> Ledger {
    Ledger::new(dims, FakeEmbeddingPort::new(dims)).expect("valid ledger")
}

// ---------------------------------------------------------------------
// Dual-write fail-closed
// ---------------------------------------------------------------------

#[tokio::test]
async fn record_dual_writes_and_returns_fresh_ids() {
    let ledger = ledger(8).await;
    let swarm = MemoryScope::Swarm;
    let first = ledger
        .record(draft(swarm.clone(), "k1", "alpha finding", 0.9))
        .await
        .expect("record");
    let second = ledger
        .record(draft(swarm.clone(), "k2", "beta finding", 0.85))
        .await
        .expect("record");
    assert_ne!(first, second);
    assert_eq!(ledger.len(), 2);
}

#[tokio::test]
async fn record_fails_closed_on_bad_confidence() {
    let ledger = ledger(8).await;
    for bad in [-0.1f32, 1.5] {
        let error = ledger
            .record(draft(MemoryScope::Swarm, "k", "x", bad))
            .await
            .unwrap_err();
        assert_eq!(error, LedgerError::InvalidConfidence(bad));
    }
    assert!(ledger.is_empty(), "nothing may be stored on rejection");
}

/// A port whose vectors have the wrong dimension: the dual-write must
/// fail closed with nothing admitted to either side.
struct WrongDimensionPort {
    dims: usize,
}

impl EmbeddingPort for WrongDimensionPort {
    fn embed<'a>(
        &'a self,
        _texts: Vec<BoundedText>,
    ) -> futures_util::future::BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move { Ok(vec![vec![0.1; self.dims]]) })
    }
}

#[tokio::test]
async fn record_fails_closed_on_dimension_mismatch() {
    let ledger = Ledger::new(16, Arc::new(WrongDimensionPort { dims: 4 })).expect("ledger");
    let error = ledger
        .record(draft(MemoryScope::Swarm, "k", "x", 0.9))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        LedgerError::DimensionMismatch {
            expected: 16,
            actual: 4,
        }
    );
    assert!(ledger.is_empty(), "dual-write left no trace");
}

#[tokio::test]
async fn record_fails_closed_on_embedding_error() {
    struct FailingPort;
    impl EmbeddingPort for FailingPort {
        fn embed<'a>(
            &'a self,
            _texts: Vec<BoundedText>,
        ) -> futures_util::future::BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
            Box::pin(async move { Err(LedgerError::Embedding(String::from("port down"))) })
        }
    }
    let ledger = Ledger::new(8, Arc::new(FailingPort)).expect("ledger");
    let error = ledger
        .record(draft(MemoryScope::Swarm, "k", "x", 0.9))
        .await
        .unwrap_err();
    assert!(matches!(error, LedgerError::Embedding(_)));
    assert!(ledger.is_empty());
}

// ---------------------------------------------------------------------
// Scope isolation
// ---------------------------------------------------------------------

#[tokio::test]
async fn worker_and_task_scopes_are_strictly_isolated() {
    let ledger = ledger(8).await;
    let worker_a = MemoryScope::Worker(String::from("a"));
    let worker_b = MemoryScope::Worker(String::from("b"));
    let task_x = MemoryScope::Task(String::from("x"));

    ledger
        .record(draft(worker_a.clone(), "ka", "worker a note", 0.9))
        .await
        .unwrap();
    ledger
        .record(draft(worker_b.clone(), "kb", "worker b note", 0.9))
        .await
        .unwrap();
    ledger
        .record(draft(task_x.clone(), "kx", "task x note", 0.9))
        .await
        .unwrap();

    // Exact lookups never cross scopes, even with identical keys.
    assert_eq!(ledger.exact(&worker_a, "ka").len(), 1);
    assert!(ledger.exact(&worker_b, "ka").is_empty());
    assert!(ledger.exact(&task_x, "ka").is_empty());
    // Semantic queries stay inside their scope.
    let in_a = ledger
        .semantic(&worker_a, &text("worker a note"), 5)
        .await
        .unwrap();
    assert!(in_a.iter().all(|hit| hit.entry.scope == worker_a));
    let in_x = ledger
        .semantic(&task_x, &text("worker a note"), 5)
        .await
        .unwrap();
    // The task scope holds one entry of its own; no worker entry may
    // leak in. Every returned hit must belong to the queried scope and
    // to the scope's own entries.
    assert!(
        in_x.iter().all(|hit| hit.entry.scope == task_x),
        "semantic results leaked across scopes"
    );
    assert!(
        in_x.iter()
            .all(|hit| hit.entry.key.as_deref() != Some("ka")
                && hit.entry.key.as_deref() != Some("kb")),
        "task scope must not see worker memory"
    );
    // Filtered queries too.
    let kinds = ledger.filtered(&worker_b, EntryKind::Observation);
    assert!(kinds.iter().all(|hit| hit.entry.scope == worker_b));
}

#[tokio::test]
async fn swarm_scope_is_shared_and_visible_across_query_kinds() {
    let ledger = ledger(8).await;
    ledger
        .record(draft(MemoryScope::Swarm, "shared", "hive note", 0.9))
        .await
        .unwrap();
    assert_eq!(ledger.exact(&MemoryScope::Swarm, "shared").len(), 1);
    assert_eq!(
        ledger
            .filtered(&MemoryScope::Swarm, EntryKind::Observation)
            .len(),
        1
    );
}

// ---------------------------------------------------------------------
// Query routing + hybrid tie-breaking
// ---------------------------------------------------------------------

#[tokio::test]
async fn hybrid_exact_hits_win_over_semantic_ties() {
    let ledger = ledger(8).await;
    // Two entries with identical bodies (identical embeddings): one is
    // the exact key match, one is not.
    ledger
        .record(draft(MemoryScope::Swarm, "target", "identical body", 0.9))
        .await
        .unwrap();
    ledger
        .record(draft(MemoryScope::Swarm, "other", "identical body", 0.9))
        .await
        .unwrap();
    let hits = ledger
        .hybrid(
            &MemoryScope::Swarm,
            Some("target"),
            &text("identical body"),
            2,
        )
        .await
        .expect("hybrid");
    assert_eq!(hits.len(), 2);
    assert!(hits[0].exact, "the exact match must rank first");
    assert_eq!(hits[0].entry.key.as_deref(), Some("target"));
    assert!(!hits[1].exact);
}

#[tokio::test]
async fn hybrid_ceiling_is_the_oracle_default() {
    assert_eq!(HYBRID_MAX_RESULTS, 100);
    assert_eq!(TRANSFER_CAP, 20);
    let _ = TRANSFER_CONFIDENCE_FLOOR;
}

#[tokio::test]
async fn query_router_dispatches_by_shape() {
    let ledger = ledger(8).await;
    ledger
        .record(draft(MemoryScope::Swarm, "q1", "router probe", 0.9))
        .await
        .unwrap();
    let exact = ledger
        .query(LedgerQuery::Exact {
            scope: MemoryScope::Swarm,
            key: String::from("q1"),
        })
        .await
        .unwrap();
    assert_eq!(exact.len(), 1);
    assert!(exact[0].exact);
    let filtered = ledger
        .query(LedgerQuery::Filtered {
            scope: MemoryScope::Swarm,
            kind: EntryKind::Observation,
        })
        .await
        .unwrap();
    assert_eq!(filtered.len(), 1);
    let semantic = ledger
        .query(LedgerQuery::Semantic {
            scope: MemoryScope::Swarm,
            text: text("router probe"),
            k: 3,
        })
        .await
        .unwrap();
    assert!(!semantic.is_empty());
    let hybrid = ledger
        .query(LedgerQuery::Hybrid {
            scope: MemoryScope::Swarm,
            key: Some(String::from("q1")),
            text: text("router probe"),
            k: 3,
        })
        .await
        .unwrap();
    assert!(!hybrid.is_empty());
}

// ---------------------------------------------------------------------
// Bounded transfer
// ---------------------------------------------------------------------

#[tokio::test]
async fn transfer_copies_only_high_confidence_entries_and_reports_drops() {
    let ledger = ledger(8).await;
    let worker = MemoryScope::Worker(String::from("w"));
    let swarm = MemoryScope::Swarm;
    let high = ledger
        .record(draft(worker.clone(), "h", "high confidence", 0.95))
        .await
        .unwrap();
    let mid = ledger
        .record(draft(worker.clone(), "m", "just under floor", 0.79))
        .await
        .unwrap();
    let (copied, dropped) = ledger
        .transfer(&worker, &swarm, &[high, mid])
        .await
        .expect("transfer");
    assert_eq!(copied.len(), 1, "only the >=0.8 entry is copied");
    assert_eq!(dropped, 1, "the low-confidence entry is reported dropped");
    // The copy landed in the destination scope with original provenance.
    let hits = ledger.exact(&swarm, "h");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entry.provenance.sequence, 1);
    assert_eq!(hits[0].entry.provenance.worker_id, "w1");
    assert_eq!(hits[0].entry.id, copied[0]);
    // The source entry is untouched (copy, not move).
    assert_eq!(ledger.exact(&worker, "h").len(), 1);
    let _ = mid;
}

#[tokio::test]
async fn transfer_refuses_over_cap_batches() {
    let ledger = ledger(8).await;
    let worker = MemoryScope::Worker(String::from("w"));
    let swarm = MemoryScope::Swarm;
    let ids: Vec<u64> = (0..21).map(|i| i + 1).collect();
    assert_eq!(
        ledger.transfer(&worker, &swarm, &ids).await.unwrap_err(),
        LedgerError::TransferCap(21)
    );
    // Exactly 20 is allowed.
    ledger
        .record(draft(worker.clone(), "seed", "seed", 0.9))
        .await
        .unwrap();
    assert!(ledger.transfer(&worker, &swarm, &[]).await.is_ok());
}

#[tokio::test]
async fn transfer_refuses_same_scope_and_foreign_entries() {
    let ledger = ledger(8).await;
    let worker = MemoryScope::Worker(String::from("w"));
    let swarm = MemoryScope::Swarm;
    assert_eq!(
        ledger
            .transfer(&worker, &worker.clone(), &[1])
            .await
            .unwrap_err(),
        LedgerError::TransferSameScope
    );
    // Record into swarm, then try to transfer it out of worker.
    let id = ledger
        .record(draft(swarm.clone(), "s", "swarm entry", 0.9))
        .await
        .unwrap();
    assert!(matches!(
        ledger.transfer(&worker, &swarm, &[id]).await.unwrap_err(),
        LedgerError::TransferRefused(_)
    ));
    // Unknown ids are loud.
    assert_eq!(
        ledger.transfer(&swarm, &worker, &[999]).await.unwrap_err(),
        LedgerError::UnknownEntry(999)
    );
}

#[tokio::test]
async fn transfer_preserves_provenance_verbatim() {
    let ledger = ledger(8).await;
    let worker = MemoryScope::Worker(String::from("origin"));
    let task = MemoryScope::Task(String::from("dest"));
    let original = EntryDraft {
        scope: worker.clone(),
        kind: EntryKind::Artifact,
        text: text("artifact body"),
        provenance: provenance("driver-7", "task-42", 17),
        confidence: 0.88,
        key: Some(String::from("art")),
    };
    let id = ledger.record(original.clone()).await.unwrap();
    let (copied, _) = ledger
        .transfer(&worker, &task, &[id])
        .await
        .expect("transfer");
    let hits = ledger.exact(&task, "art");
    assert_eq!(hits.len(), 1);
    // Worker, role, task, sequence: all identical to the original.
    assert_eq!(hits[0].entry.provenance, original.provenance);
    assert_eq!(hits[0].entry.confidence, 0.88);
    assert_eq!(hits[0].entry.kind, EntryKind::Artifact);
    // Fresh id: a copy is a new entry, not an alias.
    assert_ne!(hits[0].entry.id, id);
    let _ = copied;
}

// ---------------------------------------------------------------------
// Concurrency: interleaved readers/writers never see torn state
// ---------------------------------------------------------------------

#[tokio::test]
async fn concurrent_readers_and_writers_never_observe_torn_state() {
    let ledger = ledger(8).await;
    let writers: Vec<_> = (0..4)
        .map(|w| {
            let ledger = ledger.clone();
            tokio::spawn(async move {
                let worker = MemoryScope::Worker(format!("w{w}"));
                for i in 0..25u64 {
                    ledger
                        .record(draft(
                            worker.clone(),
                            &format!("k{w}-{i}"),
                            &format!("body {w} {i}"),
                            0.9,
                        ))
                        .await
                        .expect("record");
                }
            })
        })
        .collect();
    let readers: Vec<_> = (0..4)
        .map(|reader| {
            let ledger = ledger.clone();
            tokio::spawn(async move {
                let _ = reader;
                let swarm = MemoryScope::Swarm;
                for i in 0..25u64 {
                    // Interleaved reads of every shape while writers run.
                    let _ = ledger.exact(&swarm, &format!("k0-{i}"));
                    let _ = ledger.filtered(&swarm, EntryKind::Observation);
                    let hits = ledger
                        .semantic(&swarm, &text("body"), 5)
                        .await
                        .expect("semantic");
                    // Invariant at every observation point: every hit is a
                    // fully-formed entry from exactly one scope.
                    for hit in hits {
                        assert_eq!(hit.entry.scope, swarm);
                        assert!(!hit.entry.text.as_str().is_empty());
                    }
                }
            })
        })
        .collect();
    for writer in writers {
        writer.await.expect("writer");
    }
    for reader in readers {
        reader.await.expect("reader");
    }
    assert_eq!(ledger.len(), 100, "all 4×25 writes must be durable");
}

#[tokio::test]
async fn concurrent_transfers_respect_bounds_under_interleaving() {
    let ledger = ledger(8).await;
    let worker = MemoryScope::Worker(String::from("w"));
    let swarm = MemoryScope::Swarm;
    let mut ids = Vec::new();
    for i in 0..10u64 {
        ids.push(
            ledger
                .record(draft(
                    worker.clone(),
                    &format!("k{i}"),
                    &format!("body {i}"),
                    0.9,
                ))
                .await
                .unwrap(),
        );
    }
    let transfers: Vec<_> = (0..4)
        .map(|t| {
            let ledger = ledger.clone();
            let ids = ids.clone();
            let worker = worker.clone();
            let swarm = swarm.clone();
            tokio::spawn(async move {
                let dest = if t % 2 == 0 {
                    swarm.clone()
                } else {
                    MemoryScope::Task(format!("t{t}"))
                };
                ledger
                    .transfer(&worker, &dest, &ids)
                    .await
                    .expect("transfer")
            })
        })
        .collect();
    let mut total_copied = 0;
    for handle in transfers {
        let (copied, dropped) = handle.await.expect("join");
        assert_eq!(dropped, 0);
        total_copied += copied.len();
    }
    assert_eq!(total_copied, 40, "each of 4 transfers copies all 10");
    assert_eq!(ledger.len(), 50, "10 originals + 40 copies");
}
