//! HNSW approximate-nearest-neighbor index (VRO-15 PR-6).
//!
//! A pure-Rust port of the swarm oracle's vector index, adapted for the
//! swarm ledger: caller-supplied stable ids, runtime-configured
//! dimensions, deterministic construction via an injectable seed, and a
//! versioned binary snapshot with fail-closed loading.
//!
//! Defaults follow the oracle: `m = 16`, `ef_construction = 200`,
//! `max_elements = 1_000_000`, cosine similarity. The level draw is
//! geometric with `p = 0.5`, capped at 16 layers, driven by an inline
//! xorshift64* PRNG seeded per index — the same insertion sequence under
//! the same seed always produces a byte-identical graph.
//!
//! Anti-duplication note (VRO-15 PR-6 audit): `vesper-cognition` owns the
//! workspace's semantic-search cosine helper, but the architecture
//! allowlist keeps `vesper-swarm` independent of that crate, so this
//! module keeps a **private** distance helper rather than exporting a
//! second public cosine.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Snapshot format magic bytes (`VSWHNSW1`).
const SNAPSHOT_MAGIC: [u8; 8] = *b"VSWHNSW1";
/// Snapshot format version.
const SNAPSHOT_VERSION: u32 = 2;
/// Geometric level-draw success probability.
const LEVEL_P: f64 = 0.5;
/// Maximum layer height any node may reach.
const MAX_LEVEL: usize = 16;

/// Index construction and search parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HnswConfig {
    /// Embedding dimension; every vector must match it exactly.
    pub dimensions: usize,
    /// Bi-directional links created per layer during insert.
    pub m: usize,
    /// Candidate list size during construction search.
    pub ef_construction: usize,
    /// Hard cap on stored elements.
    pub max_elements: usize,
    /// Seed for the deterministic level draws.
    pub seed: u64,
    /// Over-fetch multiplier for filtered searches (`k * factor`
    /// candidates gathered before the predicate is applied).
    pub over_fetch_factor: usize,
}

impl HnswConfig {
    /// The oracle defaults for the given runtime dimension.
    #[must_use]
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            m: 16,
            ef_construction: 200,
            max_elements: 1_000_000,
            seed: 0x5eed_0000_0000_0001,
            over_fetch_factor: 10,
        }
    }

    /// Overrides the deterministic seed.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Fails closed on impossible shapes.
    pub fn validate(&self) -> Result<(), HnswConfigError> {
        if [
            self.dimensions,
            self.ef_construction,
            self.max_elements,
            self.over_fetch_factor,
        ]
        .iter()
        .any(|value| *value > u32::MAX as usize)
            || self.m > (u32::MAX / 2) as usize
        {
            return Err(HnswConfigError::ShapeTooLarge);
        }
        if self.dimensions == 0 {
            return Err(HnswConfigError::ZeroDimensions);
        }
        if self.m == 0 {
            return Err(HnswConfigError::ZeroM);
        }
        if self.ef_construction == 0 {
            return Err(HnswConfigError::ZeroEfConstruction);
        }
        if self.max_elements == 0 {
            return Err(HnswConfigError::ZeroMaxElements);
        }
        if self.over_fetch_factor == 0 {
            return Err(HnswConfigError::ZeroOverFetch);
        }
        Ok(())
    }
}

/// Rejections of an [`HnswConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HnswConfigError {
    /// Configuration must fit the portable binary format and link arithmetic.
    #[error("configuration exceeds portable snapshot bounds")]
    ShapeTooLarge,
    /// Dimensions must be positive.
    #[error("dimensions must be at least one")]
    ZeroDimensions,
    /// `m` must be positive.
    #[error("m must be at least one")]
    ZeroM,
    /// `ef_construction` must be positive.
    #[error("ef_construction must be at least one")]
    ZeroEfConstruction,
    /// `max_elements` must be positive.
    #[error("max_elements must be at least one")]
    ZeroMaxElements,
    /// `over_fetch_factor` must be positive.
    #[error("over_fetch_factor must be at least one")]
    ZeroOverFetch,
}

/// Errors surfaced by the index.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HnswError {
    /// The config is invalid.
    #[error("invalid hnsw config: {0}")]
    InvalidConfig(HnswConfigError),
    /// The vector's dimension disagrees with the configured one.
    #[error("dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// Configured dimension.
        expected: usize,
        /// Supplied vector length.
        actual: usize,
    },
    /// Input vectors must contain only finite values.
    #[error("vector contains nonfinite values")]
    NonfiniteVector,
    /// A point with this id already exists.
    #[error("point {0} already exists")]
    DuplicatePoint(u64),
    /// The index is at `max_elements`.
    #[error("index at capacity ({0})")]
    AtCapacity(usize),
    /// The snapshot failed validation.
    #[error("invalid snapshot: {0}")]
    InvalidSnapshot(&'static str),
}

/// One search hit: point id and cosine similarity to the query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchHit {
    /// The caller-supplied point id.
    pub id: u64,
    /// Cosine similarity in `-1.0..=1.0` (higher is closer).
    pub similarity: f32,
}

/// One stored point.
#[derive(Debug, Clone)]
struct Node {
    id: u64,
    /// Pre-normalized vector (cosine similarity is then a plain dot).
    normalized: Vec<f32>,
    /// Original finite input, preserved losslessly in snapshots.
    raw: Vec<f32>,
    /// Highest layer this node participates in.
    level: usize,
    /// Neighbor ordinals per layer (index 0 = ground layer).
    connections: Vec<Vec<usize>>,
}

/// Deterministic xorshift64* PRNG for level draws.
#[derive(Debug, Clone)]
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        // Zero state is a fixed point; mix to avoid it.
        Self {
            state: match seed ^ 0x9e37_79b9_7f4a_7c15 {
                0 => 1,
                mixed => mixed,
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn next_f64(&mut self) -> f64 {
        // 53-bit precision uniform in [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Geometric level draw: consecutive successes under `LEVEL_P`,
    /// capped at [`MAX_LEVEL`].
    fn draw_level(&mut self) -> usize {
        let mut level = 0;
        while level < MAX_LEVEL && self.next_f64() < LEVEL_P {
            level += 1;
        }
        level
    }
}

/// Totally-ordered distance key for heap ordering: f32 bits with the
/// sign flipped, so unsigned ordering matches float ordering and no
/// `Ord` bound on f32 is needed. Private, like all math helpers here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DistKey(u32);

impl DistKey {
    fn from_f32(value: f32) -> Self {
        let bits = value.to_bits();
        Self(if bits & 0x8000_0000 != 0 {
            !bits
        } else {
            bits ^ 0x8000_0000
        })
    }

    fn to_f32(self) -> f32 {
        let bits = self.0;
        f32::from_bits(if bits & 0x8000_0000 != 0 {
            bits ^ 0x8000_0000
        } else {
            !bits
        })
    }
}

/// Private cosine helpers (see module docs: deliberately not public).
fn normalize(vector: &[f32]) -> Vec<f32> {
    let magnitude = vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if magnitude == 0.0 {
        return vec![0.0; vector.len()];
    }
    vector
        .iter()
        .map(|value| (f64::from(*value) / magnitude) as f32)
        .collect()
}

fn cosine_similarity_normalized(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The HNSW index.
#[derive(Debug, Clone)]
pub struct HnswIndex {
    config: HnswConfig,
    nodes: Vec<Arc<Node>>,
    /// id → ordinal for duplicate detection.
    ordinals: std::collections::HashMap<u64, usize>,
    entry: Option<usize>,
    max_level: usize,
    rng: XorShift64,
    /// Neighbor cap at layer 0 (2m) and above (m), standard HNSW.
    m_max0: usize,
}

impl HnswIndex {
    /// Creates an empty index under a validated config.
    pub fn new(config: HnswConfig) -> Result<Self, HnswError> {
        config.validate().map_err(HnswError::InvalidConfig)?;
        let m_max0 = config.m.saturating_mul(2);
        Ok(Self {
            rng: XorShift64::new(config.seed),
            config,
            nodes: Vec::new(),
            ordinals: std::collections::HashMap::new(),
            entry: None,
            max_level: 0,
            m_max0,
        })
    }

    /// The enforced configuration.
    #[must_use]
    pub fn config(&self) -> &HnswConfig {
        &self.config
    }

    /// Number of stored points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the index stores nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Original vector for a known point, for lossless ledger transfers.
    #[must_use]
    pub fn raw_vector(&self, id: u64) -> Option<&[f32]> {
        self.ordinals
            .get(&id)
            .map(|ordinal| self.nodes[*ordinal].raw.as_slice())
    }

    /// Inserts one point with a caller-supplied stable id.
    ///
    /// Deterministic: same insertion sequence + same seed ⇒ identical
    /// graph ⇒ identical snapshot bytes.
    pub fn add_point(&mut self, id: u64, vector: &[f32]) -> Result<(), HnswError> {
        if vector.len() != self.config.dimensions {
            return Err(HnswError::DimensionMismatch {
                expected: self.config.dimensions,
                actual: vector.len(),
            });
        }
        if vector.iter().any(|value| !value.is_finite()) {
            return Err(HnswError::NonfiniteVector);
        }
        if self.ordinals.contains_key(&id) {
            return Err(HnswError::DuplicatePoint(id));
        }
        if self.nodes.len() >= self.config.max_elements {
            return Err(HnswError::AtCapacity(self.config.max_elements));
        }
        let level = self.rng.draw_level();
        let ordinal = self.nodes.len();
        let normalized = normalize(vector);

        if self.entry.is_none() {
            // First point seeds the graph and is stored immediately.
            self.nodes.push(Arc::new(Node {
                id,
                normalized,
                raw: vector.to_vec(),
                level,
                connections: vec![Vec::new(); level + 1],
            }));
            self.ordinals.insert(id, ordinal);
            self.entry = Some(ordinal);
            self.max_level = level;
            return Ok(());
        }

        // Store the node FIRST so every later link references a valid
        // ordinal; its per-layer connections are filled in below.
        self.nodes.push(Arc::new(Node {
            id,
            normalized,
            raw: vector.to_vec(),
            level,
            connections: vec![Vec::new(); level + 1],
        }));
        self.ordinals.insert(id, ordinal);

        let entry = self.entry.expect("non-empty index has an entry");
        let query = self.nodes[ordinal].normalized.clone();
        // Greedy descent through layers above the new node's level.
        let mut current = entry;
        if self.max_level > level {
            for layer in ((level + 1)..=self.max_level).rev() {
                current = self.greedy_step(current, &query, layer);
            }
        }
        // Connect from the node's top layer down to the ground.
        for layer in (0..=level.min(self.max_level)).rev() {
            let ef = self.config.ef_construction.max(self.config.m);
            let candidates = self.search_layer(current, &query, ef, layer);
            let width = if layer == 0 {
                self.m_max0
            } else {
                self.config.m
            };
            let selected = self.select_neighbors(&candidates, width);
            Arc::make_mut(&mut self.nodes[ordinal]).connections[layer] = selected.clone();
            for neighbor in selected {
                let cap = if layer == 0 {
                    self.m_max0
                } else {
                    self.config.m
                };
                self.link(neighbor, ordinal, layer, cap);
            }
            if let Some((_, closest)) = candidates.first() {
                current = *closest;
            }
        }
        if level > self.max_level {
            self.max_level = level;
            self.entry = Some(ordinal);
        }
        Ok(())
    }

    fn distance_to(&self, ordinal: usize, query: &[f32]) -> f32 {
        1.0 - cosine_similarity_normalized(&self.nodes[ordinal].normalized, query)
    }

    /// One greedy step within a layer: move to the closest neighbor that
    /// improves on the current node.
    fn greedy_step(&self, start: usize, query: &[f32], layer: usize) -> usize {
        let mut current = start;
        let mut current_distance = self.distance_to(current, query);
        loop {
            let mut improved = false;
            for &neighbor in &self.nodes[current].connections[layer] {
                let distance = self.distance_to(neighbor, query);
                if distance < current_distance {
                    current_distance = distance;
                    current = neighbor;
                    improved = true;
                }
            }
            if !improved {
                return current;
            }
        }
    }

    /// Best-first layer search returning up to `ef` (distance, ordinal)
    /// pairs ordered closest first.
    fn search_layer(
        &self,
        entry: usize,
        query: &[f32],
        ef: usize,
        layer: usize,
    ) -> Vec<(f32, usize)> {
        // Sparse per-search membership: allocation follows visited vertices,
        // not the full million-point capacity on every traversed layer.
        let mut visited = std::collections::BTreeSet::new();
        // Min-heap of candidates to expand (closest on top).
        let mut candidates: std::collections::BinaryHeap<std::cmp::Reverse<(DistKey, usize)>> =
            std::collections::BinaryHeap::new();
        // Max-heap of the current best set (worst on top).
        let mut best: std::collections::BinaryHeap<(DistKey, usize)> =
            std::collections::BinaryHeap::new();

        let entry_distance = self.distance_to(entry, query);
        visited.insert(entry);
        candidates.push(std::cmp::Reverse((
            DistKey::from_f32(entry_distance),
            entry,
        )));
        best.push((DistKey::from_f32(entry_distance), entry));

        while let Some(std::cmp::Reverse((key, ordinal))) = candidates.pop() {
            // Stop when the closest candidate is worse than the worst of
            // the full best set.
            if best.len() >= ef
                && let Some(&(worst, _)) = best.peek()
                && key > worst
            {
                break;
            }
            for &neighbor in &self.nodes[ordinal].connections[layer] {
                if !visited.insert(neighbor) {
                    continue;
                }
                let neighbor_key = DistKey::from_f32(self.distance_to(neighbor, query));
                let worst = best
                    .peek()
                    .map_or(DistKey::from_f32(f32::MAX), |&(worst, _)| worst);
                if best.len() < ef || neighbor_key < worst {
                    candidates.push(std::cmp::Reverse((neighbor_key, neighbor)));
                    best.push((neighbor_key, neighbor));
                    if best.len() > ef {
                        best.pop();
                    }
                }
            }
        }
        // into_sorted_vec yields ascending key order: closest first.
        best.into_sorted_vec()
            .into_iter()
            .map(|(key, ordinal)| (key.to_f32(), ordinal))
            .collect()
    }

    /// Selects up to `m` closest neighbors from candidates.
    fn select_neighbors(&self, candidates: &[(f32, usize)], m: usize) -> Vec<usize> {
        candidates
            .iter()
            .take(m)
            .map(|(_, ordinal)| *ordinal)
            .collect()
    }

    /// Adds a bidirectional link, pruning the far side to `cap` neighbors.
    fn link(&mut self, from: usize, to: usize, layer: usize, cap: usize) {
        let query = self.nodes[from].normalized.clone();
        let mut current =
            std::mem::take(&mut Arc::make_mut(&mut self.nodes[from]).connections[layer]);
        if !current.contains(&to) {
            current.push(to);
        }
        if current.len() > cap {
            // Prune against the owner of the adjacency list, not the incoming node.
            let mut scored: Vec<(DistKey, usize)> = current
                .iter()
                .map(|&ordinal| {
                    (
                        DistKey::from_f32(
                            1.0 - cosine_similarity_normalized(
                                &self.nodes[ordinal].normalized,
                                &query,
                            ),
                        ),
                        ordinal,
                    )
                })
                .collect();
            scored.sort();
            current = scored
                .into_iter()
                .take(cap)
                .map(|(_, ordinal)| ordinal)
                .collect();
        }
        Arc::make_mut(&mut self.nodes[from]).connections[layer] = current;
    }

    /// Approximate nearest-neighbor search: top-`k` by cosine similarity.
    #[must_use]
    pub fn search(&self, query: &[f32], k: usize, ef: usize) -> Vec<SearchHit> {
        if query.len() != self.config.dimensions
            || self.entry.is_none()
            || k == 0
            || query.iter().any(|value| !value.is_finite())
        {
            return Vec::new();
        }
        let normalized_query = normalize(query);
        let entry = self.entry.expect("checked above");
        let mut current = entry;
        for layer in (1..=self.max_level).rev() {
            current = self.greedy_step(current, &normalized_query, layer);
        }
        let ef = ef.max(k);
        let candidates = self.search_layer(current, &normalized_query, ef, 0);
        candidates
            .into_iter()
            .take(k)
            .map(|(distance, ordinal)| SearchHit {
                id: self.nodes[ordinal].id,
                similarity: 1.0 - distance,
            })
            .collect()
    }

    /// Filtered search: gathers `k * over_fetch_factor` candidates, then
    /// keeps the closest `k` satisfying the predicate. Over-fetching
    /// keeps graph traversal intact — the predicate never guides the
    /// walk, so excluded regions cannot strand reachable neighborhoods.
    #[must_use]
    pub fn search_filtered(
        &self,
        query: &[f32],
        k: usize,
        ef: usize,
        predicate: &dyn Fn(u64) -> bool,
    ) -> Vec<SearchHit> {
        let fetch = k.saturating_mul(self.config.over_fetch_factor).max(k);
        self.search(query, fetch, ef)
            .into_iter()
            .filter(|hit| predicate(hit.id))
            .take(k)
            .collect()
    }

    /// Serializes the index into the versioned binary snapshot format.
    ///
    /// Layout (all integers little-endian):
    /// magic `VSWHNSW1` · version u32 · dimensions u32 · m u32 ·
    /// ef_construction u32 · max_elements u32 · seed u64 · count u32 ·
    /// entry u32 · max_level u32 · over_fetch_factor u32 · RNG state u64 ·
    /// per node (id u64, level u32, per-layer
    /// neighbor counts and ordinals u32) · raw vectors (count × dims × f32).
    /// Version 1 lacks raw vectors and continuation state and is refused.
    #[must_use]
    pub fn to_snapshot(&self) -> Vec<u8> {
        self.encode_snapshot(Vec::new())
    }

    /// Refuses oversized snapshots before allocating their output buffer.
    pub fn to_snapshot_bounded(&self, limit: usize) -> Result<Vec<u8>, HnswError> {
        let mut length = 60usize;
        for node in &self.nodes {
            length = length
                .checked_add(16)
                .and_then(|n| node.raw.len().checked_mul(4).and_then(|v| n.checked_add(v)))
                .ok_or(HnswError::InvalidSnapshot("snapshot size overflow"))?;
            for layer in &node.connections {
                length = layer
                    .len()
                    .checked_mul(4)
                    .and_then(|n| n.checked_add(4))
                    .and_then(|n| length.checked_add(n))
                    .ok_or(HnswError::InvalidSnapshot("snapshot size overflow"))?;
            }
            if length > limit {
                return Err(HnswError::InvalidSnapshot("snapshot byte limit exceeded"));
            }
        }
        if length > limit {
            return Err(HnswError::InvalidSnapshot("snapshot byte limit exceeded"));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(length)
            .map_err(|_| HnswError::InvalidSnapshot("snapshot allocation refused"))?;
        Ok(self.encode_snapshot(out))
    }

    fn encode_snapshot(&self, mut out: Vec<u8>) -> Vec<u8> {
        out.extend_from_slice(&SNAPSHOT_MAGIC);
        out.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.config.dimensions as u32).to_le_bytes());
        out.extend_from_slice(&(self.config.m as u32).to_le_bytes());
        out.extend_from_slice(&(self.config.ef_construction as u32).to_le_bytes());
        out.extend_from_slice(&(self.config.max_elements as u32).to_le_bytes());
        out.extend_from_slice(&self.config.seed.to_le_bytes());
        out.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.entry.map_or(u32::MAX, |ord| ord as u32)).to_le_bytes());
        out.extend_from_slice(&(self.max_level as u32).to_le_bytes());
        out.extend_from_slice(&(self.config.over_fetch_factor as u32).to_le_bytes());
        out.extend_from_slice(&self.rng.state.to_le_bytes());
        for node in &self.nodes {
            out.extend_from_slice(&node.id.to_le_bytes());
            out.extend_from_slice(&(node.level as u32).to_le_bytes());
            out.extend_from_slice(&(node.connections.len() as u32).to_le_bytes());
            for layer in &node.connections {
                out.extend_from_slice(&(layer.len() as u32).to_le_bytes());
                for &ordinal in layer {
                    out.extend_from_slice(&(ordinal as u32).to_le_bytes());
                }
            }
        }
        for node in &self.nodes {
            for value in &node.raw {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out
    }

    /// Rebuilds an index from a snapshot, failing closed on any
    /// inconsistency: bad magic, unsupported version, header fields
    /// disagreeing with `expected`, truncation, or trailing bytes.
    pub fn from_snapshot(expected: &HnswConfig, bytes: &[u8]) -> Result<Self, HnswError> {
        fn take_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, HnswError> {
            let end = *offset + 4;
            if end > bytes.len() {
                return Err(HnswError::InvalidSnapshot("truncated header"));
            }
            let value = u32::from_le_bytes(bytes[*offset..end].try_into().expect("four bytes"));
            *offset = end;
            Ok(value)
        }
        fn take_u64(bytes: &[u8], offset: &mut usize) -> Result<u64, HnswError> {
            let mut buffer = [0u8; 8];
            let end = *offset + 8;
            if end > bytes.len() {
                return Err(HnswError::InvalidSnapshot("truncated header"));
            }
            buffer.copy_from_slice(&bytes[*offset..end]);
            *offset = end;
            Ok(u64::from_le_bytes(buffer))
        }

        if bytes.len() < SNAPSHOT_MAGIC.len() || bytes[..8] != SNAPSHOT_MAGIC {
            return Err(HnswError::InvalidSnapshot("bad magic"));
        }
        let mut offset = 8usize;
        let version = take_u32(bytes, &mut offset)?;
        if version != SNAPSHOT_VERSION {
            return Err(HnswError::InvalidSnapshot("unsupported version"));
        }
        let dimensions = take_u32(bytes, &mut offset)? as usize;
        let m = take_u32(bytes, &mut offset)? as usize;
        let ef_construction = take_u32(bytes, &mut offset)? as usize;
        let max_elements = take_u32(bytes, &mut offset)? as usize;
        let seed = take_u64(bytes, &mut offset)?;
        let count = take_u32(bytes, &mut offset)? as usize;
        let entry_raw = take_u32(bytes, &mut offset)?;
        let max_level = take_u32(bytes, &mut offset)? as usize;
        let over_fetch_factor = take_u32(bytes, &mut offset)? as usize;
        let rng_state = take_u64(bytes, &mut offset)?;
        if rng_state == 0 {
            return Err(HnswError::InvalidSnapshot("zero RNG state"));
        }

        // Header must agree with the expected configuration.
        if dimensions != expected.dimensions
            || m != expected.m
            || ef_construction != expected.ef_construction
            || max_elements != expected.max_elements
            || seed != expected.seed
            || over_fetch_factor != expected.over_fetch_factor
        {
            return Err(HnswError::InvalidSnapshot("header disagrees with config"));
        }
        if count > max_elements {
            return Err(HnswError::InvalidSnapshot("count exceeds capacity"));
        }

        // Each node needs at least its id/shape, one layer count and vector.
        // Bound allocations by actual bytes, not only an attacker supplied count.
        let minimum_bytes = dimensions
            .checked_mul(4)
            .and_then(|size| size.checked_add(20))
            .and_then(|size| size.checked_mul(count))
            .ok_or(HnswError::InvalidSnapshot("record size overflow"))?;
        if minimum_bytes > bytes.len().saturating_sub(offset) {
            return Err(HnswError::InvalidSnapshot("records exceed input budget"));
        }
        let mut ids = std::collections::HashSet::new();
        // Node records.
        let mut records: Vec<(u64, u32, Vec<Vec<usize>>)> = Vec::with_capacity(count);
        for record_ordinal in 0..count {
            let id = take_u64(bytes, &mut offset)?;
            if !ids.insert(id) {
                return Err(HnswError::InvalidSnapshot("duplicate point id"));
            }
            let level = take_u32(bytes, &mut offset)? as usize;
            let layers = take_u32(bytes, &mut offset)? as usize;
            if level > MAX_LEVEL || layers != level + 1 {
                return Err(HnswError::InvalidSnapshot("node layer mismatch"));
            }
            let mut connections = Vec::with_capacity(layers);
            for _ in 0..layers {
                let neighbors = take_u32(bytes, &mut offset)? as usize;
                if neighbors > m.saturating_mul(2) {
                    return Err(HnswError::InvalidSnapshot("neighbor overflow"));
                }
                if neighbors > bytes.len().saturating_sub(offset) / 4 {
                    return Err(HnswError::InvalidSnapshot("neighbors exceed input budget"));
                }
                let mut layer = Vec::with_capacity(neighbors);
                for _ in 0..neighbors {
                    let ordinal = take_u32(bytes, &mut offset)? as usize;
                    if ordinal >= count {
                        return Err(HnswError::InvalidSnapshot("neighbor ordinal out of range"));
                    }
                    if ordinal == record_ordinal || layer.contains(&ordinal) {
                        return Err(HnswError::InvalidSnapshot("self or duplicate edge"));
                    }
                    layer.push(ordinal);
                }
                connections.push(layer);
            }
            records.push((id, level as u32, connections));
        }
        // Vectors.
        let vector_bytes = count
            .checked_mul(dimensions)
            .and_then(|size| size.checked_mul(4))
            .ok_or(HnswError::InvalidSnapshot("vector size overflow"))?;
        if offset.checked_add(vector_bytes) != Some(bytes.len()) {
            return Err(HnswError::InvalidSnapshot("vector section length mismatch"));
        }
        let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(count);
        for _ in 0..count {
            let mut vector = Vec::with_capacity(dimensions);
            for _ in 0..dimensions {
                let mut buffer = [0u8; 4];
                buffer.copy_from_slice(
                    bytes
                        .get(offset..offset + 4)
                        .ok_or(HnswError::InvalidSnapshot("truncated vectors"))?,
                );
                offset += 4;
                let value = f32::from_le_bytes(buffer);
                if !value.is_finite() {
                    return Err(HnswError::InvalidSnapshot("nonfinite vector"));
                }
                vector.push(value);
            }
            vectors.push(vector);
        }

        let entry = if entry_raw == u32::MAX {
            None
        } else if (entry_raw as usize) < count {
            Some(entry_raw as usize)
        } else {
            return Err(HnswError::InvalidSnapshot("entry out of range"));
        };
        if count > 0 && entry.is_none() {
            return Err(HnswError::InvalidSnapshot("non-empty index without entry"));
        }
        if count == 0 && entry.is_some() {
            return Err(HnswError::InvalidSnapshot("empty index with entry"));
        }
        if max_level > MAX_LEVEL {
            return Err(HnswError::InvalidSnapshot("max level out of range"));
        }
        let graph_level = records
            .iter()
            .map(|(_, level, _)| *level as usize)
            .max()
            .unwrap_or(0);
        if max_level != graph_level
            || entry.is_some_and(|ordinal| records[ordinal].1 as usize != max_level)
        {
            return Err(HnswError::InvalidSnapshot("entry/header layer mismatch"));
        }
        for (_, _, connections) in &records {
            for (layer, neighbors) in connections.iter().enumerate() {
                if neighbors
                    .iter()
                    .any(|ordinal| records[*ordinal].1 < layer as u32)
                {
                    return Err(HnswError::InvalidSnapshot("edge targets missing layer"));
                }
            }
        }

        let mut index = Self::new(HnswConfig {
            dimensions,
            m,
            ef_construction,
            max_elements,
            seed,
            over_fetch_factor,
        })?;
        index.rng.state = rng_state;
        index.max_level = max_level;
        index.entry = entry;
        let mut ordinals = std::collections::HashMap::with_capacity(count);
        for ((id, level, connections), vector) in records.into_iter().zip(vectors) {
            let ordinal = index.nodes.len();
            ordinals.insert(id, ordinal);
            index.nodes.push(Arc::new(Node {
                id,
                normalized: normalize(&vector),
                raw: vector,
                level: level as usize,
                connections,
            }));
        }
        index.ordinals = ordinals;
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(dims: usize, seed: u64) -> HnswIndex {
        HnswIndex::new(HnswConfig::new(dims).with_seed(seed)).expect("valid config")
    }

    fn point(i: u64, dims: usize) -> Vec<f32> {
        // Deterministic pseudo-random vector per id.
        let mut state = i.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        (0..dims)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                ((state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / 1_048_576.0) - 0.5
            })
            .collect()
    }

    #[test]
    fn config_validation_rejects_zero_shapes() {
        for error in [
            HnswConfig {
                dimensions: 0,
                ..HnswConfig::new(4)
            }
            .validate()
            .unwrap_err(),
            HnswConfig {
                m: 0,
                ..HnswConfig::new(4)
            }
            .validate()
            .unwrap_err(),
            HnswConfig {
                ef_construction: 0,
                ..HnswConfig::new(4)
            }
            .validate()
            .unwrap_err(),
            HnswConfig {
                max_elements: 0,
                ..HnswConfig::new(4)
            }
            .validate()
            .unwrap_err(),
            HnswConfig {
                over_fetch_factor: 0,
                ..HnswConfig::new(4)
            }
            .validate()
            .unwrap_err(),
        ] {
            assert!(
                matches!(error, HnswConfigError::ZeroDimensions)
                    || matches!(error, HnswConfigError::ZeroM)
                    || matches!(error, HnswConfigError::ZeroEfConstruction)
                    || matches!(error, HnswConfigError::ZeroMaxElements)
                    || matches!(error, HnswConfigError::ZeroOverFetch)
            );
        }
    }

    #[test]
    fn defaults_match_the_oracle() {
        let config = HnswConfig::new(7);
        assert_eq!(config.m, 16);
        assert_eq!(config.ef_construction, 200);
        assert_eq!(config.max_elements, 1_000_000);
        assert_eq!(config.dimensions, 7);
    }

    #[test]
    fn add_point_rejects_mismatched_dimensions_and_duplicates() {
        let mut index = index(4, 1);
        index.add_point(1, &[0.1; 4]).expect("insert");
        assert_eq!(
            index.add_point(2, &[0.1; 5]).unwrap_err(),
            HnswError::DimensionMismatch {
                expected: 4,
                actual: 5
            }
        );
        assert_eq!(
            index.add_point(1, &[0.2; 4]).unwrap_err(),
            HnswError::DuplicatePoint(1)
        );
    }

    #[test]
    fn empty_index_search_returns_nothing() {
        let index = index(4, 1);
        assert!(index.search(&[0.1; 4], 5, 64).is_empty());
    }

    #[test]
    fn exact_match_is_found_first() {
        let mut index = index(8, 42);
        for i in 0..200 {
            index.add_point(i, &point(i, 8)).expect("insert");
        }
        let hits = index.search(&point(57, 8), 1, 64);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 57);
        assert!(hits[0].similarity > 0.999, "got {}", hits[0].similarity);
    }

    #[test]
    fn same_seed_same_sequence_produces_identical_snapshots() {
        let build = || {
            let mut index = index(6, 0xabc);
            for i in 0..500 {
                index.add_point(i, &point(i, 6)).expect("insert");
            }
            index.to_snapshot()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn different_seeds_produce_different_graphs() {
        let build = |seed: u64| {
            let mut index = index(6, seed);
            for i in 0..500 {
                index.add_point(i, &point(i, 6)).expect("insert");
            }
            index.to_snapshot()
        };
        assert_ne!(build(1), build(2));
    }

    #[test]
    fn filtered_search_respects_the_predicate() {
        let mut index = index(8, 7);
        for i in 0..300 {
            index.add_point(i, &point(i, 8)).expect("insert");
        }
        let query = point(10, 8);
        // Only even ids pass.
        let hits = index.search_filtered(&query, 5, 128, &|id| id % 2 == 0);
        assert!(!hits.is_empty());
        assert!(hits.len() <= 5);
        assert!(hits.iter().all(|hit| hit.id % 2 == 0));
        // The unfiltered top-1 (id 10) is odd-excluded when filtering odds.
        let odd_hits = index.search_filtered(&query, 1, 128, &|id| id % 2 == 1);
        assert!(odd_hits.iter().all(|hit| hit.id % 2 == 1));
        assert!(odd_hits[0].similarity < 0.999);
    }
}
