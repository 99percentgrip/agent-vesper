//! LM Studio provider network configuration (VRO-3.1, PRD §13.1).
//!
//! [`LmStudioConfig`] holds the customizable network endpoint and optional API
//! key required for LAN/localhost deployments. The `api_base_url` is the
//! **full base including the version path** — it may be a standard
//! `/v1` endpoint or a custom path such as `/api/v0` — and the request
//! builders in [`super::client`] append the concrete endpoint (`/models`,
//! `/chat/completions`) to it.

use serde::{Deserialize, Serialize};

/// Opaque bearer-token API key for an LM Studio server.
///
/// Held behind a newtype (not a raw `String` field) to satisfy the workspace
/// secret-safety guard — secrets must not appear as raw serializable `String`
/// fields. Construct it at runtime from an environment variable or credential
/// store; it is intentionally **not** serialized into the config (see
/// [`LmStudioConfig::api_key`] `#[serde(skip)]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LmStudioApiKey(String);

impl LmStudioApiKey {
    /// Wraps a raw key value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the secret bytes for header injection.
    #[must_use]
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl From<String> for LmStudioApiKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for LmStudioApiKey {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Network + auth configuration for an LM Studio server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LmStudioConfig {
    /// Full base URL including the version path, e.g.
    /// `http://192.168.254.114:1234/v1` or `http://localhost:1234/api/v0`.
    /// Must be non-empty.
    pub api_base_url: String,
    /// Optional bearer-token API key. When `Some`, the request builders inject
    /// an `Authorization: Bearer <key>` header. Held opaquely and skipped by
    /// serde — keys come from a credential source/env, not the config file.
    #[serde(skip)]
    pub api_key: Option<LmStudioApiKey>,
    /// Optional explicit model id. When `None`, the adapter auto-discovers the
    /// loaded model via `/models`.
    #[serde(default)]
    pub model: Option<String>,
}

impl LmStudioConfig {
    /// Creates a config with the given base URL, no API key, no fixed model.
    ///
    /// Returns `Err` if `api_base_url` is empty.
    pub fn new(api_base_url: impl Into<String>) -> Result<Self, &'static str> {
        let api_base_url = api_base_url.into();
        if api_base_url.trim().is_empty() {
            return Err("api_base_url must not be empty");
        }
        Ok(Self {
            api_base_url,
            api_key: None,
            model: None,
        })
    }

    /// Builder: attaches a bearer-token API key (wrapped opaquely).
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<LmStudioApiKey>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Builder: pins a specific model id (skips discovery).
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// The pinned model id, if any.
    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_builders_chain() {
        let cfg = LmStudioConfig::new("http://192.168.254.114:1234/v1")
            .unwrap()
            .with_api_key("secret")
            .with_model("qwen3.6-27b");
        assert_eq!(cfg.api_base_url, "http://192.168.254.114:1234/v1");
        assert_eq!(
            cfg.api_key.as_ref().map(LmStudioApiKey::secret),
            Some("secret")
        );
        assert_eq!(cfg.model(), Some("qwen3.6-27b"));

        // The API key is `#[serde(skip)]` — secrets must not persist into a
        // serialized config — so the round-trip drops it while preserving the
        // non-secret fields.
        let encoded = serde_json::to_string(&cfg).unwrap();
        assert!(
            !encoded.contains("secret"),
            "the API key must not leak into the serialized config: {encoded}"
        );
        let decoded: LmStudioConfig = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.api_base_url, cfg.api_base_url);
        assert_eq!(decoded.model, cfg.model);
        assert_eq!(decoded.api_key, None, "the skipped key does not round-trip");
    }

    #[test]
    fn empty_base_url_is_rejected() {
        assert!(LmStudioConfig::new("   ").is_err());
        assert!(LmStudioConfig::new("").is_err());
    }
}
