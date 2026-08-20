#![allow(clippy::too_many_lines)]
//! ACP-side cognitive-memory composition (Stage 16 / ADR 0015 + ADR 0016).
//!
//! This module is the ACP mirror of the TUI's `CognitionBundle`
//! (`apps/agent-vesper-tui/src/main.rs`). Both hosts open the SAME durable
//! stores — project-local `.agent-vesper/cognition/cognition.db` and the
//! user-global `~/.local/share/agent-vesper/cognition/cognition.db` — with
//! the same embedder selection (`embedding.json`, ADR 0016) and the same
//! extraction routing, so a memory saved from the TUI is recalled by the
//! ACP host and vice versa. Keeping the two copies behaviorally identical
//! is a standing DOX contract (`apps/agent-vesper-acp/AGENTS.md`).
//!
//! Differences from the TUI copy (documented, deliberate):
//! - No background embedder probe thread: the engine starts in the same
//!   initial search mode the TUI would pick and auto-upgrades to `Hybrid`
//!   on the first successful embed call.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as b64};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// HMAC-SHA256 + Zhipu JWT (shared helper for BigModel embeddings)
// ---------------------------------------------------------------------------

const BLOCK_SIZE: usize = 64;

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let k = if key.len() > BLOCK_SIZE {
        let mut h = Sha256::new();
        h.update(key);
        let digest = h.finalize();
        let mut padded = vec![0u8; BLOCK_SIZE];
        padded[..32].copy_from_slice(&digest);
        padded
    } else {
        let mut padded = key.to_vec();
        padded.resize(BLOCK_SIZE, 0);
        padded
    };
    let mut ipad = vec![0x36u8; BLOCK_SIZE];
    let mut opad = vec![0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(message);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(inner_digest);
    let result = outer.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Generate a Zhipu AI JWT token from an API key of the form "id.secret".
fn zhipu_jwt(api_key: &str) -> Option<String> {
    let (id, secret) = api_key.split_once('.')?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let exp_ms = now_ms + 3_600_000; // 1 hour
    let header = serde_json::json!({"alg": "HS256", "sign_type": "SIGN"});
    let payload = serde_json::json!({
        "api_key": id,
        "exp": exp_ms,
        "timestamp": now_ms,
    });
    let header_b64 = b64.encode(serde_json::to_string(&header).ok()?.as_bytes());
    let payload_b64 = b64.encode(serde_json::to_string(&payload).ok()?.as_bytes());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    let sig_b64 = b64.encode(sig);
    Some(format!("{signing_input}.{sig_b64}"))
}

// ---------------------------------------------------------------------------
// Embedding + extraction adapters (ports fulfilled at this composition
// boundary, exactly like the TUI fulfils them at its own boundary)
// ---------------------------------------------------------------------------

/// BigModel CN `embedding-3` (1024-d) embedder with per-call JWT auth.
struct BigModelEmbeddingAdapter {
    credential_source: Arc<dyn vesper_provider_glm::GlmCredentialSource>,
    client: reqwest::blocking::Client,
    endpoint_url: String,
}

impl BigModelEmbeddingAdapter {
    fn new(credential_source: Arc<dyn vesper_provider_glm::GlmCredentialSource>) -> Self {
        let endpoint =
            vesper_provider_glm::GlmEndpoint::official(vesper_provider_glm::GlmPlan::BigModel)
                .expect("static BigModel CN endpoint");
        Self {
            credential_source,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest blocking client"),
            endpoint_url: format!("{}/embeddings", endpoint.base_url()),
        }
    }

    fn resolve_jwt(&self) -> Result<String, vesper_cognition::CognitionError> {
        let cred = vesper_provider_glm::resolve_credential(self.credential_source.as_ref())
            .map_err(|_| {
                vesper_cognition::CognitionError::Embedding("credential resolution failed".into())
            })?;
        zhipu_jwt(cred.secret.expose().as_str()).ok_or_else(|| {
            vesper_cognition::CognitionError::Embedding(
                "API key missing '.' separator for JWT generation".into(),
            )
        })
    }
}

impl vesper_cognition::EmbeddingPort for BigModelEmbeddingAdapter {
    fn embed(
        &self,
        text: &str,
        _action: vesper_cognition::EmbedAction,
    ) -> Result<Vec<f32>, vesper_cognition::CognitionError> {
        let jwt = self.resolve_jwt()?;
        let body = serde_json::json!({
            "model": "embedding-3",
            "input": text,
            "dimensions": 1024,
        });
        let response = self
            .client
            .post(&self.endpoint_url)
            .bearer_auth(&jwt)
            .json(&body)
            .send()
            .map_err(|e| {
                vesper_cognition::CognitionError::Embedding(format!("HTTP send failed: {e}"))
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().unwrap_or_else(|_| "(no body)".into());
            return Err(vesper_cognition::CognitionError::Embedding(format!(
                "HTTP {status} - {body_text}"
            )));
        }
        let parsed: serde_json::Value = response
            .json()
            .map_err(|e| {
                vesper_cognition::CognitionError::Embedding(format!("JSON parse failed: {e}"))
            })?;
        let vector = parsed["data"][0]["embedding"].as_array().ok_or_else(|| {
            vesper_cognition::CognitionError::Embedding(format!(
                "missing data[0].embedding in response: {parsed}"
            ))
        })?;
        vector
            .iter()
            .map(|v| {
                v.as_f64().map(|f| f as f32).ok_or_else(|| {
                    vesper_cognition::CognitionError::Embedding(
                        "embedding vector contains non-numeric value".into(),
                    )
                })
            })
            .collect()
    }

    fn model_name(&self) -> &str {
        "bigmodel:embedding-3"
    }
}

/// LM Studio `/v1/embeddings` embedder.
struct LmStudioEmbedder {
    endpoint_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::blocking::Client,
}

impl LmStudioEmbedder {
    fn from_explicit_settings(
        endpoint_url: String,
        model: String,
        api_key: Option<String>,
    ) -> Self {
        Self {
            endpoint_url,
            api_key,
            model,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest blocking client"),
        }
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>, vesper_cognition::CognitionError> {
        let body = serde_json::json!({"model": self.model, "input": text});
        let mut request = self.client.post(&self.endpoint_url).json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let response = request.send().map_err(|e| {
            vesper_cognition::CognitionError::Embedding(format!("HTTP send failed: {e}"))
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().unwrap_or_else(|_| "(no body)".into());
            return Err(vesper_cognition::CognitionError::Embedding(format!(
                "HTTP {status} - {body_text}"
            )));
        }
        let parsed: serde_json::Value = response
            .json()
            .map_err(|e| {
                vesper_cognition::CognitionError::Embedding(format!("JSON parse failed: {e}"))
            })?;
        let vec = parsed["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| {
                vesper_cognition::CognitionError::Embedding(format!(
                    "missing data[0].embedding in response: {parsed}"
                ))
            })?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<f32>>();
        if vec.is_empty() {
            return Err(vesper_cognition::CognitionError::Embedding(
                "embedding endpoint returned empty vector".into(),
            ));
        }
        Ok(vec)
    }
}

impl vesper_cognition::EmbeddingPort for LmStudioEmbedder {
    fn embed(
        &self,
        text: &str,
        _action: vesper_cognition::EmbedAction,
    ) -> Result<Vec<f32>, vesper_cognition::CognitionError> {
        self.embed_one(text)
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Minimal persisted LM Studio settings reader
/// (`.agent-vesper/lmstudio/settings.json` — same file the TUI hub edits).
fn lmstudio_persisted_endpoint_and_model() -> Option<(String, String)> {
    let dir = std::env::var("AGENT_VESPER_LMSTUDIO_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("AGENT_VESPER_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from(".agent-vesper"))
                .join("lmstudio")
        });
    let text = std::fs::read_to_string(dir.join("settings.json")).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
    let base = parsed.get("api_base_url")?.as_str()?.trim().to_string();
    if base.is_empty() {
        return None;
    }
    let model = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var("AGENT_VESPER_COGNITION_MODEL").ok())
        .unwrap_or_else(|| "local-model".into());
    Some((base, model))
}

/// Zai chat completions extractor (Standard plan endpoint).
struct ZaiExtractionAdapter {
    credential_source: Arc<dyn vesper_provider_glm::GlmCredentialSource>,
    client: reqwest::blocking::Client,
    endpoint_url: String,
    model: String,
}

impl ZaiExtractionAdapter {
    fn new(credential_source: Arc<dyn vesper_provider_glm::GlmCredentialSource>) -> Self {
        let endpoint =
            vesper_provider_glm::GlmEndpoint::official(vesper_provider_glm::GlmPlan::Coding)
                .expect("static Zai Standard endpoint");
        Self {
            credential_source,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest blocking client"),
            endpoint_url: format!("{}/chat/completions", endpoint.base_url()),
            model: std::env::var("AGENT_VESPER_COGNITION_MODEL")
                .unwrap_or_else(|_| String::from("glm-4.6")),
        }
    }
}

impl vesper_cognition::ExtractionLlmPort for ZaiExtractionAdapter {
    fn extract(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, vesper_cognition::CognitionError> {
        let key = vesper_provider_glm::resolve_credential(self.credential_source.as_ref())
            .map_err(|_| {
                vesper_cognition::CognitionError::Extraction("credential resolution failed".into())
            })
            .map(|c| c.secret.expose().as_str().to_string())?;
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "response_format": {"type": "json_object"},
        });
        let response = self
            .client
            .post(&self.endpoint_url)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .map_err(|e| {
                vesper_cognition::CognitionError::Extraction(format!("HTTP send failed: {e}"))
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().unwrap_or_else(|_| "(no body)".into());
            return Err(vesper_cognition::CognitionError::Extraction(format!(
                "HTTP {status} - {body_text}"
            )));
        }
        let parsed: serde_json::Value = response
            .json()
            .map_err(|e| {
                vesper_cognition::CognitionError::Extraction(format!("JSON parse failed: {e}"))
            })?;
        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                vesper_cognition::CognitionError::Extraction(format!(
                    "missing choices[0].message.content in response: {parsed}"
                ))
            })
    }
}

/// LM Studio chat-completions extractor.
struct LmStudioExtractionAdapter {
    endpoint_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::blocking::Client,
}

impl LmStudioExtractionAdapter {
    fn new(base_url: String, model: String) -> Self {
        Self {
            endpoint_url: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            api_key: std::env::var("LMSTUDIO_API_KEY").ok(),
            model,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest blocking client"),
        }
    }
}

impl vesper_cognition::ExtractionLlmPort for LmStudioExtractionAdapter {
    fn extract(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, vesper_cognition::CognitionError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "response_format": {"type": "json_object"},
        });
        let mut request = self.client.post(&self.endpoint_url).json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let response = request.send().map_err(|e| {
            vesper_cognition::CognitionError::Extraction(format!("HTTP send failed: {e}"))
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().unwrap_or_else(|_| "(no body)".into());
            return Err(vesper_cognition::CognitionError::Extraction(format!(
                "HTTP {status} - {body_text}"
            )));
        }
        let parsed: serde_json::Value = response
            .json()
            .map_err(|e| {
                vesper_cognition::CognitionError::Extraction(format!("JSON parse failed: {e}"))
            })?;
        parsed["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                vesper_cognition::CognitionError::Extraction(format!(
                    "missing choices[0].message.content in response: {parsed}"
                ))
            })
    }
}

/// Always-error extractor: forces the graceful raw-text fallback.
struct NoOpExtractionAdapter;

impl vesper_cognition::ExtractionLlmPort for NoOpExtractionAdapter {
    fn extract(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> Result<String, vesper_cognition::CognitionError> {
        Err(vesper_cognition::CognitionError::Extraction(
            "no extractor available (no Z.ai credential, no LM Studio settings)".into(),
        ))
    }
}

/// Regex entity extractor behind the engine's port (mirrors the TUI copy).
struct ZaiEntityExtractor;

impl vesper_cognition::EntityExtractorPort for ZaiEntityExtractor {
    fn extract(&self, text: &str) -> Vec<vesper_cognition::EntityCandidate> {
        vesper_cognition::extract_entities(text)
    }
}

// ---------------------------------------------------------------------------
// ADR 0016 provider-independent embedding configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct EmbeddingConfig {
    source: Option<String>,
    endpoint: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    dimension: Option<usize>,
}

impl EmbeddingConfig {
    fn load(root: &std::path::Path) -> Self {
        match std::fs::read_to_string(root.join("embedding.json")) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn overrides_provider_routing(&self) -> bool {
        self.source.is_some()
    }
}

// ---------------------------------------------------------------------------
// The bundle
// ---------------------------------------------------------------------------

pub struct CognitionBundle {
    pub engine: Option<Arc<vesper_cognition::CognitiveMemory>>,
    pub global_engine: Option<Arc<vesper_cognition::CognitiveMemory>>,
    root_display: String,
    global_root_display: String,
    project_display: String,
    root: std::path::PathBuf,
}

pub fn global_cognition_root() -> std::path::PathBuf {
    if let Ok(value) = std::env::var("AGENT_VESPER_GLOBAL_COGNITION_ROOT") {
        return std::path::PathBuf::from(value);
    }
    if let Ok(value) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(value)
            .join("agent-vesper")
            .join("cognition");
    }
    if let Ok(value) = std::env::var("HOME") {
        return std::path::PathBuf::from(value)
            .join(".local")
            .join("share")
            .join("agent-vesper")
            .join("cognition");
    }
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".agent-vesper")
        .join("global-cognition")
}

fn build_independent_embedder(
    cfg: &EmbeddingConfig,
    default_dim: usize,
    credential_source: &Arc<dyn vesper_provider_glm::GlmCredentialSource>,
) -> (
    Arc<dyn vesper_cognition::EmbeddingPort>,
    Option<usize>,
    vesper_cognition::SearchMode,
) {
    match cfg.source.as_deref() {
        Some("local") => (
            Arc::new(vesper_cognition::LocalHashEmbedder::new(default_dim)),
            Some(default_dim),
            vesper_cognition::SearchMode::Hybrid,
        ),
        Some("lmstudio") => {
            let endpoint = cfg
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:1234/v1/embeddings".to_string());
            let model = cfg
                .model
                .clone()
                .unwrap_or_else(|| "text-embedding-nomic-embed-text-v1.5".to_string());
            (
                Arc::new(LmStudioEmbedder::from_explicit_settings(
                    endpoint, model, cfg.api_key.clone(),
                )) as Arc<dyn vesper_cognition::EmbeddingPort>,
                cfg.dimension,
                vesper_cognition::SearchMode::BM25Only,
            )
        }
        Some("bigmodel") => (
            Arc::new(BigModelEmbeddingAdapter::new(Arc::clone(credential_source)))
                as Arc<dyn vesper_cognition::EmbeddingPort>,
            Some(1024),
            vesper_cognition::SearchMode::BM25Only,
        ),
        _ => (
            Arc::new(vesper_cognition::LocalHashEmbedder::new(default_dim)),
            Some(default_dim),
            vesper_cognition::SearchMode::Hybrid,
        ),
    }
}

fn build_provider_routed_embedder(
    credential_source: &Arc<dyn vesper_provider_glm::GlmCredentialSource>,
    active_provider: &str,
    default_dim: usize,
    lm_settings: Option<&(String, String)>,
) -> (Arc<dyn vesper_cognition::EmbeddingPort>, Option<usize>) {
    if active_provider == "lmstudio"
        && let Some((base, model)) = lm_settings
    {
        return (
            Arc::new(LmStudioEmbedder::from_explicit_settings(
                format!("{}/embeddings", base.trim_end_matches('/')),
                std::env::var("AGENT_VESPER_COGNITION_EMBEDDING_MODEL")
                    .ok()
                    .unwrap_or_else(|| model.clone()),
                std::env::var("LMSTUDIO_API_KEY").ok(),
            )) as Arc<dyn vesper_cognition::EmbeddingPort>,
            None,
        );
    }
    // TUI parity: the provider-routed default is the zero-network
    // LocalHashEmbedder; BigModel neural embeddings are opt-in via
    // AGENT_VESPER_COGNITION_EMBEDDING_API=bigmodel (identical to the TUI's
    // default arm, so the two hosts never fight over the vector space).
    if std::env::var("AGENT_VESPER_COGNITION_EMBEDDING_API").as_deref() == Ok("bigmodel")
        && vesper_provider_glm::resolve_credential(credential_source.as_ref()).is_ok()
    {
        return (
            Arc::new(BigModelEmbeddingAdapter::new(Arc::clone(credential_source)))
                as Arc<dyn vesper_cognition::EmbeddingPort>,
            Some(1024),
        );
    }
    (
        Arc::new(vesper_cognition::LocalHashEmbedder::new(default_dim)),
        Some(default_dim),
    )
}

impl CognitionBundle {
    /// Opens the same durable stores the TUI opens, with the same
    /// embedder/extraction routing. `engine = None` only when SQLite cannot
    /// open — the slash surface then degrades to a truthful notice.
    pub fn open_default(
        credential_source: Arc<dyn vesper_provider_glm::GlmCredentialSource>,
        active_provider: &str,
    ) -> Self {
        let root = match std::env::var("AGENT_VESPER_COGNITION_ROOT") {
            Ok(value) => std::path::PathBuf::from(value),
            Err(_) => std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".agent-vesper")
                .join("cognition"),
        };
        Self::open_at(
            root,
            global_cognition_root(),
            lmstudio_persisted_endpoint_and_model(),
            credential_source,
            active_provider,
        )
    }

    /// A bundle with every store unavailable: the graceful fallback when the
    /// blocking open task itself fails. Every cognitive command answers
    /// with a truthful "store unavailable" notice.
    pub fn open_disabled() -> Self {
        Self {
            engine: None,
            global_engine: None,
            root_display: "(unavailable)".to_owned(),
            global_root_display: "(unavailable)".to_owned(),
            project_display: "(unavailable)".to_owned(),
            root: std::path::PathBuf::new(),
        }
    }

    /// Root-explicit constructor (tests and future embedded hosts). The
    /// persisted LM Studio settings are passed in by the caller so this
    /// function performs no environment reads.
    pub fn open_at(
        root: std::path::PathBuf,
        global_root: std::path::PathBuf,
        lm_settings: Option<(String, String)>,
        credential_source: Arc<dyn vesper_provider_glm::GlmCredentialSource>,
        active_provider: &str,
    ) -> Self {
        let _ = std::fs::create_dir_all(&root);
        let db_path = root.join("cognition.db");
        let root_display = root.display().to_string();
        let project_display = std::env::current_dir()
            .ok()
            .and_then(|path| path.canonicalize().ok().or(Some(path)))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .display()
            .to_string();
        let _ = std::fs::create_dir_all(&global_root);
        let global_root_display = global_root.display().to_string();
        let global_db_path = global_root.join("cognition.db");

        let embedding_config = EmbeddingConfig::load(&root);
        let default_dim = vesper_cognition::CognitiveConfig::default().embedding_dim;
        let (embedder, probed_dim, search_mode_hint) = if
            embedding_config.overrides_provider_routing() {
            build_independent_embedder(&embedding_config, default_dim, &credential_source)
        } else {
            let (embedder, dim) = build_provider_routed_embedder(
                &credential_source,
                active_provider,
                default_dim,
                lm_settings.as_ref(),
            );
            let initial_mode = if active_provider == "lmstudio" {
                vesper_cognition::SearchMode::BM25Only
            } else {
                vesper_cognition::SearchMode::Hybrid
            };
            (embedder, dim, initial_mode)
        };

        let mut config = vesper_cognition::CognitiveConfig::default();
        if let Some(dim) = probed_dim {
            config.embedding_dim = dim;
        }

        let zai_cred_ok =
            vesper_provider_glm::resolve_credential(credential_source.as_ref()).is_ok();
        let extractor: Arc<dyn vesper_cognition::ExtractionLlmPort> = if active_provider
            == "lmstudio"
        {
            lm_settings
                .clone()
                .map(|(base, model)| {
                    let arc: Arc<dyn vesper_cognition::ExtractionLlmPort> =
                        Arc::new(LmStudioExtractionAdapter::new(base, model));
                    arc
                })
                .unwrap_or_else(|| {
                    if zai_cred_ok {
                        Arc::new(ZaiExtractionAdapter::new(Arc::clone(&credential_source)))
                    } else {
                        Arc::new(NoOpExtractionAdapter)
                    }
                })
        } else if zai_cred_ok {
            Arc::new(ZaiExtractionAdapter::new(Arc::clone(&credential_source)))
        } else {
            lm_settings
                .clone()
                .map(|(base, model)| {
                    let arc: Arc<dyn vesper_cognition::ExtractionLlmPort> =
                        Arc::new(LmStudioExtractionAdapter::new(base, model));
                    arc
                })
                .unwrap_or_else(|| Arc::new(NoOpExtractionAdapter))
        };

        let ports = vesper_cognition::CognitionPorts {
            embedder,
            extractor,
            entity_nlp: Arc::new(ZaiEntityExtractor),
        };
        let global_config = vesper_cognition::CognitiveConfig {
            embedding_dim: config.embedding_dim,
            enable_conflict_detection: config.enable_conflict_detection,
            fusion_strategy: config.fusion_strategy,
            max_injection_tokens: config.max_injection_tokens,
        };
        let engine = vesper_cognition::open(&db_path, ports.clone(), config)
            .ok()
            .map(Arc::new);
        let global_engine = vesper_cognition::open(&global_db_path, ports, global_config)
            .ok()
            .map(Arc::new);
        if let Some(engine) = engine.as_ref() {
            engine.set_search_mode(search_mode_hint);
        }
        if let Some(engine) = global_engine.as_ref() {
            engine.set_search_mode(search_mode_hint);
        }
        // Record the active embedder identity so the OTHER host (TUI ↔ ACP)
        // can detect a swap and re-embed on its next open (Gap 11 semantics).
        for engine in [engine.as_ref(), global_engine.as_ref()].into_iter().flatten() {
            let active_model = engine.embedder_model_name();
            let stored_model = engine.get_meta("embedding_model").ok().flatten();
            if stored_model.as_deref().is_none_or(|stored| stored != active_model) {
                if stored_model.is_some() {
                    // Model changed since the last open (possibly by the
                    // other host): re-embed with the active embedder so the
                    // vector space stays consistent.
                    match engine.reembed_everything() {
                        Ok(_) => engine.set_search_mode(vesper_cognition::SearchMode::Hybrid),
                        Err(_) => engine.set_search_mode(vesper_cognition::SearchMode::BM25Only),
                    }
                }
                let dim = probed_dim.unwrap_or(default_dim);
                let _ = engine.set_meta("embedding_model", active_model);
                let _ = engine.set_meta("embedding_dim", &dim.to_string());
            }
        }
        Self {
            engine,
            global_engine,
            root_display,
            global_root_display,
            project_display,
            root,
        }
    }

    pub fn global_root_display(&self) -> &str {
        &self.global_root_display
    }

    /// `/embedding` — status text plus the persisted config, if any.
    pub fn embedding_status_text(&self) -> String {
        let cfg = EmbeddingConfig::load(&self.root);
        let source = cfg.source.as_deref().unwrap_or("(provider-routed)");
        let engine_state = if self.engine.is_some() { "open" } else { "unavailable" };
        format!(
            "cognition embedding:\n  source: {source}\n  engine: {engine_state}\n  project \
             store: {}\n  global store: {}\n  changes: edit {} (applies on next start), \
             or use /embedding set source=... key=value...",
            self.root_display,
            self.global_root_display,
            self.root.join("embedding.json").display()
        )
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (TUI parity)
// ---------------------------------------------------------------------------

pub fn cognition_user_scope() -> vesper_cognition::Scope {
    vesper_cognition::Scope {
        user_id: Some(
            std::env::var("AGENT_VESPER_COGNITION_USER_ID").unwrap_or_else(|_| "local".into()),
        ),
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CognitionScope {
    Smart,
    Global,
    Project,
}

fn smart_memory_scope(text: &str) -> (CognitionScope, &'static str) {
    let lower = text.to_ascii_lowercase();
    let global_signals = [
        "my name",
        "call me",
        "i prefer",
        "i like",
        "i dislike",
        "my favorite",
        "my favourite",
        "my pronouns",
        "my timezone",
        "i live in",
        "i am based in",
        "always respond",
        "never respond",
        "across projects",
    ];
    if global_signals.iter().any(|signal| lower.contains(signal)) {
        return (CognitionScope::Global, "identity or stable preference detected");
    }
    (CognitionScope::Project, "repository or task-specific fact")
}

fn add_cognitive_memory(
    engine: &vesper_cognition::CognitiveMemory,
    scope: &vesper_cognition::Scope,
    text: &str,
    infer: bool,
) -> Result<(Vec<vesper_cognition::MemoryEvent>, Option<String>), String> {
    let message = vesper_cognition::Message::user(text);
    let request = |infer| vesper_cognition::AddRequest {
        messages: std::slice::from_ref(&message),
        scope,
        extras: None,
        expiration_date: None,
        infer,
        custom_instructions: None,
        observation_date: None,
    };
    match engine.add(request(infer)) {
        Ok(events) => Ok((events, None)),
        Err(error) if infer => {
            let warning = format!("stored raw text because extraction was unavailable: {error}");
            engine
                .add(request(false))
                .map(|events| (events, Some(warning)))
                .map_err(|fallback| fallback.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn short_memory_id(id: &str) -> &str {
    &id[..8.min(id.len())]
}

fn find_scoped_memory(
    engine: &vesper_cognition::CognitiveMemory,
    scope: &vesper_cognition::Scope,
    id: &str,
) -> Result<Option<vesper_cognition::MemoryRecord>, String> {
    let records = engine
        .get_all(scope, None, 10_000, true)
        .map_err(|error| error.to_string())?;
    if let Some(exact) = records.iter().find(|record| record.id == id) {
        return Ok(Some(exact.clone()));
    }
    let mut matches = records
        .into_iter()
        .filter(|record| record.id.starts_with(id));
    let first = matches.next();
    if first.is_some() && matches.next().is_some() {
        return Err(format!("memory ID prefix {id} is ambiguous"));
    }
    Ok(first)
}

fn delete_scoped_memory(
    engine: &vesper_cognition::CognitiveMemory,
    scope: &vesper_cognition::Scope,
    id: &str,
) -> Result<bool, String> {
    let Some(record) = find_scoped_memory(engine, scope, id)? else {
        return Ok(false);
    };
    engine
        .delete(&record.id)
        .map_err(|error| error.to_string())?;
    Ok(true)
}

fn find_scoped_memory_by_data(
    engine: &vesper_cognition::CognitiveMemory,
    scope: &vesper_cognition::Scope,
    data: &str,
) -> Result<Option<vesper_cognition::MemoryRecord>, String> {
    engine
        .get_all(scope, None, 10_000, true)
        .map_err(|error| error.to_string())
        .map(|records| records.into_iter().find(|candidate| candidate.data == data))
}

/// The auto-recall context block appended to the user message each turn.
pub fn cognitive_context_for_prompt(bundle: &CognitionBundle, prompt: &str) -> Option<String> {
    let scope = cognition_user_scope();
    let mut hits: Vec<(&str, vesper_cognition::MemoryHit)> = Vec::new();
    for (label, engine) in [
        ("project", bundle.engine.as_ref()),
        ("global", bundle.global_engine.as_ref()),
    ] {
        let Some(engine) = engine else { continue };
        let request = vesper_cognition::SearchRequest {
            query: prompt,
            scope: &scope,
            filters: None,
            top_k: 5,
            threshold: 0.02,
            explain: false,
            show_expired: false,
        };
        if let Ok(found) = engine.search(request) {
            hits.extend(found.into_iter().map(|hit| (label, hit)));
        }
    }
    hits.sort_by(|left, right| right.1.score.total_cmp(&left.1.score));
    hits.truncate(5);
    if hits.is_empty() {
        return None;
    }
    let mut block =
        String::from("\n\n--- Relevant context from cognitive memory (auto-recalled):\n");
    let max_chars = 2000 * 4;
    let mut chars_used = block.len();
    for (scope_label, hit) in &hits {
        let line = format!(
            "- [{scope_label}] ({:.2}) {}\n",
            hit.score,
            hit.memory.chars().take(200).collect::<String>()
        );
        chars_used += line.len();
        if chars_used > max_chars {
            break;
        }
        block.push_str(&line);
    }
    Some(block)
}

// ---------------------------------------------------------------------------
// Slash-command execution (returns None when `name` is not cognitive)
// ---------------------------------------------------------------------------

fn cognition_scope_and_body(argument: &str) -> Result<(CognitionScope, String), String> {
    let mut scope = CognitionScope::Smart;
    let mut body = Vec::new();
    for token in argument.split_whitespace() {
        match token {
            "--global" => {
                if scope != CognitionScope::Smart {
                    return Err("choose only one of --global or --project".into());
                }
                scope = CognitionScope::Global;
            }
            "--project" | "--local" => {
                if scope != CognitionScope::Smart {
                    return Err("choose only one of --global or --project".into());
                }
                scope = CognitionScope::Project;
            }
            value if value.starts_with("--") => {
                return Err(format!("unknown memory scope flag: {value}"));
            }
            value => body.push(value),
        }
    }
    Ok((scope, body.join(" ")))
}

fn audit_text(engine: Option<&Arc<vesper_cognition::CognitiveMemory>>, query: Option<&str>, heading: &str, scope: &vesper_cognition::Scope) -> String {
    let mut out = format!("{heading}:\n");
    let Some(engine) = engine else {
        out.push_str("  unavailable\n");
        return out;
    };
    match engine.get_all(scope, None, 200, false) {
        Ok(records) => {
            let needle = query.map(str::to_ascii_lowercase);
            let filtered: Vec<_> = records
                .into_iter()
                .filter(|record| {
                    needle.as_ref().is_none_or(|needle| {
                        record.data.to_ascii_lowercase().contains(needle)
                            || record.id.to_ascii_lowercase().contains(needle)
                    })
                })
                .take(20)
                .collect();
            if filtered.is_empty() {
                out.push_str("  (no matching memories)\n");
            }
            for record in filtered {
                out.push_str(&format!(
                    "  [{}] {}\n",
                    short_memory_id(&record.id),
                    record.data.chars().take(120).collect::<String>()
                ));
            }
        }
        Err(error) => out.push_str(&format!("  listing failed: {error}\n")),
    }
    out
}

/// True for the seven Vesper-native cognitive-memory command names.
pub fn is_cognition_command(name: &str) -> bool {
    matches!(
        name,
        "remember" | "recall" | "forget" | "memories" | "promote" | "demote" | "embedding"
    )
}

/// Executes one Vesper-native cognitive-memory slash command. Returns `None`
/// when `name` is not part of this family so the caller can fall through to
/// the oracle catalog path. Result is plain text (rendered as markdown by
/// ACP clients); management turns are never persisted.
pub fn execute_cognition_slash(
    name: &str,
    argument: &str,
    bundle: &CognitionBundle,
) -> Option<String> {
    let user_scope = cognition_user_scope();
    let result: Result<String, String> = match name {
        "remember" => cognition_scope_and_body(argument)
            .map_err(|e| format!("cognition: /remember failed: {e}"))
            .and_then(|(scope, text)| {
                if text.trim().is_empty() {
                    Err("cognition: usage: /remember [--global|--project] <fact or conversation text>".into())
                } else {
                    Ok(remember_text(bundle, &user_scope, scope, &text))
                }
            }),
        "recall" => cognition_scope_and_body(argument)
            .map_err(|e| format!("cognition: /recall failed: {e}"))
            .and_then(|(scope, query)| {
                if query.trim().is_empty() {
                    Err("cognition: usage: /recall [--global|--project] <query>".into())
                } else {
                    Ok(recall_text(bundle, &user_scope, scope, &query))
                }
            }),
        "forget" => cognition_scope_and_body(argument)
            .map_err(|e| format!("cognition: /forget failed: {e}"))
            .and_then(|(scope, id)| {
                if id.trim().is_empty() {
                    Err("cognition: usage: /forget [--global|--project] <id-prefix>".into())
                } else {
                    Ok(forget_text(bundle, &user_scope, scope, &id))
                }
            }),
        "memories" => {
            let query = (!argument.trim().is_empty()).then_some(argument.trim());
            Ok(audit_text(bundle.engine.as_ref(), query, "Project memories", &user_scope)
                + &audit_text(bundle.global_engine.as_ref(), query, "Global memories", &user_scope))
        }
        "promote" => transfer_text(bundle, &user_scope, argument.trim(), "project", "global", true),
        "demote" => transfer_text(bundle, &user_scope, argument.trim(), "global", "project", false),
        "embedding" => Ok(if argument.trim().is_empty() {
            bundle.embedding_status_text()
        } else if let Some(rest) = argument.trim().strip_prefix("set ") {
            embedding_set_text(bundle, rest.trim())
        } else if argument.trim() == "set" {
            "cognition: usage: /embedding set source=<local|lmstudio|bigmodel> \
             [endpoint=...] [model=...] [api_key=...] [dimension=...]"
                .to_owned()
        } else {
            format!(
                "cognition: unknown /embedding argument `{}`. Use /embedding alone for \
                 status or /embedding set key=value...",
                argument.trim()
            )
        }),
        _ => return None,
    };
    Some(result.unwrap_or_else(|error| error))
}

fn remember_text(
    bundle: &CognitionBundle,
    user_scope: &vesper_cognition::Scope,
    scope: CognitionScope,
    text: &str,
) -> String {
    let (destination, reason) = match scope {
        CognitionScope::Smart => smart_memory_scope(text),
        CognitionScope::Global => (CognitionScope::Global, "explicit --global override"),
        CognitionScope::Project => (CognitionScope::Project, "explicit --project override"),
    };
    let (engine, label, location) = match destination {
        CognitionScope::Global => (
            bundle.global_engine.as_ref(),
            "globally",
            format!("user profile at {}", bundle.global_root_display()),
        ),
        CognitionScope::Project | CognitionScope::Smart => (
            bundle.engine.as_ref(),
            "for this project",
            format!("project store for {}", bundle.project_display),
        ),
    };
    let Some(engine) = engine else {
        return format!("cognition: {label} memory store unavailable ({location})");
    };
    match add_cognitive_memory(engine, user_scope, text, true) {
        Ok((events, fallback)) if !events.is_empty() => {
            let mut out = format!("✓ Remembered {label}\n- Scope: {location}\n- Routing: {reason}");
            if let Some(warning) = fallback {
                out.push_str(&format!("\n- Note: {warning}"));
            }
            for event in events.iter().take(10) {
                out.push_str(&format!(
                    "\n- [{}] {}",
                    short_memory_id(&event.id),
                    event.memory.chars().take(100).collect::<String>()
                ));
            }
            out
        }
        Ok(_) => format!(
            "cognition: nothing new to remember {label} (already known or no extractable facts)"
        ),
        Err(error) => format!("cognition: /remember failed: {error}"),
    }
}

fn recall_text(
    bundle: &CognitionBundle,
    user_scope: &vesper_cognition::Scope,
    scope: CognitionScope,
    query: &str,
) -> String {
    let mut hits: Vec<(&'static str, vesper_cognition::MemoryHit)> = Vec::new();
    if scope != CognitionScope::Global
        && let Some(engine) = bundle.engine.as_ref()
    {
        let request = vesper_cognition::SearchRequest {
            query,
            scope: user_scope,
            filters: None,
            top_k: 10,
            threshold: 0.05,
            explain: false,
            show_expired: false,
        };
        if let Ok(found) = engine.search(request) {
            hits.extend(found.into_iter().map(|hit| ("project", hit)));
        }
    }
    if scope != CognitionScope::Project
        && let Some(engine) = bundle.global_engine.as_ref()
    {
        let request = vesper_cognition::SearchRequest {
            query,
            scope: user_scope,
            filters: None,
            top_k: 10,
            threshold: 0.05,
            explain: false,
            show_expired: false,
        };
        if let Ok(found) = engine.search(request) {
            hits.extend(found.into_iter().map(|hit| ("global", hit)));
        }
    }
    hits.sort_by(|left, right| right.1.score.total_cmp(&left.1.score));
    hits.truncate(10);
    if hits.is_empty() {
        return format!("cognition: no memories match \"{query}\"");
    }
    let mut out = format!(
        "cognition: {} memor{} recalled for \"{query}\":",
        hits.len(),
        if hits.len() == 1 { "y" } else { "ies" }
    );
    for (label, hit) in hits {
        out.push_str(&format!(
            "\n- [{label} · {} · {:.2}] {}",
            short_memory_id(&hit.id),
            hit.score,
            hit.memory.chars().take(120).collect::<String>()
        ));
    }
    out
}

fn forget_text(
    bundle: &CognitionBundle,
    user_scope: &vesper_cognition::Scope,
    scope: CognitionScope,
    id: &str,
) -> String {
    let mut deleted = Vec::new();
    let mut errors = Vec::new();
    if scope != CognitionScope::Global
        && let Some(engine) = bundle.engine.as_ref()
    {
        match delete_scoped_memory(engine, user_scope, id) {
            Ok(true) => deleted.push("project"),
            Ok(false) => {}
            Err(error) => errors.push(format!("project: {error}")),
        }
    }
    if scope != CognitionScope::Project
        && let Some(engine) = bundle.global_engine.as_ref()
    {
        match delete_scoped_memory(engine, user_scope, id) {
            Ok(true) => deleted.push("global"),
            Ok(false) => {}
            Err(error) => errors.push(format!("global: {error}")),
        }
    }
    if !errors.is_empty() {
        format!("cognition: /forget failed — {}", errors.join("; "))
    } else if deleted.is_empty() {
        format!("cognition: no memory matches ID {id}")
    } else {
        format!(
            "✓ Deleted memory {id} from {} scope{}",
            deleted.join(" and "),
            if deleted.len() == 1 { "" } else { "s" }
        )
    }
}

fn transfer_text(
    bundle: &CognitionBundle,
    user_scope: &vesper_cognition::Scope,
    id: &str,
    source_label: &str,
    destination_label: &str,
    promote: bool,
) -> Result<String, String> {
    let _ = promote;
    if id.trim().is_empty() {
        return Err(format!(
            "cognition: usage: /{} <id-prefix>",
            if promote { "promote" } else { "demote" }
        ));
    }
    let (source, destination) = if promote {
        (bundle.engine.as_ref(), bundle.global_engine.as_ref())
    } else {
        (bundle.global_engine.as_ref(), bundle.engine.as_ref())
    };
    let (Some(source), Some(destination)) = (source, destination) else {
        return Ok("cognition: one of the scoped memory stores is unavailable".to_owned());
    };
    let record = match find_scoped_memory(source, user_scope, id.trim()) {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Ok(format!(
                "cognition: no {source_label} memory matches ID {}",
                id.trim()
            ));
        }
        Err(error) => return Err(format!("cognition: {error}")),
    };
    match add_cognitive_memory(destination, user_scope, &record.data, false) {
        Ok(_) => match find_scoped_memory_by_data(destination, user_scope, &record.data) {
            Ok(Some(destination_record)) => match source.delete(&record.id) {
                Ok(()) => Ok(format!(
                    "✓ Moved [{}] from {source_label} to {destination_label} memory as [{}]",
                    short_memory_id(&record.id),
                    short_memory_id(&destination_record.id)
                )),
                Err(error) => Ok(format!(
                    "cognition: copied to {destination_label} as [{}], but could not remove \
                     {source_label} copy: {error}",
                    short_memory_id(&destination_record.id)
                )),
            },
            Ok(None) => Ok(format!(
                "cognition: transfer failed — {destination_label} copy could not be verified; \
                 {source_label} copy was kept"
            )),
            Err(error) => Ok(format!(
                "cognition: transfer failed while verifying {destination_label} copy: {error}; \
                 {source_label} copy was kept"
            )),
        },
        Err(error) => Err(format!("cognition: transfer failed: {error}")),
    }
}

fn embedding_set_text(bundle: &CognitionBundle, argument: &str) -> String {
    let mut config = EmbeddingConfig::load(&bundle.root);
    for pair in argument.split_whitespace() {
        let Some((key, value)) = pair.split_once('=') else {
            return format!("cognition: /embedding set expects key=value pairs, got `{pair}`");
        };
        match key {
            "source" => config.source = Some(value.to_owned()),
            "endpoint" => config.endpoint = Some(value.to_owned()),
            "model" => config.model = Some(value.to_owned()),
            "api_key" => config.api_key = (value != "null").then(|| value.to_owned()),
            "dimension" => match value.parse::<usize>() {
                Ok(dim) => config.dimension = Some(dim),
                Err(_) => return format!("cognition: dimension must be a number, got `{value}`"),
            },
            other => {
                return format!(
                    "cognition: unknown /embedding key `{other}` (expected source, endpoint, \
                     model, api_key, dimension)"
                );
            }
        }
    }
    let path = bundle.root.join("embedding.json");
    let body = serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".into());
    match std::fs::write(&path, body) {
        Ok(()) => format!(
            "✓ Embedding config saved to {}. It applies the next time the agent starts \
             (both the TUI and ACP hosts read this file).",
            path.display()
        ),
        Err(error) => format!("cognition: /embedding set failed: {error}"),
    }
}

// ---------------------------------------------------------------------------
// VRO-7 procedural-memory sink (Verified Workflow Learning)
// ---------------------------------------------------------------------------

/// Persists sanitized [`ProceduralMemory`] recipes into the project
/// cognitive store so learned workflows surface in later recalls.
pub struct CognitionProceduralSink(pub Arc<vesper_cognition::CognitiveMemory>);

impl vesper_agent::vro::ProceduralMemorySink for CognitionProceduralSink {
    fn save_procedure<'a>(
        &'a self,
        procedure: &'a vesper_agent::vro::ProceduralMemory,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<String, vesper_agent::vro::LearningError>,
                > + Send
                + 'a,
        >,
    > {
        let rendered = format!(
            "# {}\n\nObjective: {}\nStrategy: {}\nVerified: {}\nExtracted: {}\n",
            procedure.title,
            procedure.objective,
            procedure.source_strategy,
            procedure.verification_summary,
            procedure.extracted_at
        );
        let engine = Arc::clone(&self.0);
        let id = procedure.id.clone();
        Box::pin(async move {
            // Blocking pool: `add` embeds the recipe, and a network
            // embedder must never run inside an async worker.
            tokio::task::spawn_blocking(move || {
                // Stored raw (infer=false): the recipe is already sanitized
                // and generalized by the orchestrator's WorkflowExtractor.
                let scope = cognition_user_scope();
                let message = vesper_cognition::Message::user(rendered);
                let request = vesper_cognition::AddRequest {
                    messages: std::slice::from_ref(&message),
                    scope: &scope,
                    extras: None,
                    expiration_date: None,
                    infer: false,
                    custom_instructions: None,
                    observation_date: None,
                };
                engine
                    .add(request)
                    .map(|_| id.clone())
                    .map_err(|error| {
                        vesper_agent::vro::LearningError::SinkRejected(error.to_string())
                    })
            })
            .await
            .map_err(|error| {
                vesper_agent::vro::LearningError::SinkRejected(error.to_string())
            })?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A credential source that never resolves anything: keeps every test in
    /// this module off live providers (project contract: no live provider
    /// calls in verification) and forces the NoOp extractor + local
    /// embedder paths deterministically.
    struct NeverCredentialSource;

    impl vesper_provider_glm::GlmCredentialSource for NeverCredentialSource {
        fn credential(&self, _name: &str) -> Option<vesper_security::SecretValue> {
            None
        }
    }

    /// Opens a bundle against isolated leaked temp roots — no environment
    /// mutation at all (`open_at` takes every path explicitly).
    fn bundle_under_temp_root(source: &str) -> CognitionBundle {
        let project = tempfile::tempdir().unwrap().keep();
        let global = tempfile::tempdir().unwrap().keep();
        std::fs::write(
            project.join("embedding.json"),
            format!("{{\"source\": \"{source}\"}}"),
        )
        .unwrap();
        CognitionBundle::open_at(
            project,
            global,
            // No LM Studio settings: keeps the extractor on the NoOp path
            // (never the local server, never the network).
            None,
            Arc::new(NeverCredentialSource),
            "zai",
        )
    }

    #[test]
    fn zhipu_jwt_is_three_url_safe_segments() {
        let token = zhipu_jwt("id123.secret456").expect("jwt");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        );
    }

    #[test]
    fn zhipu_jwt_rejects_keys_without_separator() {
        assert!(zhipu_jwt("no-separator").is_none());
    }

    #[test]
    fn smart_scope_routes_identity_globally() {
        let (scope, reason) = smart_memory_scope("My name is Alex and I prefer dark mode");
        assert_eq!(scope, CognitionScope::Global);
        assert!(reason.contains("identity"));
        let (scope, _) = smart_memory_scope("The parser lives in src/parser.rs");
        assert_eq!(scope, CognitionScope::Project);
    }

    #[test]
    fn scope_flags_reject_conflicts_and_unknowns() {
        assert!(cognition_scope_and_body("--global --project x")
            .unwrap_err()
            .contains("only one"));
        assert!(cognition_scope_and_body("--bogus x")
            .unwrap_err()
            .contains("unknown memory scope flag"));
        let (scope, body) = cognition_scope_and_body("--global my name is Alex").unwrap();
        assert_eq!(scope, CognitionScope::Global);
        assert_eq!(body, "my name is Alex");
    }

    #[test]
    fn embedding_config_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = EmbeddingConfig {
            source: Some("local".into()),
            ..Default::default()
        };
        std::fs::write(
            dir.path().join("embedding.json"),
            serde_json::to_string(&cfg).unwrap(),
        )
        .unwrap();
        let loaded = EmbeddingConfig::load(dir.path());
        assert_eq!(loaded.source.as_deref(), Some("local"));
        assert!(loaded.overrides_provider_routing());
        assert!(!EmbeddingConfig::default().overrides_provider_routing());
    }

    #[test]
    fn non_cognitive_commands_return_none() {
        let bundle = bundle_under_temp_root("local");
        assert!(execute_cognition_slash("status", "", &bundle).is_none());
        assert!(execute_cognition_slash("checkpoint", "", &bundle).is_none());
    }

    #[test]
    fn remember_recall_forget_round_trip_against_real_store() {
        let bundle = bundle_under_temp_root("local");
        let remembered = execute_cognition_slash(
            "remember",
            "--project the ACP cognition test marker is acp-7f3a",
            &bundle,
        )
        .unwrap();
        assert!(remembered.starts_with("✓ Remembered"), "{remembered}");
        let recalled = execute_cognition_slash("recall", "acp-7f3a", &bundle).unwrap();
        assert!(recalled.contains("acp-7f3a"), "{recalled}");
        // Extract the id prefix from the audit listing, then forget it.
        let audit = execute_cognition_slash("memories", "acp-7f3a", &bundle).unwrap();
        let id = audit
            .lines()
            .find(|line| line.contains("acp-7f3a"))
            .and_then(|line| line.split('[').nth(1))
            .and_then(|rest| rest.split(']').next())
            .expect("id in audit")
            .to_owned();
        let forgotten = execute_cognition_slash("forget", &id, &bundle).unwrap();
        assert!(forgotten.starts_with("✓ Deleted"), "{forgotten}");
        // Store is empty again for this marker.
        let after = execute_cognition_slash("recall", "acp-7f3a", &bundle).unwrap();
        assert!(after.contains("no memories match"), "{after}");
    }

    #[test]
    fn cognitive_context_block_is_bounded_and_formatted() {
        let bundle = bundle_under_temp_root("local");
        execute_cognition_slash(
            "remember",
            "--project the context marker zeta-42 belongs to q",
            &bundle,
        )
        .unwrap();
        let context = cognitive_context_for_prompt(&bundle, "zeta-42");
        let context = context.expect("auto-recall hit");
        assert!(context.starts_with("\n\n--- Relevant context from cognitive memory"));
        assert!(context.contains("[project]"));
        assert!(context.len() < 2000 * 4 + 512);
    }

    #[test]
    fn embedding_set_writes_config_file() {
        let bundle = bundle_under_temp_root("local");
        let out = execute_cognition_slash("embedding", "set source=local dimension=1024", &bundle)
            .unwrap();
        assert!(out.starts_with("✓ Embedding config saved"), "{out}");
        let text = std::fs::read_to_string(bundle.root.join("embedding.json")).unwrap();
        assert!(text.contains("\"source\": \"local\""));
        assert!(text.contains("\"dimension\": 1024"));
    }

    #[test]
    fn embedding_unknown_key_is_rejected() {
        let bundle = bundle_under_temp_root("local");
        let out = execute_cognition_slash("embedding", "set bogus=1", &bundle).unwrap();
        assert!(out.contains("unknown /embedding key"), "{out}");
    }

    #[test]
    fn usage_errors_are_truthful() {
        let bundle = bundle_under_temp_root("local");
        assert!(execute_cognition_slash("remember", "", &bundle)
            .unwrap()
            .contains("usage:"));
        assert!(execute_cognition_slash("recall", "", &bundle)
            .unwrap()
            .contains("usage:"));
    }

    #[tokio::test]
    async fn procedural_sink_persists_the_recipe() {
        let bundle = bundle_under_temp_root("local");
        let engine = bundle.engine.as_ref().expect("engine").clone();
        let sink = CognitionProceduralSink(engine);
        let procedure = vesper_agent::vro::ProceduralMemory {
            id: "proc-test-1".to_owned(),
            title: "Verify then write".to_owned(),
            objective: "apply the acp sink marker s1nk-77 and verify it".to_owned(),
            source_strategy: "generate_verify_repair".to_owned(),
            steps: Vec::new(),
            model_calls: 1,
            total_tokens: 0,
            verification_summary: "passed".to_owned(),
            extracted_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let saved = vesper_agent::vro::ProceduralMemorySink::save_procedure(&sink, &procedure)
            .await
            .expect("sink accepted");
        assert_eq!(saved, "proc-test-1");
        // The recipe text is recallable from the project store.
        let recalled = execute_cognition_slash("recall", "s1nk-77", &bundle).unwrap();
        assert!(recalled.contains("s1nk-77"), "{recalled}");
    }
}
