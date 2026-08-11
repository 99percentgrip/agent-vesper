//! Provider-neutral trait ports. Concrete implementations live at the
//! composition boundary (the TUI binary). This crate forbids any concrete
//! provider, runtime, ACP, sessions, agent, testkit, HTTP, or TUI dependency.

use std::sync::Arc;

use crate::error::Result;
use crate::nlp::EntityCandidate;

/// Action context for embedding calls. Mirrors mem0's `memory_action`.
/// Most providers do not differentiate; the port exists for forward
/// compatibility with providers that use distinct add/search encoders.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EmbedAction {
    Add,
    Search,
    Update,
}

/// Embeds text into a dense f32 vector.
///
/// # Contract
/// - All vectors MUST have a fixed dimension agreed at construction time
///   (see `CognitiveConfig.embedding_dim`).
/// - Implementations MUST be `Send + Sync`; the composition boundary shares
///   one `Arc<dyn EmbeddingPort>` across the TUI event loop.
pub trait EmbeddingPort: Send + Sync {
    fn embed(&self, text: &str, action: EmbedAction) -> Result<Vec<f32>>;

    /// Default sequential implementation. Override for native batch APIs.
    fn embed_batch(&self, texts: &[&str], action: EmbedAction) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t, action)).collect()
    }

    /// The model identifier used by this embedder (e.g. `"nomic-embed-text-v1.5"`).
    /// Used by the composition boundary to detect embedder swaps via the
    /// `cognition_meta` table (Gap 11) — model-name comparison is more
    /// accurate than dimension-only comparison because two distinct models
    /// may share a dimension. Default returns `"unknown"` for embedders that
    /// do not override (e.g. in-process `LocalHashEmbedder`).
    #[must_use]
    fn model_name(&self) -> &str {
        "unknown"
    }
}

/// Single LLM call returning the raw extraction response.
///
/// # Contract
/// - The implementation owns provider routing, authentication, retry, and
///   transport. This crate never sees a provider name or credential.
/// - The returned string MUST be the raw model output. JSON-object forcing
///   (`response_format={"type":"json_object"}`) is the implementation's
///   responsibility; the crate parses defensively either way.
pub trait ExtractionLlmPort: Send + Sync {
    fn extract(&self, system_prompt: &str, user_prompt: &str) -> Result<String>;
}

/// Entity extraction. The default in-crate impl is the regex fallback; a
/// future Rust NLP crate or Python sidecar can replace it without touching
/// the pipeline.
pub trait EntityExtractorPort: Send + Sync {
    fn extract(&self, text: &str) -> Vec<EntityCandidate>;
    fn extract_batch(&self, texts: &[&str]) -> Vec<Vec<EntityCandidate>> {
        texts.iter().map(|t| self.extract(t)).collect()
    }
}

/// The bundle the composition boundary constructs.
#[derive(Clone)]
pub struct CognitionPorts {
    pub embedder: Arc<dyn EmbeddingPort>,
    pub extractor: Arc<dyn ExtractionLlmPort>,
    pub entity_nlp: Arc<dyn EntityExtractorPort>,
}
