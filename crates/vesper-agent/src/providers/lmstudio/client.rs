//! LM Studio HTTP client surface (VRO-3.1, PRD §13.1).
//!
//! Separates **request construction** (pure, fully unit-testable, no HTTP
//! dependency) from **request transport** (an async trait port mocked in tests;
//! the real HTTP-backed transport is supplied at the composition boundary,
//! matching the codebase's trait-port discipline). This keeps `vesper-agent`
//! dependency-light and lets the directive's tests prove URL/auth/body shaping
//! without any live network.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::config::LmStudioConfig;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failures raised by the LM Studio client surface.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LmStudioError {
    /// The transport itself failed (connection refused, timeout, DNS, …).
    #[error("transport error: {0}")]
    Transport(String),
    /// The server replied with an unexpected HTTP status.
    #[error("unexpected HTTP status {status}")]
    HttpStatus { status: u16 },
    /// The response body could not be parsed into the expected shape.
    #[error("response parse error: {0}")]
    Parse(String),
    /// `/models` returned no loaded model.
    #[error("no model loaded on the LM Studio server")]
    NoModelLoaded,
}

// ---------------------------------------------------------------------------
// HTTP request / response data
// ---------------------------------------------------------------------------

/// HTTP method carried by an [`LmStudioHttpRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

/// A fully-constructed HTTP request ready to send. Pure data — every field is
/// inspectable by tests (URL, headers, body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LmStudioHttpRequest {
    /// HTTP method.
    pub method: HttpMethod,
    /// Fully-qualified URL (the custom `api_base_url` + endpoint).
    pub url: String,
    /// Headers, as `(name, value)` pairs. Authorization is injected here when
    /// `api_key` is set.
    pub headers: Vec<(String, String)>,
    /// Serialized JSON body (`None` for GET requests).
    pub body: Option<String>,
}

/// A parsed HTTP response.
#[derive(Debug, Clone, PartialEq)]
pub struct LmStudioHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Parsed JSON body.
    pub body: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Transport port (mockable)
// ---------------------------------------------------------------------------

/// Async transport seam for LM Studio HTTP requests.
///
/// The real implementation (an HTTP-backed client) lives at the composition
/// boundary; tests inject a fake. Object-safe via a boxed `Send` future (the
/// workspace has no `async_trait` dependency — same pattern as the VRO
/// [`Verifier`](crate::vro::Verifier) and
/// [`CandidateGenerator`](crate::vro::CandidateGenerator) traits).
pub trait LmStudioTransport: Send + Sync {
    /// Sends `req` and returns the parsed response, or a transport-level error.
    fn send<'a>(
        &'a self,
        req: &'a LmStudioHttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LmStudioHttpResponse, LmStudioError>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// Pure request builders (the testable core)
// ---------------------------------------------------------------------------

/// Joins a base URL (which may end with `/`) and an endpoint path into a single
/// URL, collapsing any doubled separator.
#[must_use]
pub fn join_url(base: &str, endpoint: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}/{endpoint}")
}

/// Builds the `Authorization` header(s) for a config: a `Bearer` token when
/// `api_key` is `Some`, otherwise none.
#[must_use]
pub fn auth_headers(config: &LmStudioConfig) -> Vec<(String, String)> {
    match &config.api_key {
        Some(key) => vec![(
            "authorization".to_string(),
            format!("Bearer {}", key.secret()),
        )],
        None => Vec::new(),
    }
}

/// Builds the `GET {api_base_url}/models` discovery request.
#[must_use]
pub fn build_models_request(config: &LmStudioConfig) -> LmStudioHttpRequest {
    LmStudioHttpRequest {
        method: HttpMethod::Get,
        url: join_url(&config.api_base_url, "models"),
        headers: auth_headers(config),
        body: None,
    }
}

/// A chat-completions message in LM Studio's OpenAI-compatible payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Builds the `POST {api_base_url}/chat/completions` request for a set of
/// messages.
///
/// The body is the OpenAI-compatible chat-completions JSON:
/// `{ "model": …, "messages": …, "stream": false }`.
#[must_use]
pub fn build_chat_request(
    config: &LmStudioConfig,
    model: &str,
    messages: &[ChatMessage],
) -> LmStudioHttpRequest {
    let mut headers = auth_headers(config);
    headers.push(("content-type".to_string(), "application/json".to_string()));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });
    LmStudioHttpRequest {
        method: HttpMethod::Post,
        url: join_url(&config.api_base_url, "chat/completions"),
        headers,
        body: Some(body.to_string()),
    }
}

/// Extracts the assistant text and completion-token count from an OpenAI-
/// compatible chat-completions response. Returns `(content, completion_tokens)`.
///
/// Returns `Err(LmStudioError::Parse)` if the expected shape is absent.
pub fn parse_chat_response(
    response: &LmStudioHttpResponse,
) -> Result<(String, u64), LmStudioError> {
    let content = response
        .body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| LmStudioError::Parse("missing choices[0].message.content".into()))?
        .to_string();
    let completion_tokens = response
        .body
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    Ok((content, completion_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lan_config() -> LmStudioConfig {
        LmStudioConfig::new("http://192.168.254.114:1234/v1")
            .unwrap()
            .with_api_key("lan-secret")
    }

    #[test]
    fn models_request_uses_custom_lan_url_and_bearer_header() {
        let req = build_models_request(&lan_config());
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.url, "http://192.168.254.114:1234/v1/models");
        assert_eq!(
            req.headers,
            vec![("authorization".to_string(), "Bearer lan-secret".to_string())]
        );
        assert!(req.body.is_none());
    }

    #[test]
    fn models_request_has_no_auth_header_when_api_key_absent() {
        let cfg = LmStudioConfig::new("http://localhost:1234/v1").unwrap();
        let req = build_models_request(&cfg);
        assert_eq!(req.url, "http://localhost:1234/v1/models");
        assert!(
            req.headers.is_empty(),
            "no API key ⇒ no Authorization header"
        );
    }

    #[test]
    fn custom_api_path_is_preserved() {
        // Directive example: a /api/v0 base path must round through unchanged.
        let cfg = LmStudioConfig::new("http://192.168.1.5:1234/api/v0")
            .unwrap()
            .with_api_key("k");
        let req = build_chat_request(&cfg, "m", &[]);
        assert_eq!(req.url, "http://192.168.1.5:1234/api/v0/chat/completions");
        assert_eq!(
            req.headers.iter().find(|(n, _)| n == "authorization"),
            Some(&("authorization".to_string(), "Bearer k".to_string()))
        );
    }

    #[test]
    fn join_url_collapses_trailing_slash() {
        assert_eq!(join_url("http://h:1/v1/", "models"), "http://h:1/v1/models");
        assert_eq!(join_url("http://h:1/v1", "models"), "http://h:1/v1/models");
    }

    #[test]
    fn chat_request_body_carries_model_and_messages() {
        let req = build_chat_request(
            &lan_config(),
            "qwen3.6-27b",
            &[
                ChatMessage {
                    role: "user".into(),
                    content: "hello".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "fix this".into(),
                },
            ],
        );
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "http://192.168.254.114:1234/v1/chat/completions");
        let body: serde_json::Value = serde_json::from_str(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["model"], "qwen3.6-27b");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn parse_chat_response_extracts_content_and_tokens() {
        let resp = LmStudioHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "42"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 1}
            }),
        };
        let (content, tokens) = parse_chat_response(&resp).unwrap();
        assert_eq!(content, "42");
        assert_eq!(tokens, 1);
    }

    #[test]
    fn parse_chat_response_errors_on_missing_choice() {
        let resp = LmStudioHttpResponse {
            status: 200,
            body: serde_json::json!({"error": "bad model"}),
        };
        assert!(parse_chat_response(&resp).is_err());
    }
}
