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
//! not timestamps) and no randomness.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

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

/// Interior ledger state.
#[derive(Debug)]
struct LedgerInner {
    entries: BTreeMap<u64, LedgerEntry>,
    /// (scope, key) → entry ids, for exact routing.
    exact_index: BTreeMap<(MemoryScope, String), Vec<u64>>,
    next_id: u64,
    hnsw: HnswIndex,
}

/// The hybrid ledger.
///
/// Clone shares state (`Arc` inside). All operations are synchronous and
/// non-blocking except [`record`](Self::record) and semantic/hybrid
/// queries, which await the [`EmbeddingPort`] once per call.
#[derive(Clone)]
pub struct Ledger {
    dimensions: usize,
    port: Arc<dyn EmbeddingPort>,
    inner: Arc<std::sync::Mutex<LedgerInner>>,
}

impl std::fmt::Debug for Ledger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().expect("ledger lock");
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
        Ok(Self {
            dimensions: config.dimensions,
            port,
            inner: Arc::new(std::sync::Mutex::new(LedgerInner {
                entries: BTreeMap::new(),
                exact_index: BTreeMap::new(),
                next_id: 0,
                hnsw: HnswIndex::new(config)?,
            })),
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
        self.inner.lock().expect("ledger lock").entries.len()
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
        if !(0.0..=1.0).contains(&draft.confidence) {
            return Err(LedgerError::InvalidConfidence(draft.confidence));
        }
        let vectors = self
            .port
            .embed(vec![draft.text.clone()])
            .await
            .map_err(|error| LedgerError::Embedding(error.to_string()))?;
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
        let mut inner = self.inner.lock().expect("ledger lock");
        inner.next_id += 1;
        let id = inner.next_id;
        // Vector side first: if it refuses, nothing is stored.
        inner.hnsw.add_point(id, vector)?;
        let entry = LedgerEntry {
            id,
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
        inner.entries.insert(id, entry);
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

    /// Exact-key lookup inside one scope.
    #[must_use]
    pub fn exact(&self, scope: &MemoryScope, key: &str) -> Vec<LedgerHit> {
        let inner = self.inner.lock().expect("ledger lock");
        inner
            .exact_index
            .get(&(scope.clone(), key.to_string()))
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| inner.entries.get(id))
                    .map(|entry| LedgerHit {
                        entry: entry.clone(),
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
        let inner = self.inner.lock().expect("ledger lock");
        let mut hits: Vec<LedgerHit> = inner
            .entries
            .values()
            .filter(|entry| entry.scope == *scope && entry.kind == kind)
            .map(|entry| LedgerHit {
                entry: entry.clone(),
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
        let inner = self.inner.lock().expect("ledger lock");
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
        let mut exact_hits = key.map(|key| self.exact(scope, key)).unwrap_or_default();
        let semantic_hits = self.semantic_above_threshold(scope, text, k).await?;

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

    async fn semantic_above_threshold(
        &self,
        scope: &MemoryScope,
        text: &BoundedText,
        k: usize,
    ) -> Result<Vec<LedgerHit>, LedgerError> {
        let query_vector = self.embed_one(text).await?;
        let inner = self.inner.lock().expect("ledger lock");
        Ok(self
            .scope_search(&inner, scope, &query_vector, k)
            .into_iter()
            .filter(|(_, similarity)| *similarity >= SEMANTIC_THRESHOLD)
            .map(|(entry, similarity)| LedgerHit {
                entry,
                similarity: Some(similarity),
                exact: false,
            })
            .collect())
    }

    fn scope_search(
        &self,
        inner: &LedgerInner,
        scope: &MemoryScope,
        vector: &[f32],
        k: usize,
    ) -> Vec<(LedgerEntry, f32)> {
        let fetch = k
            .saturating_mul(4)
            .saturating_mul(HYBRID_MAX_RESULTS)
            .max(k);
        inner
            .hnsw
            .search(vector, fetch, 64)
            .into_iter()
            .filter_map(|hit: SearchHit| {
                inner
                    .entries
                    .get(&hit.id)
                    .cloned()
                    .map(|e| (e, hit.similarity))
            })
            .filter(|(entry, _)| entry.scope == *scope)
            .take(k)
            .collect()
    }

    async fn embed_one(&self, text: &BoundedText) -> Result<Vec<f32>, LedgerError> {
        let vectors = self
            .port
            .embed(vec![text.clone()])
            .await
            .map_err(|error| LedgerError::Embedding(error.to_string()))?;
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
        if source == dest {
            return Err(LedgerError::TransferSameScope);
        }
        if entry_ids.len() > TRANSFER_CAP {
            return Err(LedgerError::TransferCap(entry_ids.len()));
        }
        let mut copied = Vec::with_capacity(entry_ids.len());
        let mut dropped = 0usize;
        for id in entry_ids {
            let entry = {
                let inner = self.inner.lock().expect("ledger lock");
                inner.entries.get(id).cloned()
            };
            let Some(entry) = entry else {
                return Err(LedgerError::UnknownEntry(*id));
            };
            if entry.scope != *source {
                return Err(LedgerError::TransferRefused(
                    "entry does not belong to the source scope",
                ));
            }
            if entry.confidence < TRANSFER_CONFIDENCE_FLOOR {
                dropped += 1;
                continue;
            }
            // Copy: fresh id, original provenance verbatim, dest scope.
            let new_id = self
                .record(EntryDraft {
                    scope: dest.clone(),
                    kind: entry.kind,
                    text: entry.text.clone(),
                    provenance: entry.provenance.clone(),
                    confidence: entry.confidence,
                    key: entry.key.clone(),
                })
                .await?;
            copied.push(new_id);
        }
        Ok((copied, dropped))
    }
}
