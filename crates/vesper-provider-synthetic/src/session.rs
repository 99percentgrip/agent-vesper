//! `ProviderSession` implementation for the synthetic adapter.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_core::Stream;
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ErrorCategory, ErrorInfo, ExtensionMap, FinishOutcome,
    NormalizedUsage, ProviderId, RedactedDiagnostics, Retryability, SafeMessage, UsageMode,
};
use vesper_provider::{
    CancellationSignal, ProviderError, ProviderEventStream, ProviderFuture, ProviderRequest,
    ProviderSession, ProviderStreamEvent,
};

/// Deterministic provider session streaming a configured reply.
pub struct SyntheticSession {
    provider_id: ProviderId,
    reply: String,
}

impl SyntheticSession {
    /// Creates a session that streams `reply` for every turn.
    #[must_use]
    pub(crate) fn new(provider_id: ProviderId, reply: String) -> Self {
        Self { provider_id, reply }
    }

    /// Provider identity owning this session.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
}

impl std::fmt::Debug for SyntheticSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyntheticSession")
            .field("provider_id", &self.provider_id)
            .finish_non_exhaustive()
    }
}

impl ProviderSession for SyntheticSession {
    fn start<'a>(
        &'a self,
        _request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        let reply = self.reply.clone();
        let provider_id = self.provider_id.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(cancellation_error(&provider_id));
            }
            let stream = SyntheticStream::for_reply(&provider_id, &reply);
            Ok(Box::pin(stream) as ProviderEventStream)
        })
    }
}

/// Manually implemented ordered stream so the adapter adds no runtime or
/// utility dependency beyond the `Stream` contract it must satisfy.
struct SyntheticStream {
    events: VecDeque<Result<ProviderStreamEvent, ProviderError>>,
}

impl SyntheticStream {
    /// Builds the deterministic event sequence for one successful turn.
    fn for_reply(provider_id: &ProviderId, reply: &str) -> Self {
        let mut events: VecDeque<Result<ProviderStreamEvent, ProviderError>> = VecDeque::new();
        events.push_back(Ok(ProviderStreamEvent::ResponseStarted {
            response_id: None,
            metadata: ExtensionMap::default(),
        }));
        if let Ok(text) = ContentText::new(reply) {
            events.push_back(Ok(ProviderStreamEvent::ContentDelta {
                stream_id: BoundedString::new("content").expect("bounded stream id"),
                part: ContentPart::Text(text),
            }));
        }
        events.push_back(Ok(ProviderStreamEvent::Usage(
            NormalizedUsage::unavailable(UsageMode::Cumulative),
        )));
        events.push_back(Ok(ProviderStreamEvent::Completed {
            finish: FinishOutcome::Stop,
            metadata: ExtensionMap::default(),
        }));
        // `provider_id` is intentionally unused beyond ownership of the
        // response; future variants (warnings, rate limits) would carry it.
        let _ = provider_id;
        Self { events }
    }
}

impl Stream for SyntheticStream {
    type Item = Result<ProviderStreamEvent, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.events.pop_front())
    }
}

fn cancellation_error(provider_id: &ProviderId) -> ProviderError {
    ProviderError {
        provider_id: provider_id.clone(),
        provider_code: None,
        http_status: None,
        continuation_possible: false,
        info: ErrorInfo {
            category: ErrorCategory::Cancellation,
            retryability: Retryability::Never,
            retry_after_ms: None,
            visible_output_emitted: false,
            safe_message: SafeMessage::new("synthetic request cancelled")
                .expect("bounded static message"),
            diagnostics: RedactedDiagnostics::default(),
            provider_code: None,
            causes: Vec::new(),
        },
        metadata: ExtensionMap::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_id;
    use futures_util::StreamExt;
    struct NeverCancelled;
    impl CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    struct AlreadyCancelled;
    impl CancellationSignal for AlreadyCancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    fn empty_request() -> ProviderRequest {
        ProviderRequest {
            request_id: vesper_domain::ProviderRequestId::new("test-1").unwrap(),
            provider_id: provider_id(),
            model: vesper_domain::QualifiedModelId {
                provider_id: provider_id(),
                model_id: vesper_domain::ModelId::new("synthetic-1").unwrap(),
            },
            endpoint_id: None,
            system_instructions: Vec::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            tool_choice: vesper_provider::ToolChoice::None,
            capabilities: Vec::new(),
            reasoning: None,
            structured_output: vesper_provider::StructuredOutputIntent::None,
            sampling: None,
            maximum_output_tokens: None,
            continuation: None,
            fallback_policy: vesper_provider::FallbackPolicy::Strict,
            provider_extensions: None,
        }
    }

    #[tokio::test]
    async fn session_streams_ordered_events_with_one_terminal_completion() {
        let session = SyntheticSession::new(provider_id(), "hello-synthetic".to_owned());
        let mut stream = match session
            .start(
                empty_request(),
                Arc::new(NeverCancelled) as Arc<dyn CancellationSignal>,
            )
            .await
        {
            Ok(stream) => stream,
            Err(_) => panic!("start succeeds when not cancelled"),
        };

        let mut completions = 0;
        let mut saw_content = false;
        while let Some(event) = stream.next().await {
            match event {
                Ok(ProviderStreamEvent::ContentDelta { .. }) => saw_content = true,
                Ok(ProviderStreamEvent::Completed { finish, .. }) => {
                    assert_eq!(finish, FinishOutcome::Stop);
                    completions += 1;
                }
                Ok(_) => {}
                Err(_) => panic!("synthetic stream must not error when not cancelled"),
            }
        }
        assert!(saw_content, "stream must carry the configured reply");
        assert_eq!(completions, 1, "exactly one terminal completion");
    }

    #[tokio::test]
    async fn session_surfaces_cancellation_as_a_classified_error() {
        let session = SyntheticSession::new(provider_id(), "ignored".to_owned());
        let error = match session
            .start(
                empty_request(),
                Arc::new(AlreadyCancelled) as Arc<dyn CancellationSignal>,
            )
            .await
        {
            Ok(_) => panic!("cancelled start must surface an error"),
            Err(error) => error,
        };
        assert_eq!(error.info.category, ErrorCategory::Cancellation);
        assert_eq!(error.info.retryability, Retryability::Never);
    }
}
