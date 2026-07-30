use std::{sync::Arc, time::SystemTime};

use vesper_domain::{ContentPart, ContentText, ErrorCategory, Retryability};
use vesper_provider::{
    AuxiliaryRequestIntent, AuxiliaryRequestPort, CancellationSignal, ProviderError,
    ProviderFuture, ProviderRequest,
};

use crate::{
    GlmSession,
    error::{adapter_error, cancelled_error, provider_error},
    request::serialize_auxiliary_request,
    response::normalize_usage,
    retry::RetryPolicy,
    transport::{AttemptFailure, bounded_json, classify_non_success, wait_cancelled},
};

const MAX_AUXILIARY_BODY_BYTES: usize = 1024 * 1024;

impl AuxiliaryRequestPort for GlmSession {
    fn execute_auxiliary<'a>(
        &'a self,
        _intent: AuxiliaryRequestIntent,
        request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ContentPart, ProviderError>> {
        Box::pin(async move {
            let body = serialize_auxiliary_request(&request, self.config())
                .map_err(|error| adapter_error(&error, false))?;
            for attempt in 0..=self.retry.maximum_retries {
                if cancellation.is_cancelled() {
                    return Err(cancelled_error(false));
                }
                let response = self
                    .http
                    .post_json(
                        self.config(),
                        &self.credential,
                        &body,
                        Arc::clone(&cancellation),
                    )
                    .await
                    .map_err(attempt_error)?;
                if response.status().is_success() {
                    let payload = bounded_json(response, MAX_AUXILIARY_BODY_BYTES, cancellation)
                        .await
                        .map_err(attempt_error)?;
                    let content = payload
                        .pointer("/choices/0/message/content")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            adapter_error(&crate::GlmAdapterError::MalformedProtocol, false)
                        })?;
                    if let Some(usage) = payload.get("usage") {
                        // Validate bounds/provenance even though the current auxiliary port
                        // returns content only. A later runtime may expose this accounting.
                        normalize_usage(usage).map_err(|error| adapter_error(&error, false))?;
                    }
                    return ContentText::new(content)
                        .map(ContentPart::Text)
                        .map_err(|_| {
                            adapter_error(
                                &crate::GlmAdapterError::Limit("auxiliary content"),
                                false,
                            )
                        });
                }
                let status = response.status().as_u16();
                let failure = classify_non_success(response, Arc::clone(&cancellation)).await;
                let retry_after = match &failure {
                    AttemptFailure::Http { retry_after, .. } => retry_after.as_deref(),
                    _ => None,
                };
                if !RetryPolicy::status_is_retryable(status) || !self.retry.permits_retry(attempt) {
                    return Err(attempt_error(failure));
                }
                let delay = self.retry.delay(
                    attempt,
                    retry_after,
                    SystemTime::now(),
                    self.jitter.as_ref(),
                );
                tokio::select! {
                    _ = wait_cancelled(Arc::clone(&cancellation)) => {
                        return Err(cancelled_error(false));
                    }
                    () = tokio::time::sleep(delay) => {}
                }
            }
            unreachable!("bounded auxiliary retry loop always returns")
        })
    }
}

fn attempt_error(error: AttemptFailure) -> ProviderError {
    match error {
        AttemptFailure::Cancelled { .. } => cancelled_error(false),
        AttemptFailure::Timeout { .. } => provider_error(
            ErrorCategory::Timeout,
            Retryability::Never,
            false,
            "GLM auxiliary request timed out",
            Some("auxiliary-timeout"),
            None,
            None,
        ),
        AttemptFailure::Transport { .. } | AttemptFailure::Incomplete(_) => provider_error(
            ErrorCategory::Transport,
            Retryability::Never,
            false,
            "GLM auxiliary request failed",
            Some("auxiliary-transport"),
            None,
            None,
        ),
        AttemptFailure::Http { status, .. } => provider_error(
            if status == 429 {
                ErrorCategory::QuotaOrRate
            } else if RetryPolicy::status_is_retryable(status) {
                ErrorCategory::TransientHttp
            } else {
                ErrorCategory::InvalidRequest
            },
            Retryability::Never,
            false,
            "GLM auxiliary request failed",
            Some("auxiliary-http"),
            Some(status),
            None,
        ),
        AttemptFailure::Adapter(error) => adapter_error(&error, false),
        AttemptFailure::ConsumerDropped => cancelled_error(false),
    }
}
