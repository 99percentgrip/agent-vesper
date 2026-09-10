//! Bounded HTTP rejection classification. Provider prose and arbitrary identifiers
//! are untrusted and must never enter a user-visible error or diagnostic field.
use crate::error;
use serde_json::Value;
use std::time::Duration;
use vesper_domain::{ErrorCategory, SafeMessage, SafeProviderCode};
use vesper_provider::{CancellationSignal, ProviderError};

const MAX_ERROR_BYTES: usize = 16 * 1024;
const ERROR_READ_BUDGET: Duration = Duration::from_secs(2);

pub(crate) async fn rejection(
    mut response: reqwest::Response,
    cancel: &dyn CancellationSignal,
) -> ProviderError {
    let status = response.status().as_u16();
    let (category, message) = match status {
        401 | 403 => (
            ErrorCategory::Authentication,
            "OpenAI denied access; check authentication and model entitlement",
        ),
        429 => (
            ErrorCategory::QuotaOrRate,
            "OpenAI rate or quota limit reached",
        ),
        400 | 404 | 422 => (
            ErrorCategory::InvalidRequest,
            "OpenAI rejected the request or selected model",
        ),
        _ => (ErrorCategory::Transport, "OpenAI service request failed"),
    };
    let mut result = error(message, category, false);
    result.http_status = Some(status);
    let read = async {
        if response
            .content_length()
            .is_some_and(|n| n > MAX_ERROR_BYTES as u64)
        {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if chunk.len() > MAX_ERROR_BYTES.saturating_sub(bytes.len()) {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<Value>(&bytes).ok()
    };
    let payload = tokio::select! { biased;
        _ = async { while !cancel.is_cancelled() { tokio::time::sleep(Duration::from_millis(25)).await; } } => {
            let mut cancelled = error("OpenAI request cancelled", ErrorCategory::Cancellation, false);
            cancelled.http_status = Some(status);
            return cancelled;
        },
        payload = tokio::time::timeout(ERROR_READ_BUDGET, read) => payload.ok().flatten(),
    };
    if let Some(payload) = payload {
        classify(&mut result, &payload);
    }
    result
}

fn classify(result: &mut ProviderError, payload: &Value) {
    let error_body = &payload["error"];
    classify_parameter(result, error_body);
    // Exact allowlists, not regex sanitation: a credential or private identifier
    // can itself be syntactically valid ASCII. Unknown codes remain unavailable.
    let code = match error_body["code"].as_str() {
        Some("context_length_exceeded") => "context_length_exceeded",
        Some("invalid_image") => "invalid_image",
        Some("invalid_image_format") => "invalid_image_format",
        Some("invalid_base64_image") => "invalid_base64_image",
        Some("image_too_large") => "image_too_large",
        Some("model_not_found") => "model_not_found",
        Some("unsupported_parameter") => "unsupported_parameter",
        Some("invalid_value") => "invalid_value",
        Some("invalid_type") => "invalid_type",
        Some("missing_required_parameter") => "missing_required_parameter",
        Some("invalid_json_schema") => "invalid_json_schema",
        Some("insufficient_quota") => "insufficient_quota",
        Some("rate_limit_exceeded") => "rate_limit_exceeded",
        _ => return,
    };
    let safe = SafeProviderCode::new(code).expect("static code");
    result.provider_code = Some(safe.clone());
    result.info.provider_code = Some(safe);
    if matches!(result.http_status, Some(400 | 404 | 422)) {
        let message = match code {
            "context_length_exceeded" => {
                result.info.category = ErrorCategory::ContextLimit;
                "OpenAI rejected the request: context window exceeded"
            }
            "invalid_image"
            | "invalid_image_format"
            | "invalid_base64_image"
            | "image_too_large" => {
                "OpenAI rejected image input; check image format, encoding and size"
            }
            "model_not_found" => {
                "OpenAI rejected the selected model; check model availability and account access"
            }
            "unsupported_parameter" => "OpenAI rejected an unsupported request parameter",
            "invalid_json_schema" => "OpenAI rejected the structured-output schema",
            _ => "OpenAI rejected a request field; see the safe provider error code",
        };
        result.info.safe_message = SafeMessage::new(message).expect("static message");
    }
    // Never change retryability/continuation based on error-body claims.
}

fn classify_parameter(result: &mut ProviderError, error_body: &Value) {
    if let Some(
        param @ ("input"
        | "model"
        | "tools"
        | "tool_choice"
        | "reasoning"
        | "reasoning.effort"
        | "text.format"
        | "max_output_tokens"
        | "instructions"
        | "include"
        | "store"
        | "stream"
        | "parallel_tool_calls"),
    ) = error_body["param"].as_str()
    {
        result
            .info
            .diagnostics
            .fields
            .insert("openai:parameter", serde_json::json!(param))
            .expect("bounded static parameter");
    }
    // Never change retryability/continuation based on error-body claims.
}
