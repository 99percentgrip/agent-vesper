// ProviderError is the shared session error DTO and is returned by value.
#![allow(clippy::result_large_err)]
//! Grok-session subscription allowance normalization.
//!
//! Evidence: xai-org/grok-build `extensions/billing.rs` and
//! `manager/enrichment.rs`, identical at pinned `f0e3be1100ef5252488e3be8bb0e91cf68d8c305`
//! and refreshed `482711333c7195dc16a272777f86086d615e2afb`. The session proxy
//! route is `GET /v1/billing?format=credits` after `GET /v1/user`. Percentage
//! and `currentPeriod` win over deprecated monthly cent fields. Missing values
//! stay missing; a present cent object with no `val` is the upstream zero.

use serde_json::Value;
use vesper_domain::ErrorCategory;
use vesper_provider::{ProviderError, ProviderUsage, UsageWindow};

use crate::{error, provider_id};

pub(crate) fn api_key_usage() -> ProviderUsage {
    ProviderUsage {
        authentication: Some("xAI API key (usage-based API billing)".into()),
        notice: Some(
            "Subscription allowance does not apply. API usage is billed separately from a Grok subscription; per-response token usage still appears on each turn."
                .into(),
        ),
        ..Default::default()
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn parse_subscription_usage(value: &Value) -> Result<ProviderUsage, ProviderError> {
    let root = value
        .as_object()
        .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
    if root.contains_key("error") && !root.contains_key("config") {
        return Err(usage_failure("Grok usage response was malformed", None));
    }
    let mut usage = ProviderUsage {
        authentication: Some("Grok account / SuperGrok (account allowance)".into()),
        plan: safe_label(root.get("subscriptionTier")),
        ..Default::default()
    };
    let Some(config) = root.get("config") else {
        usage.notice =
            Some("Account returned no allowance; remaining subscription usage is unknown.".into());
        return Ok(usage);
    };
    if config.is_null() {
        usage.notice =
            Some("Account returned no allowance; remaining subscription usage is unknown.".into());
        return Ok(usage);
    }
    let config = config
        .as_object()
        .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
    let unified = match config.get("isUnifiedBillingUser") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => return Err(usage_failure("Grok usage response was malformed", None)),
    };
    if let Some(window) = allowance_window(config, unified)? {
        usage.windows.push(window);
    }
    if let Some(window) = prepaid_window(config.get("prepaidBalance"))? {
        usage.windows.push(window);
    }
    if let Some(window) = on_demand_window(config.get("onDemandUsed"), config.get("onDemandCap"))? {
        usage.windows.push(window);
    }
    if let Some(products) = config.get("productUsage") {
        let products = products
            .as_array()
            .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
        for product in products.iter().take(8) {
            if let Some(window) = product_window(product)? {
                usage.windows.push(window);
            }
        }
    }
    if usage.windows.is_empty() {
        usage.notice = Some(
            "Account returned no allowance windows; remaining subscription usage is unknown."
                .into(),
        );
    }
    Ok(usage)
}

pub(crate) fn user_id(value: &Value) -> Result<String, ProviderError> {
    let id = value
        .get("userId")
        .and_then(Value::as_str)
        .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b'"')
    {
        return Err(usage_failure("Grok usage response was malformed", None));
    }
    Ok(id.to_owned())
}

pub(crate) fn usage_failure(message: &'static str, status: Option<u16>) -> ProviderError {
    let mut failure = error(message, ErrorCategory::MalformedProtocol, false);
    if let Some(status) = status {
        failure.http_status = Some(status);
        failure.info.category = match status {
            401 | 403 => ErrorCategory::Authentication,
            429 => ErrorCategory::QuotaOrRate,
            408 => ErrorCategory::Timeout,
            _ if (500..600).contains(&status) => ErrorCategory::Transport,
            _ => ErrorCategory::Transport,
        };
        failure.info.safe_message = vesper_domain::SafeMessage::new(match status {
            401 | 403 => "Grok usage session expired; sign in again",
            429 => "Grok usage service rate limit reached; try again later",
            _ => "Grok usage service is unavailable; conversation can continue",
        })
        .expect("static");
    }
    let _ = provider_id();
    failure
}

fn allowance_window(
    config: &serde_json::Map<String, Value>,
    unified: Option<bool>,
) -> Result<Option<UsageWindow>, ProviderError> {
    let percent = match config.get("creditUsagePercent") {
        None | Some(Value::Null) => None,
        Some(value) => Some(percent(value)?),
    };
    let period = config.get("currentPeriod").filter(|value| !value.is_null());
    let (period_type, period_end) = match period {
        None => (None, None),
        Some(period) => {
            let period = period
                .as_object()
                .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
            (
                period
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                period.get("end").and_then(Value::as_str).map(str::to_owned),
            )
        }
    };
    let legacy_end = config
        .get("billingPeriodEnd")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let resets_at_unix_ms = period_end
        .as_deref()
        .or(if percent.is_some() || period_type.is_some() {
            None
        } else {
            legacy_end.as_deref()
        })
        .map(rfc3339_unix_ms)
        .transpose()?;
    if percent.is_none() && period_type.is_none() && period_end.is_none() {
        return legacy_allowance(
            config,
            resets_at_unix_ms.or_else(|| {
                legacy_end
                    .as_deref()
                    .and_then(|text| rfc3339_unix_ms(text).ok())
            }),
        );
    }
    let resets_at_unix_ms = period_end.as_deref().map(rfc3339_unix_ms).transpose()?;
    Ok(Some(UsageWindow {
        label: allowance_label(unified, period_type.as_deref()),
        used_percent: percent,
        resets_at_unix_ms,
        detail: None,
        ..Default::default()
    }))
}

fn legacy_allowance(
    config: &serde_json::Map<String, Value>,
    resets_at_unix_ms: Option<u64>,
) -> Result<Option<UsageWindow>, ProviderError> {
    let used = cent(config.get("used"))?;
    let limit = cent(config.get("monthlyLimit"))?;
    if used.is_none() && limit.is_none() {
        return Ok(None);
    }
    let used_percent = match (used, limit) {
        (Some(used), Some(limit)) if limit > 0 => {
            Some((used as f64 * 100.0 / limit as f64).min(100.0))
        }
        _ => None,
    };
    let detail = match (used, limit) {
        (Some(used), Some(limit)) => Some(format!(
            "{} used of {}",
            format_cents(used)?,
            format_cents(limit)?
        )),
        (Some(used), None) => Some(format!("{} used", format_cents(used)?)),
        (None, Some(limit)) => Some(format!("allowance {}", format_cents(limit)?)),
        (None, None) => None,
    };
    Ok(Some(UsageWindow {
        label: "Monthly allowance".into(),
        used_percent,
        resets_at_unix_ms,
        detail,
        ..Default::default()
    }))
}

fn prepaid_window(value: Option<&Value>) -> Result<Option<UsageWindow>, ProviderError> {
    let Some(cents) = cent(value)? else {
        return Ok(None);
    };
    Ok(Some(UsageWindow {
        label: "Extra usage credits".into(),
        detail: Some(format_cents(cents)?),
        ..Default::default()
    }))
}

fn on_demand_window(
    used: Option<&Value>,
    cap: Option<&Value>,
) -> Result<Option<UsageWindow>, ProviderError> {
    let used = cent(used)?;
    let cap = cent(cap)?;
    if used.is_none() && cap.is_none() {
        return Ok(None);
    }
    let used_percent = match (used, cap) {
        (Some(used), Some(cap)) if cap > 0 => Some((used as f64 * 100.0 / cap as f64).min(100.0)),
        _ => None,
    };
    let detail = match (used, cap) {
        (Some(used), Some(cap)) => Some(format!(
            "{} used of {} cap",
            format_cents(used)?,
            format_cents(cap)?
        )),
        (Some(used), None) => Some(format!("{} used", format_cents(used)?)),
        (None, Some(cap)) => Some(format!("cap {}", format_cents(cap)?)),
        (None, None) => None,
    };
    Ok(Some(UsageWindow {
        label: "On-demand".into(),
        used_percent,
        detail,
        ..Default::default()
    }))
}

fn product_window(value: &Value) -> Result<Option<UsageWindow>, ProviderError> {
    let Some(product) = value.as_object() else {
        return Err(usage_failure("Grok usage response was malformed", None));
    };
    let Some(label) = product
        .get("product")
        .and_then(Value::as_str)
        .and_then(product_label)
    else {
        return Ok(None);
    };
    let used_percent = match product.get("usagePercent") {
        None | Some(Value::Null) => None,
        Some(value) => Some(percent(value)?),
    };
    if used_percent.is_none() {
        return Ok(None);
    }
    Ok(Some(UsageWindow {
        label,
        used_percent,
        ..Default::default()
    }))
}

fn allowance_label(unified: Option<bool>, period_type: Option<&str>) -> String {
    let period = match period_type {
        Some("USAGE_PERIOD_TYPE_WEEKLY") => "Weekly",
        Some("USAGE_PERIOD_TYPE_MONTHLY") => "Monthly",
        Some(other) => return safe_period(other, unified),
        None => {
            return if unified == Some(true) {
                "Unified allowance".into()
            } else {
                "Included allowance".into()
            };
        }
    };
    if unified == Some(true) {
        format!("Unified {period} allowance")
    } else {
        format!("{period} allowance")
    }
}

fn safe_period(period_type: &str, unified: Option<bool>) -> String {
    let cleaned = safe_label(Some(&Value::String(period_type.to_owned())))
        .unwrap_or_else(|| "Included".into());
    if unified == Some(true) {
        format!("Unified {cleaned} allowance")
    } else {
        format!("{cleaned} allowance")
    }
}

fn product_label(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > 80 || raw.chars().any(char::is_control) {
        return None;
    }
    let key = raw
        .strip_prefix("PRODUCT_")
        .unwrap_or(raw)
        .replace(['_', '-'], "")
        .to_ascii_uppercase();
    let known = match key.as_str() {
        "API" | "GROKAPI" => "API",
        "BUILD" | "GROKBUILD" => "Grok Build",
        "CHAT" | "GROKCHAT" => "Chat",
        "IMAGINE" | "GROKIMAGINE" => "Imagine",
        "VOICE" | "GROKVOICE" => "Grok Voice",
        _ => "",
    };
    if !known.is_empty() {
        return Some(known.to_owned());
    }
    let fallback = raw
        .strip_prefix("PRODUCT_")
        .unwrap_or(raw)
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ");
    (!fallback.is_empty()).then_some(fallback)
}

fn percent(value: &Value) -> Result<f64, ProviderError> {
    let number = value
        .as_f64()
        .filter(|number| number.is_finite() && (0.0..=100.0).contains(number))
        .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
    Ok(number)
}

fn cent(value: Option<&Value>) -> Result<Option<i64>, ProviderError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = value
        .as_object()
        .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
    let Some(raw) = object.get("val") else {
        return Ok(Some(0));
    };
    let cents = raw
        .as_i64()
        .or_else(|| {
            raw.as_f64()
                .filter(|number| number.is_finite() && number.fract() == 0.0)
                .and_then(|number| i64::try_from(number as i128).ok())
        })
        .filter(|cents| *cents >= 0)
        .ok_or_else(|| usage_failure("Grok usage response was malformed", None))?;
    Ok(Some(cents))
}

fn format_cents(cents: i64) -> Result<String, ProviderError> {
    if cents < 0 {
        return Err(usage_failure("Grok usage response was malformed", None));
    }
    Ok(format!("${}.{:02}", cents / 100, cents % 100))
}

fn safe_label(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    (!text.is_empty() && text.len() <= 80 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

fn rfc3339_unix_ms(value: &str) -> Result<u64, ProviderError> {
    let malformed = || usage_failure("Grok usage response was malformed", None);
    let (date, rest) = value.split_once('T').ok_or_else(malformed)?;
    let mut date = date.split('-');
    let year: i32 = date
        .next()
        .ok_or_else(malformed)?
        .parse()
        .map_err(|_| malformed())?;
    let month: u32 = date
        .next()
        .ok_or_else(malformed)?
        .parse()
        .map_err(|_| malformed())?;
    let day: u32 = date
        .next()
        .ok_or_else(malformed)?
        .parse()
        .map_err(|_| malformed())?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(malformed());
    }
    let (time, zone) = if let Some(time) = rest.strip_suffix('Z') {
        (time, 0)
    } else if let Some((time, offset)) = rest.split_once('+') {
        (time, zone_offset(offset)?)
    } else if let Some((time, offset)) = rest.rsplit_once('-') {
        (time, -zone_offset(offset)?)
    } else {
        return Err(malformed());
    };
    let mut time = time.split(':');
    let hour: u32 = time
        .next()
        .ok_or_else(malformed)?
        .parse()
        .map_err(|_| malformed())?;
    let minute: u32 = time
        .next()
        .ok_or_else(malformed)?
        .parse()
        .map_err(|_| malformed())?;
    let seconds = time.next().ok_or_else(malformed)?;
    if time.next().is_some() || hour > 23 || minute > 59 {
        return Err(malformed());
    }
    let (second_text, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second: u32 = second_text.parse().map_err(|_| malformed())?;
    if second > 60 || fraction.len() > 9 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(malformed());
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<3}")[..3].parse::<u64>().unwrap_or(0)
    };
    let days = days_from_civil(year, month, day).ok_or_else(malformed)?;
    let seconds = days
        .checked_mul(86_400)
        .and_then(|value| value.checked_add(i64::from(hour) * 3600))
        .and_then(|value| value.checked_add(i64::from(minute) * 60))
        .and_then(|value| value.checked_add(i64::from(second)))
        .and_then(|value| value.checked_sub(zone))
        .ok_or_else(malformed)?;
    if seconds < 0 {
        return Err(malformed());
    }
    u64::try_from(seconds)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1000))
        .and_then(|millis_base| millis_base.checked_add(millis))
        .ok_or_else(malformed)
}

fn zone_offset(value: &str) -> Result<i64, ProviderError> {
    let malformed = || usage_failure("Grok usage response was malformed", None);
    let (hour, minute) = value.split_once(':').ok_or_else(malformed)?;
    let hour: i64 = hour.parse().map_err(|_| malformed())?;
    let minute: i64 = minute.parse().map_err(|_| malformed())?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return Err(malformed());
    }
    Ok(hour * 3600 + minute * 60)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let mut year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prefers_percentage_and_period_over_legacy_monthly_cents() {
        let parsed = parse_subscription_usage(&json!({
            "subscriptionTier": "SuperGrok",
            "config": {
                "creditUsagePercent": 10.0,
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-06-01T00:00:00Z",
                    "end": "2026-06-08T00:00:00Z"
                },
                "monthlyLimit": {"val": 2000},
                "used": {"val": 2000},
                "billingPeriodEnd": "2026-07-01T00:00:00Z",
                "prepaidBalance": {"val": 1250},
                "productUsage": [
                    {"product": "PRODUCT_GROK_BUILD", "usagePercent": 61.2},
                    {"product": "PRODUCT_API", "usagePercent": 0}
                ],
                "history": [{"includedUsed": {"val": 1}}]
            }
        }))
        .unwrap();
        assert_eq!(parsed.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(parsed.windows[0].label, "Weekly allowance");
        assert_eq!(parsed.windows[0].used_percent, Some(10.0));
        assert_eq!(parsed.windows[0].resets_at_unix_ms, Some(1_780_876_800_000));
        assert!(parsed.windows[0].used.is_none());
        assert!(parsed.windows[0].limit.is_none());
        assert!(parsed.windows[0].remaining.is_none());
        assert!(parsed.windows[0].detail.is_none());
        assert_eq!(parsed.windows[1].label, "Extra usage credits");
        assert_eq!(parsed.windows[1].detail.as_deref(), Some("$12.50"));
        assert!(parsed.windows[1].used_percent.is_none());
        assert_eq!(parsed.windows[2].label, "Grok Build");
        assert_eq!(
            parse_subscription_usage(&json!({"config":{"productUsage":[{"product":"PRODUCT_GROKBUILD","usagePercent":68},{"product":"GrokVoice","usagePercent":1}]}})).unwrap().windows[0].label,
            "Grok Build"
        );
        assert_eq!(parsed.windows[2].used_percent, Some(61.2));
        assert_eq!(parsed.windows[3].used_percent, Some(0.0));
        assert_eq!(parsed.windows.len(), 4);
    }

    #[test]
    fn missing_credit_fields_are_not_zero_and_legacy_payload_stays_monthly() {
        let missing = parse_subscription_usage(&json!({"config": {}})).unwrap();
        assert!(missing.windows.is_empty());
        assert!(missing.notice.is_some());
        assert!(missing.plan.is_none());
        let zero = parse_subscription_usage(&json!({
            "config": {"prepaidBalance": {}, "creditUsagePercent": 0, "currentPeriod": {"type": "USAGE_PERIOD_TYPE_WEEKLY", "end": "2026-06-08T01:00:00+01:00"}}
        }))
        .unwrap();
        assert_eq!(zero.windows[0].used_percent, Some(0.0));
        assert_eq!(zero.windows[0].resets_at_unix_ms, Some(1_780_876_800_000));
        assert_eq!(zero.windows[1].detail.as_deref(), Some("$0.00"));
        let legacy = parse_subscription_usage(&json!({
            "config": {
                "monthlyLimit": {"val": 2000},
                "used": {"val": 500},
                "billingPeriodEnd": "2026-06-08T00:00:00.500Z"
            }
        }))
        .unwrap();
        assert_eq!(legacy.windows[0].label, "Monthly allowance");
        assert_eq!(legacy.windows[0].used_percent, Some(25.0));
        assert_eq!(
            legacy.windows[0].detail.as_deref(),
            Some("$5.00 used of $20.00")
        );
        assert_eq!(legacy.windows[0].resets_at_unix_ms, Some(1_780_876_800_500));
        assert!(legacy.windows[0].remaining.is_none());
    }

    #[test]
    fn malformed_percent_cents_and_identity_fail_closed() {
        assert!(parse_subscription_usage(&json!({"error": "secret-canary"})).is_err());
        assert!(parse_subscription_usage(&json!({"config": {"creditUsagePercent": 101}})).is_err());
        assert!(
            parse_subscription_usage(&json!({"config": {"prepaidBalance": {"val": -1}}})).is_err()
        );
        assert!(parse_subscription_usage(&json!({"config": []})).is_err());
        assert!(user_id(&json!({"userId": "account a"})).is_err());
        assert_eq!(
            user_id(&json!({"userId": "account-a"})).unwrap(),
            "account-a"
        );
        assert!(rfc3339_unix_ms("not-a-time").is_err());
    }

    #[test]
    fn on_demand_and_products_require_returned_fields() {
        let parsed = parse_subscription_usage(&json!({
            "config": {
                "creditUsagePercent": 40,
                "isUnifiedBillingUser": true,
                "onDemandUsed": {"val": 300},
                "onDemandCap": {"val": 5000},
                "productUsage": [{"product": "Chat"}]
            }
        }))
        .unwrap();
        assert_eq!(parsed.windows[0].label, "Unified allowance");
        assert_eq!(parsed.windows[1].label, "On-demand");
        assert_eq!(
            parsed.windows[1].detail.as_deref(),
            Some("$3.00 used of $50.00 cap")
        );
        assert_eq!(parsed.windows.len(), 2);
    }
}
