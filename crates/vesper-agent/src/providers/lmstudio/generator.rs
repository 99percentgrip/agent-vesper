//! LM Studio [`CandidateGenerator`](crate::vro::CandidateGenerator) adapter
//! (VRO-3.1, PRD §13.1).
//!
//! [`LmStudioCandidateGenerator`] implements the VRO-2.3 generation seam: it
//! turns the orchestrator's `(prompt, corrections)` into an OpenAI-compatible
//! `/chat/completions` request against the configured `api_base_url`, injects
//! the bearer API key, sends it via the [`LmStudioTransport`] port, and parses
//! the response into a [`GeneratedCandidate`]. The
//! [`CandidateGenerator`](crate::vro::CandidateGenerator) trait is infallible,
//! so transport/parse failures degrade to a degenerate candidate (the GVR loop
//! then surfaces them via the verifiers).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use vesper_domain::{InferenceCost, ModelCapabilities, VerificationFinding, VerificationSeverity};
// Re-import the VRO seam types from this crate.
use crate::vro::{CandidateGenerator, GeneratedCandidate};

use super::client::{ChatMessage, LmStudioTransport, build_chat_request, parse_chat_response};
use super::config::LmStudioConfig;

/// System prompt used when the model supports the system role.
const SYSTEM_PROMPT: &str = "You are a precise assistant. Produce output that satisfies the stated constraints and passes automated verification.";

/// An LM Studio-backed [`CandidateGenerator`].
#[derive(Clone)]
pub struct LmStudioCandidateGenerator {
    config: LmStudioConfig,
    model: String,
    capabilities: ModelCapabilities,
    transport: Arc<dyn LmStudioTransport>,
}

impl LmStudioCandidateGenerator {
    /// Creates a generator pinned to `model` with probed `capabilities`, using
    /// `transport` to reach the server.
    #[must_use]
    pub fn new(
        config: LmStudioConfig,
        model: impl Into<String>,
        capabilities: ModelCapabilities,
        transport: Arc<dyn LmStudioTransport>,
    ) -> Self {
        Self {
            config,
            model: model.into(),
            capabilities,
            transport,
        }
    }

    /// The model id this generator targets.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The observed capabilities.
    #[must_use]
    pub fn capabilities(&self) -> ModelCapabilities {
        self.capabilities
    }

    /// Builds the chat message list for a `(prompt, corrections)` pair.
    ///
    /// - A system message (when the model supports the system role).
    /// - The user prompt.
    /// - A corrections message enumerating each [`VerificationFinding`] with its
    ///   severity and location, so the model produces a targeted repair
    ///   (PRD §10.9 exact failure evidence).
    ///
    /// Pure and unit-testable independently of the transport.
    #[must_use]
    pub fn build_messages(
        prompt: &str,
        corrections: &[VerificationFinding],
        supports_system_prompts: bool,
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(2 + corrections.len().min(1));
        if supports_system_prompts {
            messages.push(ChatMessage {
                role: "system".into(),
                content: SYSTEM_PROMPT.into(),
            });
        }
        messages.push(ChatMessage {
            role: "user".into(),
            content: prompt.into(),
        });
        if !corrections.is_empty() {
            let body = corrections
                .iter()
                .enumerate()
                .map(|(idx, finding)| {
                    let loc = finding
                        .location
                        .as_deref()
                        .map(|l| format!(" ({l})"))
                        .unwrap_or_default();
                    format!(
                        "{}. [{}] {}{loc}",
                        idx + 1,
                        severity_label(finding.severity),
                        finding.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "Your previous attempt failed verification. Fix these findings:\n{body}"
                ),
            });
        }
        messages
    }
}

impl CandidateGenerator for LmStudioCandidateGenerator {
    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        corrections: &'a [VerificationFinding],
    ) -> Pin<Box<dyn Future<Output = GeneratedCandidate> + Send + 'a>> {
        Box::pin(async move {
            let messages = Self::build_messages(
                prompt,
                corrections,
                self.capabilities.supports_system_prompts,
            );
            let req = build_chat_request(&self.config, &self.model, &messages);
            match self.transport.send(&req).await {
                Ok(response) => match parse_chat_response(&response) {
                    Ok((content, completion_tokens)) => GeneratedCandidate {
                        output: serde_json::json!({ "content": content }),
                        cost: InferenceCost {
                            model_calls: 1,
                            total_tokens: completion_tokens,
                        },
                    },
                    Err(err) => degenerate_candidate(&err.to_string()),
                },
                Err(err) => degenerate_candidate(&err.to_string()),
            }
        })
    }

    fn boxed_clone(&self) -> Box<dyn CandidateGenerator> {
        // LmStudioCandidateGenerator derives Clone: the `transport` is an
        // `Arc` (cheap clone), the rest is plain data. Each VRO-4 parallel
        // branch therefore gets its own generator that shares the same
        // connection Arc — but no per-branch state can leak because the
        // generator itself is stateless.
        Box::new(self.clone())
    }
}

fn severity_label(severity: VerificationSeverity) -> &'static str {
    match severity {
        VerificationSeverity::Info => "info",
        VerificationSeverity::Warning => "warning",
        VerificationSeverity::Error => "error",
        VerificationSeverity::Critical => "critical",
    }
}

fn degenerate_candidate(reason: &str) -> GeneratedCandidate {
    GeneratedCandidate {
        output: serde_json::json!({ "error": reason }),
        cost: InferenceCost::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::lmstudio::client::{
        HttpMethod, LmStudioHttpRequest, LmStudioHttpResponse,
    };
    use std::sync::Mutex;
    use vesper_domain::VerificationFinding;

    /// Capturing fake transport: records every request and returns a programmed
    /// response.
    struct CapturingTransport {
        requests: Mutex<Vec<LmStudioHttpRequest>>,
        response: LmStudioHttpResponse,
    }

    impl CapturingTransport {
        fn new(response: LmStudioHttpResponse) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                response,
            }
        }

        fn captured(&self) -> Vec<LmStudioHttpRequest> {
            self.requests.lock().expect("poisoned").clone()
        }
    }

    impl LmStudioTransport for CapturingTransport {
        fn send<'a>(
            &'a self,
            req: &'a LmStudioHttpRequest,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<LmStudioHttpResponse, super::super::client::LmStudioError>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                self.requests.lock().expect("poisoned").push(req.clone());
                Ok(self.response.clone())
            })
        }
    }

    fn ok_chat_response(content: &str, tokens: u64) -> LmStudioHttpResponse {
        LmStudioHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"completion_tokens": tokens}
            }),
        }
    }

    fn finding(
        message: &str,
        severity: VerificationSeverity,
        location: Option<&str>,
    ) -> VerificationFinding {
        VerificationFinding {
            message: message.into(),
            severity,
            location: location.map(str::to_string),
        }
    }

    #[test]
    fn build_messages_includes_system_when_supported_and_omits_when_not() {
        let with_sys = LmStudioCandidateGenerator::build_messages("hi", &[], true);
        assert_eq!(with_sys[0].role, "system");
        assert_eq!(with_sys[1].role, "user");

        let without_sys = LmStudioCandidateGenerator::build_messages("hi", &[], false);
        assert_eq!(without_sys.len(), 1);
        assert_eq!(without_sys[0].role, "user");
    }

    #[test]
    fn chat_request_carries_injected_verification_corrections() {
        // Directive test: prove the generation request body contains the
        // injected VerificationFinding corrections.
        let config = LmStudioConfig::new("http://192.168.254.114:1234/v1")
            .unwrap()
            .with_api_key("lan-secret");
        let corrections = vec![
            finding(
                "missing semicolon",
                VerificationSeverity::Error,
                Some("src/lib.rs:7"),
            ),
            finding("unused import", VerificationSeverity::Warning, None),
        ];
        let messages =
            LmStudioCandidateGenerator::build_messages("fix the bug", &corrections, true);
        let req = build_chat_request(&config, "qwen3.6-27b", &messages);

        // URL + bearer header preserved.
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "http://192.168.254.114:1234/v1/chat/completions");
        assert_eq!(
            req.headers.iter().find(|(n, _)| n == "authorization"),
            Some(&("authorization".to_string(), "Bearer lan-secret".to_string()))
        );

        // The corrections appear in the body: the user prompt, then the
        // corrections message enumerating both findings.
        let body: serde_json::Value = serde_json::from_str(req.body.as_deref().unwrap()).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "system + user + corrections");
        let correction_msg = msgs[2]["content"].as_str().unwrap();
        assert!(correction_msg.contains("missing semicolon"));
        assert!(correction_msg.contains("src/lib.rs:7"));
        assert!(correction_msg.contains("unused import"));
        assert!(correction_msg.contains("[error]"));
        assert!(correction_msg.contains("[warning]"));
    }

    #[tokio::test]
    async fn generate_sends_request_and_parses_response_into_candidate() {
        let config = LmStudioConfig::new("http://localhost:1234/v1")
            .unwrap()
            .with_model("qwen3.6-27b");
        let transport = Arc::new(CapturingTransport::new(ok_chat_response("the answer", 5)));
        let generator = LmStudioCandidateGenerator::new(
            config,
            "qwen3.6-27b",
            ModelCapabilities::local_server_defaults(),
            transport.clone(),
        );

        let candidate = generator.generate("solve it", &[]).await;

        // Response parsed into the candidate.
        assert_eq!(candidate.output["content"], "the answer");
        assert_eq!(candidate.cost.model_calls, 1);
        assert_eq!(candidate.cost.total_tokens, 5);

        // Exactly one POST was sent to the chat-completions endpoint.
        let captured = transport.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].method, HttpMethod::Post);
        assert_eq!(captured[0].url, "http://localhost:1234/v1/chat/completions");
    }

    #[tokio::test]
    async fn generate_degrades_to_error_candidate_on_transport_failure() {
        // The CandidateGenerator trait is infallible; a transport failure must
        // surface as a degenerate candidate (caught downstream by verifiers).
        struct FailingTransport;
        impl LmStudioTransport for FailingTransport {
            fn send<'a>(
                &'a self,
                _req: &'a LmStudioHttpRequest,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = Result<
                                LmStudioHttpResponse,
                                super::super::client::LmStudioError,
                            >,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async move {
                    Err(super::super::client::LmStudioError::Transport(
                        "connection refused".into(),
                    ))
                })
            }
        }
        let generator = LmStudioCandidateGenerator::new(
            LmStudioConfig::new("http://localhost:1234/v1").unwrap(),
            "m",
            ModelCapabilities::local_server_defaults(),
            Arc::new(FailingTransport),
        );
        let candidate = generator.generate("hi", &[]).await;
        assert!(candidate.output.get("error").is_some());
        assert_eq!(candidate.cost.model_calls, 0);
    }
}
