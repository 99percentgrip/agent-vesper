//! Native passive GET /backend-api/wham/usage normalization.
//! Evidence: upstream backend-client/client/rate_limit_resets.rs at
//! 8e694e955ae02ca737230a5468c55d5847074072. Never consumes reset credits.
use serde_json::Value;
use vesper_provider::{ProviderError, ProviderUsage, UsageWindow};

#[allow(clippy::result_large_err)]
pub(crate) fn parse_usage(value: &Value) -> Result<ProviderUsage, ProviderError> {
    if !value.is_object()
        || !(value.get("rate_limit").is_some() || value.get("plan_type").is_some())
    {
        return Err(crate::wire::invalid());
    }
    let mut usage = ProviderUsage {
        authentication: Some("ChatGPT subscription".into()),
        plan: safe_label(value.get("plan_type")),
        ..Default::default()
    };
    append_windows(&mut usage.windows, value.get("rate_limit"), "");
    if let Some(additional) = value
        .get("additional_rate_limits")
        .and_then(Value::as_array)
    {
        for entry in additional.iter().take(7) {
            let label = safe_label(entry.get("limit_name"))
                .or_else(|| safe_label(entry.get("metered_feature")))
                .unwrap_or("Additional".into());
            append_windows(&mut usage.windows, entry.get("rate_limit"), &label);
        }
    }
    if usage.windows.is_empty() {
        usage.notice =
            Some("Account returned no quota windows; remaining allowance is unknown.".into());
    }
    Ok(usage)
}
fn safe_label(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    (!text.is_empty() && text.len() <= 80 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}
fn append_windows(out: &mut Vec<UsageWindow>, rate: Option<&Value>, prefix: &str) {
    let Some(rate) = rate else {
        return;
    };
    for key in ["primary_window", "secondary_window"] {
        let Some(window) = rate.get(key).filter(|v| v.is_object()) else {
            continue;
        };
        let seconds = window.get("limit_window_seconds").and_then(Value::as_u64);
        let duration = match seconds {
            Some(18_000) => "5h".into(),
            Some(604_800) => "Weekly".into(),
            Some(s) if s > 0 => format!("{}m", s / 60),
            _ => key.replace('_', " "),
        };
        let label = if prefix.is_empty() {
            duration
        } else {
            format!("{prefix} · {duration}")
        };
        out.push(UsageWindow {
            label,
            used_percent: window
                .get("used_percent")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v)),
            resets_at_unix_ms: window
                .get("reset_at")
                .and_then(Value::as_u64)
                .and_then(|s| s.checked_mul(1000)),
            ..Default::default()
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn normalizes_primary_weekly_and_premium_without_inventing_allowance() {
        let parsed = parse_usage(&json!({"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":43,"limit_window_seconds":18000,"reset_at":123},"secondary_window":{"used_percent":37,"limit_window_seconds":604800}},"additional_rate_limits":[{"limit_name":"Premium","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":18000}}}]})).unwrap();
        assert_eq!(parsed.windows.len(), 3);
        assert_eq!(parsed.windows[0].resets_at_unix_ms, Some(123000));
        assert_eq!(parsed.windows[1].label, "Weekly");
        assert_eq!(parsed.windows[2].label, "Premium · 5h");
        assert!(parsed.windows[0].limit.is_none());
        assert!(parse_usage(&json!({"error":"secret-canary"})).is_err());
        assert!(
            parse_usage(&json!({"plan_type":"plus"}))
                .unwrap()
                .notice
                .is_some()
        );
    }
}
