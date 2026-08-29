use std::sync::Arc;

use serde_json::Value;
use vesper_domain::ExtensionMap;
use vesper_provider::{CancellationSignal, ProviderError, QuotaUpdate};

use crate::{
    GlmAdapterError, GlmSession,
    error::{adapter_error, cancelled_error, provider_error},
    transport::{AttemptFailure, bounded_json, classify_non_success},
};

const MAX_QUOTA_BODY_BYTES: usize = 64 * 1024;

/// One frozen Coding Plan quota window.
#[derive(Debug, Clone, PartialEq)]
pub struct GlmQuotaWindow {
    /// `TOKENS_LIMIT` or `TIME_LIMIT`.
    pub kind: String,
    /// Provider unit.
    pub unit: Option<u64>,
    /// Provider count.
    pub number: Option<u64>,
    /// Provider limit.
    pub limit: Option<u64>,
    /// Current usage.
    pub used: Option<u64>,
    /// Remaining amount.
    pub remaining: Option<u64>,
    /// Provider percentage.
    pub percentage: Option<f64>,
    /// Provider reset epoch milliseconds.
    pub next_reset_ms: Option<u64>,
    /// Per-model usage details.
    pub usage_details: Vec<(String, u64)>,
}

/// Normalized official monitor response.
#[derive(Debug, Clone, PartialEq)]
pub struct GlmPlanUsage {
    /// Safe platform label.
    pub platform: String,
    /// Supported windows.
    pub quotas: Vec<GlmQuotaWindow>,
}

impl GlmPlanUsage {
    /// Converts windows to provider-neutral quota events.
    #[must_use]
    pub fn quota_updates(&self) -> Vec<QuotaUpdate> {
        self.quotas
            .iter()
            .map(|quota| QuotaUpdate {
                remaining: quota.remaining,
                limit: quota.limit,
                reset_after_ms: None,
                metadata: ExtensionMap::default(),
            })
            .collect()
    }
}

impl GlmSession {
    /// Queries the official allowlisted quota monitor independently from model
    /// response streaming. Custom endpoints fail before any request/auth header.
    pub async fn query_plan_usage(
        &self,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> Result<GlmPlanUsage, ProviderError> {
        if !self.config().endpoint.quota_supported() {
            return Err(adapter_error(
                &GlmAdapterError::UnsupportedRequest(
                    "quota monitoring is unavailable for custom endpoints",
                ),
                false,
            ));
        }
        let response = self
            .http
            .get_quota(self.config(), &self.credential, Arc::clone(&cancellation))
            .await
            .map_err(attempt_to_error)?;
        if !response.status().is_success() {
            return Err(attempt_to_error(
                classify_non_success(response, cancellation).await,
            ));
        }
        let payload = bounded_json(response, MAX_QUOTA_BODY_BYTES, cancellation)
            .await
            .map_err(attempt_to_error)?;
        parse_plan_usage(
            &payload,
            platform_label(self.config().endpoint.base_url().host_str()),
        )
        .map_err(|error| adapter_error(&error, false))
    }
}

fn platform_label(host: Option<&str>) -> &'static str {
    match host {
        Some("api.z.ai") => "Z.ai",
        Some("open.bigmodel.cn" | "dev.bigmodel.cn") => "BigModel (CN)",
        _ => "",
    }
}

fn attempt_to_error(error: AttemptFailure) -> ProviderError {
    match error {
        AttemptFailure::Cancelled { .. } => cancelled_error(false),
        AttemptFailure::Timeout { .. } => provider_error(
            vesper_domain::ErrorCategory::Timeout,
            vesper_domain::Retryability::Never,
            false,
            "GLM quota request timed out",
            Some("quota-timeout"),
            None,
            None,
        ),
        AttemptFailure::Transport { .. } | AttemptFailure::Interrupted { .. } => provider_error(
            vesper_domain::ErrorCategory::Transport,
            vesper_domain::Retryability::Never,
            false,
            "GLM quota request failed",
            Some("quota-transport"),
            None,
            None,
        ),
        AttemptFailure::Http { status, .. } => provider_error(
            vesper_domain::ErrorCategory::TransientHttp,
            vesper_domain::Retryability::Never,
            false,
            "GLM quota request failed",
            Some("quota-http"),
            Some(status),
            None,
        ),
        AttemptFailure::Adapter(error) => adapter_error(&error, false),
        AttemptFailure::ConsumerDropped => provider_error(
            vesper_domain::ErrorCategory::Cancellation,
            vesper_domain::Retryability::Never,
            false,
            "GLM quota consumer stopped",
            Some("consumer-dropped"),
            None,
            None,
        ),
    }
}

pub(crate) fn parse_plan_usage(
    payload: &Value,
    platform: &str,
) -> Result<GlmPlanUsage, GlmAdapterError> {
    let data = payload
        .get("data")
        .unwrap_or(payload)
        .as_object()
        .ok_or(GlmAdapterError::MalformedProtocol)?;
    let limits = data
        .get("limits")
        .and_then(Value::as_array)
        .ok_or(GlmAdapterError::MalformedProtocol)?;
    let mut quotas = Vec::new();
    for raw in limits.iter().take(16) {
        let Some(raw) = raw.as_object() else {
            continue;
        };
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or_default();
        if !matches!(kind, "TOKENS_LIMIT" | "TIME_LIMIT") {
            continue;
        }
        let mut usage_details = Vec::new();
        if let Some(details) = raw.get("usageDetails").and_then(Value::as_array) {
            for detail in details.iter().take(32) {
                let Some(detail) = detail.as_object() else {
                    continue;
                };
                let model = detail
                    .get("modelCode")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !model.is_empty()
                    && model.len() <= 80
                    && let Some(used) = safe_u64(detail.get("usage"))
                {
                    usage_details.push((model.to_owned(), used));
                }
            }
        }
        quotas.push(GlmQuotaWindow {
            kind: kind.to_owned(),
            unit: safe_u64(raw.get("unit")),
            number: safe_u64(raw.get("number")),
            limit: safe_u64(raw.get("usage")),
            used: safe_u64(raw.get("currentValue")),
            remaining: safe_u64(raw.get("remaining")),
            percentage: safe_percentage(raw.get("percentage")),
            next_reset_ms: safe_u64(raw.get("nextResetTime")),
            usage_details,
        });
    }
    if quotas.is_empty() {
        return Err(GlmAdapterError::MalformedProtocol);
    }
    Ok(GlmPlanUsage {
        platform: platform.to_owned(),
        quotas,
    })
}

fn safe_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return (number <= 1_000_000_000_000_000_000).then_some(number);
    }
    value
        .as_str()?
        .parse::<u64>()
        .ok()
        .filter(|number| *number <= 1_000_000_000_000_000_000)
}

fn safe_percentage(value: Option<&Value>) -> Option<f64> {
    let number = value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str()?.parse::<f64>().ok())
        })
        .filter(|value| value.is_finite())?;
    (0.0..=100.0).contains(&number).then_some(number)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn official_quota_is_bounded_and_normalized() {
        let result = parse_plan_usage(
            &json!({"data":{"limits":[
                {"type":"IGNORED"},
                {"type":"TOKENS_LIMIT","usage":100,"currentValue":25,"remaining":75,
                 "percentage":25.0,"usageDetails":[{"modelCode":"glm-5.2","usage":"7"}]}
            ]}}),
            "Z.ai",
        )
        .unwrap();
        assert_eq!(result.platform, "Z.ai");
        assert_eq!(result.quotas.len(), 1);
        assert_eq!(result.quotas[0].remaining, Some(75));
        assert_eq!(result.quotas[0].usage_details, [("glm-5.2".into(), 7)]);
    }

    #[test]
    fn malformed_or_empty_quota_fails_closed() {
        assert!(parse_plan_usage(&json!({"data":{"limits":[]}}), "Z.ai").is_err());
        assert!(parse_plan_usage(&json!({"data":{"limits":"bad"}}), "Z.ai").is_err());
    }
}
