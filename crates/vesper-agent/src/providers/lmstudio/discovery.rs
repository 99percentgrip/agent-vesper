//! Model discovery, health probing, and the [`CapabilityRegistry`] (VRO-3.1,
//! PRD §13.1, §10.2).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use vesper_domain::ModelCapabilities;

use super::client::{
    HttpMethod, LmStudioError, LmStudioHttpRequest, LmStudioTransport, build_models_request,
};

/// One discovered model from the LM Studio `/models` endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model id (e.g. `"qwen3.6-27b-instruct"`).
    pub id: String,
    /// Owner/source label, when reported.
    #[serde(default)]
    pub owned_by: Option<String>,
}

/// Result of a health probe against an LM Studio server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHealth {
    /// Whether the server responded to `/models` with a usable payload.
    pub reachable: bool,
    /// The first loaded model id, when one is present.
    pub loaded_model: Option<String>,
}

/// Parses an OpenAI-compatible `/models` response body into [`ModelInfo`]s.
pub fn parse_models_response(body: &serde_json::Value) -> Result<Vec<ModelInfo>, LmStudioError> {
    let data = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| LmStudioError::Parse("models response missing `data` array".into()))?;
    let mut models = Vec::with_capacity(data.len());
    for entry in data {
        let id = entry
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| LmStudioError::Parse("model entry missing `id`".into()))?
            .to_string();
        let owned_by = entry
            .get("owned_by")
            .and_then(|o| o.as_str())
            .map(str::to_string);
        models.push(ModelInfo { id, owned_by });
    }
    Ok(models)
}

/// Queries `/models` and returns the loaded model id (the first one).
///
/// Returns [`LmStudioError::NoModelLoaded`] when the server is reachable but
/// has no model loaded.
pub async fn discover_model(
    config: &super::config::LmStudioConfig,
    transport: &dyn LmStudioTransport,
) -> Result<ModelInfo, LmStudioError> {
    let models = discover_models(config, transport).await?;
    models
        .into_iter()
        .next()
        .ok_or(LmStudioError::NoModelLoaded)
}

/// Queries `/models` and returns every loaded model.
pub async fn discover_models(
    config: &super::config::LmStudioConfig,
    transport: &dyn LmStudioTransport,
) -> Result<Vec<ModelInfo>, LmStudioError> {
    let req = build_models_request(config);
    let resp = transport.send(&req).await?;
    if resp.status != 200 {
        return Err(LmStudioError::HttpStatus {
            status: resp.status,
        });
    }
    parse_models_response(&resp.body)
}

/// Performs a health probe: reachable iff `/models` returns 200.
pub async fn probe_health(
    config: &super::config::LmStudioConfig,
    transport: &dyn LmStudioTransport,
) -> ServerHealth {
    match discover_model(config, transport).await {
        Ok(info) => ServerHealth {
            reachable: true,
            loaded_model: Some(info.id),
        },
        Err(LmStudioError::NoModelLoaded) => ServerHealth {
            reachable: true,
            loaded_model: None,
        },
        Err(_) => ServerHealth {
            reachable: false,
            loaded_model: None,
        },
    }
}

/// Probes the capabilities of a discovered model.
///
/// VRO-3.1 returns [`ModelCapabilities::local_server_defaults`] for any locally
/// served model (PRD §13.2 warns against assuming exact card behavior across
/// GGUF quantizations — the empirical certification suite that refines these
/// flags per-model lands in VRO-3+). Override individual flags here once the
/// probe has model-specific evidence.
#[must_use]
pub fn probe_capabilities(_info: &ModelInfo) -> ModelCapabilities {
    ModelCapabilities::local_server_defaults()
}

/// Maps a discovered model id to its observed [`ModelCapabilities`]
/// (PRD §10.2).
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    entries: HashMap<String, ModelCapabilities>,
}

impl CapabilityRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records capabilities for a model id.
    pub fn register(&mut self, model_id: impl Into<String>, capabilities: ModelCapabilities) {
        self.entries.insert(model_id.into(), capabilities);
    }

    /// Looks up capabilities for a model id.
    #[must_use]
    pub fn get(&self, model_id: &str) -> Option<ModelCapabilities> {
        self.entries.get(model_id).copied()
    }

    /// Probes and registers capabilities for a discovered model, returning the
    /// probed capabilities.
    pub fn probe_and_register(&mut self, info: &ModelInfo) -> ModelCapabilities {
        let caps = probe_capabilities(info);
        self.register(info.id.clone(), caps);
        caps
    }

    /// Returns the registered model ids.
    #[must_use]
    pub fn model_ids(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }
}

/// Builds the health-probe request (exposed for tests/inspection).
#[must_use]
pub fn build_health_request(config: &super::config::LmStudioConfig) -> LmStudioHttpRequest {
    let mut req = build_models_request(config);
    // Health probe reuses /models; method stays GET.
    req.method = HttpMethod::Get;
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_models_response_extracts_id_and_owner() {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "qwen3.6-27b", "object": "model", "owned_by": "local"},
                {"id": "phi-4", "object": "model"}
            ]
        });
        let models = parse_models_response(&body).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "qwen3.6-27b");
        assert_eq!(models[0].owned_by.as_deref(), Some("local"));
        assert_eq!(models[1].owned_by, None);
    }

    #[test]
    fn parse_models_response_rejects_missing_data() {
        assert!(parse_models_response(&serde_json::json!({"error": "x"})).is_err());
    }

    #[test]
    fn capability_registry_probe_and_get_round_trips() {
        let mut registry = CapabilityRegistry::new();
        let info = ModelInfo {
            id: "qwen3.6-27b".into(),
            owned_by: None,
        };
        let caps = registry.probe_and_register(&info);
        // Local-server defaults: system prompts + streaming on; native tools off.
        assert!(caps.supports_system_prompts);
        assert!(!caps.supports_native_tools);
        let fetched = registry.get("qwen3.6-27b").unwrap();
        assert_eq!(fetched, caps);
        assert!(registry.model_ids().contains(&"qwen3.6-27b".to_string()));
        assert!(registry.get("unknown").is_none());
    }
}
