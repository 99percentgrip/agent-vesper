use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::SystemTime,
};

use futures_core::Stream;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ErrorCategory, ExtensionMap, FinishOutcome,
    NormalizedUsage, ProviderResponseId, Retryability, SafeMessage, StreamInterruptionCause,
    ToolId, UsageMode,
};
use vesper_provider::{
    CancellationSignal, ProviderError, ProviderEventStream, ProviderRequest, ProviderSession,
    ProviderStreamEvent, requirement_for_messages,
};
use vesper_security::SecretValue;

use crate::{
    GlmConfig, SerializedGlmRequest,
    error::{adapter_error, provider_error},
    request::serialize_request,
    response::{AttemptState, finish_outcome, tool_id_map},
    retry::{JitterSource, RetryPolicy, SystemJitter},
    transport::{AttemptFailure, GlmHttpClient, wait_cancelled},
};

const EVENT_CHANNEL_CAPACITY: usize = 32;
const CONTINUATION_MESSAGE: &str =
    "Continue exactly where you left off. Do not repeat or summarize.";

/// Production GLM provider session.
pub struct GlmSession {
    pub(crate) config: GlmConfig,
    pub(crate) credential: Arc<SecretValue>,
    pub(crate) http: GlmHttpClient,
    pub(crate) retry: RetryPolicy,
    pub(crate) jitter: Arc<dyn JitterSource>,
}

impl std::fmt::Debug for GlmSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GlmSession")
            .field("config", &self.config)
            .field("credential", &"<redacted>")
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

impl GlmSession {
    pub(crate) fn new(
        config: GlmConfig,
        credential: SecretValue,
    ) -> Result<Self, crate::GlmAdapterError> {
        let http = GlmHttpClient::build(&config)?;
        Ok(Self {
            config,
            credential: Arc::new(credential),
            http,
            retry: RetryPolicy::default(),
            jitter: Arc::new(SystemJitter),
        })
    }

    /// Constructs a scoped session from already validated configuration and an
    /// explicitly supplied secret. Applications normally use `GlmFactory`;
    /// deterministic integration tests use this boundary without process
    /// environment access.
    pub fn from_config(
        config: GlmConfig,
        credential: SecretValue,
    ) -> Result<Self, crate::GlmAdapterError> {
        config.validate()?;
        Self::new(config, credential)
    }

    /// Adapter configuration.
    #[must_use]
    pub const fn config(&self) -> &GlmConfig {
        &self.config
    }
}

impl ProviderSession for GlmSession {
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> vesper_provider::ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                let stream = ReceiverStream::single(Ok(ProviderStreamEvent::Completed {
                    finish: FinishOutcome::Cancelled,
                    metadata: ExtensionMap::default(),
                }));
                return Ok(Box::pin(stream) as ProviderEventStream);
            }
            let serialized = serialize_request(&request, &self.config)
                .map_err(|error| adapter_error(&error, false))?;
            let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
            let operation = Operation {
                config: self.config.clone(),
                credential: Arc::clone(&self.credential),
                http: self.http.clone(),
                retry: self.retry,
                jitter: Arc::clone(&self.jitter),
                request,
                serialized,
                cancellation,
                sender,
            };
            tokio::spawn(operation.run());
            Ok(Box::pin(ReceiverStream { receiver }) as ProviderEventStream)
        })
    }
}

struct ReceiverStream {
    receiver: mpsc::Receiver<Result<ProviderStreamEvent, ProviderError>>,
}

impl ReceiverStream {
    fn single(item: Result<ProviderStreamEvent, ProviderError>) -> Self {
        let (sender, receiver) = mpsc::channel(1);
        sender.try_send(item).expect("new receiver has capacity");
        Self { receiver }
    }
}

impl Stream for ReceiverStream {
    type Item = Result<ProviderStreamEvent, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context)
    }
}

struct Operation {
    config: GlmConfig,
    credential: Arc<SecretValue>,
    http: GlmHttpClient,
    retry: RetryPolicy,
    jitter: Arc<dyn JitterSource>,
    request: ProviderRequest,
    serialized: SerializedGlmRequest,
    cancellation: Arc<dyn CancellationSignal>,
    sender: mpsc::Sender<Result<ProviderStreamEvent, ProviderError>>,
}

impl Operation {
    async fn run(self) {
        if self.cancellation.is_cancelled() {
            let _ = self
                .completed(FinishOutcome::Cancelled, ExtensionMap::default())
                .await;
            return;
        }
        let mut start_metadata = ExtensionMap::default();
        start_metadata
            .insert(
                "zai:endpoint",
                json!(self.config.endpoint.endpoint_id().as_str()),
            )
            .expect("bounded endpoint metadata");
        if self
            .send(Ok(ProviderStreamEvent::ResponseStarted {
                response_id: Some(
                    ProviderResponseId::new(format!(
                        "response-{}",
                        self.request
                            .request_id
                            .as_str()
                            .chars()
                            .take(240)
                            .collect::<String>()
                    ))
                    .expect("bounded request-derived response ID"),
                ),
                metadata: start_metadata,
            }))
            .await
            .is_err()
        {
            return;
        }
        for decision in &self.serialized.fallback_decisions {
            let mut metadata = ExtensionMap::default();
            metadata
                .insert("zai:capability", json!(decision.capability.as_str()))
                .expect("bounded capability metadata");
            if self
                .send(Ok(ProviderStreamEvent::Warning {
                    message: decision.explanation.clone(),
                    metadata,
                }))
                .await
                .is_err()
            {
                return;
            }
        }

        let tool_ids = tool_id_map(&self.request.tools);
        let mut body = self.serialized.body.clone();
        let original_messages = body["messages"].clone();
        let mut total_content = String::new();
        let mut total_reasoning = String::new();
        let mut continuation_count = 0_u32;
        let continuation_limit = self.effective_continuation_limit();
        let mut aggregate_usage = NormalizedUsage::unavailable(UsageMode::Cumulative);

        loop {
            let state = match self.execute_with_retry(&body, &tool_ids).await {
                Ok(state) => state,
                Err(Terminal::Cancelled { visible }) => {
                    let mut metadata = ExtensionMap::default();
                    metadata
                        .insert("zai:visible-output", json!(visible))
                        .expect("bounded cancellation metadata");
                    let _ = self.completed(FinishOutcome::Cancelled, metadata).await;
                    return;
                }
                Err(Terminal::Interrupted { cause, state }) if state.visible => {
                    total_content.push_str(&state.content);
                    total_reasoning.push_str(&state.reasoning);
                    if let Some(usage) = state.usage.as_ref()
                        && self
                            .record_usage(&mut aggregate_usage, usage, true)
                            .await
                            .is_err()
                    {
                        return;
                    }
                    if !state.has_tool_fragments() && continuation_count < continuation_limit {
                        continuation_count += 1;
                        let mut metadata = ExtensionMap::default();
                        metadata
                            .insert(
                                "zai:interruption-recovery",
                                json!({
                                    "attempt": continuation_count,
                                    "cause": interruption_label(cause),
                                }),
                            )
                            .expect("bounded interruption metadata");
                        if self
                            .send(Ok(ProviderStreamEvent::Warning {
                                message: SafeMessage::new(
                                    "GLM stream interrupted; safely continuing from partial output",
                                )
                                .expect("bounded static warning"),
                                metadata,
                            }))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        body = continuation_body(
                            &self.serialized.body,
                            &original_messages,
                            &total_content,
                            &total_reasoning,
                            self.serialized.preserve_thinking,
                        );
                        continue;
                    }
                    if matches!(cause, StreamInterruptionCause::RemoteEof)
                        && state.tool_calls_complete()
                    {
                        let mut state = state;
                        for event in
                            match state.complete_tool_events(&self.request.request_id, &tool_ids) {
                                Ok(events) => events,
                                Err(error) => {
                                    let _ = self.send(Err(adapter_error(&error, true))).await;
                                    return;
                                }
                            }
                        {
                            if self.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                        let mut metadata = ExtensionMap::default();
                        metadata
                            .insert("zai:terminal", json!("complete-tool-call-before-eof"))
                            .expect("bounded terminal metadata");
                        let _ = self.completed(FinishOutcome::ToolCalls, metadata).await;
                        return;
                    }
                    let tool_call_started = state.has_tool_fragments();
                    let mut metadata = ExtensionMap::default();
                    metadata
                        .insert(
                            "vesper:stream-interruption",
                            json!({
                                "cause": interruption_label(cause),
                                "tool_call_started": tool_call_started,
                                "recovery_attempts": continuation_count,
                            }),
                        )
                        .expect("bounded interruption metadata");
                    let _ = self
                        .completed(
                            FinishOutcome::StreamInterrupted {
                                cause,
                                tool_call_started,
                            },
                            metadata,
                        )
                        .await;
                    return;
                }
                Err(Terminal::Error(error)) => {
                    let _ = self.send(Err(error)).await;
                    return;
                }
                Err(Terminal::ConsumerDropped) => return,
                Err(Terminal::Interrupted { .. }) => {
                    unreachable!("non-visible interruption is retried or classified as error")
                }
            };
            total_content.push_str(&state.content);
            total_reasoning.push_str(&state.reasoning);
            if let Some(usage) = state.usage.as_ref()
                && self
                    .record_usage(&mut aggregate_usage, usage, state.visible)
                    .await
                    .is_err()
            {
                return;
            }
            if state.finish_reason.as_deref() == Some("length") && !state.has_tool_calls() {
                if self.request.maximum_output_tokens.is_some()
                    || continuation_count >= continuation_limit
                {
                    let mut metadata = ExtensionMap::default();
                    metadata
                        .insert("zai:terminal", json!("continuation-limit"))
                        .expect("bounded terminal metadata");
                    let _ = self.completed(FinishOutcome::OutputLimit, metadata).await;
                    return;
                }
                continuation_count += 1;
                if self
                    .send(Ok(ProviderStreamEvent::ContentDelta {
                        stream_id: BoundedString::new("content").expect("static stream ID"),
                        part: ContentPart::Text(ContentText::new("\n").expect("static delta")),
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
                total_content.push('\n');
                body = continuation_body(
                    &self.serialized.body,
                    &original_messages,
                    &total_content[..total_content.len() - 1],
                    &total_reasoning,
                    self.serialized.preserve_thinking,
                );
                continue;
            }
            let finish = finish_outcome(state.finish_reason.as_deref());
            let _ = self.completed(finish, ExtensionMap::default()).await;
            return;
        }
    }

    fn effective_continuation_limit(&self) -> u32 {
        let mut limit = self.config.continuation_limit.min(20);
        if let Some(context) = &self.request.continuation {
            limit = limit.min(context.harness_maximum);
            if let Some(provider) = context.provider_maximum {
                limit = limit.min(provider);
            }
            if !context.may_continue() {
                return 0;
            }
        }
        limit
    }

    async fn execute_with_retry(
        &self,
        body: &Value,
        tool_ids: &BTreeMap<String, ToolId>,
    ) -> Result<AttemptState, Terminal> {
        for attempt in 0..=self.retry.maximum_retries {
            if self.cancellation.is_cancelled() {
                return Err(Terminal::Cancelled { visible: false });
            }
            match self
                .http
                .execute_stream(
                    &self.config,
                    &self.credential,
                    body,
                    &self.request.request_id,
                    tool_ids,
                    (Arc::clone(&self.cancellation), &self.sender),
                )
                .await
            {
                Ok(state) => return Ok(state),
                Err(AttemptFailure::Cancelled { visible }) => {
                    return Err(Terminal::Cancelled { visible });
                }
                Err(AttemptFailure::ConsumerDropped) => return Err(Terminal::ConsumerDropped),
                Err(AttemptFailure::Interrupted { cause, state }) if state.visible => {
                    return Err(Terminal::Interrupted { cause, state });
                }
                Err(
                    AttemptFailure::Transport { visible: true }
                    | AttemptFailure::Timeout { visible: true },
                ) => {
                    return Err(Terminal::Error(provider_error(
                        ErrorCategory::Transport,
                        Retryability::Never,
                        true,
                        "GLM stream transport failed after visible output",
                        Some("legacy-visible-interruption"),
                        None,
                        None,
                    )));
                }
                Err(AttemptFailure::Http {
                    status,
                    retry_after,
                    unsupported_content,
                }) => {
                    if !RetryPolicy::status_is_retryable(status)
                        || !self.retry.permits_retry(attempt)
                    {
                        let category = if unsupported_content {
                            ErrorCategory::UnsupportedCapability
                        } else if status == 429 {
                            ErrorCategory::QuotaOrRate
                        } else if RetryPolicy::status_is_retryable(status) {
                            ErrorCategory::TransientHttp
                        } else {
                            ErrorCategory::InvalidRequest
                        };
                        let mut error = provider_error(
                            category,
                            Retryability::Never,
                            false,
                            if unsupported_content {
                                "GLM rejected an unsupported content type"
                            } else {
                                "GLM request failed"
                            },
                            Some(if unsupported_content {
                                "unsupported-content"
                            } else {
                                "http-error"
                            }),
                            Some(status),
                            None,
                        );
                        if unsupported_content
                            && let Some(requirement) =
                                requirement_for_messages(&self.request.messages)
                        {
                            error = error.with_unsupported_requirement(&requirement);
                        }
                        return Err(Terminal::Error(error));
                    }
                    let delay = self.retry.delay(
                        attempt,
                        retry_after.as_deref(),
                        SystemTime::now(),
                        self.jitter.as_ref(),
                    );
                    self.retry_warning(attempt, delay, Some(status)).await?;
                    self.backoff(delay).await?;
                }
                Err(
                    AttemptFailure::Interrupted { .. }
                    | AttemptFailure::Transport { visible: false }
                    | AttemptFailure::Timeout { visible: false },
                ) => {
                    if !self.retry.permits_retry(attempt) {
                        return Err(Terminal::Error(provider_error(
                            ErrorCategory::Transport,
                            Retryability::Never,
                            false,
                            "GLM stream ended before a terminal event",
                            Some("incomplete-stream"),
                            None,
                            None,
                        )));
                    }
                    let delay =
                        self.retry
                            .delay(attempt, None, SystemTime::now(), self.jitter.as_ref());
                    self.retry_warning(attempt, delay, None).await?;
                    self.backoff(delay).await?;
                }
                Err(AttemptFailure::Adapter(error)) => {
                    return Err(Terminal::Error(adapter_error(&error, false)));
                }
            }
        }
        unreachable!("bounded retry loop always returns")
    }

    async fn retry_warning(
        &self,
        attempt: u32,
        delay: std::time::Duration,
        status: Option<u16>,
    ) -> Result<(), Terminal> {
        let mut metadata = ExtensionMap::default();
        metadata
            .insert(
                "zai:retry",
                json!({
                    "attempt": attempt + 1,
                    "delay_ms": u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    "status": status,
                }),
            )
            .expect("bounded retry metadata");
        self.send(Ok(ProviderStreamEvent::Warning {
            message: SafeMessage::new("GLM transient failure will be retried")
                .expect("bounded static warning"),
            metadata,
        }))
        .await
        .map_err(|()| Terminal::ConsumerDropped)
    }

    async fn backoff(&self, delay: std::time::Duration) -> Result<(), Terminal> {
        tokio::select! {
            _ = wait_cancelled(Arc::clone(&self.cancellation)) => {
                Err(Terminal::Cancelled { visible: false })
            }
            () = tokio::time::sleep(delay) => Ok(()),
        }
    }

    async fn completed(&self, finish: FinishOutcome, metadata: ExtensionMap) -> Result<(), ()> {
        self.send(Ok(ProviderStreamEvent::Completed { finish, metadata }))
            .await
    }

    async fn record_usage(
        &self,
        aggregate: &mut NormalizedUsage,
        delta: &NormalizedUsage,
        visible: bool,
    ) -> Result<(), ()> {
        if aggregate.checked_add_delta(delta).is_err() {
            let _ = self
                .send(Err(adapter_error(
                    &crate::GlmAdapterError::UsageOverflow,
                    visible,
                )))
                .await;
            return Err(());
        }
        aggregate.provider_metadata = delta.provider_metadata.clone();
        self.send(Ok(ProviderStreamEvent::Usage(aggregate.clone())))
            .await
    }

    async fn send(&self, event: Result<ProviderStreamEvent, ProviderError>) -> Result<(), ()> {
        self.sender.send(event).await.map_err(|_| ())
    }
}

fn continuation_body(
    base: &Value,
    original_messages: &Value,
    content: &str,
    reasoning: &str,
    preserve_thinking: bool,
) -> Value {
    let mut body = base
        .as_object()
        .cloned()
        .expect("serialized body is object");
    let mut messages = original_messages.as_array().cloned().unwrap_or_default();
    let mut assistant = serde_json::Map::new();
    assistant.insert("role".into(), json!("assistant"));
    assistant.insert("content".into(), json!(content));
    if preserve_thinking && !reasoning.is_empty() {
        assistant.insert("reasoning_content".into(), json!(reasoning));
    }
    messages.push(Value::Object(assistant));
    messages.push(json!({"role": "user", "content": CONTINUATION_MESSAGE}));
    body.insert("messages".into(), Value::Array(messages));
    Value::Object(body)
}

enum Terminal {
    Cancelled {
        visible: bool,
    },
    Interrupted {
        cause: StreamInterruptionCause,
        state: Box<AttemptState>,
    },
    Error(ProviderError),
    ConsumerDropped,
}

const fn interruption_label(cause: StreamInterruptionCause) -> &'static str {
    match cause {
        StreamInterruptionCause::GenerationDeadline => "generation-deadline",
        StreamInterruptionCause::ReadInactivity => "read-inactivity",
        StreamInterruptionCause::RemoteEof => "remote-eof",
        StreamInterruptionCause::Transport => "transport",
    }
}

/// Frozen exact continuation prompt owned only by the GLM adapter.
#[must_use]
pub const fn continuation_message() -> &'static str {
    CONTINUATION_MESSAGE
}
