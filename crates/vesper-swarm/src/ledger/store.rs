//! Hybrid structured + vector ledger composition (VRO-15 PR-7).
//!
//! [`Ledger`] binds the PR-6 HNSW graph to an append-only structured log
//! behind one dual-write contract: an entry is durable only when both
//! sides accept it — a vector-side failure (dimension mismatch, capacity)
//! fails the whole write. Queries auto-route: exact/filterable lookups hit
//! the structured side, semantic lookups hit HNSW, and hybrid lookups
//! merge with the oracle defaults (`semantic_threshold = 0.7`,
//! `hybrid_max_results = 100`) with **exact matches winning scoring ties**.
//!
//! Scope isolation (the PR-7 contract): [`MemoryScope::Worker`] and
//! [`MemoryScope::Task`] are strictly isolated from each other and from
//! [`MemoryScope::Swarm`]; entries move between scopes only through the
//! explicit, bounded [`Ledger::transfer`] (confidence ≥ 0.8, at most 20
//! entries per call, provenance always preserved verbatim).
//!
//! Determinism: embedding calls go through the caller-supplied async
//! [`EmbeddingPort`]; the ledger itself owns no clock (sequence numbers,
//! with optional caller-supplied Unix millisecond timestamps) and no randomness.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

pub use super::filter::LedgerFilter;
pub use super::retention::LedgerRetention;

use crate::ledger::hnsw::{HnswConfig, HnswError, HnswIndex, SearchHit};

/// The type alias the directive names: bounded text as the ledger sees it.
pub type BoundedText = vesper_domain::ContentText;

/// Provider-neutral embedding seam (composition boundary).
///
/// Implementations turn bounded texts into dense vectors at the host
/// boundary (a real provider adapter) or in tests (a deterministic fake).
/// The ledger never performs I/O to obtain embeddings.
pub trait EmbeddingPort: Send + Sync {
    /// Embeds the given texts, preserving order.
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>>;
}

/// Memory scopes. `Worker` and `Task` are strictly isolated from each
/// other; `Swarm` is the shared hive surface.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryScope {
    /// The whole hive's shared surface.
    Swarm,
    /// One worker's private scratch. Isolated from every other scope.
    Worker(String),
    /// One task's context. Isolated from every other scope.
    Task(String),
}

impl MemoryScope {
    /// Whether `other` designates the exact same scope.
    #[must_use]
    pub fn is_same(&self, other: &MemoryScope) -> bool {
        self == other
    }
}

/// Entry classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    /// A reasoning step or finding.
    Observation,
    /// A produced artifact reference.
    Artifact,
    /// An instruction or constraint.
    Instruction,
    /// A metric or measurement.
    Metric,
}

/// Where an entry came from. Preserved verbatim across transfers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The worker that produced the entry.
    pub worker_id: String,
    /// The role that worker played.
    pub role: String,
    /// The task the entry belongs to.
    pub task_id: String,
    /// The entry's sequence number within its original scope.
    pub sequence: u64,
}

/// A new entry awaiting admission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryDraft {
    /// Scope the entry lands in.
    pub scope: MemoryScope,
    /// Classification.
    pub kind: EntryKind,
    /// The bounded text payload.
    pub text: BoundedText,
    /// Producer identity and position.
    pub provenance: Provenance,
    /// Caller-assessed confidence in `0.0..=1.0`.
    pub confidence: f32,
    /// Optional exact-match key for structured lookups.
    pub key: Option<String>,
}

/// One durable ledger entry (structured side record).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// Ledger-assigned monotonic id.
    pub id: u64,
    /// Original recording time in Unix milliseconds, when supplied by the host.
    /// Absent in older snapshots; never synthesized from sequence or copy time.
    #[serde(default)]
    pub timestamp_ms: Option<u64>,
    /// Scope of the entry.
    pub scope: MemoryScope,
    /// Classification.
    pub kind: EntryKind,
    /// The bounded payload.
    pub text: BoundedText,
    /// Original producer identity, never rewritten.
    pub provenance: Provenance,
    /// Confidence as recorded.
    pub confidence: f32,
    /// Optional exact-match key.
    pub key: Option<String>,
}

/// Query shapes the ledger routes.
#[derive(Debug, Clone, PartialEq)]
pub enum LedgerQuery {
    /// Exact key match inside a scope.
    Exact {
        /// Scope to search.
        scope: MemoryScope,
        /// Key that must match exactly.
        key: String,
    },
    /// Kind filter inside a scope, most recent first.
    Filtered {
        /// Scope to search.
        scope: MemoryScope,
        /// Kind to keep.
        kind: EntryKind,
    },
    /// Semantic similarity search inside a scope.
    Semantic {
        /// Scope to search.
        scope: MemoryScope,
        /// Query text (embedded through the port).
        text: BoundedText,
        /// Result count.
        k: usize,
    },
    /// Hybrid: exact + semantic, merged with tie-breaking.
    Hybrid {
        /// Scope to search.
        scope: MemoryScope,
        /// Optional exact key.
        key: Option<String>,
        /// Query text (embedded through the port).
        text: BoundedText,
        /// Result count.
        k: usize,
    },
}

/// One query result.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerHit {
    /// The durable entry.
    pub entry: LedgerEntry,
    /// Semantic similarity when the vector side contributed (`None` for
    /// pure exact hits).
    pub similarity: Option<f32>,
    /// Whether the structured side produced this hit exactly.
    pub exact: bool,
}

/// Ledger failures.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LedgerError {
    /// Per-scope retention caps must fit the global graph capacity.
    #[error("invalid ledger retention capacity")]
    InvalidRetention,
    /// A structured predicate violates a resource or range bound.
    #[error("invalid ledger filter: {0}")]
    InvalidFilter(&'static str),
    /// Monotonic identity space is exhausted; no state is published.
    #[error("ledger identity space exhausted")]
    SequenceExhausted,
    /// A whole-ledger snapshot is invalid or exceeds its byte budget.
    #[error("invalid ledger snapshot: {0}")]
    InvalidSnapshot(&'static str),
    /// The underlying index rejected the configuration or vector.
    #[error("hnsw failure: {0}")]
    Hnsw(#[from] HnswError),
    /// The embedding port failed.
    #[error("embedding failure: {0}")]
    Embedding(String),
    /// The vector port returned the wrong arity.
    #[error("embedding port returned {actual} vectors for {expected} texts")]
    EmbeddingArity {
        /// Expected vector count.
        expected: usize,
        /// Returned vector count.
        actual: usize,
    },
    /// An embedding's dimension disagrees with the configured one.
    #[error("dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// Configured dimension.
        expected: usize,
        /// Supplied dimension.
        actual: usize,
    },
    /// Confidence outside `0.0..=1.0`.
    #[error("confidence {0} outside 0.0..=1.0")]
    InvalidConfidence(f32),
    /// A transfer bound was violated.
    #[error("transfer refused: {0}")]
    TransferRefused(&'static str),
    /// A batch exceeded the transfer cap of 20.
    #[error("transfer of {0} entries exceeds the cap of 20")]
    TransferCap(usize),
    /// Source and destination scopes are identical.
    #[error("transfer source and destination are the same scope")]
    TransferSameScope,
    /// Unknown entry id.
    #[error("entry {0} does not exist")]
    UnknownEntry(u64),
}

/// Hybrid routing defaults ported from the oracle.
pub const SEMANTIC_THRESHOLD: f32 = 0.7;
/// Hybrid merge ceiling.
pub const HYBRID_MAX_RESULTS: usize = 100;
/// Transfer confidence floor.
pub const TRANSFER_CONFIDENCE_FLOOR: f32 = 0.8;
/// Transfer per-call entry cap.
pub const TRANSFER_CAP: usize = 20;

/// Maximum accepted whole-ledger snapshot size (64 MiB).
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotLog {
    retention: LedgerRetention,
    next_id: u64,
    entries: Vec<LedgerEntry>,
}

/// Interior ledger state.
#[derive(Debug, Clone)]
struct LedgerInner {
    retention: LedgerRetention,
    entries: BTreeMap<u64, Arc<LedgerEntry>>,
    /// (scope, key) → entry ids, for exact routing.
    exact_index: BTreeMap<(MemoryScope, String), Vec<u64>>,
    next_id: u64,
    hnsw: HnswIndex,
}

/// A retained immutable generation of both the structured log and vector index.
/// Readers never acquire the writer mutex and later publications cannot alter it.
#[derive(Debug, Clone)]
pub struct LedgerSnapshot {
    inner: Arc<LedgerInner>,
}

impl LedgerSnapshot {
    /// Number of entries in this generation.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.entries.len()
    }
    /// Whether this generation has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.entries.is_empty()
    }
    /// Bounded conjunctive lookup, newest ledger identity first. A retained
    /// snapshot sees neither later records nor later transfers.
    pub fn select(
        &self,
        scope: &MemoryScope,
        filter: &LedgerFilter,
        limit: usize,
    ) -> Result<Vec<LedgerHit>, LedgerError> {
        filter.validate()?;
        Ok(self
            .inner
            .entries
            .values()
            .rev()
            .filter(|entry| entry.scope == *scope && filter.matches(entry))
            .take(limit.min(HYBRID_MAX_RESULTS))
            .map(|entry| LedgerHit {
                entry: entry.as_ref().clone(),
                similarity: None,
                exact: false,
            })
            .collect())
    }

    /// Exact-key lookup against this generation only.
    #[must_use]
    pub fn exact(&self, scope: &MemoryScope, key: &str) -> Vec<LedgerHit> {
        Ledger::exact_in(&self.inner, scope, key)
    }
}

/// The hybrid ledger.
///
/// Clone shares atomic generation publication. Readers retain immutable state;
/// writers serialize staged mutations and publish both sides atomically.
/// Record and semantic/hybrid queries await the embedding port outside the
/// writer lock. Generation cloning cost remains subject to scale acceptance.
#[derive(Clone)]
pub struct Ledger {
    dimensions: usize,
    port: Arc<dyn EmbeddingPort>,
    inner: Arc<arc_swap::ArcSwap<LedgerInner>>,
    writer: Arc<std::sync::Mutex<()>>,
    timestamp: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
}

impl std::fmt::Debug for Ledger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.load_full();
        f.debug_struct("Ledger")
            .field("dimensions", &self.dimensions)
            .field("entries", &inner.entries.len())
            .field("hnsw_points", &inner.hnsw.len())
            .finish()
    }
}

impl Ledger {
    /// Creates an empty ledger with the given embedding dimension.
    pub fn new(dimensions: usize, port: Arc<dyn EmbeddingPort>) -> Result<Self, LedgerError> {
        Self::with_hnsw_config(HnswConfig::new(dimensions), port)
    }

    /// Creates a ledger with an explicit HNSW configuration.
    pub fn with_hnsw_config(
        config: HnswConfig,
        port: Arc<dyn EmbeddingPort>,
    ) -> Result<Self, LedgerError> {
        Self::with_retention(config, port, LedgerRetention::Disabled)
    }

    /// Constructs a ledger with persisted automatic per-scope admission caps.
    /// Eviction only touches the receiving scope, never unrelated private data.
    pub fn with_retention(
        config: HnswConfig,
        port: Arc<dyn EmbeddingPort>,
        retention: LedgerRetention,
    ) -> Result<Self, LedgerError> {
        retention.validate(config.max_elements)?;
        Ok(Self {
            dimensions: config.dimensions,
            port,
            timestamp: Arc::new(|| None),
            writer: Arc::new(std::sync::Mutex::new(())),
            inner: Arc::new(arc_swap::ArcSwap::from_pointee(LedgerInner {
                retention,
                entries: BTreeMap::new(),
                exact_index: BTreeMap::new(),
                next_id: 0,
                hnsw: HnswIndex::new(config)?,
            })),
        })
    }

    /// Captures a coherent immutable generation without taking the writer lock.
    #[must_use]
    pub fn snapshot(&self) -> LedgerSnapshot {
        LedgerSnapshot {
            inner: self.inner.load_full(),
        }
    }

    /// Serializes one coherent generation: magic/version, index byte length,
    /// versioned HNSW payload, then a structured JSON log. No filesystem I/O.
    pub fn to_snapshot(&self) -> Result<Vec<u8>, LedgerError> {
        let inner = self.inner.load_full();
        let graph = inner.hnsw.to_snapshot_bounded(MAX_SNAPSHOT_BYTES - 20)?;
        // Borrow each entry directly: do not clone the entire structured log.
        #[derive(Serialize)]
        struct BorrowedLog<'a> {
            retention: LedgerRetention,
            next_id: u64,
            #[serde(serialize_with = "serialize_entries")]
            entries: &'a BTreeMap<u64, Arc<LedgerEntry>>,
        }
        fn serialize_entries<S: serde::Serializer>(
            entries: &BTreeMap<u64, Arc<LedgerEntry>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut sequence = serializer.serialize_seq(Some(entries.len()))?;
            for entry in entries.values() {
                sequence.serialize_element(entry.as_ref())?;
            }
            sequence.end()
        }
        use std::io::Write;
        let mut writer =
            super::snapshot_writer::SnapshotWriter::new(Vec::new(), MAX_SNAPSHOT_BYTES);
        for bytes in [
            b"VSWLEDG1".as_slice(),
            &3u32.to_le_bytes(),
            &(graph.len() as u64).to_le_bytes(),
            &graph,
        ] {
            writer
                .write_all(bytes)
                .map_err(|_| LedgerError::InvalidSnapshot("snapshot allocation refused"))?;
        }
        drop(graph);
        serde_json::to_writer(
            &mut writer,
            &BorrowedLog {
                retention: inner.retention,
                next_id: inner.next_id,
                entries: &inner.entries,
            },
        )
        .map_err(|_| LedgerError::InvalidSnapshot("log encoding or byte limit refused"))?;
        Ok(writer.bytes)
    }

    /// Loads a complete ledger without embeddings or external side effects.
    /// Structured/index identities and monotonic sequence must agree exactly.
    pub fn from_snapshot(
        config: HnswConfig,
        bytes: &[u8],
        port: Arc<dyn EmbeddingPort>,
    ) -> Result<Self, LedgerError> {
        let invalid = LedgerError::InvalidSnapshot;
        if bytes.len() < 20 || bytes.len() > MAX_SNAPSHOT_BYTES || &bytes[..8] != b"VSWLEDG1" {
            return Err(invalid("invalid header or byte limit"));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
        if !matches!(version, 2 | 3) {
            return Err(invalid("unsupported version"));
        }
        let graph_len = usize::try_from(u64::from_le_bytes(
            bytes[12..20].try_into().expect("eight bytes"),
        ))
        .map_err(|_| invalid("index length overflow"))?;
        let split = 20usize
            .checked_add(graph_len)
            .filter(|end| *end <= bytes.len())
            .ok_or(invalid("truncated index"))?;
        let graph = HnswIndex::from_snapshot(&config, &bytes[20..split])?;
        let mut log: SnapshotLog = serde_json::from_slice(&bytes[split..])
            .map_err(|_| invalid("invalid structured log"))?;
        if version == 2 && log.entries.iter().any(|entry| entry.timestamp_ms.is_some()) {
            return Err(invalid("timestamps require ledger snapshot version 3"));
        }
        log.retention
            .validate(config.max_elements)
            .map_err(|_| invalid("invalid retention caps"))?;
        let mut counts = BTreeMap::<MemoryScope, usize>::new();
        for entry in &log.entries {
            let count = counts.entry(entry.scope.clone()).or_default();
            *count += 1;
            if log
                .retention
                .cap(&entry.scope)
                .is_some_and(|cap| *count > cap)
            {
                return Err(invalid("scope exceeds retention cap"));
            }
        }
        log.entries.sort_by_key(|entry| entry.id);
        if graph.len() != log.entries.len() {
            return Err(invalid("index/log count mismatch"));
        }
        let mut inner = LedgerInner {
            retention: log.retention,
            entries: BTreeMap::new(),
            exact_index: BTreeMap::new(),
            next_id: log.next_id,
            hnsw: graph,
        };
        for entry in log.entries {
            if entry.id == 0
                || entry.id > inner.next_id
                || !(0.0..=1.0).contains(&entry.confidence)
                || inner.entries.contains_key(&entry.id)
                || inner.hnsw.raw_vector(entry.id).is_none()
            {
                return Err(invalid("invalid entry identity/confidence"));
            }
            if let Some(key) = &entry.key {
                inner
                    .exact_index
                    .entry((entry.scope.clone(), key.clone()))
                    .or_default()
                    .push(entry.id);
            }
            inner.entries.insert(entry.id, Arc::new(entry));
        }
        Ok(Self {
            dimensions: config.dimensions,
            port,
            timestamp: Arc::new(|| None),
            writer: Arc::new(std::sync::Mutex::new(())),
            inner: Arc::new(arc_swap::ArcSwap::from_pointee(inner)),
        })
    }

    /// Configured embedding dimension.
    #[must_use]
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Number of durable entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.load_full().entries.len()
    }

    /// Whether the ledger is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Dual-write admission (the PR-7 contract).
    ///
    /// The draft is embedded through the port, then offered to **both**
    /// sides: the structured log under a fresh monotonic id, and the HNSW
    /// graph. If either side rejects — embedding failure, dimension
    /// mismatch, duplicate point, capacity — the entire write fails and
    /// neither side keeps a trace of it.
    pub async fn record(&self, draft: EntryDraft) -> Result<u64, LedgerError> {
        self.record_at(draft, (self.timestamp)()).await
    }

    /// Attach a cheap nonblocking wall-clock source at the composition boundary.
    /// Loaded snapshots retain their original times; this affects new records only.
    pub fn with_timestamp_source(
        mut self,
        source: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
    ) -> Self {
        self.timestamp = source;
        self
    }

    /// Record an explicit original Unix millisecond timestamp transactionally.
    /// `None` means unavailable, never epoch zero or a fabricated sequence time.
    pub async fn record_at(
        &self,
        draft: EntryDraft,
        timestamp_ms: Option<u64>,
    ) -> Result<u64, LedgerError> {
        if !(0.0..=1.0).contains(&draft.confidence) {
            return Err(LedgerError::InvalidConfidence(draft.confidence));
        }
        let vectors = self
            .port
            .embed(vec![draft.text.clone()])
            .await
            .map_err(|error| LedgerError::Embedding(error.to_string()))?;
        if vectors.len() != 1 {
            return Err(LedgerError::EmbeddingArity {
                expected: 1,
                actual: vectors.len(),
            });
        }
        let Some(vector) = vectors.first() else {
            return Err(LedgerError::EmbeddingArity {
                expected: 1,
                actual: 0,
            });
        };
        if vector.len() != self.dimensions {
            return Err(LedgerError::DimensionMismatch {
                expected: self.dimensions,
                actual: vector.len(),
            });
        }
        let _writer = self.writer.lock().expect("ledger writer lock");
        let mut inner = self.inner.load_full().as_ref().clone();
        inner.next_id = inner
            .next_id
            .checked_add(1)
            .ok_or(LedgerError::SequenceExhausted)?;
        let id = inner.next_id;
        Self::reserve_scope(&mut inner, &draft.scope, 1)?;
        // Vector side first: if it refuses, eviction and admission both roll back.
        inner.hnsw.add_point(id, vector)?;
        let entry = LedgerEntry {
            id,
            timestamp_ms,
            scope: draft.scope,
            kind: draft.kind,
            text: draft.text,
            provenance: draft.provenance,
            confidence: draft.confidence,
            key: draft.key,
        };
        if let Some(key) = &entry.key {
            inner
                .exact_index
                .entry((entry.scope.clone(), key.clone()))
                .or_default()
                .push(id);
        }
        inner.entries.insert(id, Arc::new(entry));
        self.inner.store(Arc::new(inner));
        Ok(id)
    }

    /// Routes a query per its shape.
    pub async fn query(&self, query: LedgerQuery) -> Result<Vec<LedgerHit>, LedgerError> {
        match query {
            LedgerQuery::Exact { scope, key } => Ok(self.exact(&scope, &key)),
            LedgerQuery::Filtered { scope, kind } => Ok(self.filtered(&scope, kind)),
            LedgerQuery::Semantic { scope, text, k } => self.semantic(&scope, &text, k).await,
            LedgerQuery::Hybrid {
                scope,
                key,
                text,
                k,
            } => self.hybrid(&scope, key.as_deref(), &text, k).await,
        }
    }

    /// Bounded structured lookup against one immutable generation.
    pub fn select(
        &self,
        scope: &MemoryScope,
        filter: &LedgerFilter,
        limit: usize,
    ) -> Result<Vec<LedgerHit>, LedgerError> {
        self.snapshot().select(scope, filter, limit)
    }

    /// Exact-key lookup inside one scope.
    #[must_use]
    pub fn exact(&self, scope: &MemoryScope, key: &str) -> Vec<LedgerHit> {
        let inner = self.inner.load_full();
        Self::exact_in(&inner, scope, key)
    }

    fn exact_in(inner: &LedgerInner, scope: &MemoryScope, key: &str) -> Vec<LedgerHit> {
        inner
            .exact_index
            .get(&(scope.clone(), key.to_string()))
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| inner.entries.get(id))
                    .map(|entry| LedgerHit {
                        entry: entry.as_ref().clone(),
                        similarity: None,
                        exact: true,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Kind-filtered lookup, newest first, inside one scope.
    #[must_use]
    pub fn filtered(&self, scope: &MemoryScope, kind: EntryKind) -> Vec<LedgerHit> {
        let inner = self.inner.load_full();
        let mut hits: Vec<LedgerHit> = inner
            .entries
            .values()
            .filter(|entry| entry.scope == *scope && entry.kind == kind)
            .map(|entry| LedgerHit {
                entry: entry.as_ref().clone(),
                similarity: None,
                exact: false,
            })
            .collect();
        hits.reverse();
        hits
    }

    /// Semantic search inside one scope: embed, walk the graph, filter by
    /// scope membership, keep those above [`SEMANTIC_THRESHOLD`].
    pub async fn semantic(
        &self,
        scope: &MemoryScope,
        text: &BoundedText,
        k: usize,
    ) -> Result<Vec<LedgerHit>, LedgerError> {
        if k == 0 {
            return Ok(Vec::new());
        }
        let query_vector = self.embed_one(text).await?;
        let inner = self.inner.load_full();
        Ok(self
            .scope_search(&inner, scope, &query_vector, k)
            .into_iter()
            .map(|(entry, similarity)| LedgerHit {
                entry,
                similarity: Some(similarity),
                exact: false,
            })
            .collect())
    }

    /// Semantic retrieval with the same conjunctive predicates as structured
    /// selection. Filtering follows graph traversal, before result truncation;
    /// predicates never remove graph edges. Results are capped at 100.
    pub async fn semantic_filtered(
        &self,
        scope: &MemoryScope,
        text: &BoundedText,
        filter: &LedgerFilter,
        k: usize,
    ) -> Result<Vec<LedgerHit>, LedgerError> {
        filter.validate()?;
        let k = k.min(HYBRID_MAX_RESULTS);
        if k == 0 {
            return Ok(Vec::new());
        }
        let vector = self.embed_one(text).await?;
        let inner = self.inner.load_full();
        Ok(self
            .filtered_search(&inner, scope, &vector, k, filter)
            .into_iter()
            .map(|(entry, similarity)| LedgerHit {
                entry,
                similarity: Some(similarity),
                exact: false,
            })
            .collect())
    }

    /// Hybrid merge: exact hits (if any) plus semantic hits, exact wins
    /// ties, ceiling [`HYBRID_MAX_RESULTS`].
    pub async fn hybrid(
        &self,
        scope: &MemoryScope,
        key: Option<&str>,
        text: &BoundedText,
        k: usize,
    ) -> Result<Vec<LedgerHit>, LedgerError> {
        if k == 0 {
            return Ok(Vec::new());
        }
        let query_vector = self.embed_one(text).await?;
        let inner = self.inner.load_full();
        let mut exact_hits = key
            .map(|key| Self::exact_in(&inner, scope, key))
            .unwrap_or_default();
        let semantic_hits = self.semantic_above_threshold(&inner, scope, &query_vector, k);

        // Merge: exact matches first (they win ties by construction),
        // then semantic hits not already present.
        let mut seen: Vec<u64> = exact_hits.iter().map(|hit| hit.entry.id).collect();
        let mut merged = std::mem::take(&mut exact_hits);
        for hit in semantic_hits {
            if seen.contains(&hit.entry.id) {
                continue;
            }
            seen.push(hit.entry.id);
            merged.push(hit);
        }
        merged.truncate(k.min(HYBRID_MAX_RESULTS));
        Ok(merged)
    }

    fn semantic_above_threshold(
        &self,
        inner: &LedgerInner,
        scope: &MemoryScope,
        query_vector: &[f32],
        k: usize,
    ) -> Vec<LedgerHit> {
        self.scope_search(inner, scope, query_vector, k)
            .into_iter()
            .filter(|(_, similarity)| *similarity >= SEMANTIC_THRESHOLD)
            .map(|(entry, similarity)| LedgerHit {
                entry,
                similarity: Some(similarity),
                exact: false,
            })
            .collect()
    }

    fn scope_search(
        &self,
        inner: &LedgerInner,
        scope: &MemoryScope,
        vector: &[f32],
        k: usize,
    ) -> Vec<(LedgerEntry, f32)> {
        self.filtered_search(inner, scope, vector, k, &LedgerFilter::default())
    }

    fn filtered_search(
        &self,
        inner: &LedgerInner,
        scope: &MemoryScope,
        vector: &[f32],
        k: usize,
        filter: &LedgerFilter,
    ) -> Vec<(LedgerEntry, f32)> {
        inner
            .hnsw
            .search_filtered(vector, k, 64, &|id| {
                inner
                    .entries
                    .get(&id)
                    .is_some_and(|entry| entry.scope == *scope && filter.matches(entry))
            })
            .into_iter()
            .filter(|hit| hit.similarity >= SEMANTIC_THRESHOLD)
            .filter_map(|hit: SearchHit| {
                inner
                    .entries
                    .get(&hit.id)
                    .map(|entry| entry.as_ref().clone())
                    .map(|e| (e, hit.similarity))
            })
            .filter(|(entry, _)| entry.scope == *scope && filter.matches(entry))
            .take(k)
            .collect()
    }

    async fn embed_one(&self, text: &BoundedText) -> Result<Vec<f32>, LedgerError> {
        let vectors = self
            .port
            .embed(vec![text.clone()])
            .await
            .map_err(|error| LedgerError::Embedding(error.to_string()))?;
        if vectors.len() != 1 {
            return Err(LedgerError::EmbeddingArity {
                expected: 1,
                actual: vectors.len(),
            });
        }
        let Some(vector) = vectors.into_iter().next() else {
            return Err(LedgerError::EmbeddingArity {
                expected: 1,
                actual: 0,
            });
        };
        if vector.len() != self.dimensions {
            return Err(LedgerError::DimensionMismatch {
                expected: self.dimensions,
                actual: vector.len(),
            });
        }
        Ok(vector)
    }

    /// Explicit per-scope retention, publishing the log and rebuilt graph in
    /// one transaction. Swarm drops lowest-confidence entries first, oldest
    /// admission first on ties. Private Worker/Task scopes use strict FIFO.
    /// Returns removed IDs in eviction order. Zero clears just this scope.
    /// No embeddings are requested; retained snapshots remain unchanged.
    ///
    /// This is a caller-owned retention operation, not an automatic admission
    /// policy. Rebuilding the graph reclaims capacity without accumulating
    /// tombstones, but its cost remains subject to scale acceptance.
    pub fn prune_scope(&self, scope: &MemoryScope, retain: usize) -> Result<Vec<u64>, LedgerError> {
        let _writer = self.writer.lock().expect("ledger writer lock");
        let mut current = self.inner.load_full().as_ref().clone();
        let removed = Self::prune_staged(&mut current, scope, retain)?;
        if !removed.is_empty() {
            self.inner.store(Arc::new(current));
        }
        Ok(removed)
    }

    fn reserve_scope(
        inner: &mut LedgerInner,
        scope: &MemoryScope,
        incoming: usize,
    ) -> Result<(), LedgerError> {
        if let Some(cap) = inner.retention.cap(scope) {
            if incoming > cap {
                return Err(LedgerError::InvalidRetention);
            }
            Self::prune_staged(inner, scope, cap - incoming)?;
        }
        Ok(())
    }

    fn prune_staged(
        current: &mut LedgerInner,
        scope: &MemoryScope,
        retain: usize,
    ) -> Result<Vec<u64>, LedgerError> {
        let mut candidates: Vec<_> = current
            .entries
            .values()
            .filter(|entry| entry.scope == *scope)
            .collect();
        let remove = candidates.len().saturating_sub(retain);
        if remove == 0 {
            return Ok(Vec::new());
        }
        if *scope == MemoryScope::Swarm {
            candidates.sort_by(|left, right| {
                left.confidence
                    .total_cmp(&right.confidence)
                    .then_with(|| left.id.cmp(&right.id))
            });
        }
        let removed: Vec<_> = candidates
            .into_iter()
            .take(remove)
            .map(|entry| entry.id)
            .collect();
        let ids: std::collections::BTreeSet<_> = removed.iter().copied().collect();
        let mut next = LedgerInner {
            retention: current.retention,
            entries: BTreeMap::new(),
            exact_index: BTreeMap::new(),
            next_id: current.next_id,
            hnsw: HnswIndex::new(current.hnsw.config().clone())?,
        };
        for entry in current
            .entries
            .values()
            .filter(|entry| !ids.contains(&entry.id))
        {
            let vector = current
                .hnsw
                .raw_vector(entry.id)
                .ok_or(LedgerError::UnknownEntry(entry.id))?;
            next.hnsw.add_point(entry.id, vector)?;
            if let Some(key) = &entry.key {
                next.exact_index
                    .entry((entry.scope.clone(), key.clone()))
                    .or_default()
                    .push(entry.id);
            }
            next.entries.insert(entry.id, entry.clone());
        }
        *current = next;
        Ok(removed)
    }

    /// Bounded knowledge transfer between scopes.
    ///
    /// Copies (never moves) the given entry ids from `source` to `dest`:
    ///
    /// - both scopes must exist as distinct destinations;
    /// - at most [`TRANSFER_CAP`] entries per call;
    /// - only entries with `confidence >= 0.8` are copied; the rest are
    ///   **dropped and reported**, never silently copied;
    /// - the copy is a fresh entry id whose provenance is the original's,
    ///   verbatim — worker, role, task, and sequence never change.
    ///
    /// Returns `(copied_ids, dropped_count)`.
    pub async fn transfer(
        &self,
        source: &MemoryScope,
        dest: &MemoryScope,
        entry_ids: &[u64],
    ) -> Result<(Vec<u64>, usize), LedgerError> {
        self.transfer_filtered(source, dest, entry_ids, &LedgerFilter::default())
            .await
    }

    /// Transactional selective transfer. Explicit IDs are always validated for
    /// source membership, even when filtered out. Category/provenance rejects
    /// count as dropped; filters cannot lower the mandatory confidence floor.
    pub async fn transfer_filtered(
        &self,
        source: &MemoryScope,
        dest: &MemoryScope,
        entry_ids: &[u64],
        filter: &LedgerFilter,
    ) -> Result<(Vec<u64>, usize), LedgerError> {
        filter.validate()?;
        if source == dest {
            return Err(LedgerError::TransferSameScope);
        }
        if entry_ids.len() > TRANSFER_CAP {
            return Err(LedgerError::TransferCap(entry_ids.len()));
        }
        let _writer = self.writer.lock().expect("ledger writer lock");
        let mut inner = self.inner.load_full().as_ref().clone();
        let mut admitted = 0;
        for id in entry_ids {
            let entry = inner
                .entries
                .get(id)
                .ok_or(LedgerError::UnknownEntry(*id))?;
            if entry.scope != *source {
                return Err(LedgerError::TransferRefused(
                    "entry does not belong to the source scope",
                ));
            }
            if entry.confidence >= TRANSFER_CONFIDENCE_FLOOR && filter.matches(entry) {
                admitted += 1;
            }
        }
        Self::reserve_scope(&mut inner, dest, admitted)?;
        let mut copied = Vec::with_capacity(entry_ids.len());
        let mut dropped = 0usize;
        for id in entry_ids {
            let entry = inner
                .entries
                .get(id)
                .map(|entry| entry.as_ref().clone())
                .ok_or(LedgerError::UnknownEntry(*id))?;
            if entry.scope != *source {
                return Err(LedgerError::TransferRefused(
                    "entry does not belong to the source scope",
                ));
            }
            if entry.confidence < TRANSFER_CONFIDENCE_FLOOR || !filter.matches(&entry) {
                dropped += 1;
                continue;
            }
            // Reuse the original embedding rather than making another provider
            // call. Stage the complete transfer before one atomic publication.
            let vector = inner
                .hnsw
                .raw_vector(*id)
                .ok_or(LedgerError::UnknownEntry(*id))?
                .to_vec();
            inner.next_id = inner
                .next_id
                .checked_add(1)
                .ok_or(LedgerError::SequenceExhausted)?;
            let fresh = inner.next_id;
            inner.hnsw.add_point(fresh, &vector)?;
            let entry = LedgerEntry {
                id: fresh,
                scope: dest.clone(),
                ..entry
            };
            if let Some(key) = &entry.key {
                inner
                    .exact_index
                    .entry((dest.clone(), key.clone()))
                    .or_default()
                    .push(fresh);
            }
            inner.entries.insert(fresh, Arc::new(entry));
            copied.push(fresh);
        }
        self.inner.store(Arc::new(inner));
        Ok((copied, dropped))
    }
}
