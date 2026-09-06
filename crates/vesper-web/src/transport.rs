//! VRO-14 PR-3: the fetch transport port — the *only* seam where fetched
//! web content enters the perception pipeline.
//!
//! This module defines the trait and its data shapes; it performs **no
//! I/O whatsoever**. The production implementation lives in
//! `vesper-web-fetch` (`WebSandboxPort`), which routes every request
//! through a sandbox backend demanding `IsolationRequirement::Network`
//! with an explicit network grant (PRD Feature 4). The harness process
//! never performs a web fetch itself.
//!
//! A pure pre-flight egress gate ([`crate::egress`]) must clear every
//! request *before* any sandbox is provisioned — denial by policy is
//! cheaper and safer than denial by containment.

/// One fetch request, fully resolved by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    /// Absolute http(s) URL.
    pub url: String,
    /// Wall-clock bound for the whole fetch (redirects included).
    pub timeout_seconds: u64,
    /// Hard byte cap for the response body.
    pub max_body_bytes: usize,
}

impl FetchRequest {
    /// A request with the default budget (120 s, 64 KiB — the same cap the
    /// sandbox backend enforces per stream).
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            timeout_seconds: 120,
            max_body_bytes: 64 * 1024,
        }
    }
}

/// A fetched page, ready for the perception pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    /// Final URL after redirects.
    pub url: String,
    /// Body decoded to text (charset already applied).
    pub body: String,
    /// Content-Type as reported (sniffed when absent).
    pub content_type: String,
    /// Whether the body was truncated at `max_body_bytes`.
    pub truncated: bool,
}

/// Why a fetch did not produce a response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The egress gate rejected the URL before any provisioning.
    #[error("egress denied: {0}")]
    Egress(String),
    /// The sandbox backend cannot provide the demanded isolation.
    #[error("sandbox unavailable: {0}")]
    Sandbox(String),
    /// The fetch helper failed inside the sandbox.
    #[error("fetch failed: {0}")]
    Fetch(String),
    /// The response exceeded the configured cap.
    #[error("response too large: {0}")]
    TooLarge(String),
}

pub use crate::egress::{EgressDenial, EgressPolicy, EgressVerdict};

/// The transport seam. Object-safe; hosts inject the sandbox-backed
/// implementation at the composition boundary.
pub trait FetchTransport: Send + Sync {
    /// Fetch one URL. Implementations MUST run the pure egress gate first
    /// and MUST NOT perform any network I/O outside a sandbox provisioned
    /// with a network grant for this request.
    fn fetch(&self, request: &FetchRequest) -> BoxFuture<'_, Result<FetchResponse, FetchError>>;
}

/// Alias for the boxed future shape used across the workspace (no
/// `async_trait` dependency; mirrors `vesper-agent`'s seam style).
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_carries_default_budget() {
        let request = FetchRequest::new("https://example.com/x");
        assert_eq!(request.url, "https://example.com/x");
        assert_eq!(request.timeout_seconds, 120);
        assert_eq!(request.max_body_bytes, 64 * 1024);
    }

    #[test]
    fn errors_render_model_facable_text() {
        let e = FetchError::Egress("private address".into());
        assert!(e.to_string().contains("egress denied"));
        let s = FetchError::Sandbox("probe failed".into());
        assert!(s.to_string().contains("sandbox unavailable"));
    }
}
