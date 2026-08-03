#![forbid(unsafe_code)]
//! Provider-neutral cognitive memory engine. Native Rust emulation of the
//! mem0 V3 ("April 2026 new algorithm") oracle at `/home/alex/Projects/mem0`
//! (pin `29fa4155`), per ADR 0015 (Stage 16).
//!
//! ## What this crate owns
//!
//! - The 8-phase single-pass ADD-only extraction pipeline.
//! - Hybrid retrieval: `score = (semantic_cosine + bm25_normalized + entity_boost) / max_possible`.
//! - SQLite relational + FTS5 schema (`history`, `messages`, `memories`,
//!   `memories_fts`, `entities`, `entity_memory_links`).
//! - Snowball (`rust-stemmers`) lemmatization fallback for spaCy.
//! - Regex-based entity extraction fallback for spaCy NER.
//! - Three trait ports: [`EmbeddingPort`], [`ExtractionLlmPort`],
//!   [`EntityExtractorPort`]. Concrete impls live at the composition boundary.
//!
//! ## What this crate does NOT own
//!
//! - Provider routing, authentication, retry, transport. The crate never
//!   sees a provider name, API key, or HTTP client.
//! - The TUI command surface or `SessionState` plumbing.
//! - Any other crate's storage (vesper-memory's JSONL store is independent).
//!
//! ## Storage layout
//!
//! A single SQLite database at a path chosen by the composition boundary.
//! `CognitiveStore::open` requires an absolute path whose parent exists;
//! the parent directory is never created by this crate.
//!
//! ## Architectural contract
//!
//! Depends only on `vesper-domain`, `vesper-security`, `rusqlite` (bundled),
//! `rust-stemmers`, and standard utility crates. **No provider, runtime,
//! ACP, sessions, agent, testkit, HTTP, or TUI dependency.** This is the
//! first production crate to introduce SQLite (per ADR 0015; the historical
//! blanket Stage-5 ban is superseded by a per-crate allowlist exception in
//! `cargo xtask architecture`).

mod bm25;
mod embedder;
mod error;
mod extract;
mod filters;
mod nlp;
mod pipeline;
mod ports;
mod prompts;
mod score;
mod store;
mod types;

pub mod assets {
    //! Re-exposed prompt assets so callers can inspect or override them.
    pub use crate::prompts::{
        ADDITIVE_EXTRACTION_PROMPT, AGENT_CONTEXT_SUFFIX, PROCEDURAL_MEMORY_SYSTEM_PROMPT,
    };
}

pub use bm25::{ENTITY_BOOST_WEIGHT, get_bm25_params, normalize_bm25};
pub use embedder::LocalHashEmbedder;
pub use error::{CognitionError, Result};
pub use extract::{ExtractedMemory, extract_json, parse_extraction_response, remove_code_blocks};
pub use filters::{FieldOp, FilterDsl};
pub use nlp::{EntityCandidate, EntityType, extract_entities, lemmatize_for_bm25};
pub use pipeline::{AddRequest, CognitiveMemory, SearchRequest};
pub use ports::{
    CognitionPorts, EmbedAction, EmbeddingPort, EntityExtractorPort, ExtractionLlmPort,
};
pub use prompts::{ExistingMemory, generate_additive_extraction_prompt};
pub use score::cosine;
pub use store::CognitiveStore;
pub use types::{
    Attribution, HistoryEvent, MemoryEvent, MemoryHit, MemoryRecord, Message, Scope, ScoreBreakdown,
};

/// Engine configuration.
/// Fusion strategy for hybrid retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum FusionStrategy {
    /// Additive: `(semantic + bm25 + entity_boost) / max_possible` (mem0 V3 default).
    Additive,
    /// Reciprocal Rank Fusion: `sum(1/(60 + rank + 1))` across ranked lists.
    RRF,
}

pub struct CognitiveConfig {
    /// Required embedding vector dimension. Defaults to 1024.
    pub embedding_dim: usize,
    /// Enable the optional conflict-detection LLM pass after extraction.
    /// When enabled, a second LLM call compares new memories against existing
    /// ones with store/skip/update/merge actions (TencentDB-inspired).
    /// Default: false (preserves V3 ADD-only behavior).
    pub enable_conflict_detection: bool,
    /// Fusion strategy for hybrid retrieval scoring.
    pub fusion_strategy: FusionStrategy,
    /// Maximum tokens (approx ~4 chars/token) for pre-dispatch context injection.
    /// Default: 2000 (≈8000 chars).
    pub max_injection_tokens: usize,
}

impl Default for CognitiveConfig {
    fn default() -> Self {
        Self {
            embedding_dim: 1024,
            enable_conflict_detection: false,
            fusion_strategy: FusionStrategy::Additive,
            max_injection_tokens: 2000,
        }
    }
}

/// Open a `CognitiveMemory` at the given path with the supplied ports.
///
/// The composition boundary (TUI binary) calls this after constructing the
/// `CognitionPorts` bundle. The path MUST be absolute and its parent MUST
/// exist; this function never creates directories.
pub fn open(
    path: &std::path::Path,
    ports: CognitionPorts,
    config: CognitiveConfig,
) -> Result<CognitiveMemory> {
    let store = CognitiveStore::open_with_functions(path)?;
    Ok(CognitiveMemory::new(store, ports, config))
}
